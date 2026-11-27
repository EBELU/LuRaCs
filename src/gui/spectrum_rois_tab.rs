//! Spectrum ROIs bottom tab: table of fitted ROI parameters and counts,
//! CPS toggle, Clear ROIs, and ROI Info viewer.
//!
//! Ported from `luracs/gui/tabs/roi_info_tab.py`.

use eframe::egui;

use crate::containers::CountField;
use crate::core::SpectrumManager;
use crate::gui::{ConfirmAction, ConfirmModal, Modal, ModalKind};
use crate::settings::SharedSettingsExt;

pub struct SpectrumRoisTab {
    cps: bool,
    info_open: bool,
    info_selected_tag: String,
}

impl Default for SpectrumRoisTab {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumRoisTab {
    pub const fn new() -> Self {
        Self {
            cps: false,
            info_open: false,
            info_selected_tag: String::new(),
        }
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        spectrum_manager: &mut SpectrumManager,
        modal_stack: &mut Vec<Modal>,
        use_chi2_weight: bool,
    ) {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.cps, "CPS").changed() {
                spectrum_manager.roi_manager.set_cps(self.cps);
            }
            if ui.button("Clear ROIs").clicked() {
                modal_stack.push(Modal::root(ModalKind::Confirm(ConfirmModal::new(
                    "Clear ROIs",
                    "Clear all ROIs?",
                    ConfirmAction::Custom(Box::new(move |app| {
                        let chi2 = app.settings.read_settings().optimizer.use_chi2_weight;
                        app.spectrum_manager
                            .roi_manager
                            .clear_all(&mut app.spectrum_manager.spectrum_registry, chi2);
                    })),
                ))));
            }
            if ui.button("ROI Info").clicked() {
                self.info_open = true;
                if self.info_selected_tag.is_empty()
                    && let Some(first) = spectrum_manager.roi_manager.registry().keys().min()
                {
                    self.info_selected_tag = first.clone();
                }
            }
            let _ = use_chi2_weight;
        });

        ui.separator();

        // Collect (roi_tag, spectrum_name) pairs sorted deterministically.
        let mut spec_names: Vec<String> = spectrum_manager.spectrum_registry.keys().cloned().collect();
        spec_names.sort();

        let mut roi_tags: Vec<String> = spectrum_manager
            .roi_manager
            .registry()
            .keys()
            .cloned()
            .collect();
        roi_tags.sort();

        if roi_tags.is_empty() || spec_names.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No ROIs.");
            });
        } else {
            egui::ScrollArea::both()
                .id_salt("spectrum_rois_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("spectrum_rois_grid")
                        .num_columns(13)
                        .spacing([10.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for h in [
                                "ROI",
                                "Spectrum",
                                "Lower",
                                "Upper",
                                "Peak Center",
                                "FWHM",
                                "Peak Area",
                                "ROI Counts",
                                "N Counts",
                                "B Counts",
                                "Nuclide",
                                "Photo Peak",
                                "Peak Difference",
                            ] {
                                ui.strong(h);
                            }
                            ui.end_row();

                            for tag in &roi_tags {
                                let Some(base_roi) = spectrum_manager.roi_manager.get_roi(tag) else {
                                    continue;
                                };
                                for spec_name in &spec_names {
                                    if let Some(owner) = &base_roi.owner_spectrum
                                        && owner != spec_name
                                    {
                                        continue;
                                    }
                                    let Some(spec) = spectrum_manager.spectrum_registry.get(spec_name)
                                    else {
                                        continue;
                                    };
                                    let Some(roi) = spec.rois.get(tag) else {
                                        continue;
                                    };

                                    if !spec.show_in_plot {
                                        ui.label(&roi.alias);
                                        ui.label(spec_name);
                                        ui.label(format!("{:.0} keV", roi.roi_bound.0));
                                        ui.label(format!("{:.0} keV", roi.roi_bound.1));
                                        ui.label("Spectrum Hidden");
                                        for _ in 0..8 {
                                            ui.label("None");
                                        }
                                        ui.end_row();
                                        continue;
                                    }

                                    let roi_counts = format_count(
                                        roi.get_count_data(CountField::RoiCounts, self.cps),
                                        self.cps,
                                    );

                                    if let Some(fit) = &roi.fit {
                                        let peak_area = format_val_err(
                                            roi.get_count_data(CountField::PeakArea, self.cps),
                                            roi.get_count_data(CountField::PeakAreaErr, self.cps),
                                            self.cps,
                                        );
                                        let n_str = format_val_err(
                                            roi.get_count_data(CountField::N, self.cps),
                                            roi.get_count_data(CountField::NErr, self.cps),
                                            self.cps,
                                        );
                                        let b_str = format_val_err(
                                            roi.get_count_data(CountField::B, self.cps),
                                            roi.get_count_data(CountField::BErr, self.cps),
                                            self.cps,
                                        );

                                        let (nuc_name, photo_peak, diff_str) =
                                            if let Some(em) = &roi.emission {
                                                let n = em
                                                    .parent_nuclide
                                                    .as_ref()
                                                    .map_or_else(|| "Annihilation".into(), |id| id.to_name());
                                                if let Some(ekev) = &em.energy_kev {
                                                    let e = ekev.value.mean();
                                                    (
                                                        n,
                                                        format!("{e:.2} keV"),
                                                        format!("{:.3} keV", fit.mu() - e),
                                                    )
                                                } else {
                                                    (n, "None".into(), "None".into())
                                                }
                                            } else {
                                                ("None".into(), "None".into(), "None".into())
                                            };

                                        ui.label(&roi.alias);
                                        ui.label(spec_name);
                                        ui.label(format!("{:.0} keV", fit.lower));
                                        ui.label(format!("{:.0} keV", fit.upper));
                                        ui.label(format!("{:.2} ± {:.2} keV", fit.mu(), fit.mu_err()));
                                        ui.label(format!(
                                            "{:.2} ± {:.2} keV",
                                            fit.fwhm(),
                                            fit.fwhm_err()
                                        ));
                                        ui.label(peak_area);
                                        ui.label(roi_counts);
                                        ui.label(n_str);
                                        ui.label(b_str);
                                        ui.label(nuc_name);
                                        ui.label(photo_peak);
                                        ui.label(diff_str);
                                    } else {
                                        ui.label(&roi.alias);
                                        ui.label(spec_name);
                                        ui.label(format!("{:.0} keV", roi.roi_bound.0));
                                        ui.label(format!("{:.0} keV", roi.roi_bound.1));
                                        ui.label("No Fit");
                                        ui.label("None");
                                        ui.label("None");
                                        ui.label(roi_counts);
                                        for _ in 0..5 {
                                            ui.label("None");
                                        }
                                    }
                                    ui.end_row();
                                }
                            }
                        });
                });
        }

        if self.info_open {
            self.render_info_dialog(ui, spectrum_manager);
        }
    }

    fn render_info_dialog(&mut self, ui: &egui::Ui, spectrum_manager: &SpectrumManager) {
        let mut open = self.info_open;
        let mut tags: Vec<String> = spectrum_manager
            .roi_manager
            .registry()
            .keys()
            .cloned()
            .collect();
        tags.sort();
        if !tags.contains(&self.info_selected_tag) {
            self.info_selected_tag = tags.first().cloned().unwrap_or_default();
        }

        egui::Window::new("ROI Info Viewer")
            .open(&mut open)
            .default_size([500.0, 400.0])
            .resizable(true)
            .show(ui.ctx(), |ui| {
                let selected_label = spectrum_manager
                    .roi_manager
                    .get_roi(&self.info_selected_tag)
                    .map_or_else(|| "(none)".to_string(), |r| r.alias.clone());

                egui::ComboBox::from_id_salt("roi_info_combo")
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        for tag in &tags {
                            let label = spectrum_manager
                                .roi_manager
                                .get_roi(tag)
                                .map_or_else(|| tag.clone(), |r| r.alias.clone());
                            ui.selectable_value(&mut self.info_selected_tag, tag.clone(), label);
                        }
                    });

                ui.separator();

                let text = format_roi_info_text(&self.info_selected_tag, spectrum_manager);
                egui::ScrollArea::vertical()
                    .id_salt("roi_info_text_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.monospace(text);
                    });
            });
        self.info_open = open;
    }
}

