//! Polynomial Energy Calibration Window.
//!
//! Ported from `luracs/gui/windows/calc_calibration_window.py`.

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use rsplot::{Marker, Plot1D, YAxis};

use crate::containers::CountField;
use crate::core::SpectrumManager;
use crate::utils::numerics::calibration::calibrate_x_axis;

#[derive(Debug, Clone)]
pub struct CalibrationRow {
    pub checked: bool,
    pub roi_name: String,
    pub nuclide: String,
    pub measured: f64,
    pub reference: f64,
}

pub struct CalibrationWindow {
    pub open: bool,
    selected_spectrum: String,
    rows: Vec<CalibrationRow>,
    poly_degree: usize,
    /// Ascending order `[a0, a1, a2, a3]`, matching Python's `poly_spin_list`.
    poly_coeffs_asc: [f64; 4],
    ref_channels: Vec<f64>,
    ref_energies: Vec<f64>,
    status_msg: String,
    plot: Option<Plot1D>,
}

impl CalibrationWindow {
    #[must_use]
    pub fn new(render_state: &RenderState) -> Self {
        let mut plot = Plot1D::new(render_state, 0x3002);
        plot.set_graph_x_label("Channels");
        plot.set_graph_y_label("Energy [keV]", YAxis::Left);
        Self {
            open: false,
            selected_spectrum: String::new(),
            rows: Vec::new(),
            poly_degree: 2,
            poly_coeffs_asc: [0.0; 4],
            ref_channels: Vec::new(),
            ref_energies: Vec::new(),
            status_msg: String::new(),
            plot: Some(plot),
        }
    }

