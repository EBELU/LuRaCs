//! Application-wide settings and per-user file paths.
//!
//! Settings are organised into logical [`Category`] sections. Each category
//! is a self-contained struct that serialises to the correspondingly-named
//! key in `settings.json`, so the on-disk layout mirrors the UI directly.
//!
//! [`Paths`] and [`Temp`] are runtime-only and never serialised.
//!
//! [`Settings`] holds a `VecDeque<SettingsEvent>` the GUI drains each frame;
//! that replaces the Python Qt signals (`sigSettingChanged`,
//! `latestConnectionUpdated`).

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::gui::ColorScheme;

// ===========================================================================
// Errors
// ===========================================================================

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cannot determine user home / data directory")]
    NoHomeDir,
    #[error("unknown settings category: {0}")]
    UnknownCategory(String),
    #[error("unknown field {field:?} in category {category:?}")]
    UnknownField { category: String, field: String },
    #[error("value type mismatch when updating {category}.{field}")]
    TypeMismatch { category: String, field: String },
}

pub type SettingsResult<T> = std::result::Result<T, SettingsError>;

// ===========================================================================
// Category enum
// ===========================================================================

/// A logical group of settings. Every category is one section in the
/// settings UI and one key in `settings.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Theme,
    Plotting,
    Importing,
    Scanning,
    Gps,
    Optimizer,
    RealTime,
    Spectrogram,
    Logging,
    Connections,
    Workspace,
    Session,
}

impl Category {
    /// Every category, in a sensible UI order.
    pub const ALL: &'static [Self] = &[
        Self::Theme,
        Self::Plotting,
        Self::Importing,
        Self::Scanning,
        Self::Gps,
        Self::Optimizer,
        Self::RealTime,
        Self::Spectrogram,
        Self::Logging,
        Self::Connections,
        Self::Workspace,
        Self::Session,
    ];

    /// Machine-stable identifier — also the JSON key for this category.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Plotting => "plotting",
            Self::Importing => "importing",
            Self::Scanning => "scanning",
            Self::Gps => "gps",
            Self::Optimizer => "optimizer",
            Self::RealTime => "real_time",
            Self::Spectrogram => "spectrogram",
            Self::Logging => "logging",
            Self::Connections => "connections",
            Self::Workspace => "workspace",
            Self::Session => "session",
        }
    }

    /// Human-readable heading for a settings panel.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Plotting => "Plotting",
            Self::Importing => "Importing",
            Self::Scanning => "Scanning",
            Self::Gps => "GPS & map",
            Self::Optimizer => "Optimizer",
            Self::RealTime => "Real-time values",
            Self::Spectrogram => "Spectrogram",
            Self::Logging => "Logging",
            Self::Connections => "Recent connections",
            Self::Workspace => "Workspace",
            Self::Session => "Last session",
        }
    }

    /// Every field name in this category, in declaration order.
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Theme => &["name", "color_rotator_scheme", "font_size"],
            Self::Plotting => &["pen", "brush", "energy_max", "tabbed_spectrum_view"],
            Self::Importing => &["load_rois", "load_instrument", "load_calibration"],
            Self::Scanning => &[
                "update_loop_delay_s",
                "spectrum_update_delay_s",
                "ui_scan_length",
                "headless_scan_length",
            ],
            Self::Gps => &[
                "sample_frequency",
                "point_alpha",
                "move_to_current_max_rate_s",
                "save_extra_gps_data",
                "save_extra_measurement_time_data",
            ],
            Self::Optimizer => &["max_iter", "tolerance", "use_chi2_weight"],
            Self::RealTime => &["values_buffer_length"],
            Self::Spectrogram => &["history_length", "roi_display_duration_s"],
            Self::Logging => &[
                "verbose_calculations",
                "buffer_length",
                "catch_exceptions",
                "write_to_file",
                "write_to_console",
            ],
            Self::Connections => &["recent"],
            Self::Workspace => &["loaded_spectra", "roi_regions"],
            Self::Session => &[
                "last_map_url",
                "last_deconv_response_file",
                "last_network_ip",
            ],
        }
    }

    /// Whether `field` belongs to this category.
    pub fn contains_field(self, field: &str) -> bool {
        self.fields().contains(&field)
    }

    /// Find the category that owns `field` (field names are globally unique).
    pub fn of_field(field: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.contains_field(field))
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for Category {
    type Err = SettingsError;

    fn from_str(s: &str) -> SettingsResult<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| SettingsError::UnknownCategory(s.to_string()))
    }
}

