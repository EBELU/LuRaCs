//! Application theming: built-in catppuccin palettes, user themes loaded from
//! JSON/TOML/XML (serde), and applying the active theme to `egui`.
//!
//! ## Architecture
//!
//! - [`ThemeDefinition`] — the whole theme as plain serde data.
//! - [`ThemeRegistry`] — name → theme map, populated from built-ins plus any
//!   `.json` / `.toml` / `.xml` files in the built-in and user theme dirs.
//! - [`ThemeManager`] — owns the registry, the active theme, a version
//!   counter (for retained widgets), and an event queue the app drains to
//!   persist the user's choice.
//! - `build_visuals` — the only place that touches `egui::Visuals`. Swap its
//!   body for `modern_egui` if/when you want that crate's `Theme` type as the
//!   target; nothing else in this module needs to change.
//!
//! ## File format detection
//!
//! Extension-driven: `.json` → `serde_json`, `.toml` → `toml`, `.xml` →
//! `quick_xml::de`. Colours must be hex strings (`#rrggbb` or `#rrggbbaa`) in
//! all three formats; JSON/TOML additionally accept `[r,g,b]` / `[r,g,b,a]`
//! arrays via the `Color` deserializer. XML does not — `quick_xml` cannot
//! buffer for the untagged enum.
//!
//! ## Precedence
//!
//! User themes (`<config>/themes/*.ext`) shadow built-ins by `meta.name`.
//! Built-ins ship compiled-in; no disk read required to use them.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ===========================================================================
// Errors
// ===========================================================================

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("XML: {0}")]
    Xml(String),
    #[error("unsupported theme file format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid colour: {0:?}")]
    BadColor(String),
    #[error("unknown theme: {0:?}")]
    UnknownTheme(String),
    #[error("{path}: {error}")]
    Load {
        path: PathBuf,
        error: Box<ThemeError>,
    },
}

pub type ThemeResult<T> = std::result::Result<T, ThemeError>;

// ===========================================================================
// Colour
// ===========================================================================

/// A theme colour. Serialises as `"#rrggbb"` (or `"#rrggbbaa"` when the
/// alpha channel is not 255). Deserialises from that, or from `[r,g,b]` /
/// `[r,g,b,a]` arrays in JSON/TOML.
///
/// Stored as `Color32` (sRGB, non-premultiplied on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub Color32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(Color32::from_rgb(r, g, b))
    }
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(Color32::from_rgba_unmultiplied(r, g, b, a))
    }
}

impl From<Color> for Color32 {
    fn from(c: Color) -> Self {
        c.0
    }
}

impl From<Color32> for Color {
    fn from(c: Color32) -> Self {
        Self(c)
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let [r, g, b, a] = self.0.to_srgba_unmultiplied();
        if a == 255 {
            s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}"))
        } else {
            s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Hex(String),
            Array(Vec<u8>),
        }
        match Raw::deserialize(d)? {
            Raw::Hex(s) => parse_hex(&s)
                .map(Color)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid hex colour: {s:?}"))),
            Raw::Array(v) => match v.as_slice() {
                [r, g, b] => Ok(Color::rgb(*r, *g, *b)),
                [r, g, b, a] => Ok(Color::rgba(*r, *g, *b, *a)),
                other => Err(serde::de::Error::custom(format!(
                    "colour array must be [r,g,b] or [r,g,b,a]; got {other:?}"
                ))),
            },
        }
    }
}

fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#').unwrap_or(s);
    match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some(Color32::from_rgb(r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            Some(Color32::from_rgba_unmultiplied(r, g, b, a))
        }
        _ => None,
    }
}

// ===========================================================================
// Theme definition
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeMeta {
    /// Machine identifier, unique across the registry. Matches the filename
    /// stem by convention but doesn't have to.
    pub name: String,
    /// Optional human-friendly label for menus.
    #[serde(default)]
    pub display_name: Option<String>,
    pub variant: Variant,
}

impl ThemeMeta {
    /// `display_name` if set, otherwise the identifier.
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }
}

