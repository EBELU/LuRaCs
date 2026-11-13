//! Main menu bar.
//!
//! Port of `luracs.gui.main_menu_bar.MainMenuBar`. Every Qt signal becomes a
//! direct call into `LuRaCsApp` or a modal push, and every `QAction` becomes
//! a `ui.button` / `ui.checkbox` / `ui.menu_button`.
//!
//! ## Patterns
//!
//! - **Confirmations** go through `modals` (`ConfirmModal` + `ConfirmAction`).
//!   Si-mode uses `EnterSiMode` / `ExitSiMode`, which route through
//!   `App::set_si_mode` so the tab switch happens too.
//! - **Info dialogs** go through `modals` (`InfoModal`).
//! - **Windows** open via `StubWindow::open()` for now; each becomes a real
//!   window once its Python source is ported.
//! - **Settings toggles** write to `settings.temp.*` (cursor emissions, ROI
//!   labels) so they persist, and to `App::view_*` for the two that don't.
//!
//! ## Signals replaced
//!
//! - `sigSetSpectrumViewToTabs` / `ToCombined` → write
//!   `settings.appearance.tabbed_spectrum_view` then call
//!   `spectrum_screen.set_mode(...)` when the real SpectrumScreen is ported.
//!   For now, only the settings write happens.
//! - `sigUpdateSetting` → direct `settings.write().unwrap().temp.* = value`.
//! - `sigToggleSiMode` → push `ConfirmAction::EnterSiMode` / `ExitSiMode`.

use eframe::egui;

use crate::app::LuRaCsApp;
use crate::core::ImportOptions;
use crate::core::Instrument;
use crate::core::{ExportOutcome, ImportFilter, ImportOutcome};
use crate::gui::modals::{ConfirmAction, ConfirmModal, InfoModal, Modal, ModalKind};
use crate::gui::plot_screen::PlotMode;
use crate::utils::InstrumentData;

// ===========================================================================
// Entry
// ===========================================================================

pub fn render(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        file_menu(app, ui);
        view_menu(app, ui);
        detector_menu(app, ui);
        gamma_tools_menu(app, ui);
        tools_menu(app, ui);
        options_menu(app, ui);
        help_menu(app, ui);
    });
}

// ===========================================================================
// File
// ===========================================================================

