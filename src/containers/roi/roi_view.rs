// src/roi_view.rs

use eframe::egui::{self, Color32, Stroke, Ui};

use super::roi::{BkgType, FitType, Roi};
use super::roi_event::RoiEvent;
use super::roi_view_state::{Edge, RoiEditDraft, RoiInteraction, RoiViewState};

/// Called once per frame, after rsplot has produced its `Response`.
///
/// `plot_rect` is the on-screen pixel rectangle of the plot area (from the
/// rsplot Response). `x_to_px` converts a data-x (energy or channel) into a
/// screen x-coordinate; you get this from the axis you drew with rsplot.
pub fn draw_rois(
    ui: &mut Ui,
    plot_response: &egui::Response,
    rois: &mut [Roi],
    view: &mut RoiViewState,
    x_to_px: impl Fn(f64) -> f32,
    px_to_x: impl Fn(f32) -> f64,
    editable: bool,
) -> Vec<RoiEvent> {
    let mut events = Vec::new();
    let painter = ui.painter_at(plot_response.rect);

    // --- 1. Handle pointer interaction (only when hovering the plot) ----
    if editable {
        if let Some(pointer) = plot_response.hover_pos() {
            let px_x = pointer.x;
            let data_x = px_to_x(px_x);

            // Which ROI / edge is under the pointer?
            let mut hit: Option<(usize, Option<Edge>)> = None;
            for (i, roi) in rois.iter().enumerate() {
                let (lo, hi) = roi.roi_bound;
                let lo_px = x_to_px(lo);
                let hi_px = x_to_px(hi);
                const GRAB_PX: f32 = 5.0;

                if (px_x - lo_px).abs() <= GRAB_PX {
                    hit = Some((i, Some(Edge::Lower)));
                    break;
                }
                if (px_x - hi_px).abs() <= GRAB_PX {
                    hit = Some((i, Some(Edge::Upper)));
                    break;
                }
                if px_x > lo_px && px_x < hi_px {
                    hit = Some((i, None));
                    break;
                }
            }

            // Cursor feedback
            if let Some((_, edge)) = hit {
                ui.ctx().set_cursor_icon(if edge.is_some() {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::Grab
                });
            } else if !view.interaction.is_active() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }

            // Drag started?
            if plot_response.drag_started() {
                match hit {
                    Some((i, Some(edge))) => {
                        view.interaction = RoiInteraction::DraggingEdge {
                            tag: rois[i].tag.clone(),
                            edge,
                        };
                    }
                    Some((i, None)) => {
                        view.interaction = RoiInteraction::DraggingRegion {
                            tag: rois[i].tag.clone(),
                            grab_x: data_x,
                        };
                    }
                    None => {
                        // Clicked empty plot area — deselect
                        view.selected = None;
                    }
                }
            }

            // Drag in progress?
            if plot_response.dragged() {
                apply_drag(rois, view, data_x);
            }

            // Drag ended?
            if plot_response.drag_stopped() {
                if let RoiInteraction::DraggingEdge { tag, .. }
                | RoiInteraction::DraggingRegion { tag, .. } = &view.interaction
                {
                    events.push(RoiEvent::SettingsUpdated(tag.clone()));
                }
                view.interaction = RoiInteraction::Idle;
            }

            // Click (no drag)?
            if plot_response.clicked() {
                if let Some((i, _)) = hit {
                    view.selected = Some(rois[i].tag.clone());
                    events.push(RoiEvent::Selected(rois[i].tag.clone()));
                }
            }
        }
    }

    // --- 2. Draw every ROI ---------------------------------------------
    for roi in rois.iter() {
        let (lo, hi) = roi.roi_bound;
        let lo_px = x_to_px(lo);
        let hi_px = x_to_px(hi);
        let top = plot_response.rect.top();
        let bot = plot_response.rect.bottom();

        let is_selected = view.selected.as_deref() == Some(roi.tag.as_str());
        let line_color = if is_selected {
            Color32::YELLOW
        } else {
            Color32::LIGHT_BLUE
        };
        let stroke = Stroke::new(1.5, line_color);

        painter.line_segment([egui::pos2(lo_px, top), egui::pos2(lo_px, bot)], stroke);
        painter.line_segment([egui::pos2(hi_px, top), egui::pos2(hi_px, bot)], stroke);
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(lo_px, top), egui::pos2(hi_px, bot)),
            0.0,
            line_color.gamma_multiply(0.08),
        );

        // --- 3. Right-click → open editor ------------------------------
        if editable && plot_response.secondary_clicked() {
            if let Some(pointer) = plot_response.interact_pointer_pos() {
                if pointer.x > lo_px && pointer.x < hi_px {
                    view.interaction = RoiInteraction::Editing {
                        tag: roi.tag.clone(),
                    };
                    view.edit_draft = Some(RoiEditDraft::from_roi(roi));
                }
            }
        }
    }

    // --- 4. Hover tooltip (replaces setToolTip) ------------------------
    if let Some(pointer) = plot_response.hover_pos() {
        for roi in rois.iter() {
            let (lo_px, hi_px) = (x_to_px(roi.roi_bound.0), x_to_px(roi.roi_bound.1));
            if pointer.x > lo_px && pointer.x < hi_px {
                let emission_line = roi
                    .emission
                    .as_ref()
                    .map(|e| {
                        format!(
                            "Nuclide: {} {}",
                            e.parent_nuclide
                                .as_ref()
                                .map(|n| n.to_name())
                                .unwrap_or_default(),
                            e.energy_kev
                                .as_ref()
                                .map(|q| q.value.mean())
                                .unwrap_or(f64::NAN),
                        )
                    })
                    .unwrap_or_default();
                egui::Tooltip::for_widget(&plot_response)
                    .at_pointer()
                    .show(|ui| {
                        ui.label(format!("ROI: {}\nRight-click to edit", roi.alias));
                        if !emission_line.is_empty() {
                            ui.label(emission_line);
                        }
                    });
                break;
            }
        }
    }

    events
}

fn apply_drag(rois: &mut [Roi], view: &RoiViewState, data_x: f64) {
    match &view.interaction {
        RoiInteraction::DraggingEdge { tag, edge } => {
            if let Some(roi) = rois.iter_mut().find(|r| r.tag == *tag) {
                match edge {
                    Edge::Lower => roi.roi_bound.0 = data_x.min(roi.roi_bound.1),
                    Edge::Upper => roi.roi_bound.1 = data_x.max(roi.roi_bound.0),
                }
            }
        }
        RoiInteraction::DraggingRegion { tag, grab_x } => {
            if let Some(roi) = rois.iter_mut().find(|r| r.tag == *tag) {
                let delta = data_x - grab_x;
                roi.roi_bound.0 += delta;
                roi.roi_bound.1 += delta;
            }
        }
        _ => {}
    }
}

impl RoiInteraction {
    pub fn is_active(&self) -> bool {
        !matches!(self, RoiInteraction::Idle)
    }
}