/// Semantic colours. The applier derives egui's ~30 widget slots from these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeColors {
    /// Main window / panel background.
    pub background: Color,
    /// Secondary surfaces: inputs, inactive widgets.
    pub surface: Color,
    /// Elevated / hovered surfaces.
    pub surface_raised: Color,
    /// Primary text.
    pub text: Color,
    /// Secondary text, hints, disabled.
    pub text_muted: Color,
    /// Primary action / focus colour.
    pub accent: Color,
    /// Accent hover state.
    pub accent_hover: Color,
    /// Dividers, outlines.
    pub border: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
}

/// rsplot colours. Every field is optional — the applier falls back to
/// [`ThemeColors`] when omitted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlotColors {
    pub background: Option<Color>,
    pub grid: Option<Color>,
    pub axis: Option<Color>,
    /// Colour-rotator sequence. Defaults to a rotation over
    /// `accent`/`success`/`warning`/`error`/`text` when empty.
    #[serde(default)]
    pub series: Vec<Color>,
}

/// A complete theme. This is what users author in JSON/TOML/XML.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "theme")]
pub struct ThemeDefinition {
    pub meta: ThemeMeta,
    pub colors: ThemeColors,
    #[serde(default)]
    pub plot: PlotColors,
}

impl ThemeDefinition {
    /// Resolve the effective plot palette, filling gaps from `colors`.
    pub fn resolved_plot(&self) -> ResolvedPlotColors {
        let c = &self.colors;
        ResolvedPlotColors {
            background: self.plot.background.unwrap_or(c.background).0,
            grid: self.plot.grid.unwrap_or(c.surface_raised).0,
            axis: self.plot.axis.unwrap_or(c.text_muted).0,
            series: if self.plot.series.is_empty() {
                vec![c.accent.0, c.success.0, c.warning.0, c.error.0, c.text.0]
            } else {
                self.plot.series.iter().map(|c| c.0).collect()
            },
        }
    }
}

/// Convenience accessor for retained widgets (rsplot's `Plot1D`, etc.).
#[derive(Debug, Clone)]
pub struct ResolvedPlotColors {
    pub background: Color32,
    pub grid: Color32,
    pub axis: Color32,
    pub series: Vec<Color32>,
}

// ===========================================================================
// Built-in themes
// ===========================================================================

/// Canonical built-in theme names. Use these instead of string literals.
pub mod names {
    pub const LIGHT: &str = "light";
    pub const DARK: &str = "dark";
    pub const CATPPUCCIN_LATTE: &str = "catppuccin-latte";
    pub const CATPPUCCIN_FRAPPE: &str = "catppuccin-frappe";
    pub const CATPPUCCIN_MACCHIATO: &str = "catppuccin-macchiato";
    pub const CATPPUCCIN_MOCHA: &str = "catppuccin-mocha";
    pub const DEFAULT: &str = CATPPUCCIN_MOCHA;
}

pub fn catppuccin_latte() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::CATPPUCCIN_LATTE.into(),
            display_name: Some("Catppuccin Latte".into()),
            variant: Variant::Light,
        },
        colors: ThemeColors {
            background: Color::rgb(0xef, 0xf1, 0xf5),     // base
            surface: Color::rgb(0xcc, 0xd0, 0xda),        // surface0
            surface_raised: Color::rgb(0xbc, 0xc0, 0xcc), // surface1
            text: Color::rgb(0x4c, 0x4f, 0x69),
            text_muted: Color::rgb(0x6c, 0x6f, 0x85), // subtext0
            accent: Color::rgb(0x88, 0x39, 0xef),     // mauve
            accent_hover: Color::rgb(0x72, 0x87, 0xfd), // lavender
            border: Color::rgb(0xac, 0xb0, 0xbe),     // surface2
            error: Color::rgb(0xd2, 0x0f, 0x39),
            warning: Color::rgb(0xdf, 0x8e, 0x1d),
            success: Color::rgb(0x40, 0xa0, 0x2b),
        },
        plot: PlotColors::default(),
    }
}