fn file_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("File", |ui| {
        // --- Import ---
        if ui.button("Import").clicked() {
            let outcomes = app.io_manager.importer.import_generic(
                ImportFilter::All,
                &app.settings.read().unwrap().paths.spectrogram_library,
            );
            dispatch_import_outcomes(app, outcomes);
        }

        ui.separator();

        // --- Data Library ---
        if ui.button("Data Library").clicked() {
            app.data_library.open();
        }

        if ui.button("Export Data Library").clicked() {
            if let Some(ExportOutcome::Library(zip_path)) =
                app.io_manager.exporter.export_library_dialog()
            {
                let appdata = app.settings.read().unwrap().paths.appdata.clone();
                if let Err(e) = app
                    .io_manager
                    .exporter
                    .do_export_library(&appdata, &zip_path)
                {
                    tracing::error!("Library export failed: {e}");
                }
            }
        }

        if ui.button("Import Data Library").clicked() {
            let appdata = app.settings.read().unwrap().paths.appdata.clone();
            if let Err(e) = app.io_manager.importer.import_library(&appdata) {
                tracing::error!("Library import failed: {e}");
            }
        }

        if ui.button("File Location").clicked() {
            let appdata = app.settings.read().unwrap().paths.appdata.clone();
            app.modal_stack
                .push(Modal::root(ModalKind::Info(InfoModal::new(
                    "File Locations",
                    format!("Local files are stored at:\n{}", appdata.display()),
                ))));
        }

        ui.separator();

        // --- Drivers ---
        if ui.button("Import Drivers").clicked() {
            import_third_party_drivers(app);
        }
        if ui.button("Drivers").clicked() {
            app.driver_dialog.open();
        }

        ui.separator();

        if ui.button("Exit").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

fn import_third_party_drivers(app: &mut LuRaCsApp) {
    let Some(files) = rfd::FileDialog::new()
        .set_title("Import driver files")
        .pick_files()
    else {
        return;
    };

    let library = app
        .settings
        .read()
        .unwrap()
        .paths
        .third_party_drivers_library
        .clone();

    if let Err(e) = std::fs::create_dir_all(&library) {
        tracing::error!("Could not create driver library: {e}");
        return;
    }

    for src in files {
        if !src.is_file() {
            continue;
        }
        let Some(file_name) = src.file_name() else {
            continue;
        };
        let dst = library.join(file_name);
        if let Err(e) = std::fs::copy(&src, &dst) {
            tracing::error!("Could not copy {}: {e}", src.display());
        }
    }
}

fn dispatch_import_outcomes(app: &mut LuRaCsApp, outcomes: Vec<ImportOutcome>) {
    for outcome in outcomes {
        match outcome {
            ImportOutcome::Spectrum(bundle) => {
                if let Err(e) = app.spectrum_manager.import_spectrum(
                    bundle.into(),
                    ImportOptions {
                        external_import: true,
                        ..Default::default()
                    },
                ) {
                    tracing::error!("Spectrum import failed: {e}");
                }
            }
            ImportOutcome::Spectrogram(name) => {
                // TODO: open as a Spectrogram and register via RunManager
                // once SpectrogramScreen is ported.
                tracing::info!(name, "Imported spectrogram (registration pending)");
            }
            ImportOutcome::Rois(rois) => {
                app.spectrum_manager.roi_manager.load_rois(&rois);
                tracing::info!(count = rois.len(), "Imported ROIs");
            }
            ImportOutcome::Instrument(inst) => {
                if let InstrumentData::Unique(u) = inst {
                    if let Err(e) = app
                        .spectrum_manager
                        .unique_instrument_library
                        .add_instrument(Instrument::Unique(u))
                    {
                        tracing::error!("Instrument import failed: {e}");
                    }
                }
            }
            ImportOutcome::SpectrumAsBackground(_) => {}
        }
    }
}

// ===========================================================================
// View
// ===========================================================================

fn view_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("View", |ui| {
        {
            ui.menu_button("Map", |ui| {
                // TODO(map): once MapScreen is ported, wire these to its
                // `track_current_location`, `move_to_current`,
                // `move_to_start`, `move_to_end` methods.
                let mut track_current = false;
                if ui
                    .checkbox(&mut track_current, "Track Current Location")
                    .changed()
                {
                    tracing::info!("Map track-current-location not yet implemented");
                }
                if ui.button("Move to Current Location").clicked() {
                    tracing::info!("Map move-to-current not yet implemented");
                }
                if ui.button("Move to Track Start").clicked() {
                    tracing::info!("Map move-to-start not yet implemented");
                }
                if ui.button("Move to Track End").clicked() {
                    tracing::info!("Map move-to-end not yet implemented");
                }
            });
        }

        // --- Real Time Data submenu ---
        ui.menu_button("Real Time Data", |ui| {
            ui.checkbox(&mut app.view_realtime_avg_line, "Mark Average");
        });

        // --- Spectrogram ROIs submenu ---
        ui.menu_button("Spectrogram ROIs", |ui| {
            ui.checkbox(&mut app.view_spectrogram_rois_compact, "Compact View");
        });

        // --- Spectrum submenu ---
        ui.menu_button("Spectrum", |ui| {
            // Cursor emissions
            let mut show_cursor_emissions = app
                .settings
                .read()
                .unwrap()
                .temp
                .spectrum_view_emission_lines_to_cursor;
            if ui
                .checkbox(&mut show_cursor_emissions, "Show Cursor Emissions")
                .changed()
            {
                app.settings
                    .write()
                    .unwrap()
                    .temp
                    .spectrum_view_emission_lines_to_cursor = show_cursor_emissions;
            }

            // ROI labels
            let mut show_roi_labels = app
                .settings
                .read()
                .unwrap()
                .temp
                .spectrum_view_show_roi_labels;
            if ui
                .checkbox(&mut show_roi_labels, "Show ROI Labels")
                .changed()
            {
                app.settings
                    .write()
                    .unwrap()
                    .temp
                    .spectrum_view_show_roi_labels = show_roi_labels;
            }
        });
    });
}

// ===========================================================================
// Detector
// ===========================================================================

fn detector_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("Detector", |ui| {
        // --- Connections ---
        if ui.button("Connect Bluetooth").clicked() {
            app.bluetooth_window.open();
        }

        // --- Retry Last Connection (dynamic) ---
        let connections: Vec<String> = app
            .settings
            .read()
            .unwrap()
            .state
            .last_connections
            .iter()
            .cloned()
            .collect();
        ui.menu_button("Retry Last Connection", |ui| {
            if connections.is_empty() {
                ui.label("(no previous connections)");
            } else {
                for name in &connections {
                    if ui.button(name).clicked() {
                        // TODO: `RunManager::connect_bluetooth_list([name])`
                        // once the btleplug integration is wired.
                        tracing::info!(name, "Bluetooth reconnect not yet implemented");
                    }
                }
            }
        });

        if ui.button("Connect USB").clicked() {
            app.usb_window.open();
        }
        if ui.button("Connect Network").clicked() {
            app.network_window.open();
        }
        if ui.button("Connect Serial GPS").clicked() {
            app.serial_gps_window.open();
        }

        ui.separator();

        // --- Device control ---
        if ui.button("Start Acquisition").clicked() {
            app.run_manager.start_acquisition_all();
        }
        if ui.button("Stop Acquisition").clicked() {
            app.run_manager.stop_acquisition_all();
        }
        if ui.button("Reset All Spectra").clicked() {
            app.modal_stack
                .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Confirm",
                    "Reset the accumulated spectrum of all connected devices?",
                    ConfirmAction::ResetAllSpectra,
                ))));
        }
        if ui.button("Disconnect All").clicked() {
            app.modal_stack
                .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Confirm",
                    "Disconnect all devices?",
                    ConfirmAction::DisconnectAll,
                ))));
        }
    });
}

