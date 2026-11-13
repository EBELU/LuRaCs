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
    pub fn new(label: &'static str) -> Self {
        Self { label }
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
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
    pub fn new(label: &'static str) -> Self {
        Self { label, open: false }
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn render(&mut self, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }
        egui::Window::new(self.label)
            .open(&mut self.open)
            .default_size([400.0, 300.0])
            .show(ui.ctx(), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(format!("{} — not yet ported", self.label));
                });
            });
    }
}