pub fn catppuccin_frappe() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::CATPPUCCIN_FRAPPE.into(),
            display_name: Some("Catppuccin Frappé".into()),
            variant: Variant::Dark,
        },
        colors: ThemeColors {
            background: Color::rgb(0x30, 0x34, 0x46),     // base
            surface: Color::rgb(0x41, 0x45, 0x59),        // surface0
            surface_raised: Color::rgb(0x51, 0x56, 0x70), // surface1
            text: Color::rgb(0xc6, 0xd0, 0xf5),
            text_muted: Color::rgb(0xa5, 0xad, 0xce), // subtext0
            accent: Color::rgb(0xca, 0x9e, 0xe6),     // mauve
            accent_hover: Color::rgb(0xba, 0xbb, 0xf5), // lavender
            border: Color::rgb(0x62, 0x68, 0x7e),     // surface2
            error: Color::rgb(0xe7, 0x82, 0x84),
            warning: Color::rgb(0xe5, 0xc8, 0x90),
            success: Color::rgb(0xa6, 0xd1, 0x89),
        },
        plot: PlotColors::default(),
    }
}

pub fn catppuccin_macchiato() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::CATPPUCCIN_MACCHIATO.into(),
            display_name: Some("Catppuccin Macchiato".into()),
            variant: Variant::Dark,
        },
        colors: ThemeColors {
            background: Color::rgb(0x24, 0x27, 0x3a),
            surface: Color::rgb(0x36, 0x3a, 0x4f),
            surface_raised: Color::rgb(0x49, 0x4d, 0x64),
            text: Color::rgb(0xca, 0xd3, 0xf5),
            text_muted: Color::rgb(0xa5, 0xad, 0xcb),
            accent: Color::rgb(0xc6, 0xa0, 0xf6),
            accent_hover: Color::rgb(0xb7, 0xbd, 0xf8),
            border: Color::rgb(0x5b, 0x60, 0x78),
            error: Color::rgb(0xed, 0x87, 0x96),
            warning: Color::rgb(0xee, 0xd4, 0x9f),
            success: Color::rgb(0xa6, 0xda, 0x95),
        },
        plot: PlotColors::default(),
    }
}

pub fn catppuccin_mocha() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::CATPPUCCIN_MOCHA.into(),
            display_name: Some("Catppuccin Mocha".into()),
            variant: Variant::Dark,
        },
        colors: ThemeColors {
            background: Color::rgb(0x1e, 0x1e, 0x2e),
            surface: Color::rgb(0x31, 0x32, 0x44),
            surface_raised: Color::rgb(0x45, 0x47, 0x5a),
            text: Color::rgb(0xcd, 0xd6, 0xf4),
            text_muted: Color::rgb(0xa6, 0xad, 0xc8),
            accent: Color::rgb(0xcb, 0xa6, 0xf7),
            accent_hover: Color::rgb(0xb4, 0xbe, 0xfe),
            border: Color::rgb(0x58, 0x5b, 0x70),
            error: Color::rgb(0xf3, 0x8b, 0xa8),
            warning: Color::rgb(0xf9, 0xe2, 0xaf),
            success: Color::rgb(0xa6, 0xe3, 0xa1),
        },
        plot: PlotColors::default(),
    }
}

pub fn light() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::LIGHT.into(),
            display_name: Some("Light".into()),
            variant: Variant::Light,
        },
        colors: ThemeColors {
            background: Color::rgb(0xff, 0xff, 0xff),
            surface: Color::rgb(0xf2, 0xf2, 0xf2),
            surface_raised: Color::rgb(0xe6, 0xe6, 0xe6),
            text: Color::rgb(0x1a, 0x1a, 0x1a),
            text_muted: Color::rgb(0x66, 0x66, 0x66),
            accent: Color::rgb(0x1e, 0x66, 0xf5),
            accent_hover: Color::rgb(0x00, 0x4f, 0xd4),
            border: Color::rgb(0xcc, 0xcc, 0xcc),
            error: Color::rgb(0xc6, 0x28, 0x28),
            warning: Color::rgb(0xb2, 0x6a, 0x00),
            success: Color::rgb(0x2e, 0x7d, 0x32),
        },
        plot: PlotColors::default(),
    }
}

