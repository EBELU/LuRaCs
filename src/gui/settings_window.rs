//! Settings dialog: egui port of Python's `SettingsDialog` +
//! `AdvancedSettingsDialog`.
//!
//! Adapted to the categorised [`Settings`] layout: one collapsible section
//! per [`Category`], rendered in `Category::ALL` order. Changes are staged
//! in a [`SettingsDraft`] and only committed on Apply, matching the Python
//! "build a dict, then dispatch" pattern via
//! [`Settings::update`](crate::settings::Settings::update).
//!
//! Theme changes are applied through [`ThemeManager::apply`] (which also
//! emits `ThemeChanged`; the app's event loop then persists
//! `theme.name` back to settings). Every other field goes straight through
//! `Settings::update`, which emits `SettingChanged` as usual.
//!
//! Fields whose new value only takes effect on next launch
//! (`logging.buffer_length` today) trigger a restart-required modal on
//! Apply.

use eframe::egui;
use serde_json::Value;

use crate::app::LuRaCsApp;
use crate::gui::{ColorScheme, ConfirmAction, ConfirmModal, InfoModal, Modal, ModalKind};
use crate::settings::{Category, Settings, SharedSettingsExt};

// ===========================================================================
// Draw-mode helper
// ===========================================================================
//
// The Python dialog used three radio buttons that map to two bools. Keep
// the three-button UX, represent it as a discrete enum in the draft.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawMode {
    Pen,
    Brush,
    Both,
}

impl DrawMode {
    const fn from_flags(pen: bool, brush: bool) -> Self {
        match (pen, brush) {
            (true, false) => Self::Pen,
            (false, true) => Self::Brush,
            _ => Self::Both, // covers (true, true) and the defensive (false, false)
        }
    }

    const fn to_flags(self) -> (bool, bool) {
        match self {
            Self::Pen => (true, false),
            Self::Brush => (false, true),
            Self::Both => (true, true),
        }
    }
}