// ===========================================================================
// Category structs
// ===========================================================================

/// Colour palette, font size, and the palette used to colour-rotate spectra.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    /// Currently-selected theme name (matches a file in `paths.themes`).
    pub name: String,
    /// Palette used by the auto colour-rotator.
    pub color_rotator_scheme: ColorScheme,
    pub font_size: u32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "light-catppuccin".into(),
            color_rotator_scheme: ColorScheme::Catppuccin,
            font_size: 10,
        }
    }
}

/// How spectrum and map plots are rendered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Plotting {
    pub pen: bool,
    pub brush: bool,
    /// Upper bound on the spectrum plot energy axis (keV).
    pub energy_max: f64,
    /// Show spectra in tabs instead of the classic stacked view.
    pub tabbed_spectrum_view: bool,
}

impl Default for Plotting {
    fn default() -> Self {
        Self {
            pen: true,
            brush: false,
            energy_max: 3000.0,
            tabbed_spectrum_view: false,
        }
    }
}

/// What happens automatically when a spectrum or instrument is imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Importing {
    pub load_rois: bool,
    pub load_instrument: bool,
    pub load_calibration: bool,
}

impl Default for Importing {
    fn default() -> Self {
        Self {
            load_rois: true,
            load_instrument: true,
            load_calibration: true,
        }
    }
}

/// Acquisition loop timing and scan durations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Scanning {
    pub update_loop_delay_s: f64,
    pub spectrum_update_delay_s: f64,
    pub ui_scan_length: u32,
    pub headless_scan_length: u32,
}

impl Default for Scanning {
    fn default() -> Self {
        Self {
            update_loop_delay_s: 1.0,
            spectrum_update_delay_s: 1.0,
            ui_scan_length: 5,
            headless_scan_length: 2,
        }
    }
}

/// GPS sampling and everything about how points appear and are recorded on
/// the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Gps {
    pub sample_frequency: f64,
    /// 0–255 alpha channel for plotted map points.
    pub point_alpha: u8,
    pub move_to_current_max_rate_s: f64,
    pub save_extra_gps_data: bool,
    pub save_extra_measurement_time_data: bool,
}

impl Default for Gps {
    fn default() -> Self {
        Self {
            sample_frequency: 5.0,
            point_alpha: 255,
            move_to_current_max_rate_s: 1.0,
            save_extra_gps_data: false,
            save_extra_measurement_time_data: false,
        }
    }
}

/// Peak-fit optimiser knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Optimizer {
    pub max_iter: usize,
    pub tolerance: f64,
    pub use_chi2_weight: bool,
}

impl Default for Optimizer {
    fn default() -> Self {
        Self {
            max_iter: 250,
            tolerance: 1e-6,
            use_chi2_weight: true,
        }
    }
}

/// Rolling buffer for the numeric "real-time values" readout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RealTime {
    pub values_buffer_length: usize,
}

impl Default for RealTime {
    fn default() -> Self {
        Self {
            values_buffer_length: 60,
        }
    }
}

/// Spectrogram history depth and ROI overlay duration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Spectrogram {
    /// Number of recent acquisition rows kept in the spectrogram history.
    pub history_length: usize,
    /// How long (seconds) ROIs stay overlaid on the spectrogram.
    pub roi_display_duration_s: f64,
}

