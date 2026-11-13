//! Application-wide settings and per-user file paths.
//!
//! Four groups:
//! - `Appearance`, `Advanced`, `State` — serialised to `settings.json`.
//! - `Paths`, `Temp` — runtime-only, recomputed per launch.
//!
//! `Settings` holds a `Vec<SettingsEvent>` the GUI drains each frame; that
//! replaces the Python Qt signals (`sigSettingChanged`,
//! `latestConnectionUpdated`).

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::gui::ColorScheme;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cannot determine user home / data directory")]
    NoHomeDir,
    #[error("unknown setting group: {0}")]
    UnknownGroup(String),
    #[error("unknown field {field:?} in group {group:?}")]
    UnknownField { group: String, field: String },
    #[error("value type mismatch when updating {group}.{field}")]
    TypeMismatch { group: String, field: String },
}

pub type SettingsResult<T> = std::result::Result<T, SettingsError>;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Per-user file locations. Rebuilt each launch; never serialised.
#[derive(Debug, Clone)]
pub struct Paths {
    pub base: PathBuf,      // runtime base (exe dir or cargo manifest)
    pub appdata: PathBuf,   // ~/.local/share/LuRaCs on Linux, etc.
    pub config: PathBuf,    // ~/.config/LuRaCs on Linux, etc.
    pub resources: PathBuf, // base/resources
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

impl Paths {
    pub fn new() -> SettingsResult<Self> {
        let base = runtime_base();
        let config = config_dir()?;
        let appdata = app_data_dir()?;
        let mut paths = Self::from_base_and_appdata(base, config, appdata);
        paths.last_opened_dir = home_dir()?;
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

            // Placeholder — overwritten by `refresh_appdata_derived`.
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
        // BUG FIX vs Python: `datalog_library` was set only in `set_appdata`,
        // so the initial paths never had it.
        self.datalog_library = self.appdata.join("datalog_library");
        self.unique_instrument_library = self.appdata.join("unique_instrument_library");
        self.generic_instrument_library = self.appdata.join("generic_instrument_library");
        self.third_party_drivers_library = self.appdata.join("third_party_drivers_library");
        self.settings_file = self.config.join("settings.json");
    }

    /// Redirect the config root (and therefore `settings_file`).
    pub fn set_config(&mut self, path: impl Into<PathBuf>) {
        self.config = path.into();
        self.settings_file = self.config.join("settings.json");
    }

    /// Point all appdata-derived paths at a new location.
    pub fn set_appdata(&mut self, path: impl Into<PathBuf>) {
        self.appdata = path.into();
        self.refresh_appdata_derived();
    }
}

/// `ProjectDirs::data_local_dir()`, created if missing.
fn app_data_dir() -> SettingsResult<PathBuf> {
    let proj = ProjectDirs::from("", "", "LuRaCs").ok_or(SettingsError::NoHomeDir)?;
    let dir = proj.data_local_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `ProjectDirs::config_dir()`, created if missing.
///
/// Linux: `$XDG_CONFIG_HOME/LuRaCs` or `~/.config/LuRaCs`.
/// Windows: `{FOLDERID_RoamingAppData}\LuRaCs` (roams with the profile).
/// macOS: `~/Library/Application Support/LuRaCs`.
fn config_dir() -> SettingsResult<PathBuf> {
    let proj = ProjectDirs::from("", "", "LuRaCs").ok_or(SettingsError::NoHomeDir)?;
    let dir = proj.config_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn home_dir() -> SettingsResult<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .ok_or(SettingsError::NoHomeDir)
}

/// Where bundled resources live.
///
/// - **Bundled release:** the executable's directory (if a `resources/`
///   folder sits next to the exe).
/// - **Development:** the crate root (equivalent to Python's
///   `Path(__file__).parent.parent`).
fn runtime_base() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("resources").exists() {
                return parent.to_path_buf();
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub color_rotator_scheme: ColorScheme,
    pub pen: bool,
    pub brush: bool,
    pub font_size: u32,
    pub spectrum_plot_e_max: f64,
    pub tabbed_spectrum_view: bool,
    pub verbose_calculation_logging: bool,

    pub load_rois_on_import: bool,
    pub load_instrument_on_import: bool,
    pub load_calibration_from_instrument: bool,

    pub map_point_alpha_value: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "light-catppuccin".into(),
            color_rotator_scheme: ColorScheme::Catppuccin,
            pen: true,
            brush: false,
            font_size: 10,
            spectrum_plot_e_max: 3000.0,
            tabbed_spectrum_view: false,
            verbose_calculation_logging: true,
            load_rois_on_import: true,
            load_instrument_on_import: true,
            load_calibration_from_instrument: true,
            map_point_alpha_value: 255,
        }
    }
}