pub fn dark() -> ThemeDefinition {
    ThemeDefinition {
        meta: ThemeMeta {
            name: names::DARK.into(),
            display_name: Some("Dark".into()),
            variant: Variant::Dark,
        },
        colors: ThemeColors {
            background: Color::rgb(0x12, 0x12, 0x12),
            surface: Color::rgb(0x1e, 0x1e, 0x1e),
            surface_raised: Color::rgb(0x2a, 0x2a, 0x2a),
            text: Color::rgb(0xe8, 0xe8, 0xe8),
            text_muted: Color::rgb(0x99, 0x99, 0x99),
            accent: Color::rgb(0x64, 0xb5, 0xf6),
            accent_hover: Color::rgb(0x9a, 0xd2, 0xff),
            border: Color::rgb(0x3a, 0x3a, 0x3a),
            error: Color::rgb(0xef, 0x53, 0x50),
            warning: Color::rgb(0xff, 0xb7, 0x4d),
            success: Color::rgb(0x81, 0xc7, 0x84),
        },
        plot: PlotColors::default(),
    }
}

// ===========================================================================
// File loader
// ===========================================================================

#[derive(Debug, Clone, Copy)]
pub enum ThemeFormat {
    Json,
    Toml,
    Xml,
}

impl ThemeFormat {
    fn from_path(p: &Path) -> Option<Self> {
        match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "xml" => Some(Self::Xml),
            _ => None,
        }
    }

    fn parse(self, text: &str) -> ThemeResult<ThemeDefinition> {
        match self {
            Self::Json => Ok(serde_json::from_str(text)?),
            Self::Toml => Ok(toml::from_str(text)?),
            Self::Xml => quick_xml::de::from_str(text).map_err(|e| ThemeError::Xml(e.to_string())),
        }
    }
}

/// Load a theme from a single file. Format is chosen from the extension.
pub fn load_theme_file(path: &Path) -> ThemeResult<ThemeDefinition> {
    let format = ThemeFormat::from_path(path)
        .ok_or_else(|| ThemeError::UnsupportedFormat(path.display().to_string()))?;
    let text = std::fs::read_to_string(path)?;
    format.parse(&text).map_err(|error| ThemeError::Load {
        path: path.to_path_buf(),
        error: Box::new(error),
    })
}

// ===========================================================================
// Registry
// ===========================================================================

/// Name → theme map. Populated from built-ins and from files on disk.
#[derive(Default)]
pub struct ThemeRegistry {
    themes: HashMap<String, ThemeDefinition>,
    /// Display order (insertion order, with user themes appended).
    order: Vec<String>,
}

impl ThemeRegistry {
    /// Registry pre-populated with all built-in themes.
    pub fn with_builtins() -> Self {
        let mut r = Self::default();
        for t in [
            light(),
            dark(),
            catppuccin_latte(),
            catppuccin_frappe(),
            catppuccin_macchiato(),
            catppuccin_mocha(),
        ] {
            r.register(t);
        }
        r
    }

    /// Insert (or replace) a theme. Replacing keeps the original position.
    pub fn register(&mut self, theme: ThemeDefinition) {
        let name = theme.meta.name.clone();
        if !self.themes.contains_key(&name) {
            self.order.push(name.clone());
        }
        self.themes.insert(name, theme);
    }

    pub fn get(&self, name: &str) -> Option<&ThemeDefinition> {
        self.themes.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.themes.contains_key(name)
    }

    /// Iterate themes in display order.
    pub fn iter(&self) -> impl Iterator<Item = &ThemeDefinition> {
        self.order.iter().filter_map(|n| self.themes.get(n))
    }

