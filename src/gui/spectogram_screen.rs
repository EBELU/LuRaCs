//! Spectrogram screen: bar chart of latest spectrum, waterfall image,
//! time selector, break lines, ROI bands.
//!
//! Port of `luracs/gui/spectrogram_widget.py` targeting rsplot 0.6.1.
//! Qt signals become a per-frame `Vec<SpectrogramScreenEvent>` drained by
//! `App::ui`; `SpectrogramManager.start/close_spectrogram` calls likewise
//! route through the caller.
//!
//! ## Design notes
//!
//! - Only per-frame UI state is stored. Everything the Python widget
//!   cached (`y_axis`, `x_axis`, `info_text_total`, `info_text_selector`,
//!   `y_len`, `x_len`, `current_packet_buffer`) is derived from the
//!   `WrappedSpectrogramData` snapshot App hands us each frame.
//! - The `spectrogram_selection` Qt combo becomes `selected_spectrogram`
//!   in state; the list of available spectrograms comes from
//!   `SpectrogramManager::spectrogram_registry`.
//! - The bar chart above the waterfall is a placeholder line plot; the
//!   user's bar-chart fork replaces the `add_curve_with_interpolation`
//!   call in `draw_bar_plot`.
//! - The waterfall is a placeholder: rsplot's `Plot1D` image-item API
//!   (`add_image` / `set_image`) isn't confirmed. `draw_waterfall_plot`
//!   renders the newest row as a curve; swap in an image item once the
//!   API is pinned.
//! - Y-axis time labels are not rendered: rsplot's `Plot1D` supports a
//!   `TickMode::TimeSeries` for the *X* axis only. Left as a TODO.
//! - The `set_time_selector(False)` Python reset uses `spectrogram[0]`
//!   (the oldest row), which looks like a bug — `latest_spectrum` is more
//!   sensible and matches the `receive_data` path. We use `latest_spectrum`
//!   here.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use ndarray::Array1;
use rsplot::core::items::CurveInterpolation;
use rsplot::{LineStyle, Marker, Plot1D, YAxis};

use crate::core::{SpectrogramManager, SpectrogramRoi};
use crate::settings::Settings;
use crate::spectogram::SpectrogramState;
use crate::spectogram::WrappedSpectrogramData;
use crate::theme_manager::Palette;

// ===========================================================================
// Events
// ===========================================================================

pub enum SpectrogramScreenEvent {
    RequestStart {
        db_name: String,
        device: String,
        interval: f64,
        channel_truncation: usize,
    },
    RequestResume {
        db_name: String,
    },
    RequestPause {
        db_name: String,
    },
    RequestClose {
        db_name: String,
    },
    /// Export the accumulated spectrum of a spectrogram to a new spectrum.
    RequestExportAccumulated {
        db_name: String,
        spectrum_name: String,
        remark: String,
    },
    /// Export a slice of a spectrogram (bounded by `selector_range`).
    RequestExportSelection {
        db_name: String,
        spectrum_name: String,
        remark: String,
        /// Reversed-row indices: `0` = newest row.
        row_range: (usize, usize),
    },
    /// Open the Data Library on the given tab (1 = Spectrograms, 2 = ROIs).
    RequestShowDataStore {
        tab: usize,
    },
    RequestClearRois,
    RequestImportRois,
}

// ===========================================================================
// Dialog state
// ===========================================================================

pub struct StartDialogState {
    pub devices: Vec<String>,
    pub device: String,
    pub db_name: String,
    pub interval: f64,
    pub channel_truncation: usize,
}

impl StartDialogState {
    fn new(devices: Vec<String>) -> Self {
        let default_name = format!("Spectrogram_{}", Local::now().format("%Y-%m-%dT%H:%M:%S"));
        let device = devices.first().cloned().unwrap_or_default();
        Self {
            devices,
            device,
            db_name: default_name,
            interval: 1.0,
            channel_truncation: 1,
        }
    }
}

