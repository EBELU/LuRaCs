//! System Log bottom tab: live scrolling view of the application log buffer.
//!
//! Ported from `luracs/gui/tabs/logger_tab.py`.

use std::sync::Arc;

use eframe::egui;

use crate::logging::LogBuffer;

pub struct SystemLogTab {
    filter: String,
    auto_scroll: bool,
}

impl Default for SystemLogTab {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemLogTab {
    pub const fn new() -> Self {
        Self {
            filter: String::new(),
            auto_scroll: true,
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, log_buffer: &Arc<LogBuffer>) {
        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(220.0)
                    .hint_text("Search logs…"),
            );
            if !self.filter.is_empty() && ui.button("×").clicked() {
                self.filter.clear();
            }
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            if ui.button("Clear").clicked() {
                log_buffer.clear();
            }
        });

        ui.separator();

        let lines = log_buffer.snapshot();
        let needle = self.filter.trim().to_ascii_lowercase();

        egui::ScrollArea::both()
            .id_salt("system_log_scroll")
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show(ui, |ui| {
                for line in &lines {
                    if !needle.is_empty() && !line.to_ascii_lowercase().contains(&needle) {
                        continue;
                    }
                    ui.monospace(line);
                }
            });
    }
}
