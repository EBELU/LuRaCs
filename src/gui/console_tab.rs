//! Console bottom tab: interactive command input with history and output log.
//!
//! Ported from `luracs/gui/tabs/console_tab.py`.

use std::collections::VecDeque;

use eframe::egui;

use crate::app::LuRaCsApp;

const MAX_HISTORY: usize = 200;

pub struct ConsoleTab {
    history: VecDeque<String>,
    history_idx: usize,
    input: String,
    output: Vec<String>,
}

impl Default for ConsoleTab {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleTab {
    pub fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(MAX_HISTORY),
            history_idx: 0,
            input: String::new(),
            output: vec!["LuRaCs Console — type 'help' for available commands.".to_string()],
        }
    }

    pub fn render(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        app.console_tab
            .render_with(ui, &app.run_manager, &app.spectrum_manager);
    }

    pub fn render_with(
        &mut self,
        ui: &mut egui::Ui,
        run_manager: &crate::core::RunManager,
        spectrum_manager: &crate::core::SpectrumManager,
    ) {
        let mut submitted: Option<String> = None;

        egui::Panel::bottom("console_input_bar")
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.input)
                            .desired_width(ui.available_width() - 70.0)
                            .hint_text("Enter command…")
                            .font(egui::TextStyle::Monospace),
                    );

                    if resp.has_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                            && !self.history.is_empty()
                        {
                            self.history_idx = self.history_idx.saturating_sub(1);
                            if let Some(cmd) = self.history.get(self.history_idx) {
                                self.input = cmd.clone();
                            }
                        } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                            && !self.history.is_empty()
                        {
                            let len = self.history.len();
                            self.history_idx = (self.history_idx + 1).min(len);
                            self.input = self
                                .history
                                .get(self.history_idx)
                                .cloned()
                                .unwrap_or_default();
                        }
                    }

                    let enter_pressed =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button("Enter").clicked() || enter_pressed)
                        && !self.input.trim().is_empty()
                    {
                        submitted = Some(self.input.trim().to_string());
                        self.input.clear();
                        resp.request_focus();
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("console_output_scroll")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.output {
                        ui.monospace(line);
                    }
                });
        });

        if let Some(cmd) = submitted {
            self.execute(run_manager.list_devices(), spectrum_manager, &cmd);
            // Run side-effect commands that mutate managers.
            match cmd.as_str() {
                "start" => run_manager.start_acquisition_all(),
                "stop" => run_manager.stop_acquisition_all(),
                "reset" => run_manager.reset_all_spectra(),
                _ => {}
            }
        }
    }

    fn execute(
        &mut self,
        devices: Vec<String>,
        spectrum_manager: &crate::core::SpectrumManager,
        cmd: &str,
    ) {
        if self.history.len() == MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(cmd.to_string());
        self.history_idx = self.history.len();
        self.output.push(format!("> {cmd}"));

        match cmd {
            "help" => {
                self.output.push(
                    "Commands: help, clear, devices, spectra, rois, start, stop, reset"
                        .to_string(),
                );
            }
            "clear" => {
                self.output.clear();
            }
            "devices" => {
                if devices.is_empty() {
                    self.output.push("(no devices connected)".to_string());
                } else {
                    self.output.push(devices.join(", "));
                }
            }
            "spectra" => {
                let mut names: Vec<&String> = spectrum_manager.spectrum_registry.keys().collect();
                names.sort();
                if names.is_empty() {
                    self.output.push("(no spectra loaded)".to_string());
                } else {
                    self.output.push(
                        names
                            .into_iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
            }
            "rois" => {
                let mut tags: Vec<&String> =
                    spectrum_manager.roi_manager.registry().keys().collect();
                tags.sort();
                if tags.is_empty() {
                    self.output.push("(no ROIs defined)".to_string());
                } else {
                    self.output.push(
                        tags.into_iter()
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
            }
            "start" => self.output.push("Started acquisition on all devices.".to_string()),
            "stop" => self.output.push("Stopped acquisition on all devices.".to_string()),
            "reset" => self.output.push("Reset spectra on all devices.".to_string()),
            _ => self.output.push(format!("Unknown command: {cmd}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::PollConfig;
    use crate::core::{NewRoi, RunManager, SpectrumManager};
    use crate::gui::ColorScheme;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use std::path::Path;

    #[test]
    fn test_console_tab_ui_commands_and_history() {
        let dir = std::env::temp_dir().join("luracs_test_console_tab");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();

        let rm = RunManager::new(8, PollConfig::default()).unwrap();
        let mut sm = SpectrumManager::new(
            ColorScheme::default(),
            dir.clone(),
            Path::new("resources/nuclide_data"),
        )
        .unwrap();
        sm.roi_manager.add_roi(NewRoi::new(10.0, 20.0));

        let mut harness = Harness::new_ui_state(
            |ui, (tab, rm, sm): &mut (ConsoleTab, RunManager, SpectrumManager)| {
                tab.render_with(ui, rm, sm);
            },
            (ConsoleTab::default(), rm, sm),
        );
        harness.run();
        assert!(harness
            .query_by_label("LuRaCs Console — type 'help' for available commands.")
            .is_some());

        for cmd in [
            "help", "devices", "spectra", "rois", "start", "stop", "reset", "bogus",
        ] {
            harness.state_mut().0.input = cmd.into();
            harness.get_by_label("Enter").click();
            harness.run();
        }
        assert!(harness
            .query_by_label("Unknown command: bogus")
            .is_some());

        // Clear output via "clear" command
        harness.state_mut().0.input = "clear".into();
        harness.get_by_label("Enter").click();
        harness.run();
        assert!(harness.state().0.output.is_empty());

        // History bounded + ArrowUp/ArrowDown navigation
        harness.get_by_role(egui::accesskit::Role::TextInput).focus();
        harness.run();
        harness.key_press(egui::Key::ArrowUp);
        harness.run();
        assert_eq!(harness.state().0.input, "clear");
        harness.key_press(egui::Key::ArrowDown);
        harness.run();
        assert!(harness.state().0.input.is_empty());

        // Direct execute with populated devices + spectra
        {
            let (tab, _, sm) = harness.state_mut();
            tab.execute(vec!["Det1".into()], sm, "devices");
        }
        assert!(harness
            .state()
            .0
            .output
            .last()
            .unwrap()
            .contains("Det1"));

        drop(std::fs::remove_dir_all(&dir));
    }
}