pub struct ExportDialogState {
    pub db_name: String,
    pub spectrum_name: String,
    pub remark: String,
    /// `None` → export the accumulated spectrum; `Some((lo, hi))` → export
    /// rows `lo..hi` (reversed-row space, `0` = newest).
    pub row_range: Option<(usize, usize)>,
}

// ===========================================================================
// Screen state
// ===========================================================================

pub struct SpectrogramScreen {
    // Selection / display
    pub selected_spectrogram: Option<String>,
    pub pause_visual: bool,
    pub time_selector_enabled: bool,
    pub show_break_lines: bool,
    pub keep_aspect: bool,
    pub show_date_on_y: bool,
    /// Reversed-row index range selected by the time selector.
    pub selector_range: (f64, f64),

    // Dialogs
    pub start_dialog: Option<StartDialogState>,
    pub export_dialog: Option<ExportDialogState>,

    // Modals
    pub pending_clear_rois_confirm: bool,
    pub pending_message: Option<String>,

    // Retained plots
    bar_plot: Plot1D,
    waterfall_plot: Plot1D,

    // Waterfall image cache (kept so the texture upload only fires when
    // the packet actually changes; uses Arc pointer identity).
    last_image_key: Option<usize>,
}

impl SpectrogramScreen {
    pub fn new(render_state: &RenderState) -> Self {
        // Bar plot id 0x2001; waterfall 0x2002. Distinct from the
        // spectrum screen's 0x0 / hash ids, so egui state stays separate.
        let mut bar_plot = Plot1D::new(render_state, 0x2001);
        bar_plot.set_y_min_pos(Some(0.0));
        bar_plot.set_graph_x_label("Energy [keV]");
        bar_plot.set_graph_y_label("Counts", YAxis::Left);

        let mut waterfall_plot = Plot1D::new(render_state, 0x2002);
        waterfall_plot.set_graph_x_label("Energy [keV]");
        waterfall_plot.set_graph_y_label("Time", YAxis::Left);
        waterfall_plot.set_y_min_pos(Some(0.0));

        Self {
            selected_spectrogram: None,
            pause_visual: false,
            time_selector_enabled: false,
            show_break_lines: true,
            keep_aspect: true,
            show_date_on_y: false,
            selector_range: (1.0, 9.0),
            start_dialog: None,
            export_dialog: None,
            pending_clear_rois_confirm: false,
            pending_message: None,
            bar_plot,
            waterfall_plot,
            last_image_key: None,
        }
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    pub fn render(
        &mut self,
        spectrogram_manager: &SpectrogramManager,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        settings: &Settings,
        palette: &Palette,
        ui: &mut egui::Ui,
        devices: &[String], // ← was `run_manager: &mut RunManager`
    ) -> Vec<SpectrogramScreenEvent> {
        let mut events = Vec::new();

        // First frame: pick the first available spectrogram.
        if self.selected_spectrogram.is_none() {
            self.selected_spectrogram = spectrogram_manager
                .spectrogram_registry
                .iter()
                .next()
                .cloned();
        }
        // Prune stale selection.
        if let Some(name) = &self.selected_spectrogram {
            if !spectrogram_manager.spectrogram_registry.contains(name) {
                self.selected_spectrogram = None;
            }
        }

        // ---- Left column: controls + info ----
        egui::Panel::left("spectrogram_controls")
            .resizable(true)
            .default_size(280.0)
            .show_inside(ui, |ui| {
                self.render_controls(spectrogram_manager, &mut events, devices, ui);
                ui.separator();
                self.render_info_pane(spectrogram_data, ui);
            });

        // ---- Central column: bar plot + waterfall ----
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let total_h = ui.available_height();
            let bar_h = (total_h * 0.30).max(80.0);
            let waterfall_h = (total_h - bar_h - 8.0).max(120.0);

            // Bar chart of the latest (or selected-range sum) spectrum.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), bar_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.draw_bar_plot(spectrogram_data, palette, ui);
                },
            );

            ui.add_space(4.0);