    /// Names in display order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    /// Scan `dir` for `.json`/`.toml`/`.xml` files and register each.
    /// Failures are collected and returned; successful loads are always
    /// registered. Does not error out on individual file failures.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<ThemeError> {
        let mut errors = Vec::new();
        let entries = match std::fs::read_dir(dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return errors,
            Err(e) => {
                errors.push(ThemeError::Io(e));
                return errors;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if ThemeFormat::from_path(&path).is_none() {
                continue;
            }
            match load_theme_file(&path) {
                Ok(theme) => self.register(theme),
                Err(e) => errors.push(e),
            }
        }
        errors
    }
}

// ===========================================================================
// Events
// ===========================================================================

#[derive(Debug, Clone)]
pub enum ThemeManagerEvent {
    /// Emitted on every successful `apply`. The app should persist it via
    /// `settings.update(Category::Theme, "name", Value::String(name))`.
    /// The startup apply also emits — callers drain and discard once after
    /// initial construction to avoid persisting a no-op.
    ThemeChanged { name: String },
}

// ===========================================================================
// Manager
// ===========================================================================

pub struct ThemeManager {
    registry: ThemeRegistry,
    current: ThemeDefinition,
    version: u64,
    builtin_dir: PathBuf,
    user_dir: PathBuf,
    events: VecDeque<ThemeManagerEvent>,
}

impl ThemeManager {
    /// Build the manager: register built-ins, then scan `builtin_dir` and
    /// `user_dir` for extra themes. Resolves `preferred` to a theme; falls
    /// back to [`names::DEFAULT`] and finally to the first registered theme
    /// if that is somehow missing too.
    ///
    /// Nothing is applied to `ctx` here — call [`Self::apply`] (or
    /// [`Self::apply_current`]) once after construction.
    pub fn new(
        builtin_dir: impl Into<PathBuf>,
        user_dir: impl Into<PathBuf>,
        preferred: Option<&str>,
    ) -> Self {
        let builtin_dir = builtin_dir.into();
        let user_dir = user_dir.into();

        let mut registry = ThemeRegistry::with_builtins();
        for err in registry.load_dir(&builtin_dir) {
            tracing::warn!("built-in theme: {err}");
        }
        for err in registry.load_dir(&user_dir) {
            tracing::warn!("user theme: {err}");
        }

        let current = preferred
            .and_then(|n| registry.get(n).cloned())
            .or_else(|| registry.get(names::DEFAULT).cloned())
            .or_else(|| registry.iter().next().cloned())
            .unwrap_or_else(catppuccin_mocha);

        Self {
            registry,
            current,
            version: 0,
            builtin_dir,
            user_dir,
            events: VecDeque::new(),
        }
    }

    // ---- Accessors ------------------------------------------------------

    pub fn current(&self) -> &ThemeDefinition {
        &self.current
    }

    pub fn current_name(&self) -> &str {
        &self.current.meta.name
    }

