//! Spectrum Info bottom tab: table of loaded foreground and background spectra
//! with visibility toggles, colour pickers, and spectrum/background management.
//!
//! Ported from `luracs/gui/tabs/spectrum_info_tab.py`.

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;

use crate::core::{RunManager, SpectrumManager};

pub struct SpectrumInfoTab;

impl Default for SpectrumInfoTab {
    fn default() -> Self {
        Self
    }
}

impl SpectrumInfoTab {
    pub const fn new(_render_state: &RenderState) -> Self {
        Self
    }

    pub fn render(
        &self,
        ui: &mut egui::Ui,
        spectrum_manager: &mut SpectrumManager,
        run_manager: &RunManager,
    ) {
        let mut names: Vec<String> = spectrum_manager.spectrum_registry.keys().cloned().collect();
        names.sort();

        if names.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No spectra loaded.");
            });
            return;
        }

        let mut toggle_vis: Option<(String, bool)> = None;
        let mut remove_spec: Option<String> = None;
        let mut clear_bkg: Option<String> = None;
        let mut disconnect_dev: Option<String> = None;
        let mut new_color: Option<(String, &'static str, Color32)> = None;

        egui::ScrollArea::vertical()
            .id_salt("spectrum_info_table")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("spectrum_info_grid")
                    .num_columns(8)
                    .spacing([12.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("");
                        ui.strong("Spectrum");
                        ui.strong("Type");
                        ui.strong("Counts");
                        ui.strong("Live Time");
                        ui.strong("Real Time");
                        ui.strong("Calibrated");
                        ui.strong("Instrument");
                        ui.end_row();

                        for name in &names {
                            let Some(spect) = spectrum_manager.spectrum_registry.get(name) else {
                                continue;
                            };
                            let instr_name = spect
                                .instrument
                                .as_ref()
                                .map_or_else(|| "None".to_string(), |i| i.name.clone());
                            let calibrated_str = if spect.calibrated { "True" } else { "False" };
                            let show_in_plot = spect.show_in_plot;
                            let connection = spect.connection.clone();

                            if let Some(fg) = spect.foreground.as_ref() {
                                let mut fg_col = spect.color_foreground;
                                ui.horizontal(|ui| {
                                    if ui.color_edit_button_srgba(&mut fg_col).changed() {
                                        new_color = Some((name.clone(), "foreground", fg_col));
                                    }
                                    ui.menu_button("...", |ui| {
                                        let vis_label = if show_in_plot { "Hide" } else { "Show" };
                                        if ui.button(vis_label).clicked() {
                                            toggle_vis = Some((name.clone(), !show_in_plot));
                                            ui.close();
                                        }
                                        ui.separator();
                                        if let Some(conn) = &connection
                                            && ui.button("Disconnect & Remove").clicked()
                                        {
                                            disconnect_dev = Some(conn.clone());
                                            ui.close();
                                        }
                                        if ui.button("Remove Spectrum").clicked() {
                                            remove_spec = Some(name.clone());
                                            ui.close();
                                        }
                                    });
                                });

                                ui.label(name);
                                ui.label("Foreground");
                                ui.label(format_large_u64(fg.total_counts));
                                ui.label(format_hms(fg.live_time));
                                ui.label(
                                    fg.real_time
                                        .map_or_else(|| "None".to_string(), format_hms),
                                );
                                ui.label(calibrated_str);
                                ui.label(&instr_name);
                                ui.end_row();
                            }

                            if let Some(bg) = spect.background.as_ref() {
                                let mut bg_col = spect.color_background;
                                ui.horizontal(|ui| {
                                    if ui.color_edit_button_srgba(&mut bg_col).changed() {
                                        new_color = Some((name.clone(), "background", bg_col));
                                    }
                                    ui.menu_button("...", |ui| {
                                        if ui.button("Remove Background").clicked() {
                                            clear_bkg = Some(name.clone());
                                            ui.close();
                                        }
                                    });
                                });

                                ui.label(name);
                                ui.label("Background");
                                ui.label(format_large_u64(bg.total_counts));
                                ui.label(format_hms(bg.live_time));
                                ui.label(
                                    bg.real_time
                                        .map_or_else(|| "None".to_string(), format_hms),
                                );
                                ui.label(calibrated_str);
                                ui.label(&instr_name);
                                ui.end_row();
                            }
                        }
                    });
            });

        if let Some((name, state)) = toggle_vis {
            drop(spectrum_manager.update_visibility(&name, state));
        }
        if let Some((name, role, color)) = new_color {
            drop(spectrum_manager.set_color(&name, role, color));
        }
        if let Some(name) = clear_bkg {
            drop(spectrum_manager.clear_background(&name));
        }
        if let Some(conn) = disconnect_dev {
            run_manager.remove_device(conn, true);
        } else if let Some(name) = remove_spec {
            spectrum_manager.remove_spectrum(&name);
        }
    }
}

