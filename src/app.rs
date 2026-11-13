//! Top-level application: the "main window" from `luracs/application.py`.
//!
//! Owns:
//! - all managers (settings, run, spectrum, io, theme, log buffer)
//! - the top-area screen tab (Spectrum / Spectrogram / Map / Measure)
//! - the bottom-area tab (8 info panels)
//! - all floating windows (dialogs, gamma tools, libraries)
//! - the modal stack
//!
//! ## Rendering contract
//!
//! `App::ui` is:
//!
//! 1. `dispatch_all_events` — drain every manager queue, forward to the
//!    spectrum manager and to `self` (theme persistence, spectrum creation).
//! 2. Menu bar (top panel).
//! 3. `render_top_area` (top panel or full central panel in Si mode).
//! 4. `render_bottom_area` (central panel, hidden in Si mode).
//! 5. `render_windows` — every floating window with `open == true`.
//! 6. `modals::render_modals_for(self, ViewportId::ROOT, ui)`.
//!
//! ## Deviations from Python
//!
//! - Script engine not ported; script engine wiring not present.
//! - `build_application()` is split: `main.rs` builds the managers,
//!   `LuRaCsApp::new` wires the UI. This file only knows about UI.
//! - Python's `aboutToQuit` shutdown sequence (with timeout futures) becomes
//!   a synchronous `shutdown()` called from `on_exit`. Rust's `block_on`
//!   blocks deterministically; no timeouts needed.
//! - Every signal connection is replaced by a direct method call in
//!   `dispatch_all_events` or a stub the widget raises when its button is
//!   pressed. No Qt-style event bus.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::RenderState;

use crate::core::IoManager;
use crate::core::{ForegroundInput, SpectrumManager};
use crate::core::{RunManager, RunManagerEvent};
use crate::gui::{
    DeconvolutionWindow, IsotopicsTab, Modal, RealTimeValuesTab, SpectrogramScreen, SpectrumScreen,
    Stub, StubWindow, menu_bar, render_modals_for,
};
use crate::logging::LogBuffer;
use crate::settings::SharedSettings;
use crate::spectogram::{Spectrogram, SpectrogramConfig, SpectrogramMode, WrappedSpectrogramData};
use crate::theme_manager::{ThemeManager, ThemeManagerEvent};

// ===========================================================================
// Tab enums
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopTab {
    Spectrum,
    Spectrogram,
    Map,
    Measure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    SpectrumInfo,
    SpectrumRois,
    SpectrogramRois,
    RealTimeValues,
    Detectors,
    Isotopics,
    Console,
    SystemLog,
}

// ===========================================================================
// LuRaCsApp
// ===========================================================================

pub struct LuRaCsApp {
    // --- Managers ---
    pub settings: SharedSettings,
    pub run_manager: RunManager,
    pub spectrum_manager: SpectrumManager,
    pub io_manager: IoManager,
    pub theme_manager: ThemeManager,
    pub log_buffer: Arc<LogBuffer>,
    pub render_state: RenderState,

    // --- Modal stack (scoped per viewport; ROOT is the main window) ---
    pub modal_stack: Vec<Modal>,

    // --- Layout state ---
    pub top_tab: TopTab,
    pub bottom_tab: BottomTab,
    pub si_mode: bool,
    pub closing: bool,

    // --- Top-area screens ---
    pub spectrum_screen: SpectrumScreen,
    pub spectrogram_screen: SpectrogramScreen,
    pub map_screen: Stub,
    pub measure_screen: Stub,

    // --- Bottom tabs ---
    pub spectrum_info_tab: Stub,
    pub spectrum_roi_tab: Stub,
    pub spectrogram_roi_tab: Stub,
    pub real_time_values_tab: RealTimeValuesTab,
    pub devices_tab: Stub,
    pub isotopics_tab: IsotopicsTab,
    pub console_tab: Stub,
    pub log_tab: Stub,

