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
    pub const fn new(target: ViewportId, kind: ModalKind) -> Self {
        Self { target, kind }
    }

    /// Convenience for the main window.
    pub const fn root(kind: ModalKind) -> Self {
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
    /// The user clicked Cancel in the settings dialog with pending
    /// unapplied changes and confirmed the discard prompt.
    DiscardSettingsChanges,
}

// ===========================================================================
// Rendering
// ===========================================================================

enum ModalOutcome {
    Pending,
    Accepted(Option<ConfirmAction>),
    Cancelled,
}

impl ModalKind {
    fn render(&mut self, ui: &egui::Ui) -> ModalOutcome {
        match self {
            Self::Confirm(c) => render_confirm(c, ui),
            Self::Info(i) => render_info(i, ui),
        }
    }
}

fn render_confirm(c: &mut ConfirmModal, ui: &egui::Ui) -> ModalOutcome {
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
        Some(true) => ModalOutcome::Accepted(c.action.take()),
        Some(false) => ModalOutcome::Cancelled,
        None => ModalOutcome::Pending,
    }
}

fn render_info(i: &InfoModal, ui: &egui::Ui) -> ModalOutcome {
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
        ModalOutcome::Accepted(None)
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
        ConfirmAction::DiscardSettingsChanges => {
            app.settings_dialog.open = false;
        }
    }
}

// ===========================================================================
// Per-viewport rendering
// ===========================================================================

/// Whether there are any modals currently queued targeting `viewport`.
pub fn has_modal_for(app: &LuRaCsApp, viewport: ViewportId) -> bool {
    app.modal_stack.iter().any(|m| m.target == viewport)
}

/// Render any modals targeted at `viewport`. Call this at the end of the
/// `ui` closure for each window/viewport. Only the topmost modal for this
/// viewport is shown; the rest wait in the stack.
pub fn render_modals_for(app: &mut LuRaCsApp, viewport: ViewportId, ui: &egui::Ui) {
    if let Some(action) = render_modals_in_stack(&mut app.modal_stack, viewport, ui) {
        dispatch_action(app, action);
    }
}

fn render_modals_in_stack(
    stack: &mut Vec<Modal>,
    viewport: ViewportId,
    ui: &egui::Ui,
) -> Option<ConfirmAction> {
    let idx = stack.iter().rposition(|m| m.target == viewport)?;
    let mut modal = stack.remove(idx);
    match modal.kind.render(ui) {
        ModalOutcome::Pending => {
            stack.insert(idx, modal);
            None
        }
        ModalOutcome::Accepted(action) => action,
        ModalOutcome::Cancelled => None,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_viewport_targeting() {
        let root_modal = Modal::root(ModalKind::Info(InfoModal::new("Root", "Msg")));
        assert_eq!(root_modal.target, ViewportId::ROOT);

        let settings_id = ViewportId::from_hash_of(crate::gui::settings_window::VIEWPORT_ID);
        let settings_modal = Modal::new(
            settings_id,
            ModalKind::Info(InfoModal::new("Settings", "Msg")),
        );
        assert_eq!(settings_modal.target, settings_id);
        assert_ne!(settings_modal.target, ViewportId::ROOT);

        let deconv_id = ViewportId::from_hash_of(crate::gui::deconvolution_window::VIEWPORT_ID);
        let deconv_modal = Modal::new(
            deconv_id,
            ModalKind::Info(InfoModal::new("Deconv", "Msg")),
        );
        assert_eq!(deconv_modal.target, deconv_id);
        assert_ne!(deconv_modal.target, settings_id);
    }

    #[test]
    fn test_modals_kittest_confirm_cancel_and_info() {
        use egui_kittest::kittest::Queryable;
        use egui_kittest::Harness;

        #[derive(Default)]
        struct State {
            stack: Vec<Modal>,
            actions: Vec<ConfirmAction>,
        }

        let mut harness = Harness::new_ui_state(
            |ui, st: &mut State| {
                if let Some(act) = render_modals_in_stack(&mut st.stack, ViewportId::ROOT, ui) {
                    st.actions.push(act);
                }
            },
            State {
                stack: vec![
                    Modal::root(ModalKind::Info(InfoModal::new("InfoTitle", "InfoMsg"))),
                    Modal::new(
                        ViewportId::from_hash_of("other"),
                        ModalKind::Info(InfoModal::new("Other", "KeepMe")),
                    ),
                    Modal::root(ModalKind::Confirm(ConfirmModal::new(
                        "ConfirmCancel",
                        "CancelMsg",
                        ConfirmAction::DisconnectAll,
                    ))),
                    Modal::root(ModalKind::Confirm(ConfirmModal::new(
                        "ConfirmOk",
                        "OkMsg",
                        ConfirmAction::ResetAllSpectra,
                    ))),
                ],
                actions: Vec::new(),
            },
        );

        // Topmost ROOT modal is ConfirmOk
        harness.run();
        assert!(harness.query_by_label("OkMsg").is_some());
        harness.get_by_label("OK").click();
        harness.run();
        assert_eq!(harness.state().actions.len(), 1);
        assert!(matches!(harness.state().actions[0], ConfirmAction::ResetAllSpectra));

        // Next ROOT modal is ConfirmCancel
        assert!(harness.query_by_label("CancelMsg").is_some());
        harness.get_by_label("Cancel").click();
        harness.run();
        assert_eq!(harness.state().actions.len(), 1); // Cancel does not push action

        // Next ROOT modal is InfoTitle
        assert!(harness.query_by_label("InfoMsg").is_some());
        harness.get_by_label("OK").click();
        harness.run();

        // Only the "other" viewport modal remains
        assert_eq!(harness.state().stack.len(), 1);
        assert_eq!(
            harness.state().stack[0].target,
            ViewportId::from_hash_of("other")
        );
    }
}
