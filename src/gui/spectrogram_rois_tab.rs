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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalcOp {
    Add,
    Sub,
    Div,
}

impl CalcOp {
    const fn label(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Div => "/",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CalcRoi {
    pub label: String,
    pub sg1: String,
    pub roi1: String,
    pub op: CalcOp,
    pub sg2: String,
    pub roi2: String,
}

pub struct SpectrogramRoisTab {
    plots: HashMap<String, Plot1D>,
    next_plot_id: u64,
    /// `roi_tag -> (spectrogram_name -> counts_series)`
    roi_data: HashMap<String, HashMap<String, Vec<f64>>>,
    colors: HashMap<String, Color32>,
    color_rotator: ColorRotator,
    plot_colors: Option<ResolvedPlotColors>,
    calc_rois: Vec<CalcRoi>,
    calc_roi_counter: usize,
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
            calc_rois: Vec::new(),
            calc_roi_counter: 0,
        }
    }

    pub fn set_color_scheme(&mut self, scheme: ColorScheme) {
        self.color_rotator = ColorRotator::new(scheme);
        self.colors.clear();
    }

    pub fn add_calc_roi(&mut self) {
        let label = format!("CAL_{}", self.calc_roi_counter);
        self.calc_roi_counter += 1;
        self.calc_rois.push(CalcRoi {
            label,
            sg1: String::new(),
            roi1: String::new(),
            op: CalcOp::Add,
            sg2: String::new(),
            roi2: String::new(),
        });
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

        if roi_tags.is_empty() && self.calc_rois.is_empty() {
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

                            // Calc ROIs (sg1/roi1 <op> sg2/roi2)
                            let mut sg_names: Vec<String> = spectrogram_manager
                                .spectrogram_registry
                                .iter()
                                .cloned()
                                .collect();
                            sg_names.sort();
                            let mut remove_calc_idx: Option<usize> = None;
                            let mut calc_rois = std::mem::take(&mut self.calc_rois);
                            for (idx, calc) in calc_rois.iter_mut().enumerate() {
                                if calc.sg1.is_empty() && !sg_names.is_empty() {
                                    calc.sg1 = sg_names[0].clone();
                                }
                                if calc.sg2.is_empty() && !sg_names.is_empty() {
                                    calc.sg2 = sg_names[0].clone();
                                }
                                ui.horizontal(|ui| {
                                    if ui.button("Remove").clicked() {
                                        remove_calc_idx = Some(idx);
                                    }
                                    ui.label(&calc.label);
                                    egui::ComboBox::from_id_salt(("calc_sg1", &calc.label))
                                        .selected_text(&calc.sg1)
                                        .show_ui(ui, |ui| {
                                            for s in &sg_names {
                                                ui.selectable_value(&mut calc.sg1, s.clone(), s);
                                            }
                                        });
                                    egui::ComboBox::from_id_salt(("calc_r1", &calc.label))
                                        .selected_text(if calc.roi1.is_empty() { "Select ROI" } else { &calc.roi1 })
                                        .show_ui(ui, |ui| {
                                            for tag in &roi_tags {
                                                let alias = spectrogram_manager
                                                    .roi_registry
                                                    .get(tag)
                                                    .map_or(tag.as_str(), |r| r.alias.as_str());
                                                ui.selectable_value(&mut calc.roi1, tag.clone(), alias);
                                            }
                                        });
                                    egui::ComboBox::from_id_salt(("calc_op", &calc.label))
                                        .selected_text(calc.op.label())
                                        .width(44.0)
                                        .show_ui(ui, |ui| {
                                            for op in [CalcOp::Add, CalcOp::Sub, CalcOp::Div] {
                                                ui.selectable_value(&mut calc.op, op, op.label());
                                            }
                                        });
                                    egui::ComboBox::from_id_salt(("calc_sg2", &calc.label))
                                        .selected_text(&calc.sg2)
                                        .show_ui(ui, |ui| {
                                            for s in &sg_names {
                                                ui.selectable_value(&mut calc.sg2, s.clone(), s);
                                            }
                                        });
                                    egui::ComboBox::from_id_salt(("calc_r2", &calc.label))
                                        .selected_text(if calc.roi2.is_empty() { "Select ROI" } else { &calc.roi2 })
                                        .show_ui(ui, |ui| {
                                            for tag in &roi_tags {
                                                let alias = spectrogram_manager
                                                    .roi_registry
                                                    .get(tag)
                                                    .map_or(tag.as_str(), |r| r.alias.as_str());
                                                ui.selectable_value(&mut calc.roi2, tag.clone(), alias);
                                            }
                                        });
                                });

                                let d1 = self.roi_data.get(&calc.roi1).and_then(|m| m.get(&calc.sg1)).cloned();
                                let d2 = self.roi_data.get(&calc.roi2).and_then(|m| m.get(&calc.sg2)).cloned();
                                let dt1 = run_manager.get_spectrogram(&calc.sg1).map_or(1.0, |s| s.save_interval());
                                let dt2 = run_manager.get_spectrogram(&calc.sg2).map_or(1.0, |s| s.save_interval());
                                let dt = dt1.max(dt2);
                                let calc_key = calc.label.clone();
                                let plot = self.ensure_plot(&calc_key, &calc.label, render_state);
                                plot.clear();
                                if let (Some(s1), Some(s2)) = (d1, d2) {
                                    let len = s1.len().min(s2.len());
                                    if len > 0 {
                                        let y: Vec<f64> = s1[s1.len() - len..]
                                            .iter()
                                            .zip(&s2[s2.len() - len..])
                                            .map(|(&a, &b)| match calc.op {
                                                CalcOp::Add => a + b,
                                                CalcOp::Sub => a - b,
                                                CalcOp::Div => if b != 0.0 { a / b } else { 0.0 },
                                            })
                                            .collect();
                                        let x: Vec<f64> = (0..len).map(|i| ((len - 1 - i) as f64) * dt).collect();
                                        plot.add_curve_with_legend_and_interpolation(
                                            &x,
                                            &y,
                                            Color32::LIGHT_BLUE,
                                            &calc.label,
                                            CurveInterpolation::Linear,
                                        );
                                    }
                                }
                                ui.allocate_ui(egui::vec2(ui.available_width(), plot_h), |ui| {
                                    plot.show(ui);
                                });
                            }
                            if let Some(idx) = remove_calc_idx {
                                let removed = calc_rois.remove(idx);
                                self.plots.remove(&removed.label);
                            }
                            self.calc_rois = calc_rois;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spectrogram_rois_tab_calc_rois_and_state() {
        assert_eq!(CalcOp::Add.label(), "+");
        assert_eq!(CalcOp::Sub.label(), "-");
        assert_eq!(CalcOp::Div.label(), "/");

        let mut tab = SpectrogramRoisTab::new(ColorScheme::default());
        tab.add_calc_roi();
        tab.add_calc_roi();
        assert_eq!(tab.calc_rois.len(), 2);
        assert_eq!(tab.calc_rois[0].label, "CAL_0");
        assert_eq!(tab.calc_rois[1].label, "CAL_1");

        let mut counts = HashMap::new();
        counts.insert("ROI_0".to_string(), vec![1.0, 3.0, 5.0]);
        tab.on_counts_updated("sg0", &Arc::new(counts));
        assert_eq!(tab.roi_data["ROI_0"]["sg0"], vec![1.0, 3.0, 5.0]);

        tab.set_color_scheme(ColorScheme::default());
        tab.on_spectrogram_closed("sg0");
        assert!(!tab.roi_data["ROI_0"].contains_key("sg0"));
        tab.on_roi_removed("ROI_0");
        assert!(!tab.roi_data.contains_key("ROI_0"));

        assert_eq!(mean_std(&[]), (0.0, 0.0));
        let (m, s) = mean_std(&[2.0, 4.0]);
        assert!((m - 3.0).abs() < 1e-12);
        assert!((s - 1.0).abs() < 1e-12);
    }
}
