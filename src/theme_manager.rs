//! Application theming: which palette is active, applying it to egui, and
//! publishing "the user changed the theme" as an event so the caller can
//! persist it.
//!
//! ## Deviations from the Python
//!
//! - **No widget registries.** Python's `_registry_plots` / `_registry_legends` /
//!   `_registry_hist_lut` exist because Qt widgets must be told about a theme
//!   change retroactively. In egui, every render reads the current `Palette`
//!   each frame, so a theme switch is just "the palette changed" and the next
//!   frame draws the new colours. `_style_plot_widget` / `_style_legend` /
//!   `_style_hist_lut` collapse into "read `palette()` in your render".
//! - **CSS dropped.** egui has no stylesheet language; only the palette JSON
//!   is read. The `.css` files under `themes/variants/` are no longer touched.
//! - **Signal → event.** `sigUpdateSettingsTheme` becomes
//!   `ThemeManagerEvent::UpdateSettingsTheme`. `App::ui` drains it and calls
//!   `settings.update_setting(group, variable, value)` to persist the choice.
//! - **`QApplication.setStyleSheet` → `ctx.set_visuals`.** The palette
//!   generates an `egui::Visuals`, which egui applies globally next frame.
//! - **`version` counter added.** Retained widgets that cache theme-dependent
//!   GPU state (rsplot's `Plot1D` in particular) can hold a `last_themed_version`
//!   and re-theme only when it changes.
//!
//! ## rsplot interaction
//!
//! `Plot1D` is a retained widget; it does not pick up `ctx.set_visuals`
//! automatically. Each `SpectrumPlotState` should watch `theme_manager.version()`
//! and re-theme its `Plot1D` (background, axis colour, grid) when it changes.
//! The specific setter names on `Plot1D` — `set_background_color`,
//! `set_foreground_color`, `set_grid_color` in some forks, or set via a
//! `PlotStyle` struct in others — need to be confirmed against your pinned
//! `rsplot 0.6.1`. The correct place to do that work is the plot's own
//! `render` fn, not here; this module only exposes the palette and the
//! version bump.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use eframe::egui::{self, Color32};
use serde_json::Value;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid colour value in theme {theme:?}: {value}")]
    BadColor { theme: String, value: Value },
    #[error("unknown theme: {0:?}")]
    UnknownTheme(String),
}

pub type Result<T> = std::result::Result<T, ThemeError>;

// ---------------------------------------------------------------------------
// Theme enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Theme {
    Light,
    LightCatppuccin,
    DarkCatppuccin,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::LightCatppuccin => "light-catppuccin",
            Theme::DarkCatppuccin => "dark-catppuccin",
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Theme::DarkCatppuccin)
    }

    /// All themes in display order — useful for building a picker menu.
    pub const ALL: &'static [Self] = &[Self::Light, Self::LightCatppuccin, Self::DarkCatppuccin];
}