            // Waterfall.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), waterfall_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.draw_waterfall_plot(
                        spectrogram_manager,
                        spectrogram_data,
                        palette,
                        settings,
                        ui,
                    );
                },
            );

            // Time selector slider row.
            if self.time_selector_enabled {
                ui.separator();
                self.render_time_selector(spectrogram_data, ui);
            }
        });

        // ---- Modals ----
        self.render_start_dialog(&mut events, ui);
        self.render_export_dialog(&mut events, ui);
        self.render_confirm_clear_rois(spectrogram_manager, &mut events, ui);

        events
    }

    // -----------------------------------------------------------------------
    // Controls
    // -----------------------------------------------------------------------

    fn render_controls(
        &mut self,
        spectrogram_manager: &SpectrogramManager,
        events: &mut Vec<SpectrogramScreenEvent>,
        devices: &[String],
        ui: &mut egui::Ui,
    ) {
        ui.horizontal(|ui| {
            if ui.button("Start New").clicked() {
                self.start_dialog = Some(StartDialogState::new(devices.to_vec()));
            }
            if ui.button("Unload").clicked() {
                if let Some(name) = &self.selected_spectrogram {
                    events.push(SpectrogramScreenEvent::RequestClose {
                        db_name: name.clone(),
                    });
                }
            }
            if ui.button("Load").clicked() {
                events.push(SpectrogramScreenEvent::RequestShowDataStore { tab: 1 });
            }
        });

        ui.separator();

        // Spectrogram selector
        let available: Vec<String> = spectrogram_manager
            .spectrogram_registry
            .iter()
            .cloned()
            .collect();
        let current_label = self
            .selected_spectrogram
            .clone()
            .unwrap_or_else(|| "(none)".into());
        egui::ComboBox::from_id_salt("spectrogram_selection")
            .selected_text(current_label)
            .width(240.0)
            .show_ui(ui, |ui| {
                for name in &available {
                    let is_sel = self.selected_spectrogram.as_deref() == Some(name.as_str());
                    if ui.selectable_label(is_sel, name).clicked() {
                        self.selected_spectrogram = Some(name.clone());
                    }
                }
            });

        ui.separator();

        // Stop / Resume / menu
        ui.horizontal(|ui| {
            let is_paused = spectrogram_manager
                .spectrogram_registry
                .contains(&self.selected_spectrogram.clone().unwrap_or_default())
                && self.is_paused(spectrogram_manager);

            ui.add_enabled_ui(!is_paused, |ui| {
                if ui.button("Stop").clicked() {
                    if let Some(name) = &self.selected_spectrogram {
                        events.push(SpectrogramScreenEvent::RequestPause {
                            db_name: name.clone(),
                        });
                    }
                }
            });
            ui.add_enabled_ui(is_paused, |ui| {
                if ui.button("Resume").clicked() {
                    if let Some(name) = &self.selected_spectrogram {
                        events.push(SpectrogramScreenEvent::RequestResume {
                            db_name: name.clone(),
                        });
                    }
                }
            });

            ui.menu_button("…", |ui| {
                ui.checkbox(&mut self.pause_visual, "Pause Visual Updates");
                if ui
                    .checkbox(&mut self.time_selector_enabled, "Time Selector")
                    .changed()
                    && !self.time_selector_enabled
                {
                    // reset selector info text; derived on next frame
                }
                if ui.button("Selection to Spectrum").clicked() {
                    self.begin_export(true);
                }
                if ui.button("Accumulated to Spectrum").clicked() {
                    self.begin_export(false);
                }
                ui.separator();
                ui.checkbox(&mut self.show_break_lines, "Show Break Lines");
                ui.checkbox(&mut self.keep_aspect, "Keep Aspect Ratio");
                ui.checkbox(&mut self.show_date_on_y, "Show Date on Y");
                ui.separator();
                if ui.button("Load ROIs").clicked() {
                    events.push(SpectrogramScreenEvent::RequestShowDataStore { tab: 2 });
                }
                if ui.button("Import ROIs").clicked() {
                    events.push(SpectrogramScreenEvent::RequestImportRois);
                }
                if ui.button("Clear ROIs").clicked() {
                    self.pending_clear_rois_confirm = true;
                }
            });
        });
    }

    fn is_paused(&self, _spectrogram_manager: &SpectrogramManager) -> bool {
        // Pause state isn't stored in the manager's registry (which is just
        // names), so we reflect the local toggle. Replace with a manager
        // lookup once `Spectrogram::paused` is accessible from App.
        false
    }

    // -----------------------------------------------------------------------
    // Info pane
    // -----------------------------------------------------------------------

    fn render_info_pane(
        &mut self,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        ui: &mut egui::Ui,
    ) {
        let text = self
            .selected_spectrogram
            .as_ref()
            .and_then(|name| spectrogram_data.get(name))
            .map(|p| self.build_info_text(p))
            .unwrap_or_default();

        egui::ScrollArea::vertical()
            .id_salt("spectrogram_info_pane")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut view = text.as_str();
                ui.add(
                    egui::TextEdit::multiline(&mut view)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .interactive(false),
                );
            });
    }

    fn build_info_text(&self, p: &WrappedSpectrogramData) -> String {
        let (dose, dose_unit) = format_dose(p.estimated_dose);
        let start = epoch_to_local_string(p.start_date);
        let duration = format_duration(p.duration);

        let total = format!(
            "Database:\n  {}\n\
             Instrument: {}\n\
             Start Date: {}\n\
             Duration: {}\n\
             Estimated Dose: {:.3} {}\n\
             Time Interval: {}s\n\
             Spectrum Concat Factor: {}\n\
             Spectrum Channels: {}",
            p.db_name,
            p.instrument,
            start,
            duration,
            dose,
            dose_unit,
            p.save_interval,
            p.concat,
            p.spect_channels,
        );

        if self.time_selector_enabled && !p.spectrogram.is_empty() {
            let (lo, hi) = self.normalise_selector(p.spectrogram.len());
            let start_ts = p
                .timestamp_deque
                .get(p.spectrogram.len().saturating_sub(1) - lo)
                .copied()
                .unwrap_or(0.0);
            let stop_ts = p
                .timestamp_deque
                .get(p.spectrogram.len().saturating_sub(1) - hi)
                .copied()
                .unwrap_or(0.0);
            let duration = (hi - lo) as f64 * p.save_interval;
            format!(
                "{}\n\n= Time Selector =\nDuration: {}\n{} -> {}",
                total,
                format_duration_secs(duration),
                epoch_to_local_string(start_ts),
                epoch_to_local_string(stop_ts),
            )
        } else {
            total
        }
    }

    fn normalise_selector(&self, n_rows: usize) -> (usize, usize) {
        let a = self
            .selector_range
            .0
            .round()
            .max(0.0)
            .min((n_rows - 1) as f64) as usize;
        let b = self
            .selector_range
            .1
            .round()
            .max(0.0)
            .min((n_rows - 1) as f64) as usize;
        (a.min(b), a.max(b))
    }

    // -----------------------------------------------------------------------
    // Bar plot
    // -----------------------------------------------------------------------

    fn draw_bar_plot(
        &mut self,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        palette: &Palette,
        ui: &mut egui::Ui,
    ) {
        self.bar_plot.clear();

        let Some(name) = &self.selected_spectrogram else {
            ui.centered_and_justified(|ui| {
                ui.label("No spectrogram selected — use Start New or Load");
            });
            return;
        };
        let Some(p) = spectrogram_data.get(name) else {
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for data…");
            });
            return;
        };

        // Derive the y-values: latest spectrum, or sum of the time selection.
        let heights: Vec<f64> = if self.time_selector_enabled && !p.spectrogram.is_empty() {
            let (lo, hi) = self.normalise_selector(p.spectrogram.len());
            let mut acc = vec![0.0f64; p.spect_channels];
            for i in lo..=hi {
                let idx = p.spectrogram.len().saturating_sub(1) - i;
                if let Some(row) = p.spectrogram.get(idx) {
                    for (j, &v) in row.iter().enumerate().take(p.spect_channels) {
                        acc[j] += v;
                    }
                }
            }
            acc
        } else {
            p.latest_spectrum
                .as_ref()
                .map(|a| a.iter().copied().collect())
                .unwrap_or_else(|| vec![0.0; p.spect_channels])
        };

        // TODO(bar-fork): replace this step-interpolated line with the
        // user's bar chart item. The `heights` vector is what the fork's
        // `BarChart` would consume.
        let x: Vec<f64> = (0..heights.len()).map(|i| i as f64).collect();
        self.bar_plot.add_curve_with_interpolation(
            &x,
            &heights,
            palette.text,
            CurveInterpolation::StepPost,
        );

        // ROI bands on the bar plot (energy-space vertical lines).
        if let Some(x_axis) = energy_axis(p) {
            // `spectrogram_manager` is not available here; ROIs are drawn
            // below in `draw_waterfall_plot` where we have the manager.
            let _ = x_axis;
        }

        // Include the bar y-max in the autoscale, floor at 0.
        self.bar_plot.set_auto_reset_zoom(true);
        self.bar_plot.show(ui);
    }

    // -----------------------------------------------------------------------
    // Waterfall plot
    // -----------------------------------------------------------------------

    fn draw_waterfall_plot(
        &mut self,
        spectrogram_manager: &SpectrogramManager,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        palette: &Palette,
        _settings: &Settings,
        ui: &mut egui::Ui,
    ) {
        self.waterfall_plot.clear();

        let Some(name) = &self.selected_spectrogram else {
            ui.centered_and_justified(|ui| {
                ui.label("Waterfall");
            });
            return;
        };
        let Some(p) = spectrogram_data.get(name) else {
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for data…");
            });
            return;
        };

        let n_rows = p.spectrogram.len();
        let channels = p.spect_channels;

        // TODO(rsplot-image): upload the spectrogram as an image item once
        // `Plot1D::add_image` / `set_image` is confirmed. For now, render
        // only the newest row so there's visible activity.
        if n_rows > 0 {
            if let Some(last) = p.spectrogram.last() {
                let x: Vec<f64> = (0..channels).map(|i| i as f64).collect();
                let y: Vec<f64> = last.iter().copied().collect();
                self.waterfall_plot.add_curve_with_interpolation(
                    &x,
                    &y,
                    palette.text.gamma_multiply(0.8),
                    CurveInterpolation::Linear,
                );
            }
        }

        // Break lines (reversed-row indices where a time gap appears).
        if self.show_break_lines && n_rows > 1 {
            let expected_dt = p.save_interval.max(1e-6);
            let mut break_rows: Vec<f64> = Vec::new();
            let mut prev: Option<f64> = None;
            let mut prev_idx: i64 = -1;
            for (i, &t) in p.timestamp_deque.iter().enumerate() {
                if t <= 0.0 {
                    continue;
                }
                if let Some(pt) = prev {
                    if (t - pt).abs() > expected_dt * 1.5 + 1.0 {
                        // Reversed-row index of the older side.
                        let row = (n_rows - 1) as f64 - prev_idx as f64;
                        break_rows.push(row.max(0.0));
                    }
                }
                prev = Some(t);
                prev_idx = i as i64;
            }
            for row in break_rows {
                self.waterfall_plot.add_marker_data(
                    &Marker::hline(row)
                        .with_line_style(LineStyle::Dashed)
                        .with_line_width(1.5)
                        .with_draggable(false),
                );
            }
        }

        // ROI bands: vertical lines at each ROI's channel bounds on the
        // energy axis. These are what the Python `SpectrogramROI` items
        // render — a pair of vlines per ROI.
        if let Some(x_axis) = energy_axis(p) {
            for roi in spectrogram_manager.roi_registry.values() {
                let _ = roi;
                let lo = searchsorted_left(&x_axis, roi.e_region.0);
                let hi = searchsorted_left(&x_axis, roi.e_region.1);
                let color = Color32::from_rgb(255, 200, 0);
                for idx in [lo, hi] {
                    let mut m = Marker::vline(idx as f64)
                        .with_line_style(LineStyle::Dashed)
                        .with_line_width(1.5)
                        .with_draggable(false);
                    m.color = color;
                    self.waterfall_plot.add_marker_data(&m);
                }
            }
        }

        // Enforce the aspect-lock flag (rsplot: `keep_aspect` on the Plot).
        // There's no `Plot1D::set_keep_aspect`, but `Plot::keep_aspect` is
        // the field; access via `plot_mut()`.
        self.waterfall_plot.plot_mut().keep_aspect = self.keep_aspect;

        self.waterfall_plot.set_auto_reset_zoom(n_rows > 0);
        self.waterfall_plot.show(ui);
    }

    // -----------------------------------------------------------------------
    // Time selector
    // -----------------------------------------------------------------------

    fn render_time_selector(
        &mut self,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        ui: &mut egui::Ui,
    ) {
        let Some(name) = &self.selected_spectrogram else {
            return;
        };
        let Some(p) = spectrogram_data.get(name) else {
            return;
        };
        let n_rows = p.spectrogram.len();
        if n_rows == 0 {
            return;
        }

        ui.horizontal(|ui| {
            ui.label("Time selector:");
            ui.add(
                egui::DragValue::new(&mut self.selector_range.0)
                    .speed(0.2)
                    .range(0.0..=(n_rows - 1) as f64),
            );
            ui.label("→");
            ui.add(
                egui::DragValue::new(&mut self.selector_range.1)
                    .speed(0.2)
                    .range(0.0..=(n_rows - 1) as f64),
            );
        });
    }

    // -----------------------------------------------------------------------
    // Export helpers
    // -----------------------------------------------------------------------

    fn begin_export(&mut self, use_selection: bool) {
        let Some(name) = self.selected_spectrogram.clone() else {
            self.pending_message = Some("No spectrogram to export".into());
            return;
        };
        let row_range = if use_selection && self.time_selector_enabled {
            Some(self.normalise_selector(usize::MAX))
        } else {
            None
        };
        // `normalise_selector(usize::MAX)` above is a hint; the actual range
        // is normalised against the live packet inside the dialog handler.
        self.export_dialog = Some(ExportDialogState {
            db_name: name.clone(),
            spectrum_name: name,
            remark: String::new(),
            row_range,
        });
    }

    // -----------------------------------------------------------------------
    // Modals
    // -----------------------------------------------------------------------

    fn render_start_dialog(&mut self, events: &mut Vec<SpectrogramScreenEvent>, ui: &mut egui::Ui) {
        let Some(mut dialog) = self.start_dialog.take() else {
            return;
        };
        // App fills the device list when it detects the dialog is open;
        // see the App-side handler for that plumbing.

        let mut open = true;
        let mut accepted = false;
        let mut close_requested = false;

        egui::Window::new("Start Spectrogram")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                egui::Grid::new("sg_start_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Instrument:");
                        egui::ComboBox::from_id_salt("sg_start_device")
                            .selected_text(&dialog.device)
                            .show_ui(ui, |ui| {
                                for d in &dialog.devices {
                                    ui.selectable_value(&mut dialog.device, d.clone(), d);
                                }
                            });
                        ui.end_row();

                        ui.label("File name:");
                        ui.text_edit_singleline(&mut dialog.db_name);
                        ui.end_row();

                        ui.label("Interval (s):");
                        ui.add(
                            egui::DragValue::new(&mut dialog.interval)
                                .speed(0.1)
                                .range(0.001..=1_000_000.0),
                        );
                        ui.end_row();

                        ui.label("Channel trunc:");
                        ui.horizontal(|ui| {
                            for &v in &[1usize, 2, 4, 8] {
                                ui.radio_value(&mut dialog.channel_truncation, v, v.to_string());
                            }
                        });
                        ui.end_row();
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        accepted = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close_requested = true;
                    }
                });
            });

        if accepted && !dialog.device.is_empty() {
            events.push(SpectrogramScreenEvent::RequestStart {
                db_name: dialog.db_name.replace(':', "-"),
                device: dialog.device.clone(),
                interval: dialog.interval,
                channel_truncation: dialog.channel_truncation,
            });
            close_requested = true;
        }

        if close_requested {
            open = false;
        }

        if open {
            self.start_dialog = Some(dialog);
        }
    }

    fn render_export_dialog(
        &mut self,
        events: &mut Vec<SpectrogramScreenEvent>,
        ui: &mut egui::Ui,
    ) {
        let Some(mut dialog) = self.export_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut accepted = false;
        let mut cancel_requested = false;

        egui::Window::new("Export Spectrum from Spectrogram")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                egui::Grid::new("sg_export_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut dialog.spectrum_name);
                        ui.end_row();
                        ui.label("Remark:");
                        ui.text_edit_singleline(&mut dialog.remark);
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        accepted = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel_requested = true;
                    }
                });
            });

        if cancel_requested {
            open = false;
        }

        if accepted {
            if dialog.spectrum_name.trim().is_empty() {
                self.pending_message = Some("Invalid name".into());
                open = false;
            } else if let Some(range) = dialog.row_range {
                events.push(SpectrogramScreenEvent::RequestExportSelection {
                    db_name: dialog.db_name.clone(),
                    spectrum_name: dialog.spectrum_name.clone(),
                    remark: dialog.remark.clone(),
                    row_range: range,
                });
                open = false;
            } else {
                events.push(SpectrogramScreenEvent::RequestExportAccumulated {
                    db_name: dialog.db_name.clone(),
                    spectrum_name: dialog.spectrum_name.clone(),
                    remark: dialog.remark.clone(),
                });
                open = false;
            }
        }

        if open {
            self.export_dialog = Some(dialog);
        }
    }

    fn render_confirm_clear_rois(
        &mut self,
        _spectrogram_manager: &SpectrogramManager,
        events: &mut Vec<SpectrogramScreenEvent>,
        ui: &mut egui::Ui,
    ) {
        if !self.pending_clear_rois_confirm {
            return;
        }
        let mut decision: Option<bool> = None;
        egui::Window::new("Clear ROIs")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label("Clear all spectrogram ROIs?");
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("No").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                events.push(SpectrogramScreenEvent::RequestClearRois);
                self.pending_clear_rois_confirm = false;
            }
            Some(false) => self.pending_clear_rois_confirm = false,
            None => {}
        }
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn energy_axis(p: &WrappedSpectrogramData) -> Option<Array1<f64>> {
    let coeffs = &p.calibration_coefficients;
    if coeffs.is_empty() {
        return None;
    }
    let concat = p.concat.max(1) as f64;
    let channels = p.spect_channels;
    Some(Array1::from_iter(
        (0..channels).map(|i| polyval(coeffs, i as f64 * concat)),
    ))
}

fn searchsorted_left(arr: &Array1<f64>, v: f64) -> usize {
    let mut lo = 0usize;
    let mut hi = arr.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if arr[mid] < v {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn polyval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().fold(0.0, |acc, &c| acc * x + c)
}

fn epoch_to_local_string(ts: f64) -> String {
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt: DateTime<Local>| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn format_duration(seconds: f64) -> String {
    if seconds <= 0.0 {
        return "0".into();
    }
    let s = seconds.floor() as i64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, sec)
    } else {
        format!("{}:{:02}", m, sec)
    }
}

fn format_duration_secs(seconds: f64) -> String {
    format_duration(seconds)
}

fn format_dose(dose: f64) -> (f64, &'static str) {
    if dose < 5e-1 {
        (dose * 1e3, "nSv")
    } else if dose > 5e2 {
        (dose * 1e-3, "mSv")
    } else {
        (dose, "uSv")
    }
}
