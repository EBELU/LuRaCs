//! Energy Resolution Calibration Window.
//!
//! Ported from `luracs/gui/windows/calc_resolution_window.py`.

use std::collections::HashSet;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use rsplot::{Marker, Plot1D, YAxis, leastsq};

use crate::containers::CountField;
use crate::core::SpectrumManager;
use crate::utils::ResolutionSignature;
use crate::utils::numerics::calibration::physics_models::resolution::lookup as lookup_resolution;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolutionPlotMode {
    #[default]
    FwhmVsE,
    ResVsE,
    ResVsInvSqrtE,
}

#[derive(Debug, Clone)]
struct ResolutionPeakRow {
    tag: String,
    roi_alias: String,
    spectrum: String,
    nuclide: String,
    energy_kev: f64,
    fwhm_kev: f64,
    fwhm_err_kev: f64,
    resolution: f64,
}

pub struct ResolutionWindow {
    pub open: bool,
    selected_peaks: HashSet<String>,
    selected_instrument: String,
    plot_mode: ResolutionPlotMode,
    energy_points: Vec<f64>,
    fwhm_points: Vec<(f64, f64)>,
    resolution_points: Vec<f64>,
    params: Vec<f64>,
    result_text: String,
    status_msg: String,
    plot: Plot1D,
}