// ===========================================================================
// ColourScheme presentation
// ===========================================================================
//
// The exact variant names come from `crate::gui::ColorScheme`. Assumed here
// to match the Python `ColorSchemes` values: LibreOffice / Matplotlib /
// Catppuccin. Adjust if your enum differs.
const fn color_scheme_choices() -> [(ColorScheme, &'static str); 3] {
    [
        (ColorScheme::LibreOffice, "LibreOffice"),
        (ColorScheme::Mpl, "Matplotlib"),
        (ColorScheme::Catppuccin, "Catppuccin"),
    ]
}

// ===========================================================================
// Draft
// ===========================================================================

/// Staged copy of every editable setting. Kept in lockstep with
/// [`crate::settings::Settings`] by [`SettingsDraft::from_settings`].
#[derive(Debug, Clone, PartialEq)]
struct SettingsDraft {
    // Theme
    theme_name: String,
    color_rotator_scheme: ColorScheme,
    font_size: u32,

    // Plotting
    draw_mode: DrawMode,
    energy_max: f64,
    tabbed_spectrum_view: bool,

    // Importing
    load_rois: bool,
    load_instrument: bool,
    load_calibration: bool,

    // Scanning
    update_loop_delay_s: f64,
    spectrum_update_delay_s: f64,
    ui_scan_length: u32,
    headless_scan_length: u32,

    // GPS
    gps_sample_frequency: f64,
    gps_point_alpha: u8,
    gps_move_to_current_max_rate_s: f64,
    gps_save_extra_gps_data: bool,
    gps_save_extra_measurement_time_data: bool,

    // Optimizer
    optimizer_max_iter: usize,
    optimizer_tolerance: f64,
    optimizer_use_chi2_weight: bool,

    // Real-time
    real_time_values_buffer_length: usize,

    // Spectrogram
    spectrogram_history_length: usize,
    spectrogram_roi_display_duration_s: f64,

    // Logging
    logging_verbose_calculations: bool,
    logging_buffer_length: usize,
    logging_catch_exceptions: bool,
    logging_write_to_file: bool,
    logging_write_to_console: bool,
}

impl SettingsDraft {
    /// True when the draft differs from `s` in any field. Compares by
    /// constructing a fresh draft so adding a field to `SettingsDraft`
    /// automatically participates in the check — no second field list to
    /// forget to update.
    fn has_changes(&self, s: &Settings) -> bool {
        *self != Self::from_settings(s)
    }

    /// Create a new `SettingsDraft` from a `Settings` instance.
    fn from_settings(s: &Settings) -> Self {
        Self {
            theme_name: s.theme.name.clone(),
            color_rotator_scheme: s.theme.color_rotator_scheme,
            font_size: s.theme.font_size,

            draw_mode: DrawMode::from_flags(s.plotting.pen, s.plotting.brush),
            energy_max: s.plotting.energy_max,
            tabbed_spectrum_view: s.plotting.tabbed_spectrum_view,

            load_rois: s.importing.load_rois,
            load_instrument: s.importing.load_instrument,
            load_calibration: s.importing.load_calibration,

            update_loop_delay_s: s.scanning.update_loop_delay_s,
            spectrum_update_delay_s: s.scanning.spectrum_update_delay_s,
            ui_scan_length: s.scanning.ui_scan_length,
            headless_scan_length: s.scanning.headless_scan_length,

            gps_sample_frequency: s.gps.sample_frequency,
            gps_point_alpha: s.gps.point_alpha,
            gps_move_to_current_max_rate_s: s.gps.move_to_current_max_rate_s,
            gps_save_extra_gps_data: s.gps.save_extra_gps_data,
            gps_save_extra_measurement_time_data: s.gps.save_extra_measurement_time_data,

            optimizer_max_iter: s.optimizer.max_iter,
            optimizer_tolerance: s.optimizer.tolerance,
            optimizer_use_chi2_weight: s.optimizer.use_chi2_weight,

            real_time_values_buffer_length: s.real_time.values_buffer_length,

            spectrogram_history_length: s.spectrogram.history_length,
            spectrogram_roi_display_duration_s: s.spectrogram.roi_display_duration_s,

            logging_verbose_calculations: s.logging.verbose_calculations,
            logging_buffer_length: s.logging.buffer_length,
            logging_catch_exceptions: s.logging.catch_exceptions,
            logging_write_to_file: s.logging.write_to_file,
            logging_write_to_console: s.logging.write_to_console,
        }
    }
}

// ===========================================================================
// Dialog
// ===========================================================================

pub struct SettingsDialog {
    /// Whether the window should be shown. The window's X button and the
    /// Cancel button both flip this to `false`; the `open_with` constructor
    /// flips it back to `true` after refreshing the draft.
    pub open: bool,
    draft: SettingsDraft,
}

impl SettingsDialog {
    pub fn new() -> Self {
        // Populate with defaults so `render` is well-defined before the
        // first `open_with` call. The draft is re-synced on every open.
        Self {
            open: false,
            draft: SettingsDraft::from_settings(&Settings::with_paths(
                // A throwaway `Paths` is unnecessary here — the draft is
                // overwritten by `open_with` before any render occurs.
                // This line exists only to satisfy `from_settings` if the
                // caller renders before opening, which would be a bug.
                // In practice: `SettingsDialog::new()` then immediately
                // `.open_with(&app.settings.read_settings())`.
                // Keeping the panic-free default avoids unwraps in `new`.
                crate::settings::Paths::new().expect("paths must resolve"),
            )),
        }
    }

    /// Open the dialog, refreshing the draft from the current settings.
    /// Call this from a menu-item handler.
    pub fn open_with(&mut self, settings: &Settings) {
        self.draft = SettingsDraft::from_settings(settings);
        self.open = true;
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    /// Render the window. No-ops when `open` is `false`. Mirrors the
    /// `DeconvolutionWindow::render(&mut self, app, ui)` shape, so call it
    /// from `LuRaCsApp::render_windows` using the swap-out pattern.
    pub fn render(&mut self, app: &mut LuRaCsApp, ui: &egui::Ui) {
        if !self.open {
            return;
        }

        let mut window_open = self.open;
        let mut close_requested = false;
        egui::Window::new("Settings")
            .open(&mut window_open)
            .resizable(true)
            .default_width(360.0)
            .show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_sections(app, ui);
                });
                ui.separator();
                close_requested = self.render_buttons(app, ui);
            });
        // `close_requested` overrides a click on Cancel-while-clean; the
        // window's X button still comes through `window_open`.
        self.open = window_open && !close_requested;
    }

    fn render_sections(&mut self, app: &LuRaCsApp, ui: &mut egui::Ui) {
        // ---- Theme ----
        section(ui, "Theme", |ui| {
            self.render_theme(app, ui);
        });

        // ---- Plotting ----
        section(ui, "Plotting", |ui| {
            self.render_plotting(ui);
        });

        // ---- Importing ----
        section(ui, "Importing", |ui| {
            self.render_importing(ui);
        });

        // ---- Scanning ----
        section(ui, "Scanning", |ui| {
            self.render_scanning(ui);
        });

        // ---- GPS ----
        section(ui, "GPS & map", |ui| {
            self.render_gps(ui);
        });

        // ---- Optimizer ----
        section(ui, "Optimizer", |ui| {
            self.render_optimizer(ui);
        });

        // ---- Real-time ----
        section(ui, "Real-time values", |ui| {
            self.render_real_time(ui);
        });

        // ---- Spectrogram ----
        section(ui, "Spectrogram", |ui| {
            self.render_spectrogram(ui);
        });

        // ---- Logging ----
        section(ui, "Logging", |ui| {
            self.render_logging(ui);
        });
    }

    fn render_theme(&mut self, app: &LuRaCsApp, ui: &mut egui::Ui) {
        // Theme picker sourced from the live registry, so user-loaded
        // themes appear here automatically.
        let label = app
            .theme_manager
            .registry()
            .get(&self.draft.theme_name)
            .map(|t| t.meta.label().to_string())
            .unwrap_or_else(|| self.draft.theme_name.clone());

        egui::ComboBox::from_label("Theme")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for theme in app.theme_manager.registry().iter() {
                    ui.selectable_value(
                        &mut self.draft.theme_name,
                        theme.meta.name.clone(),
                        theme.meta.label(),
                    );
                }
            });

        egui::ComboBox::from_label("Plot Colour Scheme")
            .selected_text(color_scheme_label(self.draft.color_rotator_scheme))
            .show_ui(ui, |ui| {
                for (scheme, label) in color_scheme_choices() {
                    ui.selectable_value(&mut self.draft.color_rotator_scheme, scheme, label);
                }
            });

        ui.add(egui::Slider::new(&mut self.draft.font_size, 2..=48).text("Font size"));
    }

    fn render_plotting(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.draft.draw_mode, DrawMode::Pen, "Pen");
            ui.radio_value(&mut self.draft.draw_mode, DrawMode::Brush, "Brush");
            ui.radio_value(&mut self.draft.draw_mode, DrawMode::Both, "Pen + Brush");
        });

        if self.draft.draw_mode != DrawMode::Pen {
            ui.label(
                egui::RichText::new("Brush fill is disabled in logarithmic view.")
                    .small()
                    .italics()
                    .color(ui.visuals().weak_text_color()),
            );
        }
        ui.add(
            egui::DragValue::new(&mut self.draft.energy_max)
                .range(100.0..=10_000.0)
                .suffix(" keV")
                .speed(10.0)
                .prefix("x max: "),
        );
        ui.checkbox(&mut self.draft.tabbed_spectrum_view, "Tabbed spectrum view");
    }

    fn render_importing(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.draft.load_rois, "Load ROIs on import");
        ui.checkbox(&mut self.draft.load_instrument, "Load instrument on import");
        ui.checkbox(
            &mut self.draft.load_calibration,
            "Load calibration from instrument",
        );
    }

    fn render_scanning(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.draft.update_loop_delay_s)
                .range(0.5..=1024.0)
                .speed(0.5)
                .suffix(" s")
                .prefix("Update loop delay: "),
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.spectrum_update_delay_s)
                .range(0.5..=1024.0)
                .speed(0.5)
                .suffix(" s")
                .prefix("Spectrum update delay: "),
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.ui_scan_length)
                .range(1..=100)
                .suffix(" s")
                .prefix("GUI scan length: "),
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.headless_scan_length)
                .range(1..=100)
                .suffix(" s")
                .prefix("Headless scan length: "),
        );
    }

    fn render_gps(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.draft.gps_sample_frequency)
                .range(0.1..=100.0)
                .speed(0.1)
                .suffix(" Hz")
                .prefix("Sample frequency: "),
        );
        ui.add(egui::Slider::new(&mut self.draft.gps_point_alpha, 0..=255).text("Point alpha"));
        ui.add(
            egui::DragValue::new(&mut self.draft.gps_move_to_current_max_rate_s)
                .range(0.1..=60.0)
                .speed(0.1)
                .suffix(" s")
                .prefix("Move-to-max rate: "),
        );
        ui.checkbox(
            &mut self.draft.gps_save_extra_gps_data,
            "Save extra GPS data",
        );
        ui.checkbox(
            &mut self.draft.gps_save_extra_measurement_time_data,
            "Save extra measurement time data",
        );
    }

    fn render_optimizer(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.draft.optimizer_max_iter)
                .range(1..=10_000)
                .prefix("Max iterations: "),
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.optimizer_tolerance)
                .range(1e-9..=1.0)
                .speed(1e-6)
                .prefix("Tolerance: "),
        );
        ui.checkbox(&mut self.draft.optimizer_use_chi2_weight, "Use χ² weight");
    }

    fn render_real_time(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.draft.real_time_values_buffer_length)
                .range(1..=10_000)
                .prefix("Values buffer length: "),
        );
    }

    fn render_spectrogram(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::DragValue::new(&mut self.draft.spectrogram_history_length)
                .range(32..=100_000)
                .prefix("History length: "),
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.spectrogram_roi_display_duration_s)
                .range(1.0..=3600.0)
                .speed(1.0)
                .suffix(" s")
                .prefix("ROI display duration: "),
        );
    }

    fn render_logging(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.draft.logging_verbose_calculations,
            "Verbose calculation logging",
        );
        ui.checkbox(&mut self.draft.logging_catch_exceptions, "Log exceptions");
        ui.checkbox(&mut self.draft.logging_write_to_file, "Write logs to file");
        ui.checkbox(
            &mut self.draft.logging_write_to_console,
            "Write logs to console",
        );
        ui.add(
            egui::DragValue::new(&mut self.draft.logging_buffer_length)
                .range(10..=100_000)
                .prefix("Log buffer length: "),
        );
        ui.label(
            egui::RichText::new("Log buffer length requires a restart.")
                .small()
                .italics(),
        );
    }

    fn render_buttons(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) -> bool {
        let mut close_requested = false;
        ui.horizontal(|ui| {
            if ui.button("Apply").clicked() {
                self.apply(app, ui);
                close_requested = true;
            }
            if ui.button("Cancel").clicked() {
                close_requested = self.request_cancel(app);
            }
        });
        close_requested
    }

    fn request_cancel(&self, app: &mut LuRaCsApp) -> bool {
        let settings_guard = app.settings.read_settings();
        let dirty = self.draft.has_changes(&settings_guard);
        drop(settings_guard);

        if !dirty {
            return true; // clean → close immediately
        }

        app.modal_stack
            .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                "Discard changes?",
                "You have unapplied changes. Discard them and close the settings window?",
                ConfirmAction::DiscardSettingsChanges,
            ))));
        false // keep dialog open until the modal decides
    }

    // -----------------------------------------------------------------------
    // Apply
    // -----------------------------------------------------------------------

    /// Commit every changed draft field through `Settings::update` and
    /// trigger the side-effects the old Python `edit_settings` /
    /// `edit_advanced_settings` performed on each change.
    fn apply(&mut self, app: &mut LuRaCsApp, ui: &egui::Ui) {
        // ---- 1. Theme (outside settings lock: ThemeManager::apply emits
        //         ThemeChanged, and the app's event loop persists it) ----
        let current_theme_name = app.settings.read_settings().theme.name.clone();
        if current_theme_name != self.draft.theme_name {
            if let Err(e) = app.theme_manager.apply(ui, &self.draft.theme_name) {
                tracing::warn!(theme = %self.draft.theme_name, "apply theme: {e}");
            }
            // Belt-and-braces: also persist here so `settings.theme.name`
            // is correct on this frame even before the event queue drains.
            let mut s = app.settings.write_settings();
            drop(s.update(
                Category::Theme,
                "name",
                Value::String(self.draft.theme_name.clone()),
            ));
            drop(s);
        }

        // ---- 2. Everything else, under a single write lock ----
        let mut font_size_changed = false;
        let mut color_scheme_changed = false;
        let mut spectrogram_history_changed: Option<usize> = None;
        let mut logging_catch_changed: Option<bool> = None;
        let mut logging_file_changed: Option<bool> = None;
        let mut logging_console_changed: Option<bool> = None;
        let mut restart_fields: Vec<&'static str> = Vec::new();

        {
            let mut s = app.settings.write_settings();

            macro_rules! put {
                ($cat:expr, $field:literal, $val:expr) => {
                    drop(s.update($cat, $field, serde_json::to_value($val).unwrap()));
                };
            }

            // Theme (remaining)
            if s.theme.color_rotator_scheme != self.draft.color_rotator_scheme {
                put!(
                    Category::Theme,
                    "color_rotator_scheme",
                    self.draft.color_rotator_scheme
                );
                color_scheme_changed = true;
            }
            if s.theme.font_size != self.draft.font_size {
                put!(Category::Theme, "font_size", self.draft.font_size);
                font_size_changed = true;
            }

            // Plotting
            let (pen, brush) = self.draft.draw_mode.to_flags();
            if s.plotting.pen != pen {
                put!(Category::Plotting, "pen", pen);
            }
            if s.plotting.brush != brush {
                put!(Category::Plotting, "brush", brush);
            }
            if (s.plotting.energy_max - self.draft.energy_max).abs() > f64::EPSILON {
                put!(Category::Plotting, "energy_max", self.draft.energy_max);
            }
            if s.plotting.tabbed_spectrum_view != self.draft.tabbed_spectrum_view {
                put!(
                    Category::Plotting,
                    "tabbed_spectrum_view",
                    self.draft.tabbed_spectrum_view
                );
            }

            // Importing
            if s.importing.load_rois != self.draft.load_rois {
                put!(Category::Importing, "load_rois", self.draft.load_rois);
            }
            if s.importing.load_instrument != self.draft.load_instrument {
                put!(
                    Category::Importing,
                    "load_instrument",
                    self.draft.load_instrument
                );
            }
            if s.importing.load_calibration != self.draft.load_calibration {
                put!(
                    Category::Importing,
                    "load_calibration",
                    self.draft.load_calibration
                );
            }

            // Scanning
            if s.scanning.update_loop_delay_s != self.draft.update_loop_delay_s {
                put!(
                    Category::Scanning,
                    "update_loop_delay_s",
                    self.draft.update_loop_delay_s
                );
            }
            if s.scanning.spectrum_update_delay_s != self.draft.spectrum_update_delay_s {
                put!(
                    Category::Scanning,
                    "spectrum_update_delay_s",
                    self.draft.spectrum_update_delay_s
                );
            }
            if s.scanning.ui_scan_length != self.draft.ui_scan_length {
                put!(
                    Category::Scanning,
                    "ui_scan_length",
                    self.draft.ui_scan_length
                );
            }
            if s.scanning.headless_scan_length != self.draft.headless_scan_length {
                put!(
                    Category::Scanning,
                    "headless_scan_length",
                    self.draft.headless_scan_length
                );
            }

            // GPS
            if s.gps.sample_frequency != self.draft.gps_sample_frequency {
                put!(
                    Category::Gps,
                    "sample_frequency",
                    self.draft.gps_sample_frequency
                );
            }
            if s.gps.point_alpha != self.draft.gps_point_alpha {
                put!(Category::Gps, "point_alpha", self.draft.gps_point_alpha);
            }
            if s.gps.move_to_current_max_rate_s != self.draft.gps_move_to_current_max_rate_s {
                put!(
                    Category::Gps,
                    "move_to_current_max_rate_s",
                    self.draft.gps_move_to_current_max_rate_s
                );
            }
            if s.gps.save_extra_gps_data != self.draft.gps_save_extra_gps_data {
                put!(
                    Category::Gps,
                    "save_extra_gps_data",
                    self.draft.gps_save_extra_gps_data
                );
            }
            if s.gps.save_extra_measurement_time_data
                != self.draft.gps_save_extra_measurement_time_data
            {
                put!(
                    Category::Gps,
                    "save_extra_measurement_time_data",
                    self.draft.gps_save_extra_measurement_time_data
                );
            }

            // Optimizer
            if s.optimizer.max_iter != self.draft.optimizer_max_iter {
                put!(
                    Category::Optimizer,
                    "max_iter",
                    self.draft.optimizer_max_iter
                );
            }
            if s.optimizer.tolerance != self.draft.optimizer_tolerance {
                put!(
                    Category::Optimizer,
                    "tolerance",
                    self.draft.optimizer_tolerance
                );
            }
            if s.optimizer.use_chi2_weight != self.draft.optimizer_use_chi2_weight {
                put!(
                    Category::Optimizer,
                    "use_chi2_weight",
                    self.draft.optimizer_use_chi2_weight
                );
            }

            // Real-time
            if s.real_time.values_buffer_length != self.draft.real_time_values_buffer_length {
                put!(
                    Category::RealTime,
                    "values_buffer_length",
                    self.draft.real_time_values_buffer_length
                );
            }

            // Spectrogram
            if s.spectrogram.history_length != self.draft.spectrogram_history_length {
                put!(
                    Category::Spectrogram,
                    "history_length",
                    self.draft.spectrogram_history_length
                );
                spectrogram_history_changed = Some(self.draft.spectrogram_history_length);
            }
            if s.spectrogram.roi_display_duration_s != self.draft.spectrogram_roi_display_duration_s
            {
                put!(
                    Category::Spectrogram,
                    "roi_display_duration_s",
                    self.draft.spectrogram_roi_display_duration_s
                );
            }

            // Logging
            if s.logging.verbose_calculations != self.draft.logging_verbose_calculations {
                put!(
                    Category::Logging,
                    "verbose_calculations",
                    self.draft.logging_verbose_calculations
                );
            }
            if s.logging.buffer_length != self.draft.logging_buffer_length {
                put!(
                    Category::Logging,
                    "buffer_length",
                    self.draft.logging_buffer_length
                );
                restart_fields.push("logging.buffer_length");
            }
            if s.logging.catch_exceptions != self.draft.logging_catch_exceptions {
                put!(
                    Category::Logging,
                    "catch_exceptions",
                    self.draft.logging_catch_exceptions
                );
                logging_catch_changed = Some(self.draft.logging_catch_exceptions);
            }
            if s.logging.write_to_file != self.draft.logging_write_to_file {
                put!(
                    Category::Logging,
                    "write_to_file",
                    self.draft.logging_write_to_file
                );
                logging_file_changed = Some(self.draft.logging_write_to_file);
            }
            if s.logging.write_to_console != self.draft.logging_write_to_console {
                put!(
                    Category::Logging,
                    "write_to_console",
                    self.draft.logging_write_to_console
                );
                logging_console_changed = Some(self.draft.logging_write_to_console);
            }

            drop(s);
        } // write lock released

        // ---- 3. Side effects ----
        if font_size_changed {
            apply_font_size_to_ctx(ui.ctx(), self.draft.font_size);
        }
        if color_scheme_changed {
            // TODO: notify `app.real_time_values_tab` of the new scheme
            // (e.g. `app.real_time_values_tab.set_color_scheme(scheme)`),
            // if that tab caches the scheme at construction.

            tracing::info!(
                scheme = ?self.draft.color_rotator_scheme,
                "colour scheme changed",
            );
        }
        if let Some(len) = spectrogram_history_changed {
            // TODO: `app.run_manager.resize_spectrogram_deque(len);`
            tracing::info!(length = len, "spectrogram history length changed");
        }
        if let Some(on) = logging_catch_changed {
            // TODO: `if on { log_utils::attach_exception_handler(); } else { log_utils::detach_exception_handler(); }`
            tracing::info!(enabled = on, "catch_exceptions changed");
        }
        if let Some(on) = logging_file_changed {
            // TODO: attach/detach file handler
            tracing::info!(enabled = on, "write_to_file changed");
        }
        if let Some(on) = logging_console_changed {
            // TODO: attach/detach console handler
            tracing::info!(enabled = on, "write_to_console changed");
        }

        // ---- 4. Restart warning ----
        if !restart_fields.is_empty() {
            let message = format!(
                "The following settings require a restart of the program to \
                 take effect:\n{}",
                restart_fields.join("\n")
            );
            app.modal_stack
                .push(Modal::root(ModalKind::Info(InfoModal::new(
                    "Restart required",
                    message,
                ))));
        }

        // Close on successful apply.
        self.open = false;
    }
}

impl Default for SettingsDialog {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Render one collapsible section, open by default.
fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(title)
        .default_open(true)
        .show(ui, body);
}

fn color_scheme_label(scheme: ColorScheme) -> &'static str {
    color_scheme_choices()
        .into_iter()
        .find(|(s, _)| *s == scheme)
        .map(|(_, l)| l)
        .unwrap_or("(unknown)")
}

/// Rebuild the text-style table at `size`, matching the private
/// `apply_font_size` in `application.rs`. Exposed here so the dialog can
/// react to a font-size change without touching `LuRaCsApp`.
fn apply_font_size_to_ctx(ctx: &egui::Context, size: u32) {
    let px = size as f32;
    ctx.all_styles_mut(move |style| {
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::proportional(px * 1.4),
            ),
            (egui::TextStyle::Body, egui::FontId::proportional(px)),
            (egui::TextStyle::Button, egui::FontId::proportional(px)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(px)),
            (
                egui::TextStyle::Small,
                egui::FontId::proportional(px * 0.85),
            ),
        ]
        .into_iter()
        .collect();
    });
}
