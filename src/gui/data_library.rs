//! Data Library window.
//!
//! Port of `luracs.gui.windows.data_library.DataLibrary`.
//! Renders as a multi-viewport native OS window with 5 library tabs:
//! - Spectrum Library
//! - Spectrogram Library
//! - ROI Library
//! - Instruments (Unique)
//! - Generic Instruments
//!
//! Features:
//! - Pinned bottom button bar with action handlers (Load, Export, Delete, Close).
//! - Viewport immediate mode (`.with_resizable(false)`).
//! - Support for single and multi-selection, file export (XML, CSV, XLSX, SQLite DB),
//!   and deletion with confirmation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Align2, Color32, Vec2};

use crate::app::{LuRaCsApp, TopTab};
use crate::containers::{GenericInstrument, UniqueInstrument};
use crate::core::{ImportOptions, Instrument, SaveInput, SpectrumImportBundle};
use crate::settings::SharedSettingsExt;
use crate::spectogram::{Spectrogram, SpectrogramConfig, SpectrogramMode};
use crate::utils::{csv_export, xlsx_export, DbParser, DbWriter, XmlParser};

pub const VIEWPORT_ID: &str = "data_library_window";

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataLibraryTab {
    #[default]
    Spectrum,
    Spectrogram,
    Roi,
    UniqueInstruments,
    GenericInstruments,
}

