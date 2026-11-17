// src/roi_view.rs

use std::collections::HashMap;

use eframe::egui::{self, Color32, Rect, Stroke, Ui};

use crate::containers::Roi;
use crate::containers::Spectrum;
use crate::core::{RoiManager, RoiSettingsPatch};

use super::roi_event::RoiEvent;
use super::roi_view_state::{Edge, RoiEditDraft, RoiInteraction, RoiViewState};

const GRAB_PX: f32 = 6.0;
const MIN_ROI_WIDTH: f64 = 1.0;

/// Draw and handle interaction for all ROIs visible in `plot_area`.
#[allow(clippy::too_many_arguments)]
pub fn draw_rois(
    ui: &Ui,
    plot_response: &egui::Response,
    plot_area: Rect,
    roi_manager: &mut RoiManager,
    owned_spectrum: Option<&str>,
    view: &mut RoiViewState,
    x_to_px: impl Fn(f64) -> f32,
    px_to_x: impl Fn(f32) -> f64,
    editable: bool,
) -> Vec<RoiEvent> {
    let mut events = Vec::new();
    let painter = ui.painter_at(plot_area);

    let is_visible = |roi: &Roi| -> bool {
        owned_spectrum.is_none_or(|owned| roi.owner_spectrum.as_deref() == Some(owned))
    };

    // --- 1. Active drag update & completion (independent of hover_pos) ---
    if editable && view.interaction.is_active() {
        if plot_response.dragged_by(egui::PointerButton::Primary)
            && let Some(pointer) = plot_response.interact_pointer_pos()
        {
            let data_x = px_to_x(pointer.x);
            if data_x.is_finite() {
                apply_drag(roi_manager, view, data_x);
            }
        }

        if plot_response.drag_stopped() {
            match &view.interaction {
                RoiInteraction::DraggingEdge { tag, .. }
                | RoiInteraction::DraggingRegion { tag, .. } => {
                    events.push(RoiEvent::SettingsUpdated(tag.clone()));
                }
                RoiInteraction::Idle | RoiInteraction::Editing { .. } => {}
            }
            view.interaction = RoiInteraction::Idle;
        }
    }

    // --- 2. Hover hit-testing, cursor icon, click & drag start -----------
    let mut hovered_tag: Option<String> = None;

    if let Some(pointer) = plot_response.hover_pos()
        && plot_area.contains(pointer)
    {
        let hit = hit_test_rois(roi_manager.registry(), &is_visible, &x_to_px, pointer.x);

        if let Some((ref tag, _, _)) = hit {
            hovered_tag = Some(tag.clone());
        }

        if editable {
            // Cursor icon feedback
            match &view.interaction {
                RoiInteraction::DraggingEdge { .. } => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                RoiInteraction::DraggingRegion { .. } => {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
                _ => match &hit {
                    Some((_, Some(_), true)) => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    Some((_, None, true)) => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    _ => {}
                },
            }

            // Start a new drag (only if `roi.movable` is true)
            if plot_response.drag_started_by(egui::PointerButton::Primary)
                && !view.interaction.is_active()
                && let Some(interact_pos) = plot_response.interact_pointer_pos()
            {
                let start_hit = hit_test_rois(
                    roi_manager.registry(),
                    &is_visible,
                    &x_to_px,
                    interact_pos.x,
                );
                match start_hit {
                    Some((tag, Some(edge), true)) => {
                        view.interaction = RoiInteraction::DraggingEdge { tag, edge };
                    }
                    Some((tag, None, true)) => {
                        let grab_x = px_to_x(interact_pos.x);
                        if let (true, Some(roi)) = (grab_x.is_finite(), roi_manager.get_roi(&tag)) {
                            view.interaction = RoiInteraction::DraggingRegion {
                                tag,
                                grab_x,
                                initial_bound: roi.roi_bound,
                            };
                        }
                    }
                    Some(_) => {} // ROI is locked (`movable == false`)
                    None => {
                        roi_manager.clear_selection();
                    }
                }
            }

            // Click (without drag) selects or clears selection
            if plot_response.clicked_by(egui::PointerButton::Primary)
                && !view.interaction.is_active()
            {
                match &hit {
                    Some((tag, _, _)) => {
                        roi_manager.select_roi(tag);
                        events.push(RoiEvent::Selected(tag.clone()));
                    }
                    None => {
                        roi_manager.clear_selection();
                    }
                }
            }

            // Right-click opens modal draft editor
            if plot_response.secondary_clicked()
                && let Some((tag, _, _)) = &hit
                && let Some(roi) = roi_manager.get_roi(tag)
            {
                view.interaction = RoiInteraction::Editing { tag: tag.clone() };
                view.edit_draft = Some(RoiEditDraft::from_roi(roi));
            }
        }
    }

    // --- 3. Render ROI bands & edges -------------------------------------
    let top = plot_area.top();
    let bot = plot_area.bottom();
    let selected_tag = roi_manager.selected_roi();

    for roi in roi_manager.registry().values().filter(|r| is_visible(r)) {
        let (lo, hi) = roi.roi_bound;
        let lo_px = x_to_px(lo);
        let hi_px = x_to_px(hi);

        let is_selected = selected_tag == Some(roi.tag.as_str());
        let line_color = if is_selected {
            Color32::YELLOW
        } else {
            Color32::from_rgb(255, 200, 0)
        };
        let stroke = Stroke::new(if is_selected { 2.0 } else { 1.5 }, line_color);

        // Use group color if grouped, falling back to amber/yellow tint
        let group_fill = roi_manager.get_roi_color(&roi.tag);
        let fill_color = if is_selected || group_fill == Color32::TRANSPARENT {
            line_color.gamma_multiply(if is_selected { 0.14 } else { 0.07 })
        } else {
            group_fill
        };

        painter.rect_filled(
            Rect::from_min_max(egui::pos2(lo_px, top), egui::pos2(hi_px, bot)),
            0.0,
            fill_color,
        );
        painter.line_segment([egui::pos2(lo_px, top), egui::pos2(lo_px, bot)], stroke);
        painter.line_segment([egui::pos2(hi_px, top), egui::pos2(hi_px, bot)], stroke);
    }

    // --- 4. Hover tooltip (suppressed while actively dragging) -----------
    if !view.interaction.is_active()
        && let Some(tag) = hovered_tag
        && let Some(roi) = roi_manager.get_roi(&tag)
    {
        let emission_line = roi
            .emission
            .as_ref()
            .map(|e| {
                let nuclide = e
                    .parent_nuclide
                    .as_ref()
                    .map(|n| n.to_name())
                    .unwrap_or_default();
                let energy = e
                    .energy_kev
                    .as_ref()
                    .map(|q| format!("{:.1} keV", q.value.mean()))
                    .unwrap_or_default();
                format!("Nuclide: {nuclide} {energy}").trim().to_string()
            })
            .unwrap_or_default();

        egui::Tooltip::for_widget(plot_response)
            .at_pointer()
            .show(|ui| {
                let header = if roi.alias.is_empty() {
                    format!("ROI: {:.1} – {:.1} keV", roi.roi_bound.0, roi.roi_bound.1)
                } else {
                    format!(
                        "ROI: {} ({:.1} – {:.1} keV)",
                        roi.alias, roi.roi_bound.0, roi.roi_bound.1
                    )
                };
                ui.label(header);
                if !emission_line.is_empty() {
                    ui.label(emission_line);
                }
                if editable {
                    if roi.movable {
                        ui.weak("Drag edges to resize • Drag inside to move • Right-click to edit");
                    } else {
                        ui.weak("Position locked • Right-click to edit");
                    }
                }
            });
    }

    events
}

/// Returns `(tag, edge_if_near_border, movable)`.
/// Edges take priority over interior hits so narrow ROIs remain resizable.
fn hit_test_rois(
    registry: &HashMap<String, Roi>,
    is_visible: &impl Fn(&Roi) -> bool,
    x_to_px: &impl Fn(f64) -> f32,
    px_x: f32,
) -> Option<(String, Option<Edge>, bool)> {
    let mut interior_hit: Option<(String, bool)> = None;

    for roi in registry.values().filter(|r| is_visible(r)) {
        let lo_px = x_to_px(roi.roi_bound.0);
        let hi_px = x_to_px(roi.roi_bound.1);

        if (px_x - lo_px).abs() <= GRAB_PX {
            return Some((roi.tag.clone(), Some(Edge::Lower), roi.movable));
        }
        if (px_x - hi_px).abs() <= GRAB_PX {
            return Some((roi.tag.clone(), Some(Edge::Upper), roi.movable));
        }
        if interior_hit.is_none() && px_x > lo_px && px_x < hi_px {
            interior_hit = Some((roi.tag.clone(), roi.movable));
        }
    }

    interior_hit.map(|(tag, movable)| (tag, None, movable))
}

fn apply_drag(roi_manager: &mut RoiManager, view: &RoiViewState, data_x: f64) {
    match &view.interaction {
        RoiInteraction::DraggingEdge { tag, edge } => {
            if let Some(roi) = roi_manager.get_roi(tag) {
                let mut b = roi.roi_bound;
                match edge {
                    Edge::Lower => b.0 = data_x.min(b.1 - MIN_ROI_WIDTH),
                    Edge::Upper => b.1 = data_x.max(b.0 + MIN_ROI_WIDTH),
                }
                roi_manager.set_roi_bound_raw(tag, b);
            }
        }
        RoiInteraction::DraggingRegion {
            tag,
            grab_x,
            initial_bound,
        } => {
            let delta = data_x - grab_x;
            let width = initial_bound.1 - initial_bound.0;
            let new_lo = (initial_bound.0 + delta).max(0.0);
            roi_manager.set_roi_bound_raw(tag, (new_lo, new_lo + width));
        }
        _ => {}
    }
}
