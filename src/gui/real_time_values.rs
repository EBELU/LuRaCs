//! Real-time values tab: side-by-side CPS and dose-rate strip charts
//! (X inverted so newest is on the right), plus a per-device table.
//!
//! Port of `luracs/gui/tabs/real_time_values.py`. Qt's per-device `PlotDataItem`
//! and `InfiniteLine` are gone — rsplot is immediate-mode, so the plots are
//! rebuilt each frame from `cps_buffers` / `dr_buffers`. Per-device color and
//! buffer state live on the tab and are updated by `App::handle_run_manager_event`
//! when `RealTimeBuffersUpdated` / `DeviceConnected` / `DeviceRemoved` events
//! arrive.

use std::collections::HashMap;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use rsplot::core::items::CurveInterpolation;
use rsplot::{LineStyle, Marker, Plot1D, YAxis};

use crate::gui::color_rotator::{ColorRotator, ColorScheme};
use crate::settings::Settings;

pub struct RealTimeValuesTab {
    cps_plot: Plot1D,
    dr_plot: Plot1D,

    /// Per-device line color, assigned on first appearance.
    colors: HashMap<String, Color32>,
    color_rotator: ColorRotator,

    /// Current buffer per device. Populated by `update_buffers`.
    pub cps_buffers: HashMap<String, Vec<f64>>,
    pub dr_buffers: HashMap<String, Vec<f64>>,
}

