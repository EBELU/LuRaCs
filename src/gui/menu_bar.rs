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
use crate::settings::SharedSettingsExt;
use crate::utils::InstrumentData;

// ===========================================================================
// Entry
// ===========================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuBarCommand {
    Import,
    OpenDataLibrary,
    ExportDataLibrary,
    ImportDataLibrary,
    ShowFileLocation,
    ImportDrivers,
    OpenDrivers,
    Exit,
    MapMoveToCurrent,
    MapMoveToStart,
    MapMoveToEnd,
    StartAcquisition,
    StopAcquisition,
    ResetAllSpectra,
    DisconnectAll,
    ConnectBluetooth,
    RetryBluetooth(String),
    ConnectUsb,
    ConnectNetwork,
    ConnectSerialGps,
    OpenCalibration,
    OpenDeconvolution,
    OpenEfficiency,
    OpenPeakFeatures,
    OpenResolution,
    AddCalcRoi,
    OpenSettings,
    ToggleSiMode(bool),
    SetSpectrumTabbed(bool),
    OpenDocumentation,
    OpenBibliography,
    ShowAbout,
}

#[derive(Debug, Clone, Default)]
pub struct MenuBarViewState {
    pub track_current_location: bool,
    pub view_realtime_avg_line: bool,
    pub view_spectrogram_rois_compact: bool,
    pub show_cursor_emissions: bool,
    pub show_roi_labels: bool,
    pub recent_connections: Vec<String>,
    pub si_mode: bool,
    pub tabbed_spectrum_view: bool,
}

