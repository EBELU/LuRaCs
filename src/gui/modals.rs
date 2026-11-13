//! Centralized, viewport-scoped modal stack.
//!
//! Modals live in `App::modal_stack: Vec<Modal>`. Each `Modal` carries a
//! `target: ViewportId` — the window that will render it. This lets future
//! child viewports (Data Library, Gamma Tools) own their modal layer
//! without cross-contamination.
//!
//! # Rendering contract
//!
//! The end of every viewport's `ui` closure is:
//!
//! ```ignore
//! crate::gui::modals::render_modals_for(self, egui::ViewportId::ROOT, ui);
//! ```
//!
//! Only modals whose `target == my_viewport_id` are drawn, and only the
//! topmost one at a time (the rest queue). Pushing is immediate — any widget
//! can do `app.modal_stack.push(Modal::root(ModalKind::Info { .. }))`
//! without knowing who will render it.
//!
//! # Confirmation actions
//!
//! `ConfirmAction` is an enum for common cases (menu items, lifecycle
//! actions), with a `Custom(Box<dyn FnOnce>)` escape hatch for one-offs.
//! The closure is `FnOnce` because confirming consumes it; it doesn't hold
//! a reference to `App`, so calling it while `App` is `&mut` is safe.
//!
//! # Scope
//!
//! The stack is entirely on the UI thread. No `Send` bounds, no locks, no
//! channels. A future async-driven modal (e.g. "device disconnecting…")
//! would need one of those, but at that point the modal isn't a confirm /
//! info popup — it's a status widget, and belongs in the widget that owns
//! the operation.

use eframe::egui;
use eframe::egui::ViewportId;

use crate::app::LuRaCsApp;

// ===========================================================================
// Modal
// ===========================================================================

pub struct Modal {
    /// Which window renders this modal.
    pub target: ViewportId,
    pub kind: ModalKind,
}

impl Modal {
    pub fn new(target: ViewportId, kind: ModalKind) -> Self {
        Self { target, kind }
    }

    /// Convenience for the main window.
    pub fn root(kind: ModalKind) -> Self {
        Self {
            target: ViewportId::ROOT,
            kind,
        }
    }
}

// ===========================================================================
// Kinds
// ===========================================================================

pub enum ModalKind {
    Confirm(ConfirmModal),
    Info(InfoModal),
}

pub struct ConfirmModal {
    pub title: String,
    pub message: String,
    /// `None` after the user confirms (consumed once).
    action: Option<ConfirmAction>,
}

impl ConfirmModal {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            action: Some(action),
        }
    }
}

pub struct InfoModal {
    pub title: String,
    pub message: String,
}

impl InfoModal {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
        }
    }
}

// ===========================================================================
// Confirm actions
// ===========================================================================

pub enum ConfirmAction {
    /// `RunManager::reset_all_spectra`.
    ResetAllSpectra,
    /// `RunManager::remove_all_devices`.
    DisconnectAll,
    /// Enter Si-mode. Dispatched via `App::set_si_mode(true)`.
    EnterSiMode,
    /// Exit Si-mode. Dispatched via `App::set_si_mode(false)`.
    ExitSiMode,
    /// Escape hatch for ad-hoc actions.
    Custom(Box<dyn FnOnce(&mut LuRaCsApp)>),
}

// ===========================================================================
// Rendering
// ===========================================================================

enum ModalOutcome {
    Pending,
    Accepted,
    Cancelled,
}

impl ModalKind {
    fn render(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) -> ModalOutcome {
        match self {
            ModalKind::Confirm(c) => render_confirm(c, app, ui),
            ModalKind::Info(i) => render_info(i, ui),
        }
    }
}

fn render_confirm(c: &mut ConfirmModal, app: &mut LuRaCsApp, ui: &mut egui::Ui) -> ModalOutcome {
    let mut decision: Option<bool> = None;

    egui::Window::new(&c.title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(&c.message);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });

    match decision {
        Some(true) => {
            if let Some(action) = c.action.take() {
                dispatch_action(app, action);
            }
            ModalOutcome::Accepted
        }
        Some(false) => ModalOutcome::Cancelled,
        None => ModalOutcome::Pending,
    }
}

fn render_info(i: &InfoModal, ui: &mut egui::Ui) -> ModalOutcome {
    let mut accepted = false;
    egui::Window::new(&i.title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(&i.message);
            ui.add_space(8.0);
            if ui.button("OK").clicked() {
                accepted = true;
            }
        });
    if accepted {
        ModalOutcome::Accepted
    } else {
        ModalOutcome::Pending
    }
}

fn dispatch_action(app: &mut LuRaCsApp, action: ConfirmAction) {
    match action {
        ConfirmAction::ResetAllSpectra => app.run_manager.reset_all_spectra(),
        ConfirmAction::DisconnectAll => app.run_manager.remove_all_devices(),
        // Route through `set_si_mode` so the top-tab switch happens too.
        ConfirmAction::EnterSiMode => app.set_si_mode(true),
        ConfirmAction::ExitSiMode => app.set_si_mode(false),
        ConfirmAction::Custom(f) => f(app),
    }
}

// ===========================================================================
// Per-viewport rendering
// ===========================================================================

/// Render any modals targeted at `viewport`. Call this at the end of the
/// `ui` closure for each window/viewport. Only the topmost modal for this
/// viewport is shown; the rest wait in the stack.
pub fn render_modals_for(app: &mut LuRaCsApp, viewport: ViewportId, ui: &mut egui::Ui) {
    // Topmost modal for this viewport
    let Some(idx) = app.modal_stack.iter().rposition(|m| m.target == viewport) else {
        return;
    };

    // Move the modal out of the stack so `render` can safely `&mut app`
    // (including pushing new modals via `dispatch_action`).
    let mut modal = app.modal_stack.remove(idx);
    let outcome = modal.kind.render(app, ui);

    match outcome {
        ModalOutcome::Pending => {
            // Re-insert at the same position so ordering relative to
            // modals for other viewports is preserved.
            app.modal_stack.insert(idx, modal);
        }
        ModalOutcome::Accepted | ModalOutcome::Cancelled => {
            // Modal consumed; dropped.
        }
    }
}