    /// Monotonic counter, bumped on every successful [`Self::apply`].
    /// Retained widgets (rsplot `Plot1D`, custom caches) compare a local
    /// copy against this to know when to re-theme.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn registry(&self) -> &ThemeRegistry {
        &self.registry
    }

    pub fn builtin_dir(&self) -> &Path {
        &self.builtin_dir
    }

    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    // ---- Events ---------------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<ThemeManagerEvent> {
        self.events.drain(..).collect()
    }

    // ---- Applying -------------------------------------------------------

    /// Switch to `name`, apply it to `ctx`, bump the version, and emit a
    /// [`ThemeManagerEvent::ThemeChanged`]. Errors if the name is unknown.
    pub fn apply(&mut self, ui: &mut egui::Ui, name: &str) -> ThemeResult<()> {
        let theme = self
            .registry
            .get(name)
            .cloned()
            .ok_or_else(|| ThemeError::UnknownTheme(name.to_string()))?;
        self.apply_definition(ui, theme);
        Ok(())
    }

    /// Re-apply the current theme to `ctx`. Useful after registry reload if
    /// the current theme's file changed.
    pub fn apply_current(&mut self, ctx: &egui::Context) -> ThemeResult<()> {
        let name = self.current.meta.name.clone();
        let current = self
            .registry
            .get(&name)
            .cloned()
            .unwrap_or_else(|| self.current.clone());
        self.apply_definition(ctx, current);
        Ok(())
    }

    fn apply_definition(&mut self, ctx: &egui::Context, theme: ThemeDefinition) {
        let visuals = build_visuals(&theme);
        let egui_theme = match theme.meta.variant {
            Variant::Dark => egui::Theme::Dark,
            Variant::Light => egui::Theme::Light,
        };

        // Same visuals in both slots: an OS theme toggle mid-session can't
        // drop the user back onto egui's defaults.
        ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
        ctx.set_visuals_of(egui::Theme::Light, visuals);

        // And pin the active preference so `ctx.theme()` agrees with what
        // the user just picked, regardless of system setting.
        ctx.set_theme(match egui_theme {
            egui::Theme::Dark => egui::ThemePreference::Dark,
            egui::Theme::Light => egui::ThemePreference::Light,
        });

        self.current = theme;
        self.version = self.version.wrapping_add(1);
        self.events.push_back(ThemeManagerEvent::ThemeChanged {
            name: self.current.meta.name.clone(),
        });
    }

    // ---- Registry reload ------------------------------------------------

    /// Rescan both theme directories. Built-in themes stay registered
    /// unconditionally; user themes are re-discovered. Returns per-file
    /// errors without aborting on any single failure.
    pub fn reload(&mut self) -> Vec<ThemeError> {
        let mut registry = ThemeRegistry::with_builtins();
        let mut errors = registry.load_dir(&self.builtin_dir);
        errors.extend(registry.load_dir(&self.user_dir));
        self.registry = registry;
        errors
    }
}

// ===========================================================================
// egui applier
// ===========================================================================

/// Convert a [`ThemeDefinition`] into an `egui::Visuals`.
///
/// **This is the only function that knows about egui's visual slots.** If you
/// adopt `modern_egui`'s theme type later, replace this body with a
/// `modern_egui::Theme` builder and call `theme.apply(ctx)` from
/// `apply_definition` instead of `ctx.set_style`. Nothing else in this file
/// needs to change.
pub fn build_visuals(theme: &ThemeDefinition) -> egui::Visuals {
    use egui::Stroke;

    let c = &theme.colors;
    let base = egui::Visuals {
        dark_mode: matches!(theme.meta.variant, Variant::Dark),
        ..if matches!(theme.meta.variant, Variant::Dark) {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        }
    };

    let mut v = base;

    // Panel / window chrome
    v.panel_fill = c.background.0;
    v.window_fill = c.background.0;
    v.faint_bg_color = c.surface.0;
    v.extreme_bg_color = c.surface.0;
    v.window_stroke = Stroke::new(1.0, c.border.0);
    v.window_shadow = egui::epaint::Shadow::NONE;

    // Text
    v.override_text_color = Some(c.text.0);

    // Widgets
    v.widgets.noninteractive.bg_fill = c.background.0;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.border.0);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text.0);

    v.widgets.inactive.bg_fill = c.surface.0;
    v.widgets.inactive.weak_bg_fill = c.surface.0;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, c.border.0);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.text.0);

    v.widgets.hovered.bg_fill = c.surface_raised.0;
    v.widgets.hovered.weak_bg_fill = c.surface_raised.0;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, c.accent.0);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, c.text.0);

    v.widgets.active.bg_fill = c.accent.0;
    v.widgets.active.weak_bg_fill = c.accent.0;
    v.widgets.active.bg_stroke = Stroke::new(1.0, c.accent_hover.0);
    v.widgets.active.fg_stroke = Stroke::new(1.0, c.background.0);

    v.widgets.open.bg_fill = c.surface_raised.0;
    v.widgets.open.weak_bg_fill = c.surface_raised.0;

    // Selection (text edit, listbox highlight)
    v.selection.bg_fill = c.accent.0.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, c.accent.0);

    // Hyperlinks
    v.hyperlink_color = c.accent.0;

    // Warnings / errors
    v.warn_fg_color = c.warning.0;
    v.error_fg_color = c.error.0;

    v
}