pub fn draw_menus(state: &mut MenuBarViewState, ui: &mut egui::Ui) -> Vec<MenuBarCommand> {
    let mut cmds = Vec::new();
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Import").clicked() {
                cmds.push(MenuBarCommand::Import);
            }
            ui.separator();
            if ui.button("Data Library").clicked() {
                cmds.push(MenuBarCommand::OpenDataLibrary);
            }
            if ui.button("Export Data Library").clicked() {
                cmds.push(MenuBarCommand::ExportDataLibrary);
            }
            if ui.button("Import Data Library").clicked() {
                cmds.push(MenuBarCommand::ImportDataLibrary);
            }
            if ui.button("File Location").clicked() {
                cmds.push(MenuBarCommand::ShowFileLocation);
            }
            ui.separator();
            if ui.button("Import Drivers").clicked() {
                cmds.push(MenuBarCommand::ImportDrivers);
            }
            if ui.button("Drivers").clicked() {
                cmds.push(MenuBarCommand::OpenDrivers);
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                cmds.push(MenuBarCommand::Exit);
            }
        });

        ui.menu_button("View", |ui| {
            ui.menu_button("Map", |ui| {
                ui.checkbox(&mut state.track_current_location, "Track Current Location");
                if ui.button("Move to Current Location").clicked() {
                    cmds.push(MenuBarCommand::MapMoveToCurrent);
                }
                if ui.button("Move to Track Start").clicked() {
                    cmds.push(MenuBarCommand::MapMoveToStart);
                }
                if ui.button("Move to Track End").clicked() {
                    cmds.push(MenuBarCommand::MapMoveToEnd);
                }
            });
            ui.menu_button("Real Time Data", |ui| {
                ui.checkbox(&mut state.view_realtime_avg_line, "Mark Average");
            });
            ui.menu_button("Spectrogram ROIs", |ui| {
                ui.checkbox(&mut state.view_spectrogram_rois_compact, "Compact View");
            });
            ui.menu_button("Spectrum", |ui| {
                ui.checkbox(&mut state.show_cursor_emissions, "Show Cursor Emissions");
                ui.checkbox(&mut state.show_roi_labels, "Show ROI Labels");
            });
        });

        ui.menu_button("Detector", |ui| {
            if ui.button("Start Acquisition").clicked() {
                cmds.push(MenuBarCommand::StartAcquisition);
            }
            if ui.button("Stop Acquisition").clicked() {
                cmds.push(MenuBarCommand::StopAcquisition);
            }
            if ui.button("Reset All Spectra").clicked() {
                cmds.push(MenuBarCommand::ResetAllSpectra);
            }
            if ui.button("Disconnect All").clicked() {
                cmds.push(MenuBarCommand::DisconnectAll);
            }
        });

        ui.menu_button("Devices", |ui| {
            if ui.button("Connect Bluetooth").clicked() {
                cmds.push(MenuBarCommand::ConnectBluetooth);
            }
            ui.menu_button("Retry Last Connection", |ui| {
                if state.recent_connections.is_empty() {
                    ui.label("(no previous connections)");
                } else {
                    for name in &state.recent_connections {
                        if ui.button(name).clicked() {
                            cmds.push(MenuBarCommand::RetryBluetooth(name.clone()));
                        }
                    }
                }
            });
            if ui.button("Connect USB").clicked() {
                cmds.push(MenuBarCommand::ConnectUsb);
            }
            if ui.button("Connect Network").clicked() {
                cmds.push(MenuBarCommand::ConnectNetwork);
            }
            if ui.button("Connect Serial GPS").clicked() {
                cmds.push(MenuBarCommand::ConnectSerialGps);
            }
        });

        ui.menu_button("Gamma Tools", |ui| {
            if ui.button("Calibration").clicked() {
                cmds.push(MenuBarCommand::OpenCalibration);
            }
            if ui.button("Deconvolution").clicked() {
                cmds.push(MenuBarCommand::OpenDeconvolution);
            }
            if ui.button("Efficiency").clicked() {
                cmds.push(MenuBarCommand::OpenEfficiency);
            }
            if ui.button("Peak Features").clicked() {
                cmds.push(MenuBarCommand::OpenPeakFeatures);
            }
            if ui.button("Resolution").clicked() {
                cmds.push(MenuBarCommand::OpenResolution);
            }
        });

        ui.menu_button("Tools", |ui| {
            if ui.button("Add Calc ROI").clicked() {
                cmds.push(MenuBarCommand::AddCalcRoi);
            }
        });

        ui.menu_button("Options", |ui| {
            if ui.button("Settings").clicked() {
                cmds.push(MenuBarCommand::OpenSettings);
            }
            ui.separator();
            let mut si_clicked = state.si_mode;
            if ui.checkbox(&mut si_clicked, "Toggle Si-Mode").changed() {
                cmds.push(MenuBarCommand::ToggleSiMode(si_clicked));
            }
            ui.separator();
            let prev_tabbed = state.tabbed_spectrum_view;
            ui.radio_value(&mut state.tabbed_spectrum_view, false, "Combined Spectrum View");
            ui.radio_value(&mut state.tabbed_spectrum_view, true, "Tabbed Spectrum View");
            if state.tabbed_spectrum_view != prev_tabbed {
                cmds.push(MenuBarCommand::SetSpectrumTabbed(state.tabbed_spectrum_view));
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("Documentation").clicked() {
                cmds.push(MenuBarCommand::OpenDocumentation);
            }
            if ui.button("Bibliography").clicked() {
                cmds.push(MenuBarCommand::OpenBibliography);
            }
            if ui.button("About").clicked() {
                cmds.push(MenuBarCommand::ShowAbout);
            }
        });
    });
    cmds
}

