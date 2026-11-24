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
//! - The waterfall plot renders the time-series of spectra as a 2D
//!   colormapped (Viridis) image item with inverted Y (newest spectrum at
//!   the top, older spectra below). Cached by Arc pointer identity to
//!   avoid GPU texture re-uploads when data hasn't changed.
//! - Y-axis time labels: rsplot's underlying silx model supports
//!   `TickMode::TimeSeries` on the *X* axis only (`YAxis` does not support
//!   time-series formatting in silx/rsplot), so the Y axis displays row
//!   indices/steps.
//! - The `set_time_selector(False)` Python reset uses `spectrogram[0]`
//!   (the oldest row), which looks like a bug — `latest_spectrum` is more
//!   sensible and matches the `receive_data` path. We use `latest_spectrum`
//!   here.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use egui_histogram_lut::{GradientPreset, HistogramLutState, HistogramLutWidget, Orientation};
use ndarray::Array1;
use rsplot::PlotInteractionMode;
use rsplot::core::items::CurveInterpolation;
use rsplot::{Colormap, ImageGeometry, ItemHandle, LineStyle, Marker, Plot1D, YAxis};

use crate::core::{SpectrogramManager, SpectrogramRoi};
use crate::settings::Settings;
use crate::spectogram::SpectrogramState;
use crate::spectogram::WrappedSpectrogramData;
use crate::theme_manager::{self, ThemeDefinition};
use crate::utils::numerics::{polyval, searchsorted_left_arr};

// ===========================================================================
// Events
// ===========================================================================