// ===========================================================================
// Gamma Tools
// ===========================================================================

fn gamma_tools_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("Gamma Tools", |ui| {
        // Note: "Activity" is commented out in the Python source too.
        if ui.button("Calibration").clicked() {
            app.calibration_window.open();
        }
        if ui.button("Deconvolution").clicked() {
            app.deconvolution_window.open = true;
        }
        if ui.button("Efficiency").clicked() {
            app.efficiency_window.open();
        }
        if ui.button("Peak Features").clicked() {
            app.peak_features_window.open();
        }
        if ui.button("Resolution").clicked() {
            app.resolution_window.open();
        }
    });
}

// ===========================================================================
// Tools
// ===========================================================================

fn tools_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("Tools", |ui| {
        if ui.button("Add Calc ROI").clicked() {
            // TODO: route into SpectrogramScreen once that tab is ported.
            // Python: `spectrogram_roi_tab.plot_container.add_calc_roi`.
            tracing::info!("Add Calc ROI not yet ported");
        }
    });
}

// ===========================================================================
// Options
// ===========================================================================

fn options_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("Options", |ui| {
        if ui.button("Settings").clicked() {
            app.settings_dialog.open();
        }
        if ui.button("Advanced Settings").clicked() {
            // TODO: the Python has two separate dialogs
            // (`edit_settings` / `edit_advanced_settings`). Route to the
            // advanced variant once both are ported.
            app.settings_dialog.open();
        }

        ui.separator();

        // --- Si-mode toggle ---
        // The checkbox shows the current state; clicking pushes a confirm.
        // The state only changes when the modal's OK fires.
        let mut si_mode_clicked = app.si_mode;
        if ui
            .checkbox(&mut si_mode_clicked, "Toggle Si-Mode")
            .changed()
        {
            let (message, action) = if si_mode_clicked {
                (
                    "Are you sure you want to enter Si-mode?",
                    ConfirmAction::EnterSiMode,
                )
            } else {
                (
                    "Are you sure you want to exit Si-mode?",
                    ConfirmAction::ExitSiMode,
                )
            };
            app.modal_stack
                .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Confirm", message, action,
                ))));
        }

        ui.separator();

        // --- Spectrum view mode (exclusive radio) ---
        let tabbed = app.settings.read().unwrap().appearance.tabbed_spectrum_view;
        let mut new_tabbed = tabbed;
        ui.radio_value(&mut new_tabbed, false, "Combined Spectrum View");
        ui.radio_value(&mut new_tabbed, true, "Tabbed Spectrum View");

        if new_tabbed != tabbed {
            app.settings
                .write()
                .unwrap()
                .appearance
                .tabbed_spectrum_view = new_tabbed;

            let mode = if new_tabbed {
                PlotMode::Tabbed
            } else {
                PlotMode::Combined
            };
            app.spectrum_screen
                .set_mode(mode, &mut app.spectrum_manager);
        }
    });
}

// ===========================================================================
// Help
// ===========================================================================

fn help_menu(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    ui.menu_button("Help", |ui| {
        if ui.button("Documentation").clicked() {
            app.documentation_dialog.open();
        }
        if ui.button("Bibliography").clicked() {
            app.bibliography_dialog.open();
        }
        if ui.button("About").clicked() {
            app.modal_stack
                .push(Modal::root(ModalKind::Info(InfoModal::new(
                    "About",
                    "LuRaCs - Lund University radiation computing suite\n\n\
                     A free and open source tool for measuring and analysing \
                     radiation spectra.\n\n\
                     Created by Erik Ewald & Malte Axner\n\n\
                     Source code licenced under GPL-3.0\n\
                     Documentation & Images licenced under CC BY-SA 4.0",
                ))));
        }
    });
}