fn format_roi_info_text(tag: &str, spectrum_manager: &SpectrumManager) -> String {
    if tag.is_empty() {
        return "No ROI selected.".to_string();
    }
    let mut spec_names: Vec<String> = spectrum_manager.spectrum_registry.keys().cloned().collect();
    spec_names.sort();

    let mut sections = Vec::new();
    for spec_name in spec_names {
        let Some(spec) = spectrum_manager.spectrum_registry.get(&spec_name) else {
            continue;
        };
        let Some(roi) = spec.rois.get(tag) else {
            continue;
        };
        let header = format!("| {spec_name} |");
        let sep = "=".repeat(header.len());
        let mut lines = vec![
            sep.clone(),
            header,
            sep,
            format!("Tag: {}", roi.tag),
            format!("Spectrum: {}", roi.spectrum),
            format!("Alias: {}", roi.alias),
            format!("ROI Bound: ({:.3}, {:.3})", roi.roi_bound.0, roi.roi_bound.1),
            format!(
                "Region Bound: ({:.3}, {:.3})",
                roi.region_bound.0, roi.region_bound.1
            ),
            format!("ROI Counts: {:.5}", roi.roi_counts),
            format!("Live Time: {:.5}", roi.live_time),
            format!("Background Type: {:?}", roi.bkg_type),
            format!("Fit Type: {:?}", roi.fit_type),
        ];
        if let Some(f) = &roi.fit {
            lines.extend([
                String::new(),
                "-- Fit Info --".to_string(),
                format!(
                    "Region: [{}, {}]",
                    f.region_lower.map_or_else(|| "None".into(), |v| format!("{v:.5}")),
                    f.region_upper.map_or_else(|| "None".into(), |v| format!("{v:.5}")),
                ),
                format!("Bounds: [{:.5}, {:.5}]", f.lower, f.upper),
                format!("A = {:.5} ± {:.5}", f.a(), f.a_err()),
                format!("mu = {:.5} ± {:.5}", f.mu(), f.mu_err()),
                format!("sigma = {:.5} ± {:.5}", f.sigma(), f.sigma_err()),
                format!("FWHM = {:.5} ± {:.5}", f.fwhm(), f.fwhm_err()),
                format!("G = {:.5}", f.g),
                format!("B = {:.5}", f.b),
                format!("N = {:.5}", f.n),
                format!("Peak Counts = {:.5}", f.peak_area),
            ]);
        } else {
            lines.push("\n-- No Fit --".to_string());
        }
        sections.push(lines.join("\n"));
    }

    if sections.is_empty() {
        "No spectrum data for this ROI.".to_string()
    } else {
        sections.join("\n\n")
    }
}

fn format_count(val: Option<f64>, cps: bool) -> String {
    match val {
        Some(v) if cps => format!("{v:.4} CPS"),
        Some(v) => format_grouped_int(v.round() as i64),
        None => "None".to_string(),
    }
}

fn format_val_err(val: Option<f64>, err: Option<f64>, cps: bool) -> String {
    match (val, err) {
        (Some(v), Some(e)) if cps => format!("{v:.4} ± {e:.4} CPS"),
        (Some(v), Some(e)) => format!(
            "{} ± {}",
            format_grouped_int(v.round() as i64),
            format_grouped_int(e.round() as i64)
        ),
        _ => "None".to_string(),
    }
}

fn format_grouped_int(v: i64) -> String {
    let neg = v < 0;
    let s = v.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3 + 1);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    if neg {
        out.push('-');
    }
    out.chars().rev().collect()
}
