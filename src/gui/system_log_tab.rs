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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    #[test]
    fn test_system_log_tab_filter_toggle_and_clear() {
        let buf = Arc::new(LogBuffer::new(16));
        buf.push("[INFO ] app: started".into());
        buf.push("[ERROR] link: connection lost".into());

        let tab = SystemLogTab::default();
        let buf_ref = Arc::clone(&buf);
        let mut harness = Harness::new_ui_state(
            |ui, (tab, buf): &mut (SystemLogTab, Arc<LogBuffer>)| {
                tab.render(ui, buf);
            },
            (tab, buf_ref),
        );

        harness.run();
        assert!(harness.query_by_label("[INFO ] app: started").is_some());
        assert!(harness.query_by_label("[ERROR] link: connection lost").is_some());

        // Filter to only "error"
        harness.state_mut().0.filter = "error".into();
        harness.run();
        assert!(harness.query_by_label("[INFO ] app: started").is_none());
        assert!(harness.query_by_label("[ERROR] link: connection lost").is_some());

        // Clear filter via "×" button
        harness.get_by_label("×").click();
        harness.run();
        assert!(harness.state().0.filter.is_empty());
        assert!(harness.query_by_label("[INFO ] app: started").is_some());

        // Toggle Auto-scroll
        harness.get_by_label("Auto-scroll").click();
        harness.run();
        assert!(!harness.state().0.auto_scroll);

        // Clear buffer via "Clear" button
        harness.get_by_label("Clear").click();
        harness.run();
        assert_eq!(buf.len(), 0);
    }
}
