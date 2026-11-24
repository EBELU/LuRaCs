use crate::app::LuRaCsApp;
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use crate::core::{SpectrumManager};

pub struct SpectrumInfoTab {
    value: i32,
}

impl SpectrumInfoTab {
    pub fn new(render_state: &RenderState) -> Self {
        Self {
            value: 1,
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, spectrum_manager: &SpectrumManager) {
        egui::ScrollArea::vertical()
            .id_salt("spectrum_info_table")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("spectrum_info_grid")
                    .num_columns(3)
                    .spacing([10.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Table headers
                        ui.strong("");
                        ui.strong("Name");
                        ui.strong("Type");
                        ui.strong("Counts");
                        ui.strong("Live Time");
                        ui.strong("Real Time");
                        ui.strong("Calibrated");
                        ui.strong("Connection");
                        ui.end_row();

                        // Sort spectrum names alphabetically
                        let mut names: Vec<_> =
                            spectrum_manager.spectrum_registry.keys().collect();

                        names.sort();

                        for name in names {
                            let spect = &spectrum_manager.spectrum_registry[name];
                            
                            if let Some(fg) = spect.foreground.as_ref() {
                                ui.menu_button("...", |ui|{});
                                ui.label(name);
                                ui.label("Foreground");
                                ui.label(fg.total_counts.to_string());

                                let seconds = fg.live_time.round() as u64;

                                let hours = seconds / 3600;
                                let minutes = (seconds % 3600) / 60;
                                let seconds = seconds % 60;

                                ui.label(format!("{:02}:{:02}:{:02}", hours, minutes, seconds));
                                let seconds = fg.real_time.unwrap_or(0.).round() as u64;

                                let hours = seconds / 3600;
                                let minutes = (seconds % 3600) / 60;
                                let seconds = seconds % 60;

                                ui.label(format!("{:02}:{:02}:{:02}", hours, minutes, seconds));
                                
                                ui.label(spect.calibrated.to_string());
                                ui.label(spect.connection.clone().unwrap_or("None".to_string()));

                                ui.end_row();
                            }

                            if let Some(bg) = spect.background.as_ref() {
                                ui.menu_button("...", |ui|{});
                                ui.label(name);
                                ui.label("Background");
                                ui.label(bg.total_counts.to_string());

                                let seconds = bg.live_time.round() as u64;

                                let hours = seconds / 3600;
                                let minutes = (seconds % 3600) / 60;
                                let seconds = seconds % 60;

                                ui.label(format!("{:02}:{:02}:{:02}", hours, minutes, seconds));
                                let seconds = bg.real_time.unwrap_or(0.).round() as u64;

                                let hours = seconds / 3600;
                                let minutes = (seconds % 3600) / 60;
                                let seconds = seconds % 60;

                                ui.label(format!("{:02}:{:02}:{:02}", hours, minutes, seconds));
                                
                                ui.label(spect.calibrated.to_string());
                                ui.label(spect.connection.clone().unwrap_or("None".to_string()));

                                ui.end_row();
                            }

                            ui.end_row();
                        }
                    });
            });
    }
}