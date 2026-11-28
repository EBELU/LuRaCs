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
    let mut hovered_edge: Option<(String, Edge)> = None;

    if let Some(pointer) = plot_response.hover_pos()
        && plot_area.contains(pointer)
    {
        let hit = hit_test_rois(roi_manager.registry(), &is_visible, &x_to_px, pointer.x);

        if let Some((ref tag, edge_opt, _)) = hit {
            hovered_tag = Some(tag.clone());
            if let Some(edge) = edge_opt {
                hovered_edge = Some((tag.clone(), edge));
            }
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

            // Start a new drag (only if `roi.movable` is true).
            // Use `press_origin` rather than `interact_pointer_pos`: by the time
            // egui crosses its ~6px drag threshold, the current pointer has often
            // already moved beyond `GRAB_PX` (6px) from the edge.
            if plot_response.drag_started_by(egui::PointerButton::Primary)
                && !view.interaction.is_active()
                && let Some(press_pos) = ui
                    .input(|i| i.pointer.press_origin())
                    .or_else(|| plot_response.interact_pointer_pos())
                && plot_area.contains(press_pos)
            {
                let start_hit = hit_test_rois(
                    roi_manager.registry(),
                    &is_visible,
                    &x_to_px,
                    press_pos.x,
                );
                match start_hit {
                    Some((tag, Some(edge), true)) => {
                        view.interaction = RoiInteraction::DraggingEdge { tag, edge };
                        if let Some(cur_pos) = plot_response.interact_pointer_pos() {
                            let data_x = px_to_x(cur_pos.x);
                            if data_x.is_finite() {
                                apply_drag(roi_manager, view, data_x);
                            }
                        }
                    }
                    Some((tag, None, true)) => {
                        let grab_x = px_to_x(press_pos.x);
                        if let (true, Some(roi)) = (grab_x.is_finite(), roi_manager.get_roi(&tag)) {
                            view.interaction = RoiInteraction::DraggingRegion {
                                tag,
                                grab_x,
                                initial_bound: roi.roi_bound,
                            };
                            if let Some(cur_pos) = plot_response.interact_pointer_pos() {
                                let data_x = px_to_x(cur_pos.x);
                                if data_x.is_finite() {
                                    apply_drag(roi_manager, view, data_x);
                                }
                            }
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
    let active_edge = match &view.interaction {
        RoiInteraction::DraggingEdge { tag, edge } => Some((tag.as_str(), *edge)),
        _ => None,
    };

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
        let base_width = if is_selected { 1.5 } else { 1.0 };

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

        let lo_highlighted = roi.movable
            && (active_edge == Some((roi.tag.as_str(), Edge::Lower))
                || hovered_edge.as_ref() == Some(&(roi.tag.clone(), Edge::Lower)));
        let hi_highlighted = roi.movable
            && (active_edge == Some((roi.tag.as_str(), Edge::Upper))
                || hovered_edge.as_ref() == Some(&(roi.tag.clone(), Edge::Upper)));

        let highlight_color = Color32::from_rgb(100, 220, 255);
        let lo_stroke = if lo_highlighted {
            Stroke::new(2.5, highlight_color)
        } else {
            Stroke::new(base_width, line_color)
        };
        let hi_stroke = if hi_highlighted {
            Stroke::new(2.5, highlight_color)
        } else {
            Stroke::new(base_width, line_color)
        };

        painter.line_segment([egui::pos2(lo_px, top), egui::pos2(lo_px, bot)], lo_stroke);
        painter.line_segment([egui::pos2(hi_px, top), egui::pos2(hi_px, bot)], hi_stroke);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::nuclide::Emission;
    use crate::containers::roi::roi_types::{BkgType, FitType};

    fn make_roi(tag: &str, lo: f64, hi: f64, movable: bool, owner: Option<&str>) -> Roi {
        Roi {
            tag: tag.to_string(),
            alias: format!("Alias_{tag}"),
            spectrum: "s1".to_string(),
            roi_bound: (lo, hi),
            region_bound: (lo - 5.0, hi + 5.0),
            fit_type: FitType::None,
            bkg_type: BkgType::None,
            bkg_est_channels: 2,
            fit: None,
            roi_counts: 10.0,
            live_time: 1.0,
            emission: Some(Emission::annihilation()),
            meta: serde_json::Map::new(),
            merge: false,
            movable,
            poisson_weights: false,
            owner_spectrum: owner.map(str::to_string),
        }
    }

    #[test]
    fn test_hit_test_rois_and_apply_drag() {
        let mut reg = HashMap::new();
        reg.insert("r1".to_string(), make_roi("r1", 100.0, 200.0, true, Some("s1")));
        reg.insert("r2".to_string(), make_roi("r2", 300.0, 400.0, false, Some("s2")));

        let visible_s1 = |r: &Roi| r.owner_spectrum.as_deref() == Some("s1");
        let x_to_px = |x: f64| x as f32;

        // Lower edge hit
        assert_eq!(
            hit_test_rois(&reg, &visible_s1, &x_to_px, 102.0),
            Some(("r1".to_string(), Some(Edge::Lower), true))
        );
        // Upper edge hit
        assert_eq!(
            hit_test_rois(&reg, &visible_s1, &x_to_px, 198.0),
            Some(("r1".to_string(), Some(Edge::Upper), true))
        );
        // Interior hit
        assert_eq!(
            hit_test_rois(&reg, &visible_s1, &x_to_px, 150.0),
            Some(("r1".to_string(), None, true))
        );
        // Filtered out ROI not hit
        assert_eq!(hit_test_rois(&reg, &visible_s1, &x_to_px, 350.0), None);
        // Miss
        assert_eq!(hit_test_rois(&reg, &visible_s1, &x_to_px, 50.0), None);

        // apply_drag tests
        let mut mgr = RoiManager::default();
        let mut spec = crate::core::NewRoi::new(100.0, 200.0);
        spec.owner_spectrum = Some("s1".to_string());
        spec.emission = Some(Emission::annihilation());
        let tag = mgr.add_roi(spec);

        let view_lo = RoiViewState {
            interaction: RoiInteraction::DraggingEdge {
                tag: tag.clone(),
                edge: Edge::Lower,
            },
            ..Default::default()
        };
        apply_drag(&mut mgr, &view_lo, 120.0);
        assert_eq!(mgr.get_roi(&tag).unwrap().roi_bound, (120.0, 200.0));

        let view_hi = RoiViewState {
            interaction: RoiInteraction::DraggingEdge {
                tag: tag.clone(),
                edge: Edge::Upper,
            },
            ..Default::default()
        };
        apply_drag(&mut mgr, &view_hi, 250.0);
        assert_eq!(mgr.get_roi(&tag).unwrap().roi_bound, (120.0, 250.0));

        let view_reg = RoiViewState {
            interaction: RoiInteraction::DraggingRegion {
                tag: tag.clone(),
                grab_x: 150.0,
                initial_bound: (120.0, 250.0),
            },
            ..Default::default()
        };
        apply_drag(&mut mgr, &view_reg, 170.0);
        assert_eq!(mgr.get_roi(&tag).unwrap().roi_bound, (140.0, 270.0));

        // Clamping to MIN_ROI_WIDTH (1.0) on both edges
        apply_drag(&mut mgr, &view_lo, 500.0);
        assert_eq!(mgr.get_roi(&tag).unwrap().roi_bound, (269.0, 270.0));
        apply_drag(&mut mgr, &view_hi, 10.0);
        assert_eq!(mgr.get_roi(&tag).unwrap().roi_bound, (269.0, 270.0));
    }

    fn run_draw_frame(
        ctx: &egui::Context,
        raw_input: egui::RawInput,
        mgr: &mut RoiManager,
        owned: Option<&str>,
        view: &mut RoiViewState,
        editable: bool,
    ) -> (Vec<RoiEvent>, egui::FullOutput) {
        let mut events = Vec::new();
        let mut out = ctx.run_ui(raw_input, |ui| {
            let plot_area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0));
            let resp = ui.allocate_rect(plot_area, egui::Sense::click_and_drag());
            events = draw_rois(
                ui,
                &resp,
                plot_area,
                mgr,
                owned,
                view,
                |x| x as f32,
                f64::from,
                editable,
            );
        });
        out.textures_delta.clear();
        (events, out)
    }

    fn collect_shape_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        let mut texts = Vec::new();
        fn visit(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        visit(s, out);
                    }
                }
                _ => {}
            }
        }
        for cs in shapes {
            visit(&cs.shape, &mut texts);
        }
        texts
    }

    fn has_rect_fill(shapes: &[egui::epaint::ClippedShape], color: Color32) -> bool {
        shapes.iter().any(|cs| match &cs.shape {
            egui::Shape::Rect(r) => r.fill == color,
            _ => false,
        })
    }

    fn has_line_stroke(shapes: &[egui::epaint::ClippedShape], stroke: Stroke) -> bool {
        count_line_strokes(shapes, stroke) > 0
    }

    fn count_line_strokes(shapes: &[egui::epaint::ClippedShape], stroke: Stroke) -> usize {
        shapes
            .iter()
            .filter(|cs| matches!(&cs.shape, egui::Shape::LineSegment { stroke: s, .. } if *s == stroke))
            .count()
    }

    #[test]
    fn test_draw_rois_headless_egui() {
        let mut mgr = RoiManager::default();
        let mut spec1 = crate::core::NewRoi::new(100.0, 200.0);
        spec1.owner_spectrum = Some("s1".to_string());
        spec1.emission = Some(Emission::annihilation());
        let tag1 = mgr.add_roi(spec1);

        // Second ROI on another spectrum ("s2") to verify owned_spectrum filtering
        let mut spec2 = crate::core::NewRoi::new(300.0, 400.0);
        spec2.owner_spectrum = Some("s2".to_string());
        let _tag2 = mgr.add_roi(spec2);

        // Put tag1 into a 1-element group so get_roi_color returns non-transparent blue tint
        mgr.calculate_roi_groups();
        let group_color = mgr.get_roi_color(&tag1);
        assert_ne!(group_color, Color32::TRANSPARENT);

        let mut view = RoiViewState::default();
        let ctx = egui::Context::default();
        ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
        let screen = Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0)));

        // Warm-up frame so egui registers `plot_area` for hover/click hit-testing
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                time: Some(0.0),
                events: vec![egui::Event::PointerMoved(egui::pos2(150.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        ));

        // 1. Hover interior of unselected grouped ROI -> Grab cursor, group_color fill, tooltip with Nuclide
        let (ev, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                time: Some(1.0),
                events: vec![egui::Event::PointerMoved(egui::pos2(150.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(ev.is_empty());
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Grab);
        assert!(has_rect_fill(&out.shapes, group_color));
        let hl_stroke = Stroke::new(2.5, Color32::from_rgb(100, 220, 255));
        assert!(!has_line_stroke(&out.shapes, hl_stroke));
        // Stationary frame so tooltip opens
        let (_, out_tt) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                time: Some(2.0),
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        let texts = collect_shape_texts(&out_tt.shapes);
        assert!(texts.iter().any(|t| t.contains("Nuclide:")), "texts={texts:?}");

        // 2. Hover lower edge -> ResizeHorizontal cursor & highlighted lower stroke
        let (_, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(100.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
        assert!(has_line_stroke(&out.shapes, hl_stroke));

        // 3. Hover upper edge -> highlighted upper stroke
        let (_, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(200.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(has_line_stroke(&out.shapes, hl_stroke));

        // 4. Primary click on interior -> Selected event & selected fill (YELLOW tint, not group_color)
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(150.0, 150.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(150.0, 150.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        ));
        let (ev, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(150.0, 150.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(matches!(ev.as_slice(), [RoiEvent::Selected(t)] if t == &tag1));
        assert_eq!(mgr.selected_roi(), Some(tag1.as_str()));
        assert!(!has_rect_fill(&out.shapes, group_color));

        // 5. Hovering "s2" ROI at x=350 must NOT hit when owned_spectrum = Some("s1")
        let (_, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(350.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Default);

        // 6. DraggingEdge: cursor ResizeHorizontal, tooltip suppressed, editable=false blocks drag, release emits SettingsUpdated
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(98.0, 150.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(98.0, 150.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        ));
        let (_, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(105.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(matches!(view.interaction, RoiInteraction::DraggingEdge { .. }));
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
        assert!(has_line_stroke(&out.shapes, hl_stroke));
        assert!(!collect_shape_texts(&out.shapes).iter().any(|t| t.contains("Nuclide:")));

        // With editable=false, active drag must NOT mutate bounds or stop interaction
        let bound_before = mgr.get_roi(&tag1).unwrap().roi_bound;
        let (ev_ro, _) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(130.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            false,
        );
        assert!(ev_ro.is_empty());
        assert_eq!(mgr.get_roi(&tag1).unwrap().roi_bound, bound_before);

        // Release drag -> SettingsUpdated
        let (ev, _) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(110.0, 150.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(matches!(ev.as_slice(), [RoiEvent::SettingsUpdated(t)] if t == &tag1));
        assert!(!view.interaction.is_active());

        // 7. DraggingRegion -> Grabbing cursor
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(150.0, 150.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(150.0, 150.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        ));
        let (_, _) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(165.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert!(matches!(view.interaction, RoiInteraction::DraggingRegion { .. }));
        let (_, out) = run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(170.0, 150.0))],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        );
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Grabbing);
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(165.0, 150.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            &mut mgr,
            Some("s1"),
            &mut view,
            true,
        ));

        // 8. Non-movable ROI edge hover must NOT highlight either edge
        let mut locked_spec = crate::core::NewRoi::new(220.0, 260.0);
        locked_spec.owner_spectrum = Some("s1".to_string());
        locked_spec.movable = false;
        let _locked_tag = mgr.add_roi(locked_spec);
        for edge_x in [220.0, 260.0] {
            let (_, out) = run_draw_frame(
                &ctx,
                egui::RawInput {
                    screen_rect: screen,
                    events: vec![egui::Event::PointerMoved(egui::pos2(edge_x, 150.0))],
                    ..Default::default()
                },
                &mut mgr,
                Some("s1"),
                &mut view,
                true,
            );
            assert!(!has_line_stroke(&out.shapes, hl_stroke));
        }
    }

    #[test]
    fn test_draw_rois_active_edge_and_locked_roi() {
        let hl_stroke = Stroke::new(2.5, Color32::from_rgb(100, 220, 255));
        let screen = Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 300.0)));

        // 1. Locked ROI (movable = false): hovering lower or upper edge must NOT highlight
        let mut mgr_locked = RoiManager::default();
        let mut spec_locked = crate::core::NewRoi::new(50.0, 150.0);
        spec_locked.movable = false;
        mgr_locked.add_roi(spec_locked);
        let mut view_locked = RoiViewState::default();
        let ctx_locked = egui::Context::default();
        drop(run_draw_frame(
            &ctx_locked,
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            &mut mgr_locked,
            None,
            &mut view_locked,
            true,
        ));
        for edge_x in [50.0, 150.0] {
            let (_, out) = run_draw_frame(
                &ctx_locked,
                egui::RawInput {
                    screen_rect: screen,
                    events: vec![egui::Event::PointerMoved(egui::pos2(edge_x, 150.0))],
                    ..Default::default()
                },
                &mut mgr_locked,
                None,
                &mut view_locked,
                true,
            );
            assert_eq!(count_line_strokes(&out.shapes, hl_stroke), 0);
        }

        // 2. Active DraggingEdge while pointer is inside interior (no hovered_edge):
        //    must set ResizeHorizontal cursor and highlight ONLY the dragged edge.
        let mut mgr = RoiManager::default();
        let tag = mgr.add_roi(crate::core::NewRoi::new(100.0, 200.0));
        let ctx = egui::Context::default();
        let mut view = RoiViewState::default();
        drop(run_draw_frame(
            &ctx,
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            &mut mgr,
            None,
            &mut view,
            true,
        ));

        for edge in [Edge::Lower, Edge::Upper] {
            view.interaction = RoiInteraction::DraggingEdge {
                tag: tag.clone(),
                edge,
            };
            let (_, out) = run_draw_frame(
                &ctx,
                egui::RawInput {
                    screen_rect: screen,
                    events: vec![egui::Event::PointerMoved(egui::pos2(150.0, 150.0))],
                    ..Default::default()
                },
                &mut mgr,
                None,
                &mut view,
                true,
            );
            assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
            assert_eq!(count_line_strokes(&out.shapes, hl_stroke), 1);
        }
        // 3. Fast outward edge drag (> GRAB_PX in one step) still catches edge via press_origin
        let mut mgr_fast = RoiManager::default();
        let tag_fast = mgr_fast.add_roi(crate::core::NewRoi::new(100.0, 200.0));
        let ctx_fast = egui::Context::default();
        let mut view_fast = RoiViewState::default();
        drop(run_draw_frame(
            &ctx_fast,
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            &mut mgr_fast,
            None,
            &mut view_fast,
            true,
        ));
        drop(run_draw_frame(
            &ctx_fast,
            egui::RawInput {
                screen_rect: screen,
                events: vec![
                    egui::Event::PointerMoved(egui::pos2(99.0, 150.0)),
                    egui::Event::PointerButton {
                        pos: egui::pos2(99.0, 150.0),
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            &mut mgr_fast,
            None,
            &mut view_fast,
            true,
        ));
        // Move 19px left in a single frame (well past GRAB_PX = 6px)
        drop(run_draw_frame(
            &ctx_fast,
            egui::RawInput {
                screen_rect: screen,
                events: vec![egui::Event::PointerMoved(egui::pos2(80.0, 150.0))],
                ..Default::default()
            },
            &mut mgr_fast,
            None,
            &mut view_fast,
            true,
        ));
        assert!(matches!(
            view_fast.interaction,
            RoiInteraction::DraggingEdge {
                edge: Edge::Lower,
                ..
            }
        ));
        assert_eq!(mgr_fast.get_roi(&tag_fast).unwrap().roi_bound, (80.0, 200.0));
    }
}
