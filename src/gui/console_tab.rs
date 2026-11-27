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
        let mut submitted: Option<String> = None;

        egui::Panel::bottom("console_input_bar")
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.console_tab.input)
                            .desired_width(ui.available_width() - 70.0)
                            .hint_text("Enter command…")
                            .font(egui::TextStyle::Monospace),
                    );

                    if resp.has_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                            && !app.console_tab.history.is_empty()
                        {
                            app.console_tab.history_idx =
                                app.console_tab.history_idx.saturating_sub(1);
                            if let Some(cmd) =
                                app.console_tab.history.get(app.console_tab.history_idx)
                            {
                                app.console_tab.input = cmd.clone();
                            }
                        } else if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                            && !app.console_tab.history.is_empty()
                        {
                            let len = app.console_tab.history.len();
                            app.console_tab.history_idx =
                                (app.console_tab.history_idx + 1).min(len);
                            app.console_tab.input = app
                                .console_tab
                                .history
                                .get(app.console_tab.history_idx)
                                .cloned()
                                .unwrap_or_default();
                        }
                    }

                    let enter_pressed =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (ui.button("Enter").clicked() || enter_pressed)
                        && !app.console_tab.input.trim().is_empty()
                    {
                        submitted = Some(app.console_tab.input.trim().to_string());
                        app.console_tab.input.clear();
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
                    for line in &app.console_tab.output {
                        ui.monospace(line);
                    }
                });
        });

        if let Some(cmd) = submitted {
            app.console_tab.execute(app.run_manager.list_devices(), &app.spectrum_manager, &cmd);
            // Run side-effect commands that mutate managers.
            match cmd.as_str() {
                "start" => app.run_manager.start_acquisition_all(),
                "stop" => app.run_manager.stop_acquisition_all(),
                "reset" => app.run_manager.reset_all_spectra(),
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
