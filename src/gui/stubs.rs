//! Placeholders for UI modules that have not been ported yet. Each is
//! replaced by its own module; `App` swaps the stub for the real type
//! without changing anything else.

use eframe::egui;

// ---------------------------------------------------------------------------
// Stub screen (top-area tab body)
// ---------------------------------------------------------------------------

pub struct Stub {
    label: &'static str,
}

impl Stub {
    pub const fn new(label: &'static str) -> Self {
        Self { label }
    }

    pub fn render(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.label(format!("{} — not yet ported", self.label));
        });
    }
}

// ---------------------------------------------------------------------------
// Stub window (floating dialog)
// ---------------------------------------------------------------------------

pub struct StubWindow {
    label: &'static str,
    pub open: bool,
}

impl StubWindow {
    pub const fn new(label: &'static str) -> Self {
        Self { label, open: false }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    pub fn render(&mut self, ui: &egui::Ui) {
        if !self.open {
            return;
        }
        let viewport_id = egui::ViewportId::from_hash_of(self.label);
        let builder = egui::ViewportBuilder::default()
            .with_title(self.label)
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([250.0, 150.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    self.open = false;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(format!("{} — not yet ported", self.label));
                    });
                });
            });
    }
}