    // --- Windows ---
    pub data_library: StubWindow,
    pub driver_dialog: StubWindow,
    pub settings_dialog: StubWindow,
    pub bluetooth_window: StubWindow,
    pub usb_window: StubWindow,
    pub network_window: StubWindow,
    pub serial_gps_window: StubWindow,
    pub documentation_dialog: StubWindow,
    pub bibliography_dialog: StubWindow,
    pub activity_window: StubWindow,
    pub efficiency_window: StubWindow,
    pub calibration_window: StubWindow,
    pub resolution_window: StubWindow,
    pub deconvolution_window: DeconvolutionWindow,
    pub peak_features_window: StubWindow,

    pub view_realtime_avg_line: bool,
    pub view_spectrogram_rois_compact: bool,
    pub spectrogram_data: HashMap<String, std::sync::Arc<WrappedSpectrogramData>>,
}

impl LuRaCsApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: SharedSettings,
        run_manager: RunManager,
        spectrum_manager: SpectrumManager,
        io_manager: IoManager,
        mut theme_manager: ThemeManager,
        log_buffer: Arc<LogBuffer>,
    ) -> Self {
        // --- wgpu render state (required by rsplot) ---
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("wgpu render state must be present (renderer = Wgpu in main.rs)")
            .clone();

        // --- Apply the initial theme once so egui has correct visuals ---
        {
            let theme_str = settings.read().unwrap().appearance.theme.clone();
            if let Err(e) = theme_manager.apply_by_name(&cc.egui_ctx, &theme_str) {
                tracing::warn!("Could not apply theme {theme_str:?}: {e}");
            }
            // The startup apply is not a user change — discard its event so
            // we don't persist a no-op write.
            let _ = theme_manager.drain_events();
        }

        let spectrum_screen = SpectrumScreen::new(&render_state);
        let spectrogram_screen = SpectrogramScreen::new(&render_state);

        let real_time_scheme = settings.read().unwrap().appearance.color_rotator_scheme;
        let real_time_values_tab = RealTimeValuesTab::new(&render_state, real_time_scheme);
        let isotopics_tab = IsotopicsTab::new(&render_state);

        // --- Font size from settings ---
        apply_font_size(&cc.egui_ctx, &settings);

        Self {
            settings,
            run_manager,
            spectrum_manager,
            io_manager,
            theme_manager,
            log_buffer,
            render_state,
            modal_stack: Vec::new(),

            top_tab: TopTab::Spectrum,
            bottom_tab: BottomTab::SpectrumInfo,
            si_mode: false,
            closing: false,

            spectrum_screen,
            spectrogram_screen,
            map_screen: Stub::new("Map"),
            measure_screen: Stub::new("Measure"),

            spectrum_info_tab: Stub::new("Spectrum Info"),
            spectrum_roi_tab: Stub::new("Spectrum ROIs"),
            spectrogram_roi_tab: Stub::new("Spectrogram ROIs"),
            real_time_values_tab,
            devices_tab: Stub::new("Detectors"),
            isotopics_tab,
            console_tab: Stub::new("Console"),
            log_tab: Stub::new("System Log"),

            data_library: StubWindow::new("Data Library"),
            driver_dialog: StubWindow::new("Drivers"),
            settings_dialog: StubWindow::new("Settings"),
            bluetooth_window: StubWindow::new("Bluetooth"),
            usb_window: StubWindow::new("USB"),
            network_window: StubWindow::new("Network"),
            serial_gps_window: StubWindow::new("Serial GPS"),
            documentation_dialog: StubWindow::new("Documentation"),
            bibliography_dialog: StubWindow::new("Bibliography"),
            activity_window: StubWindow::new("Activity"),
            efficiency_window: StubWindow::new("Efficiency"),
            calibration_window: StubWindow::new("Calibration"),
            resolution_window: StubWindow::new("Resolution"),
            deconvolution_window: DeconvolutionWindow::new(),
            peak_features_window: StubWindow::new("Peak Features"),

            view_realtime_avg_line: false,
            view_spectrogram_rois_compact: false,
            spectrogram_data: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Si-mode
    // -----------------------------------------------------------------------

    /// Port of `MainWindow.set_Si_interface`. Matches the Python tab-index
    /// choice: `Map` in Si-only builds, `Measure` in normal or H3 builds.
    /// (The H3 case in Python would index out of range; we don't.)
    pub fn set_si_mode(&mut self, enabled: bool) {
        self.si_mode = enabled;
        if enabled {
            self.top_tab = if false { TopTab::Map } else { TopTab::Measure };
        }
    }

    // -----------------------------------------------------------------------
    // Shutdown
    // -----------------------------------------------------------------------

    pub fn shutdown(&mut self) {
        if self.closing {
            return;
        }
        self.closing = true;
        crate::shutdown_all(&mut self.run_manager, &self.settings);
    }

    fn handle_run_manager_event(&mut self, ev: RunManagerEvent) {
        match ev {
            RunManagerEvent::CreateDeviceSpectrum {
                device_name,
                channels,
                connection,
            } => {
                let load_cal = self
                    .settings
                    .read()
                    .unwrap()
                    .appearance
                    .load_calibration_from_instrument;
                if let Err(e) = self.spectrum_manager.create_spectrum(
                    &device_name,
                    channels,
                    Some(&connection),
                    load_cal,
                ) {
                    tracing::warn!(device = %device_name, "create_spectrum failed: {e}");
                }
            }
            RunManagerEvent::RemoveDeviceSpectrum { name } => {
                self.spectrum_manager.remove_spectrum(&name);
            }
            RunManagerEvent::SpectrumUpdated { name, data } => {
                let sd = crate::spectrum_data_from_wrapped(&data, Some(name.clone()));
                if let Err(e) = self
                    .spectrum_manager
                    .set_foreground_spectrum(&name, ForegroundInput::Raw(sd))
                {
                    tracing::warn!(device = %name, "set_foreground_spectrum failed: {e}");
                }

                let chi2 = self
                    .settings
                    .read()
                    .unwrap()
                    .advanced
                    .optimizer_use_chi2_weight;

                // `get_spectra_dict_mut` borrows the whole manager, so obtain the
                // dictionary pointer before borrowing the disjoint ROI manager.
                // The two fields are independent and are not accessed through
                // overlapping references during this call.
                let spectra = self.spectrum_manager.get_spectra_dict_mut() as *mut _;
                let roi_manager = &mut self.spectrum_manager.roi_manager;
                // SAFETY: `spectra` points to the spectrum dictionary owned by
                // `spectrum_manager`; `roi_manager` is a separate field.
                unsafe {
                    roi_manager.refit_roi(&mut *spectra, None, None, chi2);
                }
            }
            RunManagerEvent::SpectrogramDataUpdated { name, data } => {
                self.spectrogram_data.insert(name, data);
            }
            RunManagerEvent::DeviceConnected { name } => {
                self.real_time_values_tab.device_connected(&name);
            }
            RunManagerEvent::DeviceRemoved { name } => {
                self.real_time_values_tab.device_removed(&name);
            }
            RunManagerEvent::RealTimeBuffersUpdated { name, cps, dr } => {
                self.real_time_values_tab.update_buffers(&name, cps, dr);
            }
            // Other variants are consumed by screens that are still stubs
            // (Spectrogram, Measure, Detectors tab). Dropped for now.
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Event dispatch
    // -----------------------------------------------------------------------

    fn dispatch_all_events(&mut self, ctx: &egui::Context) {
        use crate::core::RoiManagerEvent;

        // --- Run manager → spectrum manager + App ---
        let rm_events = self.run_manager.poll();
        for ev in rm_events {
            self.handle_run_manager_event(ev);
        }

        // --- Spectrum manager events (passive) ---
        let _ = self.spectrum_manager.drain_events();

        // --- ROI manager: `Created` triggers owner adoption + fit ---
        let roi_events = self.spectrum_manager.roi_manager.drain_events();
        for ev in roi_events {
            if let RoiManagerEvent::Created { tag } = ev {
                let settings_guard = self.settings.read().unwrap();
                self.spectrum_screen.on_roi_created(
                    &tag,
                    &mut self.spectrum_manager,
                    &settings_guard,
                );
            }
        }

        // --- Theme manager → persist to settings ---
        for ev in self.theme_manager.drain_events() {
            match ev {
                ThemeManagerEvent::UpdateSettingsTheme {
                    group,
                    variable,
                    value,
                } => {
                    let mut s = self.settings.write().unwrap();
                    if let Err(e) =
                        s.update_setting(&group, &variable, serde_json::Value::String(value))
                    {
                        tracing::warn!("Could not persist theme choice: {e}");
                    }
                }
            }
        }

        // Periodic redraw for live plots and device status.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }

    // -----------------------------------------------------------------------
    // Panel rendering
    // -----------------------------------------------------------------------

    fn render_top_area(&mut self, ui: &mut egui::Ui) {
        if !self.si_mode {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.top_tab, TopTab::Spectrum, "Spectrum");
                ui.selectable_value(&mut self.top_tab, TopTab::Spectrogram, "Spectrogram");
                ui.selectable_value(&mut self.top_tab, TopTab::Map, "Map");
                ui.selectable_value(&mut self.top_tab, TopTab::Measure, "Measure");
            });
            ui.separator();
        }

        match self.top_tab {
            TopTab::Spectrum => {
                let palette = self.theme_manager.palette();
                let settings_guard = self.settings.read().unwrap();
                self.spectrum_screen.render(
                    &mut self.spectrum_manager,
                    &settings_guard,
                    &palette,
                    &self.render_state,
                    ui,
                );
            }
            TopTab::Spectrogram => {
                let devices = self.run_manager.list_devices();
                let palette = self.theme_manager.palette();
                let settings_guard = self.settings.read().unwrap();
                let events = self.spectrogram_screen.render(
                    self.run_manager.spectrogram_manager(),
                    &self.spectrogram_data,
                    &settings_guard,
                    &palette,
                    ui,
                    &devices,
                );
                drop(settings_guard);

                for ev in events {
                    match ev {
                        crate::gui::SpectrogramScreenEvent::RequestStart {
                            db_name,
                            device,
                            interval,
                            channel_truncation,
                        } => {
                            // Resolve the connected device (source of channel count + identity).
                            let Some(device_obj) = self.run_manager.get_device(&device) else {
                                tracing::warn!(device = %device, "spectrogram start: device not connected");
                                continue;
                            };
                            let channels = device_obj.channels().unwrap_or(1024);

                            // Calibration + instrument name come from the spectrum whose connection
                            // matches this device (first match wins, mirroring Python).
                            let mut calibration = Vec::new();
                            let mut device_id = device.clone();
                            for spectrum in self.spectrum_manager.get_spectra_dict().values() {
                                if spectrum.connection.as_deref() == Some(device.as_str()) {
                                    calibration = spectrum
                                        .calibration_coefficients
                                        .clone()
                                        .unwrap_or_default();
                                    if let Some(instr) = &spectrum.instrument {
                                        device_id = instr.name.clone();
                                    }
                                    break;
                                }
                            }

                            // Snapshot the settings we need, then release the lock so the rest of
                            // the handler (which mutates run_manager) isn't contending for it.
                            let (config, library_dir) = {
                                let s = self.settings.read().unwrap();
                                (
                                    SpectrogramConfig {
                                        deque_length: s.advanced.spectrogram_deque_length,
                                        map_save_extra_gps_data: s.advanced.map_save_extra_gps_data,
                                        map_save_extra_measurement_time_data: s
                                            .advanced
                                            .map_save_extra_measurement_time_data,
                                    },
                                    s.paths.spectrogram_library.clone(),
                                )
                            };

                            let mode = SpectrogramMode::New {
                                save_interval: interval,
                                spect_channels: channels,
                                device_id,
                                calibration_coeff: calibration,
                                concat_factor: channel_truncation,
                                header_meta: serde_json::json!({"version": 1, "gps_version": 1}),
                            };

                            match Spectrogram::open(&db_name, &library_dir, config, mode) {
                                Ok(sp) => {
                                    if let Err(e) =
                                        self.run_manager.add_spectrogram(db_name.clone(), sp)
                                    {
                                        tracing::error!(name = %db_name, "add_spectrogram failed: {e}");
                                    } else {
                                        tracing::info!(name = %db_name, "spectrogram started");
                                        // Show it immediately; the screen otherwise picks it up next
                                        // frame from the registry.
                                        self.spectrogram_screen.selected_spectrogram =
                                            Some(db_name);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(name = %db_name, "spectrogram open failed: {e}")
                                }
                            }
                        }
                        crate::gui::SpectrogramScreenEvent::RequestPause { db_name } => {
                            if let Some(sp) = self.run_manager.get_spectrogram_mut(&db_name) {
                                sp.set_paused(true);
                                sp.request_data();
                            }
                        }
                        crate::gui::SpectrogramScreenEvent::RequestResume { db_name } => {
                            if let Some(sp) = self.run_manager.get_spectrogram_mut(&db_name) {
                                sp.set_paused(false);
                                sp.request_data();
                            }
                        }
                        crate::gui::SpectrogramScreenEvent::RequestClose { db_name } => {
                            self.run_manager.close_spectrogram(&db_name);
                            self.spectrogram_data.remove(&db_name);
                        }
                        crate::gui::SpectrogramScreenEvent::RequestExportAccumulated {
                            db_name,
                            spectrum_name,
                            remark,
                        } => {
                            // Route to IoManager::Exporter::export_spectrogram_to_spectrum
                            tracing::info!(db_name, spectrum_name, remark, "export accumulated");
                        }
                        crate::gui::SpectrogramScreenEvent::RequestExportSelection {
                            db_name,
                            spectrum_name,
                            remark,
                            row_range,
                        } => {
                            tracing::info!(
                                db_name,
                                spectrum_name,
                                remark,
                                ?row_range,
                                "export selection"
                            );
                        }
                        crate::gui::SpectrogramScreenEvent::RequestShowDataStore { tab } => {
                            self.data_library.open();
                            // pass `tab` to the data library if it uses a tab index
                        }
                        crate::gui::SpectrogramScreenEvent::RequestClearRois => {
                            self.run_manager.spectrogram_manager_mut().clear_rois();
                        }
                        crate::gui::SpectrogramScreenEvent::RequestImportRois => {
                            if let Some((path, _)) = self
                                .io_manager
                                .importer
                                .pick_file(crate::core::ImportFilter::Rois)
                            {
                                if let Ok(parser) = crate::utils::XmlParser::open(&path, false) {
                                    let rois: Vec<_> = parser.get_rois().to_vec();
                                    self.run_manager.spectrogram_manager_mut().load_rois(&rois);
                                }
                            }
                        }
                    }
                }
            }
            TopTab::Map => self.map_screen.render(ui),
            TopTab::Measure => self.measure_screen.render(ui),
        }
    }

    fn render_bottom_area(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::SpectrumInfo,
                "Spectrum Info",
            );
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::SpectrumRois,
                "Spectrum ROIs",
            );
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::SpectrogramRois,
                "Spectrogram ROIs",
            );
            ui.selectable_value(
                &mut self.bottom_tab,
                BottomTab::RealTimeValues,
                "Real Time Values",
            );
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Detectors, "Detectors");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Isotopics, "Isotopics");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::Console, "Console");
            ui.selectable_value(&mut self.bottom_tab, BottomTab::SystemLog, "System Log");
        });
        ui.separator();

        match self.bottom_tab {
            BottomTab::SpectrumInfo => self.spectrum_info_tab.render(ui),
            BottomTab::SpectrumRois => self.spectrum_roi_tab.render(ui),
            BottomTab::SpectrogramRois => self.spectrogram_roi_tab.render(ui),
            BottomTab::RealTimeValues => {
                let show_means = self.view_realtime_avg_line;
                let settings_guard = self.settings.read().unwrap();
                self.real_time_values_tab
                    .render(show_means, &settings_guard, ui);
            }
            BottomTab::Detectors => self.devices_tab.render(ui),
            BottomTab::Isotopics => {
                let settings_guard = self.settings.read().unwrap();
                self.isotopics_tab
                    .render(&mut self.spectrum_manager, &settings_guard, ui)
            }
            BottomTab::Console => self.console_tab.render(ui),
            BottomTab::SystemLog => self.log_tab.render(ui),
        }
    }

    fn render_windows(&mut self, ui: &mut egui::Ui) {
        // Each StubWindow no-ops when `.open == false`. Real windows keep
        // the same contract: `render` checks its own `open` flag.
        self.data_library.render(ui);
        self.driver_dialog.render(ui);
        self.settings_dialog.render(ui);
        self.bluetooth_window.render(ui);
        self.usb_window.render(ui);
        self.network_window.render(ui);
        self.serial_gps_window.render(ui);
        self.documentation_dialog.render(ui);
        self.bibliography_dialog.render(ui);
        self.activity_window.render(ui);
        self.efficiency_window.render(ui);
        self.calibration_window.render(ui);
        self.resolution_window.render(ui);
        // DeconvolutionWindow needs `&mut self` (the app) while rendering, so
        // we briefly swap it out to avoid the double-borrow.
        let mut deconv =
            std::mem::replace(&mut self.deconvolution_window, DeconvolutionWindow::new());
        deconv.render(self, ui);
        self.deconvolution_window = deconv;
        self.peak_features_window.render(ui);
    }
}