pub fn render(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
    let mut state = {
        let s = app.settings.read_settings();
        MenuBarViewState {
            track_current_location: app.map_screen.track_current_location,
            view_realtime_avg_line: app.view_realtime_avg_line,
            view_spectrogram_rois_compact: app.view_spectrogram_rois_compact,
            show_cursor_emissions: s.temp.spectrum_view_emission_lines_to_cursor,
            show_roi_labels: s.temp.spectrum_view_show_roi_labels,
            recent_connections: s.connections.recent.iter().cloned().collect(),
            si_mode: app.si_mode,
            tabbed_spectrum_view: s.plotting.tabbed_spectrum_view,
        }
    };

    let cmds = draw_menus(&mut state, ui);

    app.map_screen.track_current_location = state.track_current_location;
    app.view_realtime_avg_line = state.view_realtime_avg_line;
    app.view_spectrogram_rois_compact = state.view_spectrogram_rois_compact;
    {
        let mut s = app.settings.write_settings();
        s.temp.spectrum_view_emission_lines_to_cursor = state.show_cursor_emissions;
        s.temp.spectrum_view_show_roi_labels = state.show_roi_labels;
    }

    for cmd in cmds {
        dispatch_command(app, ui, cmd);
    }
}

fn dispatch_command(app: &mut LuRaCsApp, ui: &egui::Ui, cmd: MenuBarCommand) {
    match cmd {
        MenuBarCommand::Import => {
            let dlg = app
                .io_manager
                .importer
                .build_pick_files_dialog(ImportFilter::All);
            app.file_dialogs.pick_files(ui.ctx(), dlg, |app, files| {
                let Some(paths) = files else { return };
                if paths.is_empty() {
                    return;
                }
                if let Some(parent) = paths[0].parent() {
                    app.io_manager
                        .importer
                        .set_last_opened_dir(parent.to_path_buf());
                }
                let spec_lib = app
                    .settings
                    .read()
                    .unwrap()
                    .paths
                    .spectrogram_library
                    .clone();
                let outcomes = app.io_manager.importer.import_paths(
                    &paths,
                    ImportFilter::All,
                    &spec_lib,
                );
                dispatch_import_outcomes(app, outcomes);
            });
        }
        MenuBarCommand::OpenDataLibrary => app.data_library.open(),
        MenuBarCommand::ExportDataLibrary => {
            let dlg = app.io_manager.exporter.build_export_library_dialog();
            app.file_dialogs.save_file(ui.ctx(), dlg, |app, path| {
                let Some(zip_path) = path else { return };
                if let Some(parent) = zip_path.parent() {
                    app.io_manager
                        .exporter
                        .set_last_opened_dir(parent.to_path_buf());
                }
                let appdata = app.settings.read().unwrap().paths.appdata.clone();
                if let Err(e) = app
                    .io_manager
                    .exporter
                    .do_export_library(&appdata, &zip_path)
                {
                    tracing::error!("Library export failed: {e}");
                }
            });
        }
        MenuBarCommand::ImportDataLibrary => {
            let dlg = app.io_manager.importer.build_import_library_dialog();
            app.file_dialogs.pick_file(ui.ctx(), dlg, |app, path| {
                let Some(zip_path) = path else { return };
                let appdata = app.settings.read().unwrap().paths.appdata.clone();
                if let Err(e) = app
                    .io_manager
                    .importer
                    .do_import_library(&zip_path, &appdata)
                {
                    tracing::error!("Library import failed: {e}");
                }
            });
        }
        MenuBarCommand::ShowFileLocation => {
            let appdata = app.settings.read().unwrap().paths.appdata.clone();
            app.modal_stack
                .push(Modal::root(ModalKind::Info(InfoModal::new(
                    "File Locations",
                    format!("Local files are stored at:\n{}", appdata.display()),
                ))));
        }
        MenuBarCommand::ImportDrivers => import_third_party_drivers(app, ui.ctx()),
        MenuBarCommand::OpenDrivers => app.driver_dialog.open(),
        MenuBarCommand::Exit => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
        MenuBarCommand::MapMoveToCurrent => app.map_screen.move_to_current(),
        MenuBarCommand::MapMoveToStart => app.map_screen.move_to_start(),
        MenuBarCommand::MapMoveToEnd => app.map_screen.move_to_end(),
        MenuBarCommand::StartAcquisition => app.run_manager.start_acquisition_all(),
        MenuBarCommand::StopAcquisition => app.run_manager.stop_acquisition_all(),
        MenuBarCommand::ResetAllSpectra => {
            app.modal_stack
                .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Confirm",
                    "Reset the accumulated spectrum of all connected devices?",
                    ConfirmAction::ResetAllSpectra,
                ))));
        }
        MenuBarCommand::DisconnectAll => {
            app.modal_stack
                .push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Confirm",
                    "Disconnect all devices?",
                    ConfirmAction::DisconnectAll,
                ))));
        }
        MenuBarCommand::ConnectBluetooth => app
            .bluetooth_window
            .open_and_scan(app.run_manager.runtime_handle(), ui.ctx()),
        MenuBarCommand::RetryBluetooth(name) => {
            if let Err(e) = app.run_manager.connect_bluetooth_list(
                std::slice::from_ref(&name),
                std::time::Duration::from_secs(5),
            ) {
                tracing::warn!(name, error = %e, "Bluetooth reconnect failed");
            }
        }
        MenuBarCommand::ConnectUsb => app
            .usb_window
            .open_and_scan(app.run_manager.runtime_handle(), ui.ctx()),
        MenuBarCommand::ConnectNetwork => {
            let last_ip = app.settings.read_settings().session.last_network_ip.clone();
            app.network_window.open_with_last_ip(&last_ip);
        }
        MenuBarCommand::ConnectSerialGps => app.serial_gps_window.open_and_scan(),
        MenuBarCommand::OpenCalibration => app.calibration_window.open(),
        MenuBarCommand::OpenDeconvolution => app.deconvolution_window.open = true,
        MenuBarCommand::OpenEfficiency => app.efficiency_window.open(),
        MenuBarCommand::OpenPeakFeatures => app.peak_features_window.open(),
        MenuBarCommand::OpenResolution => app.resolution_window.open(),
        MenuBarCommand::AddCalcRoi => {
            app.spectrogram_roi_tab.add_calc_roi();
            app.bottom_tab = crate::app::BottomTab::SpectrogramRois;
        }
        MenuBarCommand::OpenSettings => {
            app.settings_dialog.open_with(&app.settings.read_settings());
        }
        MenuBarCommand::ToggleSiMode(enter) => {
            let (message, action) = if enter {
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
        MenuBarCommand::SetSpectrumTabbed(tabbed) => {
            app.settings.write_settings().plotting.tabbed_spectrum_view = tabbed;
            let mode = if tabbed {
                PlotMode::Tabbed
            } else {
                PlotMode::Combined
            };
            app.spectrum_screen
                .set_mode(mode, &mut app.spectrum_manager);
        }
        MenuBarCommand::OpenDocumentation => app.documentation_dialog.open(),
        MenuBarCommand::OpenBibliography => app.bibliography_dialog.open(),
        MenuBarCommand::ShowAbout => {
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
    }
}

fn import_third_party_drivers(app: &mut LuRaCsApp, ctx: &egui::Context) {
    let dlg = rfd::FileDialog::new().set_title("Import driver files");
    app.file_dialogs.pick_files(ctx, dlg, |app, files| {
        let Some(files) = files else { return };

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
    });
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
                use crate::app::TopTab;
                use crate::spectogram::{Spectrogram, SpectrogramConfig, SpectrogramMode};

                let library_dir = app
                    .settings
                    .read_settings()
                    .paths
                    .spectrogram_library
                    .clone();
                let config = {
                    let s = app.settings.read_settings();
                    SpectrogramConfig {
                        deque_length: s.spectrogram.history_length,
                        map_save_extra_gps_data: s.gps.save_extra_gps_data,
                        map_save_extra_measurement_time_data: s
                            .gps
                            .save_extra_measurement_time_data,
                    }
                };

                match Spectrogram::open(&name, &library_dir, config, SpectrogramMode::Resume) {
                    Ok(sp) => {
                        if let Err(e) = app.run_manager.add_spectrogram(name.clone(), sp) {
                            tracing::error!(name = %name, "add_spectrogram failed: {e}");
                        } else {
                            app.spectrogram_screen.selected_spectrogram = Some(name);
                            app.top_tab = TopTab::Spectrogram;
                        }
                    }
                    Err(e) => {
                        tracing::error!(name = %name, "Spectrogram::open(Resume) failed: {e}");
                        app.modal_stack
                            .push(Modal::root(ModalKind::Info(InfoModal::new(
                                "Import failed",
                                format!("Could not open {name}: {e}"),
                            ))));
                    }
                }
            }
            ImportOutcome::Rois(rois) => {
                app.spectrum_manager.roi_manager.load_rois(&rois);
                tracing::info!(count = rois.len(), "Imported ROIs");
            }
            ImportOutcome::Instrument(inst) => {
                if let InstrumentData::Unique(u) = inst
                    && let Err(e) = app
                        .spectrum_manager
                        .unique_instrument_library
                        .add_instrument(Instrument::Unique(u))
                {
                    tracing::error!("Instrument import failed: {e}");
                }
            }
            ImportOutcome::SpectrumAsBackground(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    #[test]
    fn test_kittest_menu_bar_all_menus_and_commands() {
        struct State {
            ui_state: MenuBarViewState,
            cmds: Vec<MenuBarCommand>,
        }
        let mut harness = Harness::builder()
            .with_size(egui::vec2(900.0, 400.0))
            .build_ui_state(
                |ui, st: &mut State| {
                    let c = draw_menus(&mut st.ui_state, ui);
                    st.cmds.extend(c);
                },
                State {
                    ui_state: MenuBarViewState {
                        recent_connections: vec!["RadiaCode-102".into()],
                        ..Default::default()
                    },
                    cmds: Vec::new(),
                },
            );
        harness.run();

        // File -> Data Library & About in Help
        harness.get_by_label("File").click();
        harness.run();
        harness.get_by_label("Data Library").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::OpenDataLibrary));

        // Detector -> Start Acquisition
        harness.get_by_label("Detector").click();
        harness.run();
        harness.get_by_label("Start Acquisition").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::StartAcquisition));

        // Devices -> Retry Last Connection -> RadiaCode-102
        harness.get_by_label("Devices").click();
        harness.run();
        harness.get_by_label("Connect USB").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::ConnectUsb));

        // Gamma Tools -> Calibration
        harness.get_by_label("Gamma Tools").click();
        harness.run();
        harness.get_by_label("Calibration").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::OpenCalibration));

        // Tools -> Add Calc ROI
        harness.get_by_label("Tools").click();
        harness.run();
        harness.get_by_label("Add Calc ROI").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::AddCalcRoi));

        // Options -> Toggle Si-Mode & Tabbed Spectrum View
        harness.get_by_label("Options").click();
        harness.run();
        harness.get_by_label("Toggle Si-Mode").click();
        harness.run();
        assert_eq!(
            harness.state().cmds.last(),
            Some(&MenuBarCommand::ToggleSiMode(true))
        );
        if harness.query_by_label("Tabbed Spectrum View").is_none() {
            harness.get_by_label("Options").click();
            harness.run();
        }
        harness.get_by_label("Tabbed Spectrum View").click();
        harness.run();
        assert_eq!(
            harness.state().cmds.last(),
            Some(&MenuBarCommand::SetSpectrumTabbed(true))
        );

        // View -> Real Time Data -> Mark Average
        harness.get_by_label("View").click();
        harness.run();
        harness.get_by_label("Real Time Data").hover();
        harness.run();
        harness.get_by_label("Mark Average").click();
        harness.run();
        assert!(harness.state().ui_state.view_realtime_avg_line);

        // Help -> About
        harness.get_by_label("Help").click();
        harness.run();
        harness.get_by_label("About").click();
        harness.run();
        assert_eq!(harness.state().cmds.last(), Some(&MenuBarCommand::ShowAbout));
    }
}