impl Default for Spectrogram {
    fn default() -> Self {
        Self {
            history_length: 256,
            roi_display_duration_s: 60.0,
        }
    }
}

/// Logging verbosity and destinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Logging {
    /// Emit verbose per-step messages during peak-fit calculations.
    pub verbose_calculations: bool,
    /// Maximum number of log lines kept in memory.
    pub buffer_length: usize,
    pub catch_exceptions: bool,
    pub write_to_file: bool,
    pub write_to_console: bool,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            verbose_calculations: true,
            buffer_length: 1024,
            catch_exceptions: true,
            write_to_file: false,
            write_to_console: false,
        }
    }
}

/// Bounded list of recently-used BLE connection names.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Connections {
    pub recent: VecDeque<String>,
}

impl Connections {
    /// Maximum number of remembered connections.
    pub const MAX_ITEMS: usize = 10;

    /// Append `name` if new, keeping the deque bounded to [`Self::MAX_ITEMS`].
    pub fn push(&mut self, name: impl Into<String>) {
        let name = name.into();
        if self.recent.iter().any(|c| c == &name) {
            return;
        }
        while self.recent.len() >= Self::MAX_ITEMS {
            self.recent.pop_front();
        }
        self.recent.push_back(name);
    }

    /// Enforce [`Self::MAX_ITEMS`] after loading from disk.
    pub fn trim(&mut self) {
        while self.recent.len() > Self::MAX_ITEMS {
            self.recent.pop_front();
        }
    }
}

/// Persisted working state: which spectra were loaded and which ROIs were
/// defined, so the next launch restores the same workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub loaded_spectra: Option<Value>,
    pub roi_regions: Option<Value>,
}

/// Last-used values from the previous session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub last_map_url: String,
    pub last_deconv_response_file: String,
    pub last_network_ip: String,
}

// ===========================================================================
// SettingsFile (on-disk shape)
// ===========================================================================

/// The exact shape of `settings.json`. Mirrors [`Settings`] minus the
/// runtime-only fields (`headless`, `paths`, `temp`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsFile {
    pub theme: Theme,
    pub plotting: Plotting,
    pub importing: Importing,
    pub scanning: Scanning,
    pub gps: Gps,
    pub optimizer: Optimizer,
    pub real_time: RealTime,
    pub spectrogram: Spectrogram,
    pub logging: Logging,
    pub connections: Connections,
    pub workspace: Workspace,
    pub session: Session,
}

// ===========================================================================
// Paths
// ===========================================================================

/// Per-user file locations. Rebuilt each launch; never serialised.
#[derive(Debug, Clone)]
pub struct Paths {
    pub base: PathBuf,
    pub appdata: PathBuf,
    pub config: PathBuf,
    pub resources: PathBuf,
    pub themes: PathBuf,
    pub nuclide_data: PathBuf,
    pub bibliography: PathBuf,
    pub documentation_dir: PathBuf,

    pub spectrum_library: PathBuf,
    pub spectrogram_library: PathBuf,
    pub roi_library: PathBuf,
    pub datalog_library: PathBuf,
    pub unique_instrument_library: PathBuf,
    pub generic_instrument_library: PathBuf,
    pub third_party_drivers_library: PathBuf,
    pub settings_file: PathBuf,

    pub last_opened_dir: PathBuf,
}

/// Overrides for [`Paths::with_overrides`]. Useful in tests and whenever a
/// machine-specific layout must be pinned down.
#[derive(Debug, Clone, Default)]
pub struct PathOverrides {
    pub base: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub last_opened_dir: Option<PathBuf>,
}

impl PathOverrides {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn base(mut self, p: impl Into<PathBuf>) -> Self {
        self.base = Some(p.into());
        self
    }
    pub fn config(mut self, p: impl Into<PathBuf>) -> Self {
        self.config = Some(p.into());
        self
    }
    pub fn appdata(mut self, p: impl Into<PathBuf>) -> Self {
        self.appdata = Some(p.into());
        self
    }
    pub fn last_opened_dir(mut self, p: impl Into<PathBuf>) -> Self {
        self.last_opened_dir = Some(p.into());
        self
    }
}