#[allow(clippy::enum_variant_names)]
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
    /// Open the Data Library on the given tab (1 = Spectrograms, 2 = ROI data).
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

    // Waterfall image & marker cache (kept so texture/marker rebuilds only fire
    // when data, LUT, or marker toggles change).
    last_image_key: Option<usize>,
    last_bar_zoom_key: Option<(String, usize)>,
    last_zoom_spectrogram: Option<String>,
    last_markers_key: Option<(usize, bool, bool, usize)>,
    selector_handles: Option<(ItemHandle, ItemHandle)>,
    lut_state: HistogramLutState,
    lut_auto: bool,
    lut_dirty: bool,
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
        waterfall_plot.set_y_inverted(true);

        bar_plot.set_interaction_mode(PlotInteractionMode::Select);
        waterfall_plot.set_interaction_mode(PlotInteractionMode::Select);

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
            last_bar_zoom_key: None,
            last_zoom_spectrogram: None,
            last_markers_key: None,
            selector_handles: None,
            lut_state: HistogramLutState::new_with_stops(GradientPreset::Viridis, 4),
            lut_auto: true,
            lut_dirty: false,
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
        theme_def: &ThemeDefinition,
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
        if let Some(name) = &self.selected_spectrogram
            && !spectrogram_manager.spectrogram_registry.contains(name)
        {
            self.selected_spectrogram = None;
        }

        // ---- Left column: controls + info ----
        egui::Panel::left("spectrogram_controls")
            .resizable(true)
            .default_size(280.0)
            .show(ui, |ui| {
                self.render_controls(spectrogram_manager, &mut events, devices, ui);
                ui.separator();
                self.render_info_pane(spectrogram_data, ui);
            });

        // ---- Right LUT sidebar ----
        egui::Panel::right("spectrogram_lut_panel")
            .exact_size(142.0)
            .resizable(false)
            .show(ui, |ui| {
                let h = ui.available_height();
                let resp = HistogramLutWidget::new("spectrogram_lut_widget")
                    .orientation(Orientation::Vertical)
                    .size(egui::vec2(128.0, (h - 36.0).max(120.0)))
                    .show(ui, &mut self.lut_state);
                if resp.levels_changed {
                    self.lut_auto = false;
                    self.lut_dirty = true;
                }
                if resp.gradient_changed {
                    self.lut_dirty = true;
                }
                if ui.checkbox(&mut self.lut_auto, "Auto").changed() && self.lut_auto {
                    self.lut_state.auto_level();
                    self.lut_state.auto_range();
                    self.lut_dirty = true;
                }
            });

        // ---- Central column: bar plot + waterfall ----
        egui::CentralPanel::default().show(ui, |ui| {
            let total_h = ui.available_height();
            let bar_h = (total_h * 0.30).max(80.0);
            let waterfall_h = (total_h - bar_h - 8.0).max(120.0);

            // Bar chart of the latest (or selected-range sum) spectrum.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), bar_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    self.draw_bar_plot(spectrogram_data, theme_def, ui);
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
                        theme_def,
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
            if ui.button("Unload").clicked()
                && let Some(name) = &self.selected_spectrogram
            {
                events.push(SpectrogramScreenEvent::RequestClose {
                    db_name: name.clone(),
                });
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
                if ui.button("Stop").clicked()
                    && let Some(name) = &self.selected_spectrogram
                {
                    events.push(SpectrogramScreenEvent::RequestPause {
                        db_name: name.clone(),
                    });
                }
            });
            ui.add_enabled_ui(is_paused, |ui| {
                if ui.button("Resume").clicked()
                    && let Some(name) = &self.selected_spectrogram
                {
                    events.push(SpectrogramScreenEvent::RequestResume {
                        db_name: name.clone(),
                    });
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

    const fn is_paused(&self, _spectrogram_manager: &SpectrogramManager) -> bool {
        // Pause state isn't stored in the manager's registry (which is just
        // names), so we reflect the local toggle. Replace with a manager
        // lookup once `Spectrogram::paused` is accessible from App.
        false
    }

    // -----------------------------------------------------------------------
    // Info pane
    // -----------------------------------------------------------------------

    fn render_info_pane(
        &self,
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
        theme_def: &theme_manager::ThemeDefinition,
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

        let x: Vec<f64> = (0..heights.len()).map(|i| i as f64).collect();
        self.bar_plot.add_curve_with_interpolation(
            &x,
            &heights,
            theme_def.colors.text,
            CurveInterpolation::StepPost,
        );

        let bar_zoom_key = (name.clone(), heights.len());
        let reset_bar_zoom = self.last_bar_zoom_key.as_ref() != Some(&bar_zoom_key);
        if reset_bar_zoom {
            self.last_bar_zoom_key = Some(bar_zoom_key);
        }
        self.bar_plot.set_auto_reset_zoom(reset_bar_zoom);
        self.bar_plot.show(ui);
    }

    // -----------------------------------------------------------------------
    // Waterfall plot
    // -----------------------------------------------------------------------

    fn draw_waterfall_plot(
        &mut self,
        spectrogram_manager: &SpectrogramManager,
        spectrogram_data: &HashMap<String, Arc<WrappedSpectrogramData>>,
        _theme_def: &ThemeDefinition,
        _settings: &Settings,
        ui: &mut egui::Ui,
    ) {
        let Some(name) = &self.selected_spectrogram else {
            self.waterfall_plot.clear();
            self.last_image_key = None;
            ui.centered_and_justified(|ui| {
                ui.label("Waterfall");
            });
            return;
        };
        let Some(p) = spectrogram_data.get(name) else {
            self.waterfall_plot.clear();
            self.last_image_key = None;
            ui.centered_and_justified(|ui| {
                ui.label("Waiting for data…");
            });
            return;
        };

        let n_rows = p.spectrogram.len();
        let channels = p.spect_channels;

        if n_rows == 0 || channels == 0 {
            self.waterfall_plot.clear();
            self.last_image_key = None;
            self.waterfall_plot.show(ui);
            return;
        }

        let x_cal = energy_axis(p);

        // Upload the spectrogram 2D colormapped image when the data or LUT changes.
        // Row 0 is the newest spectrum (top of the waterfall, adjacent to the bar plot).
        // Row n_rows - 1 is the oldest spectrum (bottom of the waterfall).
        let current_key = Arc::as_ptr(p) as usize;
        let data_changed = self.last_image_key != Some(current_key);
        let is_new_spec = self.last_zoom_spectrogram.as_deref() != Some(name.as_str());
        if !self.pause_visual && (data_changed || self.lut_dirty) {
            self.waterfall_plot.clear_images();

            let mut data = vec![0.0f32; n_rows * channels];
            let mut max_val = 0.0f32;

            for r in 0..n_rows {
                let spec = &p.spectrogram[n_rows - 1 - r];
                let count = spec.len().min(channels);
                let row_offset = r * channels;
                for c in 0..count {
                    let v = spec[c] as f32;
                    data[row_offset + c] = v;
                    if v.is_finite() && v > max_val {
                        max_val = v;
                    }
                }
            }

            if data_changed {
                let stride = (data.len() / 8192).max(1);
                let samples: Vec<f64> =
                    data.iter().step_by(stride).map(|&v| f64::from(v)).collect();
                self.lut_state.set_data(&samples, 48, false);
                if self.lut_auto {
                    let vmax = f64::from(max_val).max(5.0);
                    self.lut_state.set_levels(0.0, vmax);
                    if is_new_spec {
                        self.lut_state.auto_range();
                    }
                }
            }

            let lut_rgba: Vec<[u8; 4]> = self
                .lut_state
                .get_lookup_table(256)
                .into_iter()
                .map(|c| c.to_array())
                .collect();
            let vmin = self.lut_state.levels.0;
            let vmax = self.lut_state.levels.1.max(vmin + 1e-6);
            let colormap = Colormap::from_colors(&lut_rgba, vmin, vmax)
                .unwrap_or_else(|| Colormap::viridis(vmin, vmax));
            let geometry = ImageGeometry {
                origin: (0.0, 0.0),
                scale: (1.0, 1.0),
                alpha: 1.0,
            };

            if let Err(e) = self.waterfall_plot.add_image_with_geometry(
                channels as u32,
                n_rows as u32,
                &data,
                colormap,
                geometry,
            ) {
                tracing::warn!("Failed to add waterfall image: {e}");
            }

            self.last_image_key = Some(current_key);
            self.lut_dirty = false;
        }

        // Sync draggable time-selector markers before checking whether markers need rebuild.
        let max_row = (n_rows - 1) as f64;
        if self.time_selector_enabled
            && let Some((h1, h2)) = self.selector_handles
        {
            if let Some((_, y1)) = self.waterfall_plot.marker_position(h1) {
                self.selector_range.0 = y1.clamp(0.0, max_row);
            }
            if let Some((_, y2)) = self.waterfall_plot.marker_position(h2) {
                self.selector_range.1 = y2.clamp(0.0, max_row);
            }
        }

        // Rebuild markers only when data/toggles/ROIs change so active marker drags persist.
        let markers_key = (
            current_key,
            self.show_break_lines,
            self.time_selector_enabled,
            spectrogram_manager.roi_registry.len(),
        );
        if self.last_markers_key != Some(markers_key) {
            self.waterfall_plot.clear_markers();
            self.selector_handles = None;

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
                    if let Some(pt) = prev
                        && (t - pt).abs() > f64::mul_add(expected_dt, 1.5, 1.0)
                    {
                        let row = (n_rows - 1) as f64 - prev_idx as f64;
                        break_rows.push(row.max(0.0));
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

            // Draggable time selector horizontal lines on the waterfall plot.
            if self.time_selector_enabled {
                let mut m1 = Marker::hline(self.selector_range.0.clamp(0.0, max_row))
                    .with_line_style(LineStyle::Solid)
                    .with_line_width(2.0)
                    .with_draggable(true);
                m1.color = Color32::from_rgb(80, 200, 255);
                let h1 = self.waterfall_plot.add_marker_data(&m1);

                let mut m2 = Marker::hline(self.selector_range.1.clamp(0.0, max_row))
                    .with_line_style(LineStyle::Solid)
                    .with_line_width(2.0)
                    .with_draggable(true);
                m2.color = Color32::from_rgb(80, 200, 255);
                let h2 = self.waterfall_plot.add_marker_data(&m2);
                self.selector_handles = Some((h1, h2));
            }

            // ROI bands: vertical lines at each ROI's channel bounds on the energy axis.
            if let Some(x_axis) = &x_cal {
                let color = Color32::from_rgb(255, 200, 0);
                for roi in spectrogram_manager.roi_registry.values() {
                    let lo = searchsorted_left_arr(x_axis, roi.e_region.0);
                    let hi = searchsorted_left_arr(x_axis, roi.e_region.1);
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

            self.last_markers_key = Some(markers_key);
        } else if self.time_selector_enabled
            && let Some((h1, h2)) = self.selector_handles
        {
            self.waterfall_plot.set_marker_position(
                h1,
                0.0,
                self.selector_range.0.clamp(0.0, max_row),
            );
            self.waterfall_plot.set_marker_position(
                h2,
                0.0,
                self.selector_range.1.clamp(0.0, max_row),
            );
        }

        // Enforce aspect-lock, bounds, and inverted Y (newest row at the top).
        self.waterfall_plot.set_x_min_pos(Some(0.0));
        self.waterfall_plot.set_x_max_pos(Some(channels as f64));
        self.waterfall_plot.set_y_min_pos(Some(0.0));
        self.waterfall_plot.set_y_max_pos(Some(n_rows as f64));
        self.waterfall_plot.set_y_inverted(true);
        self.waterfall_plot
            .set_keep_data_aspect_ratio(self.keep_aspect);

        self.waterfall_plot.set_auto_reset_zoom(is_new_spec);
        if is_new_spec {
            self.last_zoom_spectrogram = Some(name.clone());
        }
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

    fn render_start_dialog(&mut self, events: &mut Vec<SpectrogramScreenEvent>, ui: &egui::Ui) {
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

    fn render_export_dialog(&mut self, events: &mut Vec<SpectrogramScreenEvent>, ui: &egui::Ui) {
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
            } else if let Some(range) = dialog.row_range {
                events.push(SpectrogramScreenEvent::RequestExportSelection {
                    db_name: dialog.db_name.clone(),
                    spectrum_name: dialog.spectrum_name.clone(),
                    remark: dialog.remark.clone(),
                    row_range: range,
                });
            } else {
                events.push(SpectrogramScreenEvent::RequestExportAccumulated {
                    db_name: dialog.db_name.clone(),
                    spectrum_name: dialog.spectrum_name.clone(),
                    remark: dialog.remark.clone(),
                });
            }
            open = false;
        }

        if open {
            self.export_dialog = Some(dialog);
        }
    }

    fn render_confirm_clear_rois(
        &mut self,
        _spectrogram_manager: &SpectrogramManager,
        events: &mut Vec<SpectrogramScreenEvent>,
        ui: &egui::Ui,
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