impl ResolutionWindow {
    #[must_use]
    pub fn new(render_state: &RenderState) -> Self {
        let mut plot = Plot1D::new(render_state, 0x3003);
        plot.set_graph_x_label("E [keV]");
        plot.set_graph_y_label("FWHM [keV]", YAxis::Left);
        Self {
            open: false,
            selected_peaks: HashSet::new(),
            selected_instrument: String::new(),
            plot_mode: ResolutionPlotMode::FwhmVsE,
            energy_points: Vec::new(),
            fwhm_points: Vec::new(),
            resolution_points: Vec::new(),
            params: Vec::new(),
            result_text: String::new(),
            status_msg: String::new(),
            plot,
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    pub fn retheme(&mut self, colors: &crate::theme_manager::ResolvedPlotColors) {
        colors.apply_to(&mut self.plot);
    }

    fn collect_peaks(spectrum_manager: &SpectrumManager) -> Vec<ResolutionPeakRow> {
        let mut out = Vec::new();
        for roi in spectrum_manager.roi_manager.registry().values() {
            let Some(energy_kev) = roi
                .get_count_data(CountField::Mu, false)
                .filter(|v| v.is_finite() && *v > 0.0)
            else {
                continue;
            };
            let Some(fwhm_kev) = roi
                .get_count_data(CountField::Fwhm, false)
                .filter(|v| v.is_finite() && *v > 0.0)
            else {
                continue;
            };
            let fwhm_err_kev = roi
                .get_count_data(CountField::FwhmErr, false)
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(0.0);
            let nuclide = roi
                .emission
                .as_ref()
                .and_then(|em| em.parent_nuclide)
                .map_or_else(|| "None".to_string(), |n| n.to_string());
            out.push(ResolutionPeakRow {
                tag: roi.tag.clone(),
                roi_alias: roi.alias.clone(),
                spectrum: roi.spectrum.clone(),
                nuclide,
                energy_kev,
                fwhm_kev,
                fwhm_err_kev,
                resolution: fwhm_kev / energy_kev,
            });
        }
        out
    }

    fn calculate(&mut self, peaks: &[ResolutionPeakRow]) {
        self.energy_points.clear();
        self.fwhm_points.clear();
        self.resolution_points.clear();
        self.params.clear();

        for row in peaks {
            if self.selected_peaks.contains(&row.tag) {
                self.energy_points.push(row.energy_kev);
                self.fwhm_points.push((row.fwhm_kev, row.fwhm_err_kev));
                self.resolution_points.push(row.resolution);
            }
        }
        if self.energy_points.is_empty() {
            self.status_msg = "Select at least one fitted peak.".into();
            self.rebuild_plot();
            return;
        }

        let eval_fn = lookup_resolution(ResolutionSignature::KOverSqrtE);
        let model = |xs: &[f64], p: &[f64]| -> Vec<f64> {
            xs.iter().map(|&e| eval_fn(e, p).unwrap_or(0.0)).collect()
        };
        match leastsq(
            model,
            &self.energy_points,
            &self.resolution_points,
            &[1.0],
            None,
            200,
            1e-8,
            false,
        ) {
            Ok(res) => {
                self.params = res.parameters;
                let predicted = model(&self.energy_points, &self.params);
                let r2 = r_squared(&self.resolution_points, &predicted);
                let k = self.params.first().copied().unwrap_or(0.0);
                self.result_text = format!("R(E) = {k:.4}/SQRT(E)   R² = {r2:.4}");
                self.status_msg.clear();
            }
            Err(e) => {
                self.status_msg = format!("Resolution fit failed: {e:?}");
            }
        }
        self.rebuild_plot();
    }

    fn rebuild_plot(&mut self) {
        self.plot.clear();
        if self.energy_points.is_empty() || self.params.is_empty() {
            return;
        }
        let eval_fn = lookup_resolution(ResolutionSignature::KOverSqrtE);
        let max_e = self.energy_points.iter().copied().fold(100.0_f64, f64::max);
        let n_curve = 250usize;
        let e_hi = max_e + 500.0;
        let x_curve: Vec<f64> = (0..n_curve)
            .map(|i| 25.0 + (i as f64) * (e_hi - 25.0) / ((n_curve - 1) as f64))
            .collect();
        let r_curve: Vec<f64> = x_curve
            .iter()
            .map(|&e| eval_fn(e, &self.params).unwrap_or(0.0))
            .collect();

        match self.plot_mode {
            ResolutionPlotMode::FwhmVsE => {
                self.plot.set_graph_x_label("E [keV]");
                self.plot.set_graph_y_label("FWHM [keV]", YAxis::Left);
                for (&e, &(fwhm, _)) in self.energy_points.iter().zip(self.fwhm_points.iter()) {
                    let mut m = Marker::point(e, fwhm);
                    m.color = Color32::from_rgb(80, 200, 255);
                    self.plot.add_marker_data(&m);
                }
                let y_curve: Vec<f64> =
                    r_curve.iter().zip(x_curve.iter()).map(|(r, e)| r * e).collect();
                self.plot
                    .add_curve(&x_curve, &y_curve, Color32::from_rgb(255, 200, 60));
            }
            ResolutionPlotMode::ResVsE => {
                self.plot.set_graph_x_label("E [keV]");
                self.plot.set_graph_y_label("Resolution [%]", YAxis::Left);
                for (&e, &r) in self.energy_points.iter().zip(self.resolution_points.iter()) {
                    let mut m = Marker::point(e, r * 100.0);
                    m.color = Color32::from_rgb(80, 200, 255);
                    self.plot.add_marker_data(&m);
                }
                let y_curve: Vec<f64> = r_curve.iter().map(|r| r * 100.0).collect();
                self.plot
                    .add_curve(&x_curve, &y_curve, Color32::from_rgb(255, 200, 60));
            }
            ResolutionPlotMode::ResVsInvSqrtE => {
                self.plot.set_graph_x_label("1 / √(E) [1/√(keV)]");
                self.plot.set_graph_y_label("Resolution [%]", YAxis::Left);
                for (&e, &r) in self.energy_points.iter().zip(self.resolution_points.iter()) {
                    let mut m = Marker::point(1.0 / e.sqrt(), r * 100.0);
                    m.color = Color32::from_rgb(80, 200, 255);
                    self.plot.add_marker_data(&m);
                }
                let inv_x: Vec<f64> = x_curve.iter().map(|e| 1.0 / e.sqrt()).collect();
                let y_curve: Vec<f64> = r_curve.iter().map(|r| r * 100.0).collect();
                self.plot
                    .add_curve(&inv_x, &y_curve, Color32::from_rgb(255, 200, 60));
            }
        }
        self.plot.set_auto_reset_zoom(true);
    }

    fn assign_to_instruments(
        &mut self,
        spectrum_manager: &mut SpectrumManager,
        include_all_of_model: bool,
    ) {
        if self.params.is_empty() {
            self.status_msg = "Calculate a resolution fit first.".into();
            return;
        }
        let target_key = self.selected_instrument.clone();
        let mut count = 0usize;

        if let Some(instr) = spectrum_manager
            .unique_instrument_library
            .get_instrument_by_name(&target_key)
        {
            let model = instr.model().to_string();
            let keys: Vec<String> = if include_all_of_model {
                spectrum_manager
                    .unique_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .unique_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect()
            } else {
                vec![target_key]
            };
            for k in &keys {
                if spectrum_manager
                    .unique_instrument_library
                    .set_resolution(k, &self.params, &self.energy_points, &self.fwhm_points)
                    .is_ok()
                {
                    count += 1;
                }
            }
            if include_all_of_model {
                let g_keys: Vec<String> = spectrum_manager
                    .generic_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .generic_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect();
                for k in &g_keys {
                    if spectrum_manager
                        .generic_instrument_library
                        .set_resolution(k, &self.params, &self.energy_points, &self.fwhm_points)
                        .is_ok()
                    {
                        count += 1;
                    }
                }
            }
        } else if let Some(instr) = spectrum_manager
            .generic_instrument_library
            .get_instrument_by_name(&target_key)
        {
            let model = instr.model().to_string();
            let keys: Vec<String> = if include_all_of_model {
                spectrum_manager
                    .generic_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .generic_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect()
            } else {
                vec![target_key]
            };
            for k in &keys {
                if spectrum_manager
                    .generic_instrument_library
                    .set_resolution(k, &self.params, &self.energy_points, &self.fwhm_points)
                    .is_ok()
                {
                    count += 1;
                }
            }
        }
        self.status_msg = format!("Assigned resolution curve to {count} instrument(s).");
    }

    pub fn render(&mut self, ui: &egui::Ui, spectrum_manager: &mut SpectrumManager) {
        if !self.open {
            return;
        }

        let peaks = Self::collect_peaks(spectrum_manager);
        let mut instruments = spectrum_manager
            .unique_instrument_library
            .get_instrument_names();
        instruments.extend(
            spectrum_manager
                .generic_instrument_library
                .get_instrument_names(),
        );
        if self.selected_instrument.is_empty()
            && let Some(first) = instruments.first()
        {
            self.selected_instrument = first.clone();
        }

        let mut close_requested = false;
        let mut do_calc = false;
        let mut do_assign_one = false;
        let mut do_assign_model = false;

        let viewport_id = egui::ViewportId::from_hash_of("resolution_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Resolution Window")
            .with_inner_size([920.0, 620.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("Peaks:");
                egui::ScrollArea::vertical()
                    .id_salt("res_peaks_scroll")
                    .max_height(150.0)
                    .show(ui, |ui| {
                        egui::Grid::new("res_peaks_grid")
                            .striped(true)
                            .num_columns(7)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                for h in [
                                    "",
                                    "ROI",
                                    "Spectrum",
                                    "Nuclide",
                                    "Energy",
                                    "FWHM",
                                    "Resolution",
                                ] {
                                    ui.strong(h);
                                }
                                ui.end_row();
                                for row in &peaks {
                                    let mut checked = self.selected_peaks.contains(&row.tag);
                                    if ui.checkbox(&mut checked, "").changed() {
                                        if checked {
                                            self.selected_peaks.insert(row.tag.clone());
                                        } else {
                                            self.selected_peaks.remove(&row.tag);
                                        }
                                    }
                                    ui.label(&row.roi_alias);
                                    ui.label(&row.spectrum);
                                    ui.label(&row.nuclide);
                                    ui.label(format!("{:.2} keV", row.energy_kev));
                                    ui.label(format!("{:.2} keV", row.fwhm_kev));
                                    ui.label(format!("{:.2}%", row.resolution * 100.0));
                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Instrument:");
                    egui::ComboBox::from_id_salt("res_instr_combo")
                        .selected_text(&self.selected_instrument)
                        .show_ui(ui, |ui| {
                            for k in &instruments {
                                ui.selectable_value(&mut self.selected_instrument, k.clone(), k);
                            }
                        });
                    if ui.button("Calculate").clicked() {
                        do_calc = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Plot option:");
                    let prev_mode = self.plot_mode;
                    ui.radio_value(&mut self.plot_mode, ResolutionPlotMode::FwhmVsE, "FWHM / E");
                    ui.radio_value(&mut self.plot_mode, ResolutionPlotMode::ResVsE, "R / E");
                    ui.radio_value(
                        &mut self.plot_mode,
                        ResolutionPlotMode::ResVsInvSqrtE,
                        "R / (1 / √(E))",
                    );
                    if self.plot_mode != prev_mode {
                        self.rebuild_plot();
                    }
                });

                if !self.result_text.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label("Result fn:");
                        ui.monospace(&self.result_text);
                    });
                }

                ui.allocate_ui(egui::vec2(ui.available_width(), 220.0), |ui| {
                    self.plot.show(ui);
                });

                ui.horizontal(|ui| {
                    if ui.button("Assign to Instrument Model").clicked() {
                        do_assign_model = true;
                    }
                    if ui.button("Assign to Selected Instrument").clicked() {
                        do_assign_one = true;
                    }
                    if ui.button("Close").clicked() {
                        close_requested = true;
                    }
                    if !self.status_msg.is_empty() {
                        ui.label(&self.status_msg);
                    }
                });
                });
            });