impl Paths {
    pub fn new() -> SettingsResult<Self> {
        Self::with_overrides(PathOverrides::default())
    }

    pub fn with_overrides(ov: PathOverrides) -> SettingsResult<Self> {
        let base = ov.base.unwrap_or_else(runtime_base);
        let config = match ov.config {
            Some(p) => ensure_dir(p)?,
            None => config_dir()?,
        };
        let appdata = match ov.appdata {
            Some(p) => ensure_dir(p)?,
            None => app_data_dir()?,
        };
        let mut paths = Self::from_base_and_appdata(base, config, appdata);
        paths.last_opened_dir = match ov.last_opened_dir {
            Some(d) => d,
            None => home_dir()?,
        };
        Ok(paths)
    }

    fn from_base_and_appdata(base: PathBuf, config: PathBuf, appdata: PathBuf) -> Self {
        let resources = base.join("resources");
        let mut p = Self {
            resources: resources.clone(),
            themes: resources.join("themes"),
            nuclide_data: resources.join("nuclide_data"),
            bibliography: resources.join("docs").join("bibliography.md"),
            documentation_dir: resources.join("docs").join("documentation"),

            spectrum_library: PathBuf::new(),
            spectrogram_library: PathBuf::new(),
            roi_library: PathBuf::new(),
            datalog_library: PathBuf::new(),
            unique_instrument_library: PathBuf::new(),
            generic_instrument_library: PathBuf::new(),
            third_party_drivers_library: PathBuf::new(),
            settings_file: PathBuf::new(),

            last_opened_dir: PathBuf::new(),
            base,
            config,
            appdata,
        };
        p.refresh_appdata_derived();
        p
    }

    fn refresh_appdata_derived(&mut self) {
        self.spectrum_library = self.appdata.join("spectrum_library");
        self.spectrogram_library = self.appdata.join("spectrogram_library");
        self.roi_library = self.appdata.join("roi_library");
        // BUG FIX vs Python: `datalog_library` was only set in `set_appdata`,
        // so the initial paths never had it.
        self.datalog_library = self.appdata.join("datalog_library");
        self.unique_instrument_library = self.appdata.join("unique_instrument_library");
        self.generic_instrument_library = self.appdata.join("generic_instrument_library");
        self.third_party_drivers_library = self.appdata.join("third_party_drivers_library");
        self.settings_file = self.config.join("settings.json");
    }

    pub fn set_config(&mut self, path: impl Into<PathBuf>) {
        self.config = path.into();
        self.settings_file = self.config.join("settings.json");
    }

    pub fn set_appdata(&mut self, path: impl Into<PathBuf>) {
        self.appdata = path.into();
        self.refresh_appdata_derived();
    }
}

fn ensure_dir(p: PathBuf) -> SettingsResult<PathBuf> {
    fs::create_dir_all(&p)?;
    Ok(p)
}

fn app_data_dir() -> SettingsResult<PathBuf> {
    let proj = ProjectDirs::from("", "", "LuRaCs").ok_or(SettingsError::NoHomeDir)?;
    ensure_dir(proj.data_local_dir().to_path_buf())
}

fn config_dir() -> SettingsResult<PathBuf> {
    let proj = ProjectDirs::from("", "", "LuRaCs").ok_or(SettingsError::NoHomeDir)?;
    ensure_dir(proj.config_dir().to_path_buf())
}

fn home_dir() -> SettingsResult<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .ok_or(SettingsError::NoHomeDir)
}