// ---------------------------------------------------------------------------
// Advanced
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    pub update_loop_delay: f64,
    pub spectrum_update_delay: f64,
    pub ui_scan_length: u32,
    pub headless_scan_length: u32,
    pub gps_sample_frequency: f64,

    pub optimizer_max_iter: usize,
    pub optimizer_tolerance: f64,
    pub optimizer_use_chi2_weight: bool,

    pub log_buffer_length: usize,
    pub log_catch_exceptions: bool,
    pub log_write_to_file: bool,
    pub log_write_to_console: bool,

    pub real_time_values_deque_length: usize,

    pub spectrogram_deque_length: usize,
    pub spectrogram_roi_display_length_s: f64,

    pub map_move_to_current_max_rate_s: f64,
    pub map_save_extra_gps_data: bool,
    pub map_save_extra_measurement_time_data: bool,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            update_loop_delay: 1.0,
            spectrum_update_delay: 1.0,
            ui_scan_length: 5,
            headless_scan_length: 2,
            gps_sample_frequency: 5.0,
            optimizer_max_iter: 250,
            optimizer_tolerance: 1e-6,
            optimizer_use_chi2_weight: true,
            log_buffer_length: 1024,
            log_catch_exceptions: true,
            log_write_to_file: false,
            log_write_to_console: false,
            real_time_values_deque_length: 60,
            spectrogram_deque_length: 256,
            spectrogram_roi_display_length_s: 60.0,
            map_move_to_current_max_rate_s: 1.0,
            map_save_extra_gps_data: false,
            map_save_extra_measurement_time_data: false,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The maximum number of stored connection names.
pub const MAX_LAST_CONNECTIONS: usize = 10;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Bounded on save and load to `MAX_LAST_CONNECTIONS`.
    pub last_connections: VecDeque<String>,

    /// Round-trips through JSON. `None` matches Python's usual save-time
    /// value; if a caller ever sets this to a non-JSON value at save time,
    /// `save()` errors (Python would crash inside `json.dump`).
    pub loaded_spectra: Option<Value>,
    pub roi_regions: Option<Value>,

    pub map_last_online_url: String,
    pub deconv_last_response_file: String,
    pub last_network_connection_ip: String,
}

impl State {
    /// Append a connection name if new and within the bounded deque.
    pub fn push_connection(&mut self, name: impl Into<String>) {
        let name = name.into();
        if self.last_connections.iter().any(|c| c == &name) {
            return;
        }
        while self.last_connections.len() >= MAX_LAST_CONNECTIONS {
            self.last_connections.pop_front();
        }
        self.last_connections.push_back(name);
    }

    /// Enforce maxlen after loading.
    fn trim(&mut self) {
        while self.last_connections.len() > MAX_LAST_CONNECTIONS {
            self.last_connections.pop_front();
        }
    }
}

// ---------------------------------------------------------------------------
// Temp (runtime-only)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Temp {
    pub spectrum_view_cursor: bool,
    pub spectrum_view_emission_lines_to_cursor: bool,
    pub spectrum_view_show_roi_labels: bool,
    pub is_si: bool,
}