fn format_hms(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "00:00:00".to_string();
    }
    let total = secs.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn format_large_u64(val: u64) -> String {
    let s = val.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(' ');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::PollConfig;
    use crate::containers::SpectrumData;
    use crate::core::ForegroundInput;
    use crate::gui::ColorScheme;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use ndarray::Array1;
    use std::path::Path;

    fn make_data(counts: u64, live_time: f64, real_time: Option<f64>) -> SpectrumData {
        SpectrumData {
            y_axis: Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0]),
            channels: 4,
            total_counts: counts,
            live_time,
            real_time,
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: None,
            instrument: None,
        }
    }

    #[test]
    fn test_spectrum_info_tab_and_formatters() {
        assert_eq!(format_hms(f64::NAN), "00:00:00");
        assert_eq!(format_hms(-5.0), "00:00:00");
        assert_eq!(format_hms(3665.0), "01:01:05");
        assert_eq!(format_large_u64(1_234_567), "1 234 567");

        let dir = std::env::temp_dir().join("luracs_test_spec_info_tab");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();

        let rm = RunManager::new(8, PollConfig::default()).unwrap();
        let mut sm = SpectrumManager::new(
            ColorScheme::default(),
            dir.clone(),
            Path::new("resources/nuclide_data"),
        )
        .unwrap();

        let mut harness = Harness::new_ui_state(
            |ui, (tab, sm, rm): &mut (SpectrumInfoTab, SpectrumManager, RunManager)| {
                tab.render(ui, sm, rm);
            },
            (SpectrumInfoTab, sm, rm),
        );
        harness.run();
        assert!(harness.query_by_label("No spectra loaded.").is_some());

        // Add spectrum with foreground + background + connection
        {
            let sm = &mut harness.state_mut().1;
            sm.create_spectrum("SpecA", 4, Some("Conn1"), false)
                .unwrap();
            sm.set_foreground_spectrum(
                "SpecA",
                ForegroundInput::Raw(make_data(1234, 3665.0, Some(3700.0))),
            )
            .unwrap();
            sm.set_background_spectrum("SpecA", make_data(500, 60.0, None))
                .unwrap();
        }
        harness.run();
        assert!(harness.query_by_label("Foreground").is_some());
        assert!(harness.query_by_label("Background").is_some());
        assert!(harness.query_by_label("1 234").is_some());
        assert!(harness.query_by_label("01:01:05").is_some());

        // Toggle Hide -> Show via foreground "..." menu
        harness.query_all_by_label("...").next().unwrap().click();
        harness.run();
        harness.get_by_label("Hide").click();
        harness.run();
        assert!(!harness.state().1.spectrum_registry["SpecA"].show_in_plot);

        harness.query_all_by_label("...").next().unwrap().click();
        harness.run();
        harness.get_by_label("Show").click();
        harness.run();
        assert!(harness.state().1.spectrum_registry["SpecA"].show_in_plot);

        // Remove Background via background "..." menu (2nd "...")
        harness.query_all_by_label("...").nth(1).unwrap().click();
        harness.run();
        harness.get_by_label("Remove Background").click();
        harness.run();
        assert!(harness.state().1.spectrum_registry["SpecA"].background.is_none());

        // Disconnect & Remove
        harness.get_by_label("...").click();
        harness.run();
        harness.get_by_label("Disconnect & Remove").click();
        harness.run();

        // Remove Spectrum
        harness.get_by_label("...").click();
        harness.run();
        harness.get_by_label("Remove Spectrum").click();
        harness.run();
        assert!(harness.state().1.spectrum_registry.is_empty());

        drop(std::fs::remove_dir_all(&dir));
    }
}