/// Bundled-release exe directory (if `resources/` sits next to the exe),
/// otherwise the crate root (development).
fn runtime_base() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && parent.join("resources").exists()
    {
        return parent.to_path_buf();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ===========================================================================
// Temp (runtime-only)
// ===========================================================================

#[derive(Debug, Clone, Default)]
pub struct Temp {
    pub spectrum_view_cursor: bool,
    pub spectrum_view_emission_lines_to_cursor: bool,
    pub spectrum_view_show_roi_labels: bool,
    pub is_si: bool,
}

impl Temp {
    pub const fn new() -> Self {
        Self {
            spectrum_view_cursor: false,
            spectrum_view_emission_lines_to_cursor: false,
            spectrum_view_show_roi_labels: true,
            is_si: false,
        }
    }
}

// ===========================================================================
// Events
// ===========================================================================

/// A single setting was changed.
#[derive(Debug, Clone)]
pub struct SettingChanged {
    pub category: Category,
    pub field: String,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub enum SettingsEvent {
    /// Equivalent to `sigSettingChanged.emit(group, variable, value)`.
    SettingChanged(SettingChanged),
    /// Equivalent to `latestConnectionUpdated.emit(list)`.
    LatestConnectionUpdated(Vec<String>),
}

impl SettingsEvent {
    pub const fn as_setting_changed(&self) -> Option<&SettingChanged> {
        match self {
            Self::SettingChanged(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_latest_connection_updated(&self) -> Option<&[String]> {
        match self {
            Self::LatestConnectionUpdated(v) => Some(v),
            _ => None,
        }
    }
}

// ===========================================================================
// Settings
// ===========================================================================

pub struct Settings {
    // Serialised categories (mirrors `SettingsFile`).
    pub theme: Theme,
    pub plotting: Plotting,
    pub importing: Importing,
    pub scanning: Scanning,
    pub gps: Gps,
    pub optimizer: Optimizer,
    pub real_time: RealTime,
    pub spectrogram: Spectrogram,
    pub logging: Logging,
    pub connections: Connections,
    pub workspace: Workspace,
    pub session: Session,

    // Runtime-only.
    pub headless: bool,
    pub paths: Paths,
    pub temp: Temp,

    events: VecDeque<SettingsEvent>,
}

impl Settings {
    /// Discover paths and build defaults.
    pub fn new() -> SettingsResult<Self> {
        Ok(Self::with_paths(Paths::new()?))
    }

    /// Build defaults around caller-supplied paths (tests, embedding).
    pub fn with_paths(paths: Paths) -> Self {
        Self {
            theme: Theme::default(),
            plotting: Plotting::default(),
            importing: Importing::default(),
            scanning: Scanning::default(),
            gps: Gps::default(),
            optimizer: Optimizer::default(),
            real_time: RealTime::default(),
            spectrogram: Spectrogram::default(),
            logging: Logging::default(),
            connections: Connections::default(),
            workspace: Workspace::default(),
            session: Session::default(),

            headless: false,
            paths,
            temp: Temp::new(),

            events: VecDeque::new(),
        }
    }

    pub fn builder() -> SettingsBuilder {
        SettingsBuilder::new()
    }

    // ---- Event queue -----------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<SettingsEvent> {
        self.events.drain(..).collect()
    }

    pub fn drain_events_into(&mut self, out: &mut Vec<SettingsEvent>) {
        out.extend(self.events.drain(..));
    }

    fn emit(&mut self, event: SettingsEvent) {
        self.events.push_back(event);
    }

    fn emit_latest_connections(&mut self) {
        self.emit(SettingsEvent::LatestConnectionUpdated(
            self.connections.recent.iter().cloned().collect(),
        ));
    }

    // ---- Persistence -----------------------------------------------------

    /// Load from `paths.settings_file`. Missing file is OK (first run).
    pub fn load(&mut self) -> SettingsResult<()> {
        let path = self.paths.settings_file.clone();
        self.load_from(path)
    }

    /// Load from an explicit file. Missing file is OK.
    pub fn load_from(&mut self, path: impl AsRef<Path>) -> SettingsResult<()> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(());
        }
        let text = fs::read_to_string(path)?;
        self.load_from_str(&text)
    }

    /// Load from a JSON string.
    pub fn load_from_str(&mut self, text: &str) -> SettingsResult<()> {
        let file: SettingsFile = serde_json::from_str(text)?;
        self.theme = file.theme;
        self.plotting = file.plotting;
        self.importing = file.importing;
        self.scanning = file.scanning;
        self.gps = file.gps;
        self.optimizer = file.optimizer;
        self.real_time = file.real_time;
        self.spectrogram = file.spectrogram;
        self.logging = file.logging;
        self.connections = file.connections;
        self.workspace = file.workspace;
        self.session = file.session;
        self.connections.trim();
        self.emit_latest_connections();
        Ok(())
    }

    /// Write to `paths.settings_file`.
    pub fn save(&self) -> SettingsResult<()> {
        self.save_to(&self.paths.settings_file)
    }

    /// Write to an explicit file. Pretty-printed.
    pub fn save_to(&self, path: impl AsRef<Path>) -> SettingsResult<()> {
        let file = self.to_file();
        let text = serde_json::to_string_pretty(&file)?;
        fs::write(path, text)?;
        Ok(())
    }

    /// Current on-disk representation.
    pub fn to_file(&self) -> SettingsFile {
        SettingsFile {
            theme: self.theme.clone(),
            plotting: self.plotting.clone(),
            importing: self.importing.clone(),
            scanning: self.scanning.clone(),
            gps: self.gps.clone(),
            optimizer: self.optimizer.clone(),
            real_time: self.real_time.clone(),
            spectrogram: self.spectrogram.clone(),
            logging: self.logging.clone(),
            connections: self.connections.clone(),
            workspace: self.workspace.clone(),
            session: self.session.clone(),
        }
    }

    // ---- Category-based update ------------------------------------------

    /// Update a single field within a category.
    ///
    /// Type-checks the value via a JSON round-trip and emits a
    /// [`SettingChanged`] on success.
    pub fn update(&mut self, category: Category, field: &str, value: Value) -> SettingsResult<()> {
        let update_value = value.clone();
        match category {
            Category::Theme => update_field(&mut self.theme, category, field, update_value)?,
            Category::Plotting => update_field(&mut self.plotting, category, field, update_value)?,
            Category::Importing => {
                update_field(&mut self.importing, category, field, update_value)?
            }
            Category::Scanning => update_field(&mut self.scanning, category, field, update_value)?,
            Category::Gps => update_field(&mut self.gps, category, field, update_value)?,
            Category::Optimizer => {
                update_field(&mut self.optimizer, category, field, update_value)?
            }
            Category::RealTime => update_field(&mut self.real_time, category, field, update_value)?,
            Category::Spectrogram => {
                update_field(&mut self.spectrogram, category, field, update_value)?
            }
            Category::Logging => update_field(&mut self.logging, category, field, update_value)?,
            Category::Connections => {
                update_field(&mut self.connections, category, field, update_value)?
            }
            Category::Workspace => {
                update_field(&mut self.workspace, category, field, update_value)?
            }
            Category::Session => update_field(&mut self.session, category, field, update_value)?,
        }
        self.emit(SettingsEvent::SettingChanged(SettingChanged {
            category,
            field: field.to_string(),
            value,
        }));
        Ok(())
    }

    /// Snapshot every field+value in a category as a JSON object.
    pub fn category_snapshot(&self, category: Category) -> Value {
        match category {
            Category::Theme => serde_json::to_value(&self.theme),
            Category::Plotting => serde_json::to_value(&self.plotting),
            Category::Importing => serde_json::to_value(&self.importing),
            Category::Scanning => serde_json::to_value(&self.scanning),
            Category::Gps => serde_json::to_value(&self.gps),
            Category::Optimizer => serde_json::to_value(&self.optimizer),
            Category::RealTime => serde_json::to_value(&self.real_time),
            Category::Spectrogram => serde_json::to_value(&self.spectrogram),
            Category::Logging => serde_json::to_value(&self.logging),
            Category::Connections => serde_json::to_value(&self.connections),
            Category::Workspace => serde_json::to_value(&self.workspace),
            Category::Session => serde_json::to_value(&self.session),
        }
        .unwrap_or(Value::Null)
    }

    /// Read a single field value.
    pub fn get_field(&self, category: Category, field: &str) -> Option<Value> {
        self.category_snapshot(category).get(field).cloned()
    }

    // ---- Connection tracking --------------------------------------------

    /// Register a new BLE connection name.
    pub fn add_new_connection(&mut self, wrapper_name: &str, connection_name: &str) {
        if connection_name != "BLE" {
            return;
        }
        self.connections.push(wrapper_name.to_string());
        self.emit_latest_connections();
    }
}

// ===========================================================================
// Builder
// ===========================================================================

/// Fluent constructor for [`Settings`].
#[derive(Debug, Default)]
pub struct SettingsBuilder {
    headless: Option<bool>,
    paths: Option<Paths>,
    temp: Option<Temp>,
    load_from_disk: bool,
}

impl SettingsBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub const fn headless(mut self, v: bool) -> Self {
        self.headless = Some(v);
        self
    }
    pub fn paths(mut self, v: Paths) -> Self {
        self.paths = Some(v);
        self
    }
    pub const fn temp(mut self, v: Temp) -> Self {
        self.temp = Some(v);
        self
    }
    /// Load from `paths.settings_file` after construction. Missing file OK.
    pub const fn load_from_disk(mut self, v: bool) -> Self {
        self.load_from_disk = v;
        self
    }

    pub fn build(self) -> SettingsResult<Settings> {
        let paths = match self.paths {
            Some(p) => p,
            None => Paths::new()?,
        };
        let mut settings = Settings::with_paths(paths);
        if let Some(v) = self.headless {
            settings.headless = v;
        }
        if let Some(t) = self.temp {
            settings.temp = t;
        }
        if self.load_from_disk {
            settings.load()?;
        }
        Ok(settings)
    }
}

// ===========================================================================
// Generic field updater
// ===========================================================================

fn update_field<T>(
    target: &mut T,
    category: Category,
    field: &str,
    value: Value,
) -> SettingsResult<()>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let mut current = serde_json::to_value(&*target)?;
    let obj = current
        .as_object_mut()
        .ok_or_else(|| SettingsError::TypeMismatch {
            category: category.as_str().to_string(),
            field: field.to_string(),
        })?;
    if !obj.contains_key(field) {
        return Err(SettingsError::UnknownField {
            category: category.as_str().to_string(),
            field: field.to_string(),
        });
    }
    obj.insert(field.to_string(), value);
    *target = serde_json::from_value(current).map_err(|_| SettingsError::TypeMismatch {
        category: category.as_str().to_string(),
        field: field.to_string(),
    })?;
    Ok(())
}

// ===========================================================================
// Shared handle
// ===========================================================================

/// `Arc<RwLock<Settings>>` lets the GUI hold a read lock for rendering and
/// take a write lock only when a setting changes.
pub type SharedSettings = Arc<RwLock<Settings>>;

pub trait SharedSettingsExt {
    fn read_settings(&self) -> RwLockReadGuard<'_, Settings>;
    fn write_settings(&self) -> RwLockWriteGuard<'_, Settings>;
}

impl SharedSettingsExt for SharedSettings {
    fn read_settings(&self) -> RwLockReadGuard<'_, Settings> {
        self.read().expect("settings lock poisoned")
    }
    fn write_settings(&self) -> RwLockWriteGuard<'_, Settings> {
        self.write().expect("settings lock poisoned")
    }
}