impl Temp {
    pub fn new() -> Self {
        Self {
            spectrum_view_cursor: false,
            spectrum_view_emission_lines_to_cursor: false,
            spectrum_view_show_roi_labels: true,
            is_si: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SettingChanged {
    pub group: String,
    pub variable: String,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub enum SettingsEvent {
    /// Equivalent to `sigSettingChanged.emit(group, variable, value)`.
    SettingChanged(SettingChanged),
    /// Equivalent to `latestConnectionUpdated.emit(list)`.
    LatestConnectionUpdated(Vec<String>),
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub struct Settings {
    pub headless: bool,
    pub advanced: Advanced,
    pub state: State,
    pub appearance: Appearance,
    pub paths: Paths,
    pub temp: Temp,

    events: VecDeque<SettingsEvent>,
}

impl Settings {
    pub fn new() -> SettingsResult<Self> {
        Ok(Self {
            headless: false,
            advanced: Advanced::default(),
            state: State::default(),
            appearance: Appearance::default(),
            paths: Paths::new()?,
            temp: Temp::new(),
            events: VecDeque::new(),
        })
    }

    // ---- Event queue (drained by the GUI each frame) --------------------

    pub fn drain_events(&mut self) -> Vec<SettingsEvent> {
        self.events.drain(..).collect()
    }

    fn emit(&mut self, event: SettingsEvent) {
        self.events.push_back(event);
    }

    // ---- Persistence -----------------------------------------------------

    /// Load from `paths.settings_file`. Missing file is OK (first run).
    /// Unknown fields in the file are ignored.
    pub fn load(&mut self) -> SettingsResult<()> {
        let path = &self.paths.settings_file;
        if !path.exists() {
            return Ok(());
        }
        let text = fs::read_to_string(path)?;
        let content: Value = serde_json::from_str(&text)?;

        if let Some(v) = content.get("appearance") {
            self.appearance = serde_json::from_value(v.clone())?;
        }
        if let Some(v) = content.get("advanced") {
            self.advanced = serde_json::from_value(v.clone())?;
        }
        if let Some(v) = content.get("state") {
            self.state = serde_json::from_value(v.clone())?;
            self.state.trim();
        }
        self.emit(SettingsEvent::LatestConnectionUpdated(
            self.state.last_connections.iter().cloned().collect(),
        ));
        Ok(())
    }

    /// Write to `paths.settings_file`. Pretty-printed, matching Python.
    pub fn save(&self) -> SettingsResult<()> {
        let content = serde_json::json!({
            "appearance": self.appearance,
            "advanced": self.advanced,
            "state": self.state,
        });
        let text = serde_json::to_string_pretty(&content)?;
        fs::write(&self.paths.settings_file, text)?;
        Ok(())
    }

    // ---- Dynamic update (mirrors `update_setting(group, variable, value)`) ----

    /// Update a single field by group name and field name.
    ///
    /// Uses a JSON round-trip to enforce type checking. `Paths` and `Temp`
    /// are deliberately not addressable: `Paths` is machine-specific and
    /// `Temp` is transient UI state that should be mutated through its
    /// owning view.
    ///
    /// Emits a `SettingChanged` event on success.
    pub fn update_setting(
        &mut self,
        group: &str,
        variable: &str,
        value: Value,
    ) -> SettingsResult<()> {
        match group {
            "Appearance" => update_field(&mut self.appearance, group, variable, value.clone())?,
            "Advanced" => update_field(&mut self.advanced, group, variable, value.clone())?,
            "State" => update_field(&mut self.state, group, variable, value.clone())?,
            other => return Err(SettingsError::UnknownGroup(other.to_string())),
        }
        self.emit(SettingsEvent::SettingChanged(SettingChanged {
            group: group.to_string(),
            variable: variable.to_string(),
            value,
        }));
        Ok(())
    }

    // ---- Connection tracking --------------------------------------------

    /// Register a new BLE connection name. Mirrors Python's
    /// `add_new_connection`, with the `DeviceWrapper` fields flattened to
    /// plain strings so this module doesn't depend on the device layer.
    pub fn add_new_connection(&mut self, wrapper_name: &str, connection_name: &str) {
        if connection_name != "BLE" {
            return;
        }
        self.state.push_connection(wrapper_name.to_string());
        self.emit(SettingsEvent::LatestConnectionUpdated(
            self.state.last_connections.iter().cloned().collect(),
        ));
    }
}

// ---------------------------------------------------------------------------
// Generic field updater
// ---------------------------------------------------------------------------

fn update_field<T>(target: &mut T, group: &str, field: &str, value: Value) -> SettingsResult<()>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let mut current = serde_json::to_value(&*target)?;
    let obj = current
        .as_object_mut()
        .ok_or_else(|| SettingsError::TypeMismatch {
            group: group.to_string(),
            field: field.to_string(),
        })?;
    if !obj.contains_key(field) {
        return Err(SettingsError::UnknownField {
            group: group.to_string(),
            field: field.to_string(),
        });
    }
    obj.insert(field.to_string(), value);
    *target = serde_json::from_value(current)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared handle
// ---------------------------------------------------------------------------

/// Convenience alias for the typical sharing pattern.
///
/// `Arc<RwLock<Settings>>` lets the GUI hold a read lock for rendering and
/// take a write lock only when a setting changes. If settings must be
/// mutated from background threads (e.g. `add_new_connection` from the BLE
/// worker), the `RwLock` handles the synchronisation; if the app is
/// single-threaded, `Arc<Mutex<…>>` is equivalent and slightly simpler.
pub type SharedSettings = std::sync::Arc<std::sync::RwLock<Settings>>;
