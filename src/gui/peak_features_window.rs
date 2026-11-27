//! Peak Features calculator dialog.
//!
//! Computes Backscatter, Compton Edge, Photopeak, 1st Escape Peak, and
//! 2nd Escape Peak energies for a given photopeak energy in keV, and draws
//! or removes the corresponding vertical feature lines on `SpectrumScreen`.
//!
//! Ported from `luracs/gui/dialogs/calc_peak_features.py`.

use eframe::egui::{self, Color32};

use crate::gui::SpectrumScreen;

const PAIR_PRODUCTION_THRESHOLD_KEV: f64 = 1022.0;
const ELECTRON_REST_MASS_KEV: f64 = 511.0;

#[derive(Debug, Clone)]
pub struct PeakFeatureRow {
    pub label: &'static str,
    pub enabled: bool,
    pub energy_kev: Option<f64>,
    pub color: Color32,
}

#[derive(Debug, Clone)]
pub struct PeakFeaturesWindow {
    pub open: bool,
    pub input_energy: f64,
    pub rows: [PeakFeatureRow; 5],
}

impl Default for PeakFeaturesWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl PeakFeaturesWindow {
    #[must_use]
    pub fn new() -> Self {
        let mut win = Self {
            open: false,
            input_energy: 661.66,
            rows: [
                PeakFeatureRow {
                    label: "Backscatter",
                    enabled: true,
                    energy_kev: None,
                    color: Color32::from_rgb(80, 200, 255),
                },
                PeakFeatureRow {
                    label: "Compton Edge",
                    enabled: true,
                    energy_kev: None,
                    color: Color32::from_rgb(255, 190, 60),
                },
                PeakFeatureRow {
                    label: "Photopeak",
                    enabled: true,
                    energy_kev: None,
                    color: Color32::from_rgb(90, 230, 120),
                },
                PeakFeatureRow {
                    label: "1st Escape Peak",
                    enabled: true,
                    energy_kev: None,
                    color: Color32::from_rgb(255, 120, 180),
                },
                PeakFeatureRow {
                    label: "2nd Escape Peak",
                    enabled: true,
                    energy_kev: None,
                    color: Color32::from_rgb(200, 130, 255),
                },
            ],
        };
        win.calculate();
        win
    }

    pub fn open(&mut self) {
        self.open = true;
        self.calculate();
    }

    pub fn calculate(&mut self) {
        let photopeak = self.input_energy.clamp(0.0, 10_000.0);
        let backscatter = photopeak / (1.0 + 2.0 * photopeak / ELECTRON_REST_MASS_KEV);
        let compton_edge = photopeak - backscatter;
        let (first_escape, second_escape) = if photopeak > PAIR_PRODUCTION_THRESHOLD_KEV {
            (
                Some(photopeak - ELECTRON_REST_MASS_KEV),
                Some(ELECTRON_REST_MASS_KEV.mul_add(-2.0, photopeak)),
            )
        } else {
            (None, None)
        };

        self.rows[0].energy_kev = Some(backscatter);
        self.rows[1].energy_kev = Some(compton_edge);
        self.rows[2].energy_kev = Some(photopeak);
        self.rows[3].energy_kev = first_escape;
        self.rows[4].energy_kev = second_escape;
    }

    pub fn apply(&self, spectrum_screen: &mut SpectrumScreen) {
        for row in &self.rows {
            if let Some(val) = row.energy_kev
                && val > 0.0
            {
                spectrum_screen.set_feature_line(row.label, row.enabled, val, row.color);
            } else {
                spectrum_screen.set_feature_line(row.label, false, 0.0, row.color);
            }
        }
    }

    pub fn clear_lines(&self, spectrum_screen: &mut SpectrumScreen) {
        for row in &self.rows {
            spectrum_screen.set_feature_line(row.label, false, 0.0, row.color);
        }
    }

    pub fn render(&mut self, ui: &egui::Ui, spectrum_screen: &mut SpectrumScreen) {
        if !self.open {
            return;
        }

        let mut close_requested = false;
        let mut apply_requested = false;
        let mut clear_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("peak_features_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Peak Features")
            .with_inner_size([360.0, 230.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal(|ui| {
                    ui.label("Input Energy:");
                    if ui
                        .add(
                            egui::DragValue::new(&mut self.input_energy)
                                .range(0.0..=10_000.0)
                                .speed(1.0)
                                .max_decimals(2)
                                .suffix(" keV"),
                        )
                        .changed()
                    {
                        self.calculate();
                    }
                });

                ui.separator();

                egui::Grid::new("peak_features_grid")
                    .num_columns(4)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        for row in &mut self.rows {
                            ui.label(row.label);
                            ui.checkbox(&mut row.enabled, "");
                            let text = row
                                .energy_kev
                                .map_or_else(|| "nan".to_string(), |v| format!("{v:.2} keV"));
                            ui.add_sized([95.0, 20.0], egui::Label::new(text));
                            ui.color_edit_button_srgba(&mut row.color);
                            ui.end_row();
                        }
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Remove Lines").clicked() {
                        clear_requested = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply_requested = true;
                    }
                    if ui.button("OK").clicked() {
                        apply_requested = true;
                        close_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close_requested = true;
                    }
                });
                });
            });

        if clear_requested {
            self.clear_lines(spectrum_screen);
        }
        if apply_requested {
            self.apply(spectrum_screen);
        }
        self.open = !close_requested;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peak_features_cs137_and_k40() {
        let mut win = PeakFeaturesWindow::new();
        win.input_energy = 661.66;
        win.calculate();

        let bs = win.rows[0].energy_kev.unwrap();
        let ce = win.rows[1].energy_kev.unwrap();
        let pp = win.rows[2].energy_kev.unwrap();
        assert!((pp - 661.66).abs() < 1e-6);
        assert!((bs - 184.33).abs() < 0.05);
        assert!((ce - 477.33).abs() < 0.05);
        assert!(win.rows[3].energy_kev.is_none());
        assert!(win.rows[4].energy_kev.is_none());

        win.input_energy = 1460.8;
        win.calculate();
        assert!((win.rows[3].energy_kev.unwrap() - 949.8).abs() < 1e-6);
        assert!((win.rows[4].energy_kev.unwrap() - 438.8).abs() < 1e-6);
    }
}