// ===========================================================================
// eframe::App
// ===========================================================================

impl eframe::App for LuRaCsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.dispatch_all_events(ui.ctx());

        // --- Menu bar ---
        egui::Panel::top("menu_bar").show(ui, |ui| {
            menu_bar::render(self, ui);
        });

        // --- Top / bottom areas ---
        if self.si_mode {
            egui::CentralPanel::default().show(ui, |ui| {
                self.render_top_area(ui);
            });
        } else {
            egui::Panel::top("top_area")
                .resizable(true)
                .default_size(ui.available_height() * 0.6)
                .show(ui, |ui| {
                    self.render_top_area(ui);
                });
            egui::CentralPanel::default().show(ui, |ui| {
                self.render_bottom_area(ui);
            });
        }

        if self.isotopics_tab.auto_set_emissions_requested {
            self.isotopics_tab.auto_set_emissions_requested = false;

            let use_chi2 = self
                .settings
                .read()
                .unwrap()
                .advanced
                .optimizer_use_chi2_weight;
            let matches = self.spectrum_screen.match_nuclide_to_rois(
                &mut self.spectrum_manager,
                75.0,
                use_chi2,
            );
            let message = crate::gui::format_match_result(&matches);
            self.modal_stack
                .push(crate::gui::Modal::root(crate::gui::ModalKind::Info(
                    crate::gui::InfoModal::new("Nuclide match", message),
                )));
        }

        // --- Floating windows ---
        self.render_windows(ui);

        // --- Modals for the main viewport ---
        render_modals_for(self, egui::ViewportId::ROOT, ui);
    }

    fn on_exit(&mut self) {
        self.shutdown();
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn apply_font_size(ctx: &egui::Context, settings: &SharedSettings) {
    let size = settings.read().unwrap().appearance.font_size as f32;

    ctx.all_styles_mut(move |style| {
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::proportional(size * 1.4),
            ),
            (egui::TextStyle::Body, egui::FontId::proportional(size)),
            (egui::TextStyle::Button, egui::FontId::proportional(size)),
            (egui::TextStyle::Monospace, egui::FontId::monospace(size)),
            (
                egui::TextStyle::Small,
                egui::FontId::proportional(size * 0.85),
            ),
        ]
        .into_iter()
        .collect();
    });
}