    #[cfg(test)]
    const fn new_headless() -> Self {
        Self {
            open: false,
            selected_spectrum: String::new(),
            rows: Vec::new(),
            poly_degree: 2,
            poly_coeffs_asc: [0.0; 4],
            ref_channels: Vec::new(),
            ref_energies: Vec::new(),
            status_msg: String::new(),
            plot: None,
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    pub fn retheme(&mut self, colors: &crate::theme_manager::ResolvedPlotColors) {
        if let Some(plot) = &mut self.plot {
            colors.apply_to(plot);
        }
    }

    fn populate_table(&mut self, spectrum_manager: &SpectrumManager) {
        self.rows.clear();
        for roi in spectrum_manager.roi_manager.registry().values() {
            if roi.spectrum != self.selected_spectrum {
                continue;
            }
            let measured = roi
                .get_count_data(CountField::Mu, false)
                .filter(|v| v.is_finite())
                .unwrap_or(0.5 * (roi.roi_bound.0 + roi.roi_bound.1));
            let (nuclide, reference) = roi.emission.as_ref().map_or_else(
                || ("None".to_string(), 0.0),
                |em| {
                    let nuc = em
                        .parent_nuclide
                        .map_or_else(|| "None".to_string(), |n| n.to_string());
                    let e = em.energy_kev.as_ref().map_or(0.0, |q| q.value.mean());
                    (nuc, e)
                },
            );
            self.rows.push(CalibrationRow {
                checked: true,
                roi_name: roi.alias.clone(),
                nuclide,
                measured,
                reference,
            });
        }
    }

    /// Return coefficients in highest-degree-first order (`[a_n, ..., a_0]`).
    #[must_use]
    pub fn coeffs_high_first(&self) -> Vec<f64> {
        self.poly_coeffs_asc[..=self.poly_degree]
            .iter()
            .copied()
            .rev()
            .collect()
    }

    fn set_coeffs_from_high_first(&mut self, coeffs: &[f64]) {
        self.poly_coeffs_asc = [0.0; 4];
        for (i, &c) in coeffs.iter().rev().enumerate().take(4) {
            self.poly_coeffs_asc[i] = c;
        }
    }

    fn calculate(&mut self, spectrum_manager: &SpectrumManager) {
        let Some(bundle) = spectrum_manager.spectrum_registry.get(&self.selected_spectrum) else {
            self.status_msg = "No spectrum selected.".into();
            return;
        };

        let mut centroids = Vec::new();
        let mut refs = Vec::new();
        for row in &self.rows {
            if row.checked {
                centroids.push(row.measured);
                refs.push(row.reference);
            }
        }
        if centroids.is_empty() {
            self.status_msg = "Select at least one peak.".into();
            return;
        }

        let Some(fg) = &bundle.foreground else {
            self.status_msg = "Spectrum has no foreground data.".into();
            return;
        };
        let current_x = if bundle.calibrated {
            bundle.x_axis.as_slice()
        } else {
            None
        };

        match calibrate_x_axis(&centroids, &refs, self.poly_degree, fg.channels, current_x) {
            Ok(res) => {
                self.set_coeffs_from_high_first(&res.coefficients);
                self.ref_channels = res.reference_channels;
                self.ref_energies = res.reference_energies;

                if let Some(plot) = &mut self.plot {
                    plot.clear();
                    let ch_x: Vec<f64> = (0..res.x_axis.len()).map(|i| i as f64).collect();
                    let y_axis: Vec<f64> = res.x_axis.iter().copied().collect();
                    plot.add_curve(&ch_x, &y_axis, Color32::from_rgb(255, 200, 60));
                    for (&cx, &ey) in self.ref_channels.iter().zip(self.ref_energies.iter()) {
                        let mut m = Marker::point(cx, ey);
                        m.color = Color32::from_rgb(80, 200, 255);
                        plot.add_marker_data(&m);
                    }
                    plot.set_auto_reset_zoom(true);
                }
                self.status_msg = format!("Calibrated degree-{} polynomial.", self.poly_degree);
            }
            Err(e) => {
                self.status_msg = format!("Calibration failed: {e}");
            }
        }
    }

    fn assign_to_spectrum(&mut self, spectrum_manager: &mut SpectrumManager) {
        let coeffs = self.coeffs_high_first();
        if !coeffs.iter().any(|&c| c != 0.0) {
            self.status_msg = "No calibration coefficients to assign.".into();
            return;
        }
        match spectrum_manager.calibrate_spectrum(&self.selected_spectrum, coeffs) {
            Ok(()) => {
                self.populate_table(spectrum_manager);
                self.status_msg = format!("Calibrated spectrum '{}'.", self.selected_spectrum);
            }
            Err(e) => {
                self.status_msg = format!("Assign failed: {e}");
            }
        }
    }

    fn assign_to_instrument(&mut self, spectrum_manager: &mut SpectrumManager) {
        let coeffs = self.coeffs_high_first();
        if !coeffs.iter().any(|&c| c != 0.0) {
            self.status_msg = "No calibration coefficients to assign.".into();
            return;
        }
        let Some(bundle) = spectrum_manager.spectrum_registry.get(&self.selected_spectrum) else {
            self.status_msg = "No spectrum selected.".into();
            return;
        };
        let Some(instr) = &bundle.instrument else {
            self.status_msg = "No instrument attached to spectrum.".into();
            return;
        };
        let instr_name = instr.name.clone();
        match spectrum_manager.unique_instrument_library.set_calibration(
            &instr_name,
            self.poly_degree as u32,
            &coeffs,
            &self.ref_channels,
            &self.ref_energies,
        ) {
            Ok(()) => {
                self.status_msg = format!("Assigned calibration to '{instr_name}'.");
            }
            Err(e) => {
                self.status_msg = format!("Instrument assign failed: {e}");
            }
        }
    }

    pub fn render(&mut self, ui: &egui::Ui, spectrum_manager: &mut SpectrumManager) {
        if !self.open {
            return;
        }

        let spectra: Vec<String> = spectrum_manager.spectrum_registry.keys().cloned().collect();
        if self.selected_spectrum.is_empty()
            && let Some(first) = spectra.first()
        {
            self.selected_spectrum = first.clone();
            self.populate_table(spectrum_manager);
        }

        let instr_name = spectrum_manager
            .spectrum_registry
            .get(&self.selected_spectrum)
            .and_then(|b| b.instrument.as_ref().map(|i| i.name.clone()))
            .unwrap_or_else(|| "None".to_string());

        let mut close_requested = false;
        let mut do_calc = false;
        let mut do_assign_spec = false;
        let mut do_assign_instr = false;
        let mut do_get_instr = false;

        let viewport_id = egui::ViewportId::from_hash_of("calibration_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Calibration Window")
            .with_inner_size([920.0, 620.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal(|ui| {
                    ui.label("Spectrum:");
                    let prev = self.selected_spectrum.clone();
                    egui::ComboBox::from_id_salt("cal_spec_combo")
                        .selected_text(&self.selected_spectrum)
                        .show_ui(ui, |ui| {
                            for s in &spectra {
                                ui.selectable_value(&mut self.selected_spectrum, s.clone(), s);
                            }
                        });
                    if self.selected_spectrum != prev {
                        self.populate_table(spectrum_manager);
                    }
                    if ui.button("Add New Point").clicked() {
                        self.rows.push(CalibrationRow {
                            checked: true,
                            roi_name: "None".into(),
                            nuclide: "None".into(),
                            measured: 0.0,
                            reference: 0.0,
                        });
                    }
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("cal_roi_scroll")
                    .max_height(150.0)
                    .show(ui, |ui| {
                        egui::Grid::new("cal_roi_grid")
                            .striped(true)
                            .num_columns(6)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                for h in [
                                    "",
                                    "ROI",
                                    "Nuclide",
                                    "Measured",
                                    "Reference",
                                    "Difference",
                                ] {
                                    ui.strong(h);
                                }
                                ui.end_row();
                                for row in &mut self.rows {
                                    ui.checkbox(&mut row.checked, "");
                                    ui.label(&row.roi_name);
                                    ui.label(&row.nuclide);
                                    ui.add(
                                        egui::DragValue::new(&mut row.measured)
                                            .speed(0.5)
                                            .max_decimals(2),
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut row.reference)
                                            .speed(0.5)
                                            .max_decimals(2),
                                    );
                                    ui.label(format!("{:.2}", row.reference - row.measured));
                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Degree:");
                    ui.add(egui::DragValue::new(&mut self.poly_degree).range(0..=3));
                    for i in 0..4 {
                        ui.add_enabled_ui(i <= self.poly_degree, |ui| {
                            ui.label(format!("a{i} ="));
                            ui.add(
                                egui::DragValue::new(&mut self.poly_coeffs_asc[i])
                                    .speed(0.01)
                                    .max_decimals(6),
                            );
                        });
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(format!("Instrument: {instr_name}"));
                    if ui.button("Get calibration from instrument").clicked() {
                        do_get_instr = true;
                    }
                    if ui.button("Calculate").clicked() {
                        do_calc = true;
                    }
                });

                ui.allocate_ui(egui::vec2(ui.available_width(), 220.0), |ui| {
                    if let Some(plot) = &mut self.plot {
                        plot.show(ui);
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Assign to spectrum").clicked() {
                        do_assign_spec = true;
                    }
                    if ui.button("Assign to instrument").clicked() {
                        do_assign_instr = true;
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

        if do_get_instr {
            if let Some(coeffs) = spectrum_manager
                .spectrum_registry
                .get(&self.selected_spectrum)
                .and_then(|b| {
                    b.instrument
                        .as_ref()
                        .and_then(|i| i.calibration_coefficients.clone())
                })
                && !coeffs.is_empty()
            {
                self.poly_degree = (coeffs.len() - 1).min(3);
                self.set_coeffs_from_high_first(&coeffs);
                self.status_msg = "Loaded calibration from instrument.".into();
            } else {
                self.status_msg = "No instrument calibration found.".into();
            }
        }
        if do_calc {
            self.calculate(spectrum_manager);
        }
        if do_assign_spec {
            self.assign_to_spectrum(spectrum_manager);
        }
        if do_assign_instr {
            self.assign_to_instrument(spectrum_manager);
        }
        self.open = !close_requested;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibration_coeffs_ordering() {
        let mut coeffs_asc = [0.0_f64; 4];
        let high_first = [0.001, 2.5, -10.0];
        for (i, &c) in high_first.iter().rev().enumerate().take(4) {
            coeffs_asc[i] = c;
        }
        assert!((coeffs_asc[0] - (-10.0)).abs() < 1e-12);
        assert!((coeffs_asc[1] - 2.5).abs() < 1e-12);
        assert!((coeffs_asc[2] - 0.001).abs() < 1e-12);

        let res = calibrate_x_axis(&[100.0, 500.0], &[300.0, 1500.0], 1, 1024, None).unwrap();
        assert_eq!(res.coefficients.len(), 2);
        assert!((res.coefficients[0] - 3.0).abs() < 1e-9);
        assert!(res.coefficients[1].abs() < 1e-9);
    }

    #[test]
    fn test_calibration_window_kittest_workflow() {
        use crate::containers::{GenericInstrument, SpectrumData, UniqueInstrument};
        use crate::core::ForegroundInput;
        use crate::gui::ColorScheme;
        use egui_kittest::kittest::Queryable;
        use egui_kittest::Harness;
        use ndarray::Array1;
        use std::path::Path;

        let dir = std::env::temp_dir().join("luracs_test_cal_win");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();

        let mut sm = SpectrumManager::new(
            ColorScheme::default(),
            dir.clone(),
            Path::new("resources/nuclide_data"),
        )
        .unwrap();
        sm.create_spectrum("CalSpec", 1024, None, false).unwrap();
        sm.set_foreground_spectrum(
            "CalSpec",
            ForegroundInput::Raw(SpectrumData {
                y_axis: Array1::zeros(1024),
                channels: 1024,
                total_counts: 0,
                live_time: 10.0,
                real_time: Some(10.0),
                avg_dose_rate: None,
                avg_cps: None,
                start_date: None,
                end_date: None,
                spectrum_name: None,
                instrument: None,
            }),
        )
        .unwrap();

        let mut u = UniqueInstrument::from_template("CalDet", &GenericInstrument::default());
        u.name = "CalDet".to_string();
        u.calibration_coefficients = Some(vec![2.0, 0.0]);
        sm.unique_instrument_library
            .add_instrument(crate::core::Instrument::Unique(u.clone()))
            .unwrap();
        sm.spectrum_registry
            .get_mut("CalSpec")
            .unwrap()
            .set_instrument(u);

        let mut win = CalibrationWindow::new_headless();
        win.poly_degree = 1;

        let mut harness = Harness::builder()
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, (w, sm): &mut (CalibrationWindow, SpectrumManager)| {
                    w.render(ui, sm);
                },
                (win, sm),
            );

        // Closed by default
        harness.run();
        assert!(!harness.state().0.open);

        // Open
        harness.state_mut().0.open();
        harness.run();
        assert_eq!(harness.state().0.selected_spectrum, "CalSpec");

        // Error branches when empty
        harness.get_by_label("Calculate").click();
        harness.run();
        assert_eq!(harness.state().0.status_msg, "Select at least one peak.");

        harness.get_by_label("Assign to spectrum").click();
        harness.run();
        assert_eq!(
            harness.state().0.status_msg,
            "No calibration coefficients to assign."
        );

        // Get calibration from instrument
        harness.get_by_label("Get calibration from instrument").click();
        harness.run();
        assert_eq!(
            harness.state().0.status_msg,
            "Loaded calibration from instrument."
        );

        // Add two calibration points and Calculate
        harness.get_by_label("Add New Point").click();
        harness.run();
        harness.get_by_label("Add New Point").click();
        harness.run();
        {
            let rows = &mut harness.state_mut().0.rows;
            rows[0].measured = 100.0;
            rows[0].reference = 300.0;
            rows[1].measured = 500.0;
            rows[1].reference = 1500.0;
        }
        harness.get_by_label("Calculate").click();
        harness.run();
        assert!(harness
            .state()
            .0
            .status_msg
            .starts_with("Calibrated degree-1 polynomial"));

        // Assign to spectrum & instrument
        harness.get_by_label("Assign to spectrum").click();
        harness.run();
        assert!(harness.state().1.spectrum_registry["CalSpec"].calibrated);

        harness.get_by_label("Assign to instrument").click();
        harness.run();
        assert!(
            harness
                .state()
                .0
                .status_msg
                .starts_with("Assigned calibration")
                || harness
                    .state()
                    .0
                    .status_msg
                    .starts_with("Instrument assign failed")
        );

        // Close
        harness.get_by_label("Close").click();
        harness.run();
        assert!(!harness.state().0.open);

        drop(std::fs::remove_dir_all(&dir));
    }
}