// ---------------------------------------------------------------------------
// Modals State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum PendingDelete {
    Paths {
        kind: DataLibraryTab,
        paths: Vec<PathBuf>,
    },
    Instruments {
        kind: DataLibraryTab,
        keys: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct EditSpectrumState {
    path: PathBuf,
    name: String,
    remark: String,
}

#[derive(Debug, Clone)]
struct EditSpectrogramState {
    path: PathBuf,
    name: String,
}

#[derive(Debug, Clone)]
struct EditRoiState {
    path: PathBuf,
    name: String,
}

#[derive(Debug, Clone)]
struct EditInstrumentState {
    is_new: bool,
    is_unique: bool,
    key: Option<String>,
    name: String,
    model: String,
    manufacturer: String,
    remark: String,
    selected_generic_template: Option<String>,
}

// ---------------------------------------------------------------------------
// DataLibraryWindow
// ---------------------------------------------------------------------------

pub struct DataLibraryWindow {
    pub open: bool,
    pub active_tab: DataLibraryTab,
    pub needs_refresh: bool,

    // Selections per tab
    pub selected_spectrums: BTreeSet<PathBuf>,
    pub selected_spectrograms: BTreeSet<PathBuf>,
    pub selected_rois: BTreeSet<PathBuf>,
    pub selected_unique_instruments: BTreeSet<String>,
    pub selected_generic_instruments: BTreeSet<String>,

    // Tab options
    pub load_instrument: bool,
    pub load_rois: bool,
    pub export_spectrum_data: bool,

    // User feedback
    status_message: Option<(String, bool)>,

    // Modals
    pending_delete: Option<PendingDelete>,
    edit_spectrum: Option<EditSpectrumState>,
    edit_spectrogram: Option<EditSpectrogramState>,
    edit_roi: Option<EditRoiState>,
    edit_instrument: Option<EditInstrumentState>,
}

impl Default for DataLibraryWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl DataLibraryWindow {
    pub const fn new() -> Self {
        Self {
            open: false,
            active_tab: DataLibraryTab::Spectrum,
            needs_refresh: true,
            selected_spectrums: BTreeSet::new(),
            selected_spectrograms: BTreeSet::new(),
            selected_rois: BTreeSet::new(),
            selected_unique_instruments: BTreeSet::new(),
            selected_generic_instruments: BTreeSet::new(),
            load_instrument: true,
            load_rois: true,
            export_spectrum_data: false,
            status_message: None,
            pending_delete: None,
            edit_spectrum: None,
            edit_spectrogram: None,
            edit_roi: None,
            edit_instrument: None,
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
        self.needs_refresh = true;
    }

    pub const fn open_tab(&mut self, tab: usize) {
        self.open = true;
        self.needs_refresh = true;
        self.active_tab = match tab {
            1 => DataLibraryTab::Spectrogram,
            2 => DataLibraryTab::Roi,
            3 => DataLibraryTab::UniqueInstruments,
            4 => DataLibraryTab::GenericInstruments,
            _ => DataLibraryTab::Spectrum,
        };
    }

    // -----------------------------------------------------------------------
    // Main Render
    // -----------------------------------------------------------------------

    pub fn render(&mut self, app: &mut LuRaCsApp, ui: &egui::Ui) {
        if !self.open {
            return;
        }

        if self.needs_refresh {
            app.io_manager.file_index.run_index_all();
            self.needs_refresh = false;
        }

        let viewport_id = egui::ViewportId::from_hash_of(VIEWPORT_ID);
        let builder = egui::ViewportBuilder::default()
            .with_title("Data Library")
            .with_inner_size([820.0, 540.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    self.open = false;
                }

                let modal_open = self.has_open_modal()
                    || crate::gui::modals::has_modal_for(app, viewport_id);

                egui::Panel::bottom("data_library_buttons").show(ui, |ui| {
                    ui.add_enabled_ui(!modal_open, |ui| {
                        self.render_bottom_bar(app, ui);
                    });
                });

                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_enabled_ui(!modal_open, |ui| {
                        self.render_content(app, ui);
                    });
                    self.render_modals(app, ui.ctx());
                });

                crate::gui::modals::render_modals_for(app, viewport_id, ui);
            });
    }

    const fn has_open_modal(&self) -> bool {
        self.pending_delete.is_some()
            || self.edit_spectrum.is_some()
            || self.edit_spectrogram.is_some()
            || self.edit_roi.is_some()
            || self.edit_instrument.is_some()
    }

    // -----------------------------------------------------------------------
    // Top Tabs & Tables
    // -----------------------------------------------------------------------

    fn render_content(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.active_tab,
                DataLibraryTab::Spectrum,
                "Spectrum Library",
            );
            ui.selectable_value(
                &mut self.active_tab,
                DataLibraryTab::Spectrogram,
                "Spectrogram Library",
            );
            ui.selectable_value(&mut self.active_tab, DataLibraryTab::Roi, "ROI Library");
            ui.selectable_value(
                &mut self.active_tab,
                DataLibraryTab::UniqueInstruments,
                "Instruments",
            );
            ui.selectable_value(
                &mut self.active_tab,
                DataLibraryTab::GenericInstruments,
                "Generic Instruments",
            );
        });

        ui.separator();

        if let Some((msg, is_err)) = &self.status_message {
            let color = if *is_err {
                Color32::from_rgb(220, 60, 60)
            } else {
                Color32::from_rgb(60, 180, 60)
            };
            ui.colored_label(color, msg);
            ui.separator();
        }

        match self.active_tab {
            DataLibraryTab::Spectrum => self.render_spectrum_tab(app, ui),
            DataLibraryTab::Spectrogram => self.render_spectrogram_tab(app, ui),
            DataLibraryTab::Roi => self.render_roi_tab(app, ui),
            DataLibraryTab::UniqueInstruments => self.render_unique_instruments_tab(app, ui),
            DataLibraryTab::GenericInstruments => self.render_generic_instruments_tab(app, ui),
        }
    }

    // -----------------------------------------------------------------------
    // Tab 1: Spectrum
    // -----------------------------------------------------------------------

    fn render_spectrum_tab(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        let registry = &app.io_manager.file_index.spectrum;
        let mut paths: Vec<PathBuf> = registry.paths().cloned().collect();
        paths.sort();

        let all_selected = !paths.is_empty() && paths.iter().all(|p| self.selected_spectrums.contains(p));
        let mut toggle_all = all_selected;

        let mut double_clicked_path = None;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("spectrum_lib_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Header
                        if ui.checkbox(&mut toggle_all, "").clicked() {
                            if all_selected {
                                self.selected_spectrums.clear();
                            } else {
                                self.selected_spectrums.extend(paths.iter().cloned());
                            }
                        }
                        ui.strong("Name");
                        ui.strong("Date");
                        ui.strong("Live Time");
                        ui.strong("Background");
                        ui.strong("ROIs");
                        ui.strong("Instrument");
                        ui.end_row();

                        // Rows
                        for path in &paths {
                            let item = registry.get(path);
                            let xml = item.and_then(|f| f.as_xml());

                            let header = xml.map(|x| x.get_header());
                            let fg = xml.and_then(|x| x.get_foreground_spectrum());
                            let bg = xml.and_then(|x| x.get_background_spectrum());
                            let rois_count = xml.map_or(0, |x| x.get_rois().len());

                            let stem = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let name = header
                                .as_ref()
                                .and_then(|h| if h.name.is_empty() { None } else { Some(h.name.clone()) })
                                .unwrap_or(stem);

                            let date = fg
                                .as_ref()
                                .and_then(|f| f.start_date)
                                .map_or_else(|| "—".into(), |d| d.format("%Y-%m-%d %H:%M:%S").to_string());
                            let live_time = fg
                                .as_ref()
                                .map_or_else(|| "—".into(), |f| format_duration(f.live_time));
                            let has_bg = if bg.is_some() { "Yes" } else { "No" };
                            let instr = header
                                .as_ref()
                                .and_then(|h| h.instrument_model.as_ref().or(h.instrument_id.as_ref()))
                                .map_or("None", |s| s.as_str());

                            let is_sel = self.selected_spectrums.contains(path);
                            let mut row_sel = is_sel;
                            if ui.checkbox(&mut row_sel, "").clicked() {
                                if row_sel {
                                    self.selected_spectrums.insert(path.clone());
                                } else {
                                    self.selected_spectrums.remove(path);
                                }
                            }

                            let label_resp = ui.selectable_label(is_sel, &name);
                            if label_resp.double_clicked() {
                                double_clicked_path = Some(path.clone());
                            } else if label_resp.clicked() {
                                if is_sel {
                                    self.selected_spectrums.remove(path);
                                } else {
                                    self.selected_spectrums.insert(path.clone());
                                }
                            }

                            ui.label(date);
                            ui.label(live_time);
                            ui.label(has_bg);
                            ui.label(rois_count.to_string());
                            ui.label(instr);
                            ui.end_row();
                        }
                    });
            });

        if let Some(path) = double_clicked_path {
            self.load_spectrums(app, &[path]);
        }
    }

    // -----------------------------------------------------------------------
    // Tab 2: Spectrogram
    // -----------------------------------------------------------------------

    fn render_spectrogram_tab(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        let registry = &app.io_manager.file_index.spectrogram;
        let mut paths: Vec<PathBuf> = registry.paths().cloned().collect();
        paths.sort();

        let all_selected = !paths.is_empty() && paths.iter().all(|p| self.selected_spectrograms.contains(p));
        let mut toggle_all = all_selected;

        let mut double_clicked_path = None;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("spectrogram_lib_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Header
                        if ui.checkbox(&mut toggle_all, "").clicked() {
                            if all_selected {
                                self.selected_spectrograms.clear();
                            } else {
                                self.selected_spectrograms.extend(paths.iter().cloned());
                            }
                        }
                        ui.strong("Name");
                        ui.strong("Start Date");
                        ui.strong("End Date");
                        ui.strong("Duration");
                        ui.strong("Instrument");
                        ui.end_row();

                        // Rows
                        for path in &paths {
                            let item = registry.get(path);
                            let db = item.and_then(|f| f.as_db());

                            let header = db.and_then(|p| p.get_header().ok());
                            let summary = db.and_then(|p| p.get_summary().ok());

                            let stem = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();

                            let start_date = header
                                .as_ref()
                                .map_or_else(|| "—".into(), |h| h.created.format("%Y-%m-%d %H:%M:%S").to_string());
                            let end_date = summary
                                .as_ref()
                                .map_or_else(|| "—".into(), |s| s.last_update.format("%Y-%m-%d %H:%M:%S").to_string());
                            let duration = summary
                                .as_ref()
                                .map_or_else(|| "—".into(), |s| format_duration(s.total_duration));
                            let instr = header
                                .as_ref()
                                .map_or_else(|| "—".into(), |h| h.device_id.clone());

                            let is_sel = self.selected_spectrograms.contains(path);
                            let mut row_sel = is_sel;
                            if ui.checkbox(&mut row_sel, "").clicked() {
                                if row_sel {
                                    self.selected_spectrograms.insert(path.clone());
                                } else {
                                    self.selected_spectrograms.remove(path);
                                }
                            }

                            let label_resp = ui.selectable_label(is_sel, &stem);
                            if label_resp.double_clicked() {
                                double_clicked_path = Some(path.clone());
                            } else if label_resp.clicked() {
                                if is_sel {
                                    self.selected_spectrograms.remove(path);
                                } else {
                                    self.selected_spectrograms.insert(path.clone());
                                }
                            }

                            ui.label(start_date);
                            ui.label(end_date);
                            ui.label(duration);
                            ui.label(instr);
                            ui.end_row();
                        }
                    });
            });

        if let Some(path) = double_clicked_path {
            self.load_spectrograms(app, &[path]);
        }
    }

    // -----------------------------------------------------------------------
    // Tab 3: ROI
    // -----------------------------------------------------------------------

    fn render_roi_tab(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        let registry = &app.io_manager.file_index.roi;
        let mut paths: Vec<PathBuf> = registry.paths().cloned().collect();
        paths.sort();

        let all_selected = !paths.is_empty() && paths.iter().all(|p| self.selected_rois.contains(p));
        let mut toggle_all = all_selected;

        let mut double_clicked_path = None;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("roi_lib_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Header
                        if ui.checkbox(&mut toggle_all, "").clicked() {
                            if all_selected {
                                self.selected_rois.clear();
                            } else {
                                self.selected_rois.extend(paths.iter().cloned());
                            }
                        }
                        ui.strong("Name");
                        ui.strong("ROIs");
                        ui.strong("Regions");
                        ui.end_row();

                        // Rows
                        for path in &paths {
                            let item = registry.get(path);
                            let xml = item.and_then(|f| f.as_xml());

                            let stem = path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();

                            let rois = xml.map_or(&[][..], |x| x.get_rois());
                            let rois_count = rois.len();
                            let regions = rois
                                .iter()
                                .map(|r| format!("[{:.0}, {:.0}]", r.roi_bound.0, r.roi_bound.1))
                                .collect::<Vec<_>>()
                                .join(", ");

                            let is_sel = self.selected_rois.contains(path);
                            let mut row_sel = is_sel;
                            if ui.checkbox(&mut row_sel, "").clicked() {
                                if row_sel {
                                    self.selected_rois.insert(path.clone());
                                } else {
                                    self.selected_rois.remove(path);
                                }
                            }

                            let label_resp = ui.selectable_label(is_sel, &stem);
                            if label_resp.double_clicked() {
                                double_clicked_path = Some(path.clone());
                            } else if label_resp.clicked() {
                                if is_sel {
                                    self.selected_rois.remove(path);
                                } else {
                                    self.selected_rois.insert(path.clone());
                                }
                            }

                            ui.label(rois_count.to_string());
                            ui.label(regions);
                            ui.end_row();
                        }
                    });
            });

        if let Some(path) = double_clicked_path {
            self.load_rois_to_spectrum(app, &[path]);
        }
    }

    // -----------------------------------------------------------------------
    // Tab 4: Instruments (Unique)
    // -----------------------------------------------------------------------

    fn render_unique_instruments_tab(&mut self, app: &LuRaCsApp, ui: &mut egui::Ui) {
        let mut keys: Vec<String> = app
            .spectrum_manager
            .unique_instrument_library
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();

        let all_selected = !keys.is_empty() && keys.iter().all(|k| self.selected_unique_instruments.contains(k));
        let mut toggle_all = all_selected;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("unique_instruments_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Header
                        if ui.checkbox(&mut toggle_all, "").clicked() {
                            if all_selected {
                                self.selected_unique_instruments.clear();
                            } else {
                                self.selected_unique_instruments.extend(keys.iter().cloned());
                            }
                        }
                        ui.strong("Name");
                        ui.strong("Model");
                        ui.strong("Calibration");
                        ui.strong("Resolution");
                        ui.strong("Efficiency");
                        ui.end_row();

                        // Rows
                        for key in &keys {
                            let inst = app
                                .spectrum_manager
                                .unique_instrument_library
                                .get(key)
                                .and_then(|i| i.as_unique());

                            let Some(u) = inst else { continue };

                            let is_sel = self.selected_unique_instruments.contains(key);
                            let mut row_sel = is_sel;
                            if ui.checkbox(&mut row_sel, "").clicked() {
                                if row_sel {
                                    self.selected_unique_instruments.insert(key.clone());
                                } else {
                                    self.selected_unique_instruments.remove(key);
                                }
                            }

                            let label_resp = ui.selectable_label(is_sel, &u.name);
                            if label_resp.clicked() {
                                if is_sel {
                                    self.selected_unique_instruments.remove(key);
                                } else {
                                    self.selected_unique_instruments.insert(key.clone());
                                }
                            }

                            ui.label(&u.generic.model);
                            ui.label(if u.calibration_coefficients.is_some() { "Yes" } else { "No" });
                            ui.label(if u.generic.resolution_fn.is_some() { "Yes" } else { "No" });
                            ui.label(if u.generic.int_efficiency_fn.is_some() { "Yes" } else { "No" });
                            ui.end_row();
                        }
                    });
            });
    }

    // -----------------------------------------------------------------------
    // Tab 5: Generic Instruments
    // -----------------------------------------------------------------------

    fn render_generic_instruments_tab(&mut self, app: &LuRaCsApp, ui: &mut egui::Ui) {
        let mut keys: Vec<String> = app
            .spectrum_manager
            .generic_instrument_library
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        keys.sort();

        let all_selected = !keys.is_empty() && keys.iter().all(|k| self.selected_generic_instruments.contains(k));
        let mut toggle_all = all_selected;

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("generic_instruments_grid")
                    .striped(true)
                    .min_col_width(60.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        // Header
                        if ui.checkbox(&mut toggle_all, "").clicked() {
                            if all_selected {
                                self.selected_generic_instruments.clear();
                            } else {
                                self.selected_generic_instruments.extend(keys.iter().cloned());
                            }
                        }
                        ui.strong("Model");
                        ui.strong("Resolution");
                        ui.strong("Efficiency");
                        ui.end_row();

                        // Rows
                        for key in &keys {
                            let inst = app
                                .spectrum_manager
                                .generic_instrument_library
                                .get(key)
                                .and_then(|i| i.as_generic());

                            let Some(g) = inst else { continue };

                            let is_sel = self.selected_generic_instruments.contains(key);
                            let mut row_sel = is_sel;
                            if ui.checkbox(&mut row_sel, "").clicked() {
                                if row_sel {
                                    self.selected_generic_instruments.insert(key.clone());
                                } else {
                                    self.selected_generic_instruments.remove(key);
                                }
                            }

                            let label_resp = ui.selectable_label(is_sel, &g.model);
                            if label_resp.clicked() {
                                if is_sel {
                                    self.selected_generic_instruments.remove(key);
                                } else {
                                    self.selected_generic_instruments.insert(key.clone());
                                }
                            }

                            ui.label(if g.resolution_fn.is_some() { "Yes" } else { "No" });
                            ui.label(if g.int_efficiency_fn.is_some() { "Yes" } else { "No" });
                            ui.end_row();
                        }
                    });
            });
    }

    // -----------------------------------------------------------------------
    // Pinned Bottom Bar
    // -----------------------------------------------------------------------

    fn render_bottom_bar(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Left: Delete button
            let has_selection = match self.active_tab {
                DataLibraryTab::Spectrum => !self.selected_spectrums.is_empty(),
                DataLibraryTab::Spectrogram => !self.selected_spectrograms.is_empty(),
                DataLibraryTab::Roi => !self.selected_rois.is_empty(),
                DataLibraryTab::UniqueInstruments => !self.selected_unique_instruments.is_empty(),
                DataLibraryTab::GenericInstruments => !self.selected_generic_instruments.is_empty(),
            };

            if ui
                .add_enabled(has_selection, egui::Button::new("Delete"))
                .clicked()
            {
                self.initiate_delete();
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    self.open = false;
                }

                match self.active_tab {
                    DataLibraryTab::Spectrum => {
                        let sel: Vec<PathBuf> = self.selected_spectrums.iter().cloned().collect();
                        if ui
                            .add_enabled(!sel.is_empty(), egui::Button::new("Load"))
                            .clicked()
                        {
                            self.load_spectrums(app, &sel);
                        }

                        // Export menu
                        ui.menu_button("Export", |ui| {
                            if ui.button("LuRaCs XML (*.xml)").clicked() {
                                export_same(app, ui.ctx(), &sel, "xml", "LuRaCs XML (*.xml)");
                            }
                            if ui.button("CSV (*.csv)").clicked() {
                                export_csv_spectrum(app, ui.ctx(), &sel);
                            }
                            if ui.button("Excel Workbook (*.xlsx)").clicked() {
                                export_xlsx_spectrum(app, ui.ctx(), &sel);
                            }
                        });

                        // Edit
                        if ui
                            .add_enabled(sel.len() == 1, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.start_edit_spectrum(app, &sel[0]);
                        }

                        ui.checkbox(&mut self.load_rois, "Load ROIs");
                        ui.checkbox(&mut self.load_instrument, "Load Instrument");
                    }
                    DataLibraryTab::Spectrogram => {
                        let sel: Vec<PathBuf> = self.selected_spectrograms.iter().cloned().collect();
                        if ui
                            .add_enabled(!sel.is_empty(), egui::Button::new("Load"))
                            .clicked()
                        {
                            self.load_spectrograms(app, &sel);
                        }

                        // Export menu
                        ui.menu_button("Export", |ui| {
                            if ui.button("LuRaCs SQLite Database (*.db)").clicked() {
                                export_same(app, ui.ctx(), &sel, "db", "LuRaCs SQLite Database (*.db)");
                            }
                            if ui.button("Excel Workbook (*.xlsx)").clicked() {
                                export_xlsx_spectrogram(app, ui.ctx(), &sel, self.export_spectrum_data);
                            }
                        });

                        // Edit
                        if ui
                            .add_enabled(sel.len() == 1, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.start_edit_spectrogram(app, &sel[0]);
                        }

                        ui.checkbox(&mut self.export_spectrum_data, "Export Spectrum Data");
                    }
                    DataLibraryTab::Roi => {
                        let sel: Vec<PathBuf> = self.selected_rois.iter().cloned().collect();
                        if ui
                            .add_enabled(!sel.is_empty(), egui::Button::new("Load to Spectrogram"))
                            .clicked()
                        {
                            self.load_rois_to_spectrogram(app, &sel);
                        }
                        if ui
                            .add_enabled(!sel.is_empty(), egui::Button::new("Load to Spectrum"))
                            .clicked()
                        {
                            self.load_rois_to_spectrum(app, &sel);
                        }

                        // Export menu
                        ui.menu_button("Export", |ui| {
                            if ui.button("LuRaCs ROI File (*.xml)").clicked() {
                                export_same(app, ui.ctx(), &sel, "xml", "LuRaCs ROI File (*.xml)");
                            }
                        });

                        // Edit
                        if ui
                            .add_enabled(sel.len() == 1, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.start_edit_roi(app, &sel[0]);
                        }
                    }
                    DataLibraryTab::UniqueInstruments => {
                        let sel: Vec<String> = self.selected_unique_instruments.iter().cloned().collect();
                        if ui.button("New").clicked() {
                            self.start_new_instrument(true);
                        }

                        // Export menu
                        ui.menu_button("Export", |ui| {
                            if ui.button("LuRaCs Instrument File (*.xml)").clicked() {
                                let paths: Vec<PathBuf> = sel.iter().map(PathBuf::from).filter(|p| p.exists()).collect();
                                export_same(app, ui.ctx(), &paths, "xml", "LuRaCs Instrument File (*.xml)");
                            }
                        });

                        // Edit
                        if ui
                            .add_enabled(sel.len() == 1, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.start_edit_instrument(app, true, &sel[0]);
                        }
                    }
                    DataLibraryTab::GenericInstruments => {
                        let sel: Vec<String> = self.selected_generic_instruments.iter().cloned().collect();
                        if ui.button("New").clicked() {
                            self.start_new_instrument(false);
                        }

                        // Export menu
                        ui.menu_button("Export", |ui| {
                            if ui.button("LuRaCs Instrument File (*.xml)").clicked() {
                                let paths: Vec<PathBuf> = sel.iter().map(PathBuf::from).filter(|p| p.exists()).collect();
                                export_same(app, ui.ctx(), &paths, "xml", "LuRaCs Instrument File (*.xml)");
                            }
                        });

                        // Edit
                        if ui
                            .add_enabled(sel.len() == 1, egui::Button::new("Edit"))
                            .clicked()
                        {
                            self.start_edit_instrument(app, false, &sel[0]);
                        }
                    }
                }
            });
        });
    }

    // -----------------------------------------------------------------------
    // Action Implementations
    // -----------------------------------------------------------------------

    fn load_spectrums(&mut self, app: &mut LuRaCsApp, paths: &[PathBuf]) {
        for path in paths {
            if let Some(parsed) = app.io_manager.importer.parse_spectrum(path) {
                let mut bundle = SpectrumImportBundle::from(parsed);
                if !self.load_rois {
                    bundle.peaks.clear();
                }
                if !self.load_instrument {
                    bundle.instrument = None;
                }
                let opts = ImportOptions {
                    external_import: true,
                    load_rois_on_import: self.load_rois,
                    load_instrument_on_import: self.load_instrument,
                    tabbed_spectrum_view: app.io_manager.importer.tabbed_spectrum_view,
                    load_calibration_from_instrument: true,
                };
                if let Err(e) = app.spectrum_manager.import_spectrum(bundle, opts) {
                    self.status_message = Some((format!("Failed to import spectrum: {e}"), true));
                } else {
                    app.top_tab = TopTab::Spectrum;
                }
            } else {
                self.status_message = Some((format!("Failed to parse {}", path.display()), true));
            }
        }
    }

    fn load_spectrograms(&mut self, app: &mut LuRaCsApp, paths: &[PathBuf]) {
        for path in paths {
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let library_dir = app.settings.read_settings().paths.spectrogram_library.clone();
            let config = {
                let s = app.settings.read_settings();
                SpectrogramConfig {
                    deque_length: s.spectrogram.history_length,
                    map_save_extra_gps_data: s.gps.save_extra_gps_data,
                    map_save_extra_measurement_time_data: s.gps.save_extra_measurement_time_data,
                }
            };

            match Spectrogram::open(&name, &library_dir, config, SpectrogramMode::Resume) {
                Ok(sp) => {
                    if let Err(e) = app.run_manager.add_spectrogram(name.clone(), sp) {
                        self.status_message = Some((format!("Failed to add spectrogram: {e}"), true));
                    } else {
                        app.spectrogram_screen.selected_spectrogram = Some(name);
                        app.top_tab = TopTab::Spectrogram;
                    }
                }
                Err(e) => {
                    self.status_message = Some((format!("Failed to open spectrogram: {e}"), true));
                }
            }
        }
    }

    fn load_rois_to_spectrum(&mut self, app: &mut LuRaCsApp, paths: &[PathBuf]) {
        for path in paths {
            if let Ok(parser) = XmlParser::open(path, false) {
                let rois = parser.get_rois();
                app.spectrum_manager.roi_manager.load_rois(rois);
                self.status_message = Some((format!("Loaded {} ROIs to Spectrum", rois.len()), false));
            }
        }
    }

    fn load_rois_to_spectrogram(&mut self, app: &mut LuRaCsApp, paths: &[PathBuf]) {
        for path in paths {
            if let Ok(parser) = XmlParser::open(path, false) {
                let rois = parser.get_rois();
                app.run_manager.spectrogram_manager_mut().load_rois(rois);
                self.status_message = Some((format!("Loaded {} ROIs to Spectrogram", rois.len()), false));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Delete Initiator & Performer
    // -----------------------------------------------------------------------

    fn initiate_delete(&mut self) {
        match self.active_tab {
            DataLibraryTab::Spectrum => {
                let paths: Vec<PathBuf> = self.selected_spectrums.iter().cloned().collect();
                self.pending_delete = Some(PendingDelete::Paths {
                    kind: DataLibraryTab::Spectrum,
                    paths,
                });
            }
            DataLibraryTab::Spectrogram => {
                let paths: Vec<PathBuf> = self.selected_spectrograms.iter().cloned().collect();
                self.pending_delete = Some(PendingDelete::Paths {
                    kind: DataLibraryTab::Spectrogram,
                    paths,
                });
            }
            DataLibraryTab::Roi => {
                let paths: Vec<PathBuf> = self.selected_rois.iter().cloned().collect();
                self.pending_delete = Some(PendingDelete::Paths {
                    kind: DataLibraryTab::Roi,
                    paths,
                });
            }
            DataLibraryTab::UniqueInstruments => {
                let keys: Vec<String> = self.selected_unique_instruments.iter().cloned().collect();
                self.pending_delete = Some(PendingDelete::Instruments {
                    kind: DataLibraryTab::UniqueInstruments,
                    keys,
                });
            }
            DataLibraryTab::GenericInstruments => {
                let keys: Vec<String> = self.selected_generic_instruments.iter().cloned().collect();
                self.pending_delete = Some(PendingDelete::Instruments {
                    kind: DataLibraryTab::GenericInstruments,
                    keys,
                });
            }
        }
    }

    fn perform_delete(&mut self, app: &mut LuRaCsApp, pending: PendingDelete) {
        match pending {
            PendingDelete::Paths { kind, paths } => match kind {
                DataLibraryTab::Spectrum => {
                    for path in &paths {
                        drop(app.io_manager.file_index.spectrum.delete_file(path));
                    }
                    self.selected_spectrums.clear();
                    self.needs_refresh = true;
                }
                DataLibraryTab::Spectrogram => {
                    for path in &paths {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if app
                            .run_manager
                            .spectrogram_manager()
                            .spectrogram_registry
                            .contains(name.as_ref())
                        {
                            self.status_message = Some((
                                format!(
                                    "Spectrogram '{name}' must be unloaded from memory before it can be deleted."
                                ),
                                true,
                            ));
                            return;
                        }
                    }
                    for path in &paths {
                        drop(app.io_manager.file_index.spectrogram.delete_file(path));
                    }
                    self.selected_spectrograms.clear();
                    self.needs_refresh = true;
                }
                DataLibraryTab::Roi => {
                    for path in &paths {
                        drop(app.io_manager.file_index.roi.delete_file(path));
                    }
                    self.selected_rois.clear();
                    self.needs_refresh = true;
                }
                _ => {}
            },
            PendingDelete::Instruments { kind, keys } => match kind {
                DataLibraryTab::UniqueInstruments => {
                    for key in &keys {
                        app.spectrum_manager.unique_instrument_library.remove_instrument(key);
                        let p = Path::new(key);
                        if p.exists() {
                            drop(app.io_manager.file_index.unique_instrument.delete_file(p));
                        }
                    }
                    self.selected_unique_instruments.clear();
                    self.needs_refresh = true;
                }
                DataLibraryTab::GenericInstruments => {
                    for key in &keys {
                        app.spectrum_manager.generic_instrument_library.remove_instrument(key);
                        let p = Path::new(key);
                        if p.exists() {
                            drop(app.io_manager.file_index.generic_instrument.delete_file(p));
                        }
                    }
                    self.selected_generic_instruments.clear();
                    self.needs_refresh = true;
                }
                _ => {}
            },
        }
    }

    // -----------------------------------------------------------------------
    // Edit & New Starters
    // -----------------------------------------------------------------------

    fn start_edit_spectrum(&mut self, app: &LuRaCsApp, path: &Path) {
        let xml = app
            .io_manager
            .file_index
            .spectrum
            .get(path)
            .and_then(|f| f.as_xml());
        let header = xml.map(|x| x.get_header());
        let name = header
            .as_ref()
            .and_then(|h| if h.name.is_empty() { None } else { Some(h.name.clone()) })
            .unwrap_or_else(|| {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });
        let remark = header
            .and_then(|h| h.remark)
            .unwrap_or_default();

        self.edit_spectrum = Some(EditSpectrumState {
            path: path.to_path_buf(),
            name,
            remark,
        });
    }

    fn start_edit_spectrogram(&mut self, app: &LuRaCsApp, path: &Path) {
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if app
            .run_manager
            .spectrogram_manager()
            .spectrogram_registry
            .contains(name.as_ref())
        {
            self.status_message = Some((
                "A loaded spectrogram cannot be renamed. Please unload before renaming.".into(),
                true,
            ));
            return;
        }

        self.edit_spectrogram = Some(EditSpectrogramState {
            path: path.to_path_buf(),
            name: stem,
        });
    }

    fn start_edit_roi(&mut self, _app: &LuRaCsApp, path: &Path) {
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        self.edit_roi = Some(EditRoiState {
            path: path.to_path_buf(),
            name: stem,
        });
    }

    fn start_new_instrument(&mut self, is_unique: bool) {
        self.edit_instrument = Some(EditInstrumentState {
            is_new: true,
            is_unique,
            key: None,
            name: String::new(),
            model: String::new(),
            manufacturer: String::new(),
            remark: String::new(),
            selected_generic_template: None,
        });
    }

    fn start_edit_instrument(&mut self, app: &LuRaCsApp, is_unique: bool, key: &str) {
        if is_unique {
            if let Some(inst) = app
                .spectrum_manager
                .unique_instrument_library
                .get(key)
                .and_then(|i| i.as_unique())
            {
                self.edit_instrument = Some(EditInstrumentState {
                    is_new: false,
                    is_unique: true,
                    key: Some(key.to_string()),
                    name: inst.name.clone(),
                    model: inst.generic.model.clone(),
                    manufacturer: inst.generic.manufacturer.clone(),
                    remark: inst.generic.remark.clone(),
                    selected_generic_template: None,
                });
            }
        } else if let Some(inst) = app
            .spectrum_manager
            .generic_instrument_library
            .get(key)
            .and_then(|i| i.as_generic())
        {
            self.edit_instrument = Some(EditInstrumentState {
                is_new: false,
                is_unique: false,
                key: Some(key.to_string()),
                name: String::new(),
                model: inst.model.clone(),
                manufacturer: inst.manufacturer.clone(),
                remark: inst.remark.clone(),
                selected_generic_template: None,
            });
        }
    }

    // -----------------------------------------------------------------------
    // Modals Rendering
    // -----------------------------------------------------------------------

    fn render_modals(&mut self, app: &mut LuRaCsApp, ctx: &egui::Context) {
        // Confirmation dialog for deletion
        if let Some(pending) = self.pending_delete.clone() {
            let count = match &pending {
                PendingDelete::Paths { paths, .. } => paths.len(),
                PendingDelete::Instruments { keys, .. } => keys.len(),
            };

            let mut choice = None;
            egui::Window::new("Confirm Deletion")
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Do you want to delete {count} selected item(s)?"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            choice = Some(true);
                        }
                        if ui.button("No").clicked() {
                            choice = Some(false);
                        }
                    });
                });

            if let Some(confirm) = choice {
                if confirm {
                    self.perform_delete(app, pending);
                }
                self.pending_delete = None;
            }
        }

        // Edit Spectrum
        if let Some(mut state) = self.edit_spectrum.clone() {
            let mut close_dialog = false;
            egui::Window::new("Edit Spectrum")
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("edit_spectrum_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut state.name);
                            ui.end_row();

                            ui.label("Remark:");
                            ui.text_edit_multiline(&mut state.remark);
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if !state.name.trim().is_empty()
                                && let Ok(parser) = XmlParser::open(&state.path, false)
                                && let Some(mut spec) = parser.get_spectrum()
                            {
                                spec.name.clone_from(&state.name);
                                spec.remark.clone_from(&state.remark);
                                drop(app.io_manager.file_index.spectrum.delete_file(&state.path));
                                drop(app.io_manager.file_index.spectrum.save_file(SaveInput::Spectrum(&spec)));
                                self.needs_refresh = true;
                                self.status_message = Some(("Spectrum updated".into(), false));
                            }
                            close_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if close_dialog {
                self.edit_spectrum = None;
            } else {
                self.edit_spectrum = Some(state);
            }
        }

        // Edit Spectrogram
        if let Some(mut state) = self.edit_spectrogram.clone() {
            let mut close_dialog = false;
            egui::Window::new("Edit Spectrogram")
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("edit_spectrogram_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut state.name);
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let new_name = state.name.trim();
                            if !new_name.is_empty() {
                                let new_path = state.path.with_file_name(format!("{new_name}.db"));
                                if let Err(e) = app.io_manager.file_index.spectrogram.rename_file(&state.path, &new_path) {
                                    self.status_message = Some((format!("Failed to rename: {e}"), true));
                                } else {
                                    self.needs_refresh = true;
                                    self.status_message = Some(("Spectrogram renamed".into(), false));
                                }
                            }
                            close_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if close_dialog {
                self.edit_spectrogram = None;
            } else {
                self.edit_spectrogram = Some(state);
            }
        }

        // Edit ROI
        if let Some(mut state) = self.edit_roi.clone() {
            let mut close_dialog = false;
            egui::Window::new("Edit ROI File")
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("edit_roi_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(&mut state.name);
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let new_name = state.name.trim();
                            if !new_name.is_empty()
                                && let Ok(parser) = XmlParser::open(&state.path, false)
                            {
                                let rois = parser.get_rois().to_vec();
                                drop(app.io_manager.file_index.roi.delete_file(&state.path));
                                drop(app.io_manager.file_index.roi.save_file(SaveInput::Rois {
                                    rois: &rois,
                                    name: new_name,
                                }));
                                self.needs_refresh = true;
                                self.status_message = Some(("ROI file updated".into(), false));
                            }
                            close_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if close_dialog {
                self.edit_roi = None;
            } else {
                self.edit_roi = Some(state);
            }
        }

        // Edit / New Instrument
        if let Some(mut state) = self.edit_instrument.clone() {
            let mut close_dialog = false;
            let title = if state.is_new {
                if state.is_unique { "New Unique Instrument" } else { "New Generic Instrument" }
            } else if state.is_unique {
                "Edit Unique Instrument"
            } else {
                "Edit Generic Instrument"
            };

            egui::Window::new(title)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("edit_instrument_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            if state.is_unique {
                                ui.label("Generic Template:");
                                let current_template = state.selected_generic_template.clone().unwrap_or_else(|| "None".into());
                                egui::ComboBox::from_id_salt("inst_template_combo")
                                    .selected_text(&current_template)
                                    .width(200.0)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(state.selected_generic_template.is_none(), "None").clicked() {
                                            state.selected_generic_template = None;
                                        }
                                        for (key, inst) in app.spectrum_manager.generic_instrument_library.iter() {
                                            let selected = state.selected_generic_template.as_deref() == Some(key);
                                            if ui.selectable_label(selected, inst.name_or_model()).clicked() {
                                                state.selected_generic_template = Some(key.clone());
                                                if let Some(g) = inst.as_generic() {
                                                    state.model.clone_from(&g.model);
                                                    state.manufacturer.clone_from(&g.manufacturer);
                                                }
                                            }
                                        }
                                    });
                                ui.end_row();

                                ui.label("Name:");
                                ui.text_edit_singleline(&mut state.name);
                                ui.end_row();
                            }

                            ui.label("Model:");
                            ui.text_edit_singleline(&mut state.model);
                            ui.end_row();

                            ui.label("Manufacturer:");
                            ui.text_edit_singleline(&mut state.manufacturer);
                            ui.end_row();

                            ui.label("Remark:");
                            ui.text_edit_multiline(&mut state.remark);
                            ui.end_row();
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if state.is_unique {
                                if !state.name.trim().is_empty() {
                                    let base_generic = state
                                        .selected_generic_template
                                        .as_ref()
                                        .and_then(|k| app.spectrum_manager.generic_instrument_library.get(k))
                                        .and_then(|i| i.as_generic().cloned())
                                        .unwrap_or_default();

                                    let mut u = UniqueInstrument::from_template(state.name.trim(), &base_generic);
                                    u.generic.model.clone_from(&state.model);
                                    u.generic.manufacturer.clone_from(&state.manufacturer);
                                    u.generic.remark.clone_from(&state.remark);

                                    if let Some(old_key) = &state.key {
                                        drop(app.spectrum_manager.unique_instrument_library.rename_instrument(
                                            old_key,
                                            Instrument::Unique(u.clone()),
                                        ));
                                        drop(app.io_manager.file_index.unique_instrument.delete_file(Path::new(old_key)));
                                    } else {
                                        drop(app.spectrum_manager.unique_instrument_library.add_instrument(
                                            Instrument::Unique(u.clone()),
                                        ));
                                    }
                                    drop(app.io_manager.file_index.unique_instrument.save_file(SaveInput::UniqueInstrument(&u)));
                                    self.needs_refresh = true;
                                    self.status_message = Some(("Unique instrument saved".into(), false));
                                }
                            } else if !state.model.trim().is_empty() {
                                let mut g = GenericInstrument {
                                    model: state.model.clone(),
                                    manufacturer: state.manufacturer.clone(),
                                    remark: state.remark.clone(),
                                    ..Default::default()
                                };
                                if let Some(old_key) = &state.key {
                                    if let Some(orig) = app.spectrum_manager.generic_instrument_library.get(old_key).and_then(|i| i.as_generic()) {
                                        g = orig.clone();
                                        g.model.clone_from(&state.model);
                                        g.manufacturer.clone_from(&state.manufacturer);
                                        g.remark.clone_from(&state.remark);
                                    }
                                    drop(app.spectrum_manager.generic_instrument_library.rename_instrument(
                                        old_key,
                                        Instrument::Generic(g.clone()),
                                    ));
                                    drop(app.io_manager.file_index.generic_instrument.delete_file(Path::new(old_key)));
                                } else {
                                    drop(app.spectrum_manager.generic_instrument_library.add_instrument(
                                        Instrument::Generic(g.clone()),
                                    ));
                                }
                                drop(app.io_manager.file_index.generic_instrument.save_file(SaveInput::GenericInstrument(&g)));
                                self.needs_refresh = true;
                                self.status_message = Some(("Generic instrument saved".into(), false));
                            }
                            close_dialog = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_dialog = true;
                        }
                    });
                });

            if close_dialog {
                self.edit_instrument = None;
            } else {
                self.edit_instrument = Some(state);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_duration(seconds: f64) -> String {
    let s = if seconds > 0.0 { seconds as u64 } else { 0 };
    let hours = s / 3600;
    let minutes = (s % 3600) / 60;
    let secs = s % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

fn export_same(
    app: &mut LuRaCsApp,
    ctx: &egui::Context,
    selected: &[PathBuf],
    ext: &str,
    filter_label: &str,
) {
    if selected.is_empty() {
        return;
    }
    if selected.len() == 1 {
        let src = selected[0].clone();
        let default_name = src
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let ext = ext.to_string();
        let dlg = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter(filter_label, &[&ext]);
        app.file_dialogs.save_file(ctx, dlg, move |app, dest| {
            let Some(dest) = dest else { return };
            let dest = dest.with_extension(&ext);
            if let Err(e) = std::fs::copy(&src, &dest) {
                app.data_library.status_message = Some((format!("Export failed: {e}"), true));
            } else {
                app.data_library.status_message =
                    Some((format!("Exported to {}", dest.display()), false));
            }
        });
    } else {
        let selected = selected.to_vec();
        let dlg = rfd::FileDialog::new().set_title("Select Destination Folder");
        app.file_dialogs.pick_folder(ctx, dlg, move |app, folder| {
            let Some(folder) = folder else { return };
            let mut count = 0;
            for src in &selected {
                if let Some(fname) = src.file_name() {
                    let dest = folder.join(fname);
                    if std::fs::copy(src, dest).is_ok() {
                        count += 1;
                    }
                }
            }
            app.data_library.status_message = Some((
                format!("Exported {count} items to {}", folder.display()),
                false,
            ));
        });
    }
}

fn export_csv_spectrum(app: &mut LuRaCsApp, ctx: &egui::Context, selected: &[PathBuf]) {
    if selected.is_empty() {
        return;
    }
    if selected.len() == 1 {
        let src = selected[0].clone();
        let default_name = src
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let dlg = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("CSV (*.csv)", &["csv"]);
        app.file_dialogs.save_file(ctx, dlg, move |app, dest| {
            let Some(dest) = dest else { return };
            let dest = dest.with_extension("csv");
            match XmlParser::open(&src, false) {
                Ok(parser) => {
                    if let Some(spec) = parser.get_spectrum() {
                        if let Err(e) = csv_export::spectrum(&spec, &dest) {
                            app.data_library.status_message =
                                Some((format!("Export failed: {e}"), true));
                        } else {
                            app.data_library.status_message =
                                Some((format!("Exported to {}", dest.display()), false));
                        }
                    } else {
                        app.data_library.status_message =
                            Some(("File contains no foreground spectrum".into(), true));
                    }
                }
                Err(e) => {
                    app.data_library.status_message =
                        Some((format!("Failed to read XML: {e}"), true));
                }
            }
        });
    } else {
        let selected = selected.to_vec();
        let dlg = rfd::FileDialog::new().set_title("Select Destination Folder");
        app.file_dialogs.pick_folder(ctx, dlg, move |app, folder| {
            let Some(folder) = folder else { return };
            let mut count = 0;
            for src in &selected {
                if let Ok(parser) = XmlParser::open(src, false)
                    && let Some(spec) = parser.get_spectrum()
                {
                    let stem = src.file_stem().unwrap_or_default();
                    let dest = folder.join(stem).with_extension("csv");
                    if csv_export::spectrum(&spec, dest).is_ok() {
                        count += 1;
                    }
                }
            }
            app.data_library.status_message = Some((
                format!("Exported {count} CSV files to {}", folder.display()),
                false,
            ));
        });
    }
}

fn export_xlsx_spectrum(app: &mut LuRaCsApp, ctx: &egui::Context, selected: &[PathBuf]) {
    if selected.is_empty() {
        return;
    }
    if selected.len() == 1 {
        let src = selected[0].clone();
        let default_name = src
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let dlg = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Excel Workbook (*.xlsx)", &["xlsx"]);
        app.file_dialogs.save_file(ctx, dlg, move |app, dest| {
            let Some(dest) = dest else { return };
            let dest = dest.with_extension("xlsx");
            match XmlParser::open(&src, false) {
                Ok(parser) => {
                    if let Some(spec) = parser.get_spectrum() {
                        if let Err(e) = xlsx_export::spectrum(&spec, &dest) {
                            app.data_library.status_message =
                                Some((format!("Export failed: {e}"), true));
                        } else {
                            app.data_library.status_message =
                                Some((format!("Exported to {}", dest.display()), false));
                        }
                    } else {
                        app.data_library.status_message =
                            Some(("File contains no foreground spectrum".into(), true));
                    }
                }
                Err(e) => {
                    app.data_library.status_message =
                        Some((format!("Failed to read XML: {e}"), true));
                }
            }
        });
    } else {
        let selected = selected.to_vec();
        let dlg = rfd::FileDialog::new().set_title("Select Destination Folder");
        app.file_dialogs.pick_folder(ctx, dlg, move |app, folder| {
            let Some(folder) = folder else { return };
            let mut count = 0;
            for src in &selected {
                if let Ok(parser) = XmlParser::open(src, false)
                    && let Some(spec) = parser.get_spectrum()
                {
                    let stem = src.file_stem().unwrap_or_default();
                    let dest = folder.join(stem).with_extension("xlsx");
                    if xlsx_export::spectrum(&spec, dest).is_ok() {
                        count += 1;
                    }
                }
            }
            app.data_library.status_message = Some((
                format!("Exported {count} XLSX files to {}", folder.display()),
                false,
            ));
        });
    }
}

fn export_xlsx_spectrogram(
    app: &mut LuRaCsApp,
    ctx: &egui::Context,
    selected: &[PathBuf],
    export_spectrum_data: bool,
) {
    if selected.is_empty() {
        return;
    }
    if selected.len() == 1 {
        let src = selected[0].clone();
        let default_name = src
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let dlg = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("Excel Workbook (*.xlsx)", &["xlsx"]);
        app.file_dialogs.save_file(ctx, dlg, move |app, dest| {
            let Some(dest) = dest else { return };
            let dest = dest.with_extension("xlsx");
            match DbParser::open(&src) {
                Ok(parser) => {
                    if let Err(e) =
                        DbWriter::export_full_xlsx(&parser, &dest, export_spectrum_data)
                    {
                        app.data_library.status_message =
                            Some((format!("Export failed: {e}"), true));
                    } else {
                        app.data_library.status_message =
                            Some((format!("Exported to {}", dest.display()), false));
                    }
                }
                Err(e) => {
                    app.data_library.status_message =
                        Some((format!("Failed to read DB: {e}"), true));
                }
            }
        });
    } else {
        let selected = selected.to_vec();
        let dlg = rfd::FileDialog::new().set_title("Select Destination Folder");
        app.file_dialogs.pick_folder(ctx, dlg, move |app, folder| {
            let Some(folder) = folder else { return };
            let mut count = 0;
            for src in &selected {
                if let Ok(parser) = DbParser::open(src) {
                    let stem = src.file_stem().unwrap_or_default();
                    let dest = folder.join(stem).with_extension("xlsx");
                    if DbWriter::export_full_xlsx(&parser, dest, export_spectrum_data).is_ok() {
                        count += 1;
                    }
                }
            }
            app.data_library.status_message = Some((
                format!("Exported {count} XLSX files to {}", folder.display()),
                false,
            ));
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_library_initial_state() {
        let win = DataLibraryWindow::new();
        assert!(!win.open);
        assert_eq!(win.active_tab, DataLibraryTab::Spectrum);
        assert!(win.load_instrument);
        assert!(win.load_rois);
        assert!(!win.export_spectrum_data);
        assert!(!win.has_open_modal());
    }

    #[test]
    fn test_data_library_open_tab() {
        let mut win = DataLibraryWindow::new();
        win.open_tab(0);
        assert!(win.open);
        assert_eq!(win.active_tab, DataLibraryTab::Spectrum);

        win.open_tab(1);
        assert_eq!(win.active_tab, DataLibraryTab::Spectrogram);

        win.open_tab(2);
        assert_eq!(win.active_tab, DataLibraryTab::Roi);

        win.open_tab(3);
        assert_eq!(win.active_tab, DataLibraryTab::UniqueInstruments);

        win.open_tab(4);
        assert_eq!(win.active_tab, DataLibraryTab::GenericInstruments);

        win.open_tab(99);
        assert_eq!(win.active_tab, DataLibraryTab::Spectrum);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0.0), "00:00");
        assert_eq!(format_duration(45.0), "00:45");
        assert_eq!(format_duration(65.0), "01:05");
        assert_eq!(format_duration(3600.0), "01:00:00");
        assert_eq!(format_duration(3665.0), "01:01:05");
    }
}

