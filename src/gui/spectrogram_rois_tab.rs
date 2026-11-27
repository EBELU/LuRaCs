//! Spectrogram ROIs bottom tab: per-ROI strip charts (newest on the right)
//! plus a live statistics panel (last value, mean ± std).
//!
//! Ported from `luracs/gui/tabs/spectrogram_roi_tab.py`.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use rsplot::core::items::CurveInterpolation;
use rsplot::{Plot1D, YAxis};

use crate::core::{RunManager, SpectrogramManager};
use crate::gui::color_rotator::{ColorRotator, ColorScheme};
use crate::settings::Settings;
use crate::theme_manager::ResolvedPlotColors;

pub struct SpectrogramRoisTab {
    plots: HashMap<String, Plot1D>,
    next_plot_id: u64,
    /// `roi_tag -> (spectrogram_name -> counts_series)`
    roi_data: HashMap<String, HashMap<String, Vec<f64>>>,
    colors: HashMap<String, Color32>,
    color_rotator: ColorRotator,
    plot_colors: Option<ResolvedPlotColors>,
}

impl SpectrogramRoisTab {
    pub fn new(scheme: ColorScheme) -> Self {
        Self {
            plots: HashMap::new(),
            next_plot_id: 0x5000,
            roi_data: HashMap::new(),
            colors: HashMap::new(),
            color_rotator: ColorRotator::new(scheme),
            plot_colors: None,
        }
    }

    pub fn retheme(&mut self, colors: &ResolvedPlotColors) {
        for plot in self.plots.values_mut() {
            colors.apply_to(plot);
        }
        self.plot_colors = Some(colors.clone());
    }

    pub fn on_counts_updated(&mut self, spec_name: &str, counts: &Arc<HashMap<String, Vec<f64>>>) {
        self.ensure_color(spec_name);
        for (roi_tag, series) in counts.iter() {
            self.roi_data
                .entry(roi_tag.clone())
                .or_default()
                .insert(spec_name.to_string(), series.clone());
        }
    }

    pub fn on_roi_removed(&mut self, roi_tag: &str) {
        self.roi_data.remove(roi_tag);
        self.plots.remove(roi_tag);
    }

    pub fn on_spectrogram_closed(&mut self, spec_name: &str) {
        self.colors.remove(spec_name);
        for map in self.roi_data.values_mut() {
            map.remove(spec_name);
        }
    }

    fn ensure_color(&mut self, spec_name: &str) -> Color32 {
        if let Some(&c) = self.colors.get(spec_name) {
            return c;
        }
        let c = self.color_rotator.next_color();
        self.colors.insert(spec_name.to_string(), c);
        c
    }