impl RealTimeValuesTab {
    pub fn new(render_state: &RenderState, scheme: ColorScheme) -> Self {
        // IDs 0x3001 / 0x3002 — distinct from the spectrum (0x0000+h)
        // and spectrogram (0x2001/0x2002) plot ids.
        let mut cps_plot = Plot1D::new(render_state, 0x3001);
        cps_plot.set_graph_x_label("Time [s]");
        cps_plot.set_graph_y_label("CPS", YAxis::Left);
        cps_plot.plot_mut().x_inverted = true;
        cps_plot.set_x_min_pos(Some(0.0));

        let mut dr_plot = Plot1D::new(render_state, 0x3002);
        dr_plot.set_graph_x_label("Time [s]");
        dr_plot.set_graph_y_label("Dose Rate [μSv/h]", YAxis::Left);
        dr_plot.plot_mut().x_inverted = true;
        dr_plot.set_x_min_pos(Some(0.0));

        Self {
            cps_plot,
            dr_plot,
            colors: HashMap::new(),
            color_rotator: ColorRotator::new(scheme),
            cps_buffers: HashMap::new(),
            dr_buffers: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Event hooks (called from App::handle_run_manager_event)
    // -----------------------------------------------------------------------

    pub fn device_connected(&mut self, name: &str) {
        self.ensure_color(name);
        self.cps_buffers.entry(name.to_string()).or_default();
        self.dr_buffers.entry(name.to_string()).or_default();
    }

    pub fn device_removed(&mut self, name: &str) {
        self.colors.remove(name);
        self.cps_buffers.remove(name);
        self.dr_buffers.remove(name);
    }

    pub fn update_buffers(&mut self, name: &str, cps: Vec<f64>, dr: Vec<f64>) {
        self.ensure_color(name);
        self.cps_buffers.insert(name.to_string(), cps);
        self.dr_buffers.insert(name.to_string(), dr);
    }

    fn ensure_color(&mut self, name: &str) -> Color32 {
        if let Some(&c) = self.colors.get(name) {
            return c;
        }
        let c = self.color_rotator.next_color();
        self.colors.insert(name.to_string(), c);
        c
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    pub fn render(&mut self, show_mean_lines: bool, settings: &Settings, ui: &mut egui::Ui) {
        let queue_len = settings.advanced.real_time_values_deque_length;
        let dt = settings.advanced.update_loop_delay;
        let x_max = queue_len as f64 * dt;

        // X axis: time-ago, i.e. [(n-1)*dt, ..., dt, 0]. Oldest value at
        // index 0 (left edge after X inversion), newest at index n-1
        // (right edge).
        let x_axis: Vec<f64> = (0..queue_len)
            .map(|i| (queue_len - 1 - i) as f64 * dt)
            .collect();

        // Ensure every device with a buffer has a color (safety net in case
        // an update arrived before its `DeviceConnected` event was drained).
        for name in self.cps_buffers.keys().cloned().collect::<Vec<_>>() {
            self.ensure_color(&name);
        }
        for name in self.dr_buffers.keys().cloned().collect::<Vec<_>>() {
            self.ensure_color(&name);
        }

        // Shared Y limits across all devices so both plots stay proportional.
        let y_max_cps = self
            .cps_buffers
            .values()
            .flat_map(|b| b.iter().copied())
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(0.0_f64, f64::max)
            .max(1.0);
        let y_max_dr = self
            .dr_buffers
            .values()
            .flat_map(|b| b.iter().copied())
            .filter(|v| v.is_finite() && *v > 0.0)
            .fold(0.0_f64, f64::max)
            .max(0.01);

        // Pin the view. X is fixed (0..x_max) so the scroll window is
        // stable; Y auto-fits the data each frame (pyqtgraph does the same
        // by default).
        self.cps_plot.plot_mut().limits = (0.0, x_max, 0.0, y_max_cps * 1.10);
        self.dr_plot.plot_mut().limits = (0.0, x_max, 0.0, y_max_dr * 1.10);

        // Two plots share 2/3 of the row; table gets the remaining 1/3.
        let total_w = ui.available_width();
        let total_h = ui.available_height();
        let plots_w = (total_w * 0.66).max(200.0);
        let half = (plots_w - 6.0) * 0.5;
        let table_w = (total_w - plots_w - 12.0).max(160.0);

        // Split borrows so the closures can each take a disjoint field.
        let cps_plot = &mut self.cps_plot;
        let dr_plot = &mut self.dr_plot;
        let colors = &self.colors;
        let cps_bufs = &self.cps_buffers;
        let dr_bufs = &self.dr_buffers;

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(half, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    draw_strip_plot(cps_plot, &x_axis, cps_bufs, colors, show_mean_lines, ui);
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(half, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    draw_strip_plot(dr_plot, &x_axis, dr_bufs, colors, show_mean_lines, ui);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(table_w, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    draw_table(cps_bufs, dr_bufs, ui);
                },
            );
        });
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

fn draw_strip_plot(
    plot: &mut Plot1D,
    x_axis: &[f64],
    buffers: &HashMap<String, Vec<f64>>,
    colors: &HashMap<String, Color32>,
    show_mean_lines: bool,
    ui: &mut egui::Ui,
) {
    plot.clear();

    // Stable iteration order so the plot looks consistent frame-to-frame.
    let mut names: Vec<&String> = buffers.keys().collect();
    names.sort();

    for name in names {
        let color = colors.get(name).copied().unwrap_or(Color32::LIGHT_BLUE);
        let Some(buf) = buffers.get(name) else {
            continue;
        };
        if buf.is_empty() {
            continue;
        }

        // Python: np.clip(cps_buffer, -1, None) — floor at -1 so small
        // negative spikes from uncertainty are visible but deep negatives
        // aren't.
        let clipped: Vec<f64> = buf.iter().map(|&v| v.max(-1.0)).collect();

        // Buffer is [oldest, ..., newest]. Our x_axis is likewise
        // [(n-1)*dt, ..., 0] so oldest gets the largest x. Skip NaN.
        let pairs: Vec<[f64; 2]> = x_axis
            .iter()
            .zip(clipped.iter())
            .filter(|&(_, &y)| y.is_finite())
            .map(|(&x, &y)| [x, y])
            .collect();
        if pairs.is_empty() {
            continue;
        }

        let xs: Vec<f64> = pairs.iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = pairs.iter().map(|p| p[1]).collect();
        plot.add_curve_with_legend_and_interpolation(
            &xs,
            &ys,
            color,
            name,
            CurveInterpolation::Linear,
        );

        // Mean line (dashed, same color) when enabled.
        if show_mean_lines {
            if let Some(mean) = nanmean(&clipped) {
                if mean.is_finite() {
                    let mut m = Marker::hline(mean)
                        .with_line_style(LineStyle::Dashed)
                        .with_line_width(1.0)
                        .with_draggable(false);
                    m.color = color;
                    plot.add_marker_data(&m);
                }
            }
        }
    }

    plot.show(ui);
}

fn draw_table(
    cps_buffers: &HashMap<String, Vec<f64>>,
    dr_buffers: &HashMap<String, Vec<f64>>,
    ui: &mut egui::Ui,
) {
    egui::ScrollArea::vertical()
        .id_salt("realtime_values_table")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("realtime_values_grid")
                .num_columns(3)
                .spacing([10.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Device");
                    ui.strong("Count Rate\n[s⁻¹]");
                    ui.strong("Dose Rate\n[μSv/h]");
                    ui.end_row();

                    let mut names: Vec<&String> = cps_buffers.keys().collect();
                    names.sort();
                    for name in names {
                        // Python: cps_array[-1] / dr_array[-1] — the most
                        // recent sample.
                        let cps = cps_buffers
                            .get(name)
                            .and_then(|v| v.last())
                            .copied()
                            .unwrap_or(f64::NAN);
                        let dr = dr_buffers
                            .get(name)
                            .and_then(|v| v.last())
                            .copied()
                            .unwrap_or(f64::NAN);

                        ui.label(name);
                        ui.monospace(format_cell(cps, 2));
                        ui.monospace(format_cell(dr, 3));
                        ui.end_row();
                    }
                });
        });
}

fn format_cell(v: f64, decimals: usize) -> String {
    if v.is_finite() {
        format!("{:.*}", decimals, v)
    } else {
        "—".into()
    }
}

fn nanmean(v: &[f64]) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0usize;
    for &x in v {
        if x.is_finite() {
            sum += x;
            count += 1;
        }
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}