impl std::fmt::Display for Theme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Theme {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "light" => Ok(Theme::Light),
            "light-catppuccin" => Ok(Theme::LightCatppuccin),
            "dark-catppuccin" => Ok(Theme::DarkCatppuccin),
            other => Err(ThemeError::UnknownTheme(other.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Three named colours from the theme JSON plus a dark/light flag.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub base: Color32,
    pub text: Color32,
    pub surface0: Color32,
    pub dark: bool,
}

impl Palette {
    /// The built-in `Light` theme — no JSON required.
    fn light() -> Self {
        Self {
            base: Color32::WHITE,
            text: Color32::BLACK,
            surface0: Color32::WHITE,
            dark: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ThemeManagerEvent {
    /// Emitted once per `apply` (including the startup apply). Consumers
    /// should persist it by calling
    /// `settings.update_setting(group, variable, value)`.
    ///
    /// Note: `group` / `variable` are always `"Appearance"` / `"theme"`, but
    /// they're carried on the event for parity with the Python signal and to
    /// let `App::ui` route it through the generic settings-update path
    /// without hard-coding the names.
    UpdateSettingsTheme {
        group: String,
        variable: String,
        value: String,
    },
}

// ---------------------------------------------------------------------------
// ThemeManager
// ---------------------------------------------------------------------------

pub struct ThemeManager {
    mode: Theme,
    palette: Palette,
    /// Bumped on every `apply`. Retained widgets compare against a local
    /// copy to know when to re-theme.
    version: u64,
    themes_path: PathBuf,
    events: VecDeque<ThemeManagerEvent>,
}

impl ThemeManager {
    /// Construct with the default theme `LightCatppuccin`, matching the
    /// Python default. Nothing is applied to `ctx` yet — call `apply` (or
    /// `apply_by_name`) once after construction.
    pub fn new(themes_path: impl Into<PathBuf>) -> Self {
        Self {
            mode: Theme::LightCatppuccin,
            palette: Palette::light(),
            version: 0,
            themes_path: themes_path.into(),
            events: VecDeque::new(),
        }
    }

    // ---- Accessors -----------------------------------------------------

    pub fn mode(&self) -> Theme {
        self.mode
    }

    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// Monotonic counter incremented on every `apply`. Retained widgets
    /// (rsplot plots, egui style caches) should compare their own copy
    /// against this to know when to refresh.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn themes_path(&self) -> &Path {
        &self.themes_path
    }

    // ---- Events --------------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<ThemeManagerEvent> {
        self.events.drain(..).collect()
    }

    // ---- Applying ------------------------------------------------------

    /// Load the palette for `theme`, install matching egui `Visuals`, bump
    /// the version counter, and emit `UpdateSettingsTheme`.
    pub fn apply(&mut self, ctx: &egui::Context, theme: Theme) -> Result<()> {
        let palette = self.load_palette(theme)?;

        self.mode = theme;
        self.palette = palette;
        self.version = self.version.wrapping_add(1);

        ctx.set_visuals(visuals_from_palette(palette));

        self.events
            .push_back(ThemeManagerEvent::UpdateSettingsTheme {
                group: "Appearance".to_string(),
                variable: "theme".to_string(),
                value: theme.as_str().to_string(),
            });

        Ok(())
    }

    /// Convenience for the startup path: parses the setting string and
    /// applies it. Returns `Err` for an unknown theme string.
    pub fn apply_by_name(&mut self, ctx: &egui::Context, name: &str) -> Result<()> {
        let theme: Theme = name.parse()?;
        self.apply(ctx, theme)
    }

    // ---- Palette loading -----------------------------------------------

    fn load_palette(&self, theme: Theme) -> Result<Palette> {
        if theme == Theme::Light {
            return Ok(Palette::light());
        }

        let path = self
            .themes_path
            .join("variants")
            .join(format!("{}.json", theme.as_str()));

        eprint!("Loading theme {:?} from {:?} ... ", theme, path);

        let text = std::fs::read_to_string(&path)?;
        let value: Value = serde_json::from_str(&text)?;

        let base = parse_color(&value["base"], theme)?;
        let text_col = parse_color(&value["text"], theme)?;
        let surface0 = parse_color(&value["surface0"], theme)?;

        Ok(Palette {
            base,
            text: text_col,
            surface0,
            dark: theme.is_dark(),
        })
    }
}

// ---------------------------------------------------------------------------
// Palette helpers
// ---------------------------------------------------------------------------

/// Accept both `"#rrggbb"` (or `"rrggbb"`) and `[r, g, b]` — the existing
/// theme JSON files use both forms.
fn parse_color(value: &Value, theme: Theme) -> Result<Color32> {
    match value {
        Value::String(s) => parse_hex(s).ok_or_else(|| ThemeError::BadColor {
            theme: theme.as_str().to_string(),
            value: value.clone(),
        }),
        Value::Array(a) if a.len() >= 3 => {
            let comp = |i: usize| -> u8 { a[i].as_u64().map(|x| x as u8).unwrap_or(0) };
            Ok(Color32::from_rgb(comp(0), comp(1), comp(2)))
        }
        _ => Err(ThemeError::BadColor {
            theme: theme.as_str().to_string(),
            value: value.clone(),
        }),
    }
}

fn parse_hex(s: &str) -> Option<Color32> {
    let s = s.strip_prefix('#').unwrap_or(s);
    let r = u8::from_str_radix(s.get(0..2)?, 16).ok()?;
    let g = u8::from_str_radix(s.get(2..4)?, 16).ok()?;
    let b = u8::from_str_radix(s.get(4..6)?, 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// Build an `egui::Visuals` from the palette.
///
/// This is deliberately **level 2** — panel/window fills, text override, and
/// the widget-background slots for hovered/active states. Level 3 (custom
/// widget layer with typed accessors like `theme.series_color(i)`) is
/// deferred; add it when a design spec requires it.
fn visuals_from_palette(p: Palette) -> egui::Visuals {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // Panel / window chrome
    v.panel_fill = p.base;
    v.window_fill = p.base;
    v.faint_bg_color = p.surface0;
    v.extreme_bg_color = p.surface0;

    // Text
    v.override_text_color = Some(p.text);

    // Widget backgrounds — small enough that the UI doesn't look flat
    v.widgets.noninteractive.bg_fill = p.base;
    v.widgets.inactive.bg_fill = p.surface0;
    v.widgets.inactive.weak_bg_fill = p.surface0;
    v.widgets.hovered.bg_fill = p.surface0;
    v.widgets.active.bg_fill = p.surface0;

    v
}