    fn ensure_plot<'a>(
        &'a mut self,
        roi_tag: &str,
        alias: &str,
        render_state: &RenderState,
    ) -> &'a mut Plot1D {
        if !self.plots.contains_key(roi_tag) {
            let id = self.next_plot_id;
            self.next_plot_id += 1;
            let mut plot = Plot1D::new(render_state, id);
            plot.set_graph_x_label("Time [s]");
            plot.set_graph_y_label(format!("CPS ({alias})"), YAxis::Left);
            plot.plot_mut().x_inverted = true;
            plot.set_x_min_pos(Some(0.0));
            if let Some(colors) = &self.plot_colors {
                colors.apply_to(&mut plot);
            }
            self.plots.insert(roi_tag.to_string(), plot);
        }
        self.plots.get_mut(roi_tag).unwrap()
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        run_manager: &RunManager,
        spectrogram_manager: &SpectrogramManager,
        settings: &Settings,
        render_state: &RenderState,
        compact: bool,
    ) {
        let mut roi_tags: Vec<String> = spectrogram_manager
            .roi_registry
            .keys()
            .cloned()
            .collect();
        roi_tags.sort();

        if roi_tags.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No spectrogram ROIs defined.");
            });
            return;
        }

        let display_len = settings.spectrogram.roi_display_duration_s;

        // Build stats text before mutably borrowing `self.plots`.
        let mut stats_sections = Vec::with_capacity(roi_tags.len());
        for tag in &roi_tags {
            let alias = spectrogram_manager
                .roi_registry
                .get(tag)
                .map_or(tag.as_str(), |r| r.alias.as_str());
            let mut lines = vec![format!("== {alias} ==")];
            if let Some(series_map) = self.roi_data.get(tag) {
                let mut specs: Vec<&String> = series_map.keys().collect();
                specs.sort();
                for spec in specs {
                    let data = &series_map[spec];
                    let dt = run_manager
                        .get_spectrogram(spec)
                        .map_or(1.0, |s| s.save_interval());
                    let window_vals: Vec<f64> = data
                        .iter()
                        .rev()
                        .enumerate()
                        .take_while(|(i, _)| (*i as f64) * dt <= display_len)
                        .map(|(_, &v)| v)
                        .filter(|v| v.is_finite())
                        .collect();
                    let last = data.last().copied().unwrap_or(0.0);
                    let (mean, std) = mean_std(&window_vals);
                    lines.push(format!("|{spec}| {last:.2}, ({mean:.2} ± {std:.2})"));
                }
            }
            stats_sections.push(lines.join("\n"));
        }
        let stats_text = format!(
            "| Spectrogram | Last Value (mean ± std) |\n{}",
            stats_sections.join("\n\n")
        );

        let total_w = ui.available_width();
        let left_w = (total_w * 0.72).max(200.0);
        let right_w = (total_w - left_w - 12.0).max(150.0);
        let avail_h = ui.available_height();

        ui.horizontal_top(|ui| {
            // Left: scrollable per-ROI strip charts
            ui.allocate_ui_with_layout(
                egui::vec2(left_w, avail_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("sg_roi_plots_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let plot_h = if compact { 70.0 } else { 150.0 };
                            for tag in &roi_tags {
                                let alias = spectrogram_manager
                                    .roi_registry
                                    .get(tag)
                                    .map_or(tag.as_str(), |r| r.alias.as_str())
                                    .to_string();
                                let series_snapshot: Vec<(String, Vec<f64>, Color32, f64)> = self
                                    .roi_data
                                    .get(tag)
                                    .map(|m| {
                                        let mut specs: Vec<&String> = m.keys().collect();
                                        specs.sort();
                                        specs
                                            .into_iter()
                                            .map(|spec| {
                                                let col = self
                                                    .colors
                                                    .get(spec)
                                                    .copied()
                                                    .unwrap_or(Color32::LIGHT_BLUE);
                                                let dt = run_manager
                                                    .get_spectrogram(spec)
                                                    .map_or(1.0, |s| s.save_interval());
                                                (spec.clone(), m[spec].clone(), col, dt)
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                let plot = self.ensure_plot(tag, &alias, render_state);
                                plot.clear();
                                for (spec_name, data, color, dt) in &series_snapshot {
                                    if data.is_empty() {
                                        continue;
                                    }
                                    let n = data.len();
                                    let x: Vec<f64> =
                                        (0..n).map(|i| ((n - 1 - i) as f64) * dt).collect();
                                    plot.add_curve_with_legend_and_interpolation(
                                        &x,
                                        data,
                                        *color,
                                        spec_name,
                                        CurveInterpolation::Linear,
                                    );
                                }

                                ui.allocate_ui(egui::vec2(ui.available_width(), plot_h), |ui| {
                                    plot.show(ui);
                                });
                            }
                        });
                },
            );

            ui.separator();

            // Right: statistics text
            ui.allocate_ui_with_layout(
                egui::vec2(right_w, avail_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("sg_roi_stats_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.monospace(stats_text);
                        });
                },
            );
        });
    }
}

fn mean_std(vals: &[f64]) -> (f64, f64) {
    if vals.is_empty() {
        return (0.0, 0.0);
    }
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}