        if do_calc {
            self.calculate(&peaks);
        }
        if do_assign_one {
            self.assign_to_instruments(spectrum_manager, false);
        }
        if do_assign_model {
            self.assign_to_instruments(spectrum_manager, true);
        }
        self.open = !close_requested;
    }
}

#[must_use]
pub fn r_squared(observed: &[f64], predicted: &[f64]) -> f64 {
    if observed.is_empty() || observed.len() != predicted.len() {
        return 0.0;
    }
    let mean = observed.iter().sum::<f64>() / (observed.len() as f64);
    let ss_tot: f64 = observed.iter().map(|y| (y - mean).powi(2)).sum();
    let ss_res: f64 = observed
        .iter()
        .zip(predicted.iter())
        .map(|(y, yp)| (y - yp).powi(2))
        .sum();
    if ss_tot <= f64::EPSILON {
        1.0
    } else {
        1.0 - (ss_res / ss_tot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolution_fit_and_r_squared() {
        let energies = [100.0_f64, 500.0, 1000.0, 1500.0];
        let k_true = 1.8_f64;
        let res_obs: Vec<f64> = energies.iter().map(|e| k_true / e.sqrt()).collect();
        let eval_fn = lookup_resolution(ResolutionSignature::KOverSqrtE);
        let model = |xs: &[f64], p: &[f64]| -> Vec<f64> {
            xs.iter().map(|&e| eval_fn(e, p).unwrap_or(0.0)).collect()
        };
        let fit = leastsq(model, &energies, &res_obs, &[1.0], None, 200, 1e-8, false).unwrap();
        assert!((fit.parameters[0] - k_true).abs() < 1e-6);
        let pred = model(&energies, &fit.parameters);
        assert!((r_squared(&res_obs, &pred) - 1.0).abs() < 1e-9);
    }
}
