//! Spectrum plot: primary / background / ROI curves, nuclide reference
//! lines, cursor, feature lines.
//!
//! Port of `luracs.gui.spectrum_plot.SpectrumPlot` targeting **rsplot 0.6.1**.
//!
//! ## rsplot integration (verified against docs.rs 0.6.1)
//!
//! * `Plot1D::new(&RenderState, u64)` — one per plot; the second arg is a
//!   unique widget id. In tabbed mode each tab uses `plot_id_from(name)`.
//! * `plot.clear()` every frame, then re-add curves / markers. This mirrors
//!   Python's `plot_widget.clear()` on every `_redraw()`. With
//!   `set_auto_reset_zoom(false)` the axis limits survive across frames, so
//!   user pan/zoom is preserved.
//! * Log axis: `plot.set_y_log(true/false)`. The x/y data stays linear;
//!   rsplot labels and scales the axis.
//! * Solid curves: `plot.add_curve_with_legend(&x, &y, color, "legend")`.
//! * Dashed curves (ROI linear background): `add_curve` then
//!   `update_curve_spec(handle, CurveSpec::new(...).with line_style =
//!   LineStyle::Dashed)`.
//! * Markers: `plot.add_marker_data(&Marker::vline(x).with_line_style(...))`
//!   for nuclide / feature / cursor vertical lines.
//! * Pointer position: `response.transform.pixel_to_data(pos)` after
//!   `plot.show(ui)`.
//!
//! ## Bugs fixed vs Python
//!
//! * **CPS desync**: returning from Overlay/Subtract to None used to leave
//!   `cps = true` forever. We remember `user_cps_pref` and restore it.
//! * **Cursor crash**: `parent_nuclide + "..."` raised on `None`. We branch
//!   on `Option`.
//! * **Dead guard** removed: `live_time is not None`.
//!
//! ## TODO
//!
//! * **Fill-under-line**: rsplot supports it via `CurveSpec.fill = true`;
//!   add once we settle how it interacts with log mode (baseline 0 is -inf).
//! * **Y-axis lock**: rsplot has no per-axis mouse-enablement yet. The
//!   `y_axis_locked` flag is stored but currently has no visual effect;
//!   leave as-is until a per-axis interaction mode is available.
//! * **Cursor emission lines**: the setting is read; drawing them requires
//!   reading the cursor nuclide's emissions and pushing more markers —
//!   follows once the data path is confirmed.

use std::collections::HashMap;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use ndarray::Array1;
use nucleide_nuclei::NuclideId;
use rsplot::core::items::CurveInterpolation;
use rsplot::{CurveSpec, LineStyle, Marker, Plot1D, YAxis};

use crate::containers::EmissionType;
use crate::containers::{BkgType, Roi};
use crate::core::NewRoi;
use crate::core::SpectrumManager;
use crate::settings::Settings;
use crate::theme_manager::Palette;
use crate::utils::multi_gaussian;

// ===========================================================================
// Mode
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BkgDisplayMode {
    #[default]
    None,
    Overlay,
    Subtract,
}

impl BkgDisplayMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "No Background",
            Self::Overlay => "Background Overlay",
            Self::Subtract => "Background Subtract",
        }
    }
}

// ===========================================================================
// Feature lines
// ===========================================================================

#[derive(Debug, Clone)]
pub struct FeatureLine {
    pub value: f64,
    pub color: Color32,
}

// ===========================================================================
// State
// ===========================================================================

#[derive(Debug, Clone, Copy)]
enum RoiSide {
    Lower,
    Upper,
}

#[derive(Debug, Clone)]
struct DraggingRoi {
    tag: String,
    side: RoiSide,
}

pub struct SpectrumPlotState {
    /// Retained rsplot widget (owns wgpu resources for this plot).
    plot: Plot1D,

    // View toggles
    pub log: bool,
    pub cps: bool,
    /// Remembered cps preference while Overlay/Subtract force cps = true.
    pub user_cps_pref: bool,
    pub bkg_mode: BkgDisplayMode,
    pub cursor_enabled: bool,
    pub y_axis_locked: bool,

    /// `Some(name)` in tabbed mode; `None` in combined mode.
    pub owned_spectrum: Option<String>,

    // View bounds (read back from the plot each frame)
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub x_max: f64,

    // Cursor
    pub cursor_x: f64,
    pub cursor_nuclide: Option<NuclideId>,
    pub cursor_info: String,
    /// Matched nuclide's significant emissions, refreshed each frame while
    /// the cursor is enabled and the setting is on. `(energy_keV, is_xray)`.
    /// Drawn as amber vlines so the cursor shows *all* of the matched
    /// nuclide's peaks, not just the one under the pointer.
    pub cursor_emission_lines: Vec<(f64, bool)>,

    // Feature lines
    pub feature_lines: HashMap<String, FeatureLine>,

    // When true, the next frame is allowed to auto-fit after rebuilding.
    reset_pending: bool,

    dragging: Option<DraggingRoi>,
    hovered: Option<(String, RoiSide)>,
}

impl SpectrumPlotState {
    pub fn new(render_state: &RenderState) -> Self {
        Self::with_id(render_state, 0)
    }

    pub fn for_spectrum(render_state: &RenderState, name: impl Into<String>) -> Self {
        let name = name.into();
        let id = plot_id_from(&name);
        let mut s = Self::with_id(render_state, id);
        s.owned_spectrum = Some(name);
        s
    }

    fn with_id(render_state: &RenderState, plot_id: u64) -> Self {
        Self {
            plot: {
                let mut plot = Plot1D::new(render_state, plot_id);
                // Disable left-drag rectangle zoom; keep wheel zoom and ROI drag working.
                plot.set_interaction_mode(rsplot::PlotInteractionMode::Select); // = false; // ← candidate field name
                plot.set_x_min_pos(Some(0.0));

                plot
            },
            log: false,
            cps: false,
            user_cps_pref: false,
            bkg_mode: BkgDisplayMode::None,
            cursor_enabled: false,
            y_axis_locked: true,
            owned_spectrum: None,
            x_range: (0.0, 3000.0),
            y_range: (1e-3, 1e4),
            x_max: 3000.0,
            cursor_x: 250.0,
            cursor_nuclide: None,
            cursor_info: String::new(),
            cursor_emission_lines: Vec::new(),
            feature_lines: HashMap::new(),
            reset_pending: true,
            dragging: None,
            hovered: None,
        }
    }

    fn sync_settings(&mut self, settings: &Settings) {
        self.x_max = settings.appearance.spectrum_plot_e_max;
    }

    pub fn request_reset(&mut self) {
        self.reset_pending = true;
    }
}

fn plot_id_from(name: &str) -> u64 {
    // FNV-1a, stable across runs so egui's widget state (zoom, hover) is
    // preserved when a tab is closed and reopened.
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ===========================================================================
// ROI drag handling
// ===========================================================================

/// Hit radius in pixels — how close the pointer has to be to a bound to
/// grab it.
const ROI_HIT_PX: f32 = 8.0;

fn handle_roi_drag(
    state: &mut SpectrumPlotState,
    spectrum_manager: &mut SpectrumManager,
    settings: &Settings,
    response: &rsplot::widget::plot_widget::PlotResponse,
    ui: &egui::Ui,
) {
    // --- Hover cursor feedback (only when not already dragging) ---
    if state.dragging.is_none() {
        state.hovered = None;
        if let Some(pointer) = response.response.hover_pos() {
            if response.transform.area.contains(pointer) {
                state.hovered = nearest_bound(state, spectrum_manager, response, pointer);
                if state.hovered.is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
    }

    // --- Drag start ---
    if response.response.drag_started() {
        if let Some(pointer) = response.response.interact_pointer_pos() {
            if let Some((tag, side)) = nearest_bound(state, spectrum_manager, response, pointer) {
                state.dragging = Some(DraggingRoi { tag, side });
            }
        }
    }

    // --- During drag: update bounds (no refit yet) ---
    if let Some(drag) = state.dragging.clone() {
        if let Some(pointer) = response.response.interact_pointer_pos() {
            let (x, _) = response.transform.pixel_to_data(pointer);
            if x.is_finite() {
                if let Some(roi) = spectrum_manager.roi_manager.get_roi(&drag.tag) {
                    let mut b = roi.roi_bound;
                    match drag.side {
                        RoiSide::Lower => b.0 = x.min(b.1 - 1.0),
                        RoiSide::Upper => b.1 = x.max(b.0 + 1.0),
                    }
                    spectrum_manager.roi_manager.set_roi_bound_raw(&drag.tag, b);
                }
            }
        }
    }

    // --- Drag stop: commit + refit ---
    if response.response.drag_stopped() {
        if let Some(drag) = state.dragging.take() {
            let use_chi2 = settings.advanced.optimizer_use_chi2_weight;
            spectrum_manager.roi_manager.on_roi_change(
                &mut spectrum_manager.spectrum_registry,
                &drag.tag,
                use_chi2,
                true,
            );
        }
    }
}

/// Find the ROI bound closest to `pointer` in pixel space, if within
/// `ROI_HIT_PX`. Only considers ROIs whose owner matches `state.owned_spectrum`
/// (so tabbed mode doesn't grab ROIs from other tabs).
fn nearest_bound(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
    response: &rsplot::widget::plot_widget::PlotResponse,
    pointer: egui::Pos2,
) -> Option<(String, RoiSide)> {
    let mut best: Option<(String, RoiSide, f32)> = None;

    for roi in spectrum_manager.roi_manager.registry().values() {
        if let Some(owned) = &state.owned_spectrum {
            if roi.owner_spectrum.as_deref() != Some(owned) {
                continue;
            }
        }
        for (side, x_val) in [
            (RoiSide::Lower, roi.roi_bound.0),
            (RoiSide::Upper, roi.roi_bound.1),
        ] {
            let (px, _) = response.transform.data_to_pixel(x_val, 0.0).into();
            let dist = (pointer.x - px).abs();
            if dist <= ROI_HIT_PX {
                match &best {
                    Some((_, _, d)) if *d <= dist => {}
                    _ => best = Some((roi.tag.clone(), side, dist)),
                }
            }
        }
    }

    best.map(|(tag, side, _)| (tag, side))
}

// ===========================================================================
// Render
// ===========================================================================

pub fn render(
    state: &mut SpectrumPlotState,
    spectrum_manager: &mut SpectrumManager,
    settings: &Settings,
    palette: &Palette,
    ui: &mut egui::Ui,
) {
    state.sync_settings(settings);

    render_toolbar(state, spectrum_manager, settings, ui);

    // --- Axis labels ---
    state.plot.set_graph_x_label("Energy [keV]");
    state
        .plot
        .set_graph_y_label(if state.cps { "CPS" } else { "Counts" }, YAxis::Left);

    // --- Log mode ---
    state.plot.set_y_log(state.log);

    // Match pyqtgraph: log mode floors at log10(y) = -10 (y ≈ 1e-10);
    // linear mode floors at y = 0.
    state
        .plot
        .set_y_min_pos(Some(if state.log { -10.0 } else { 0.0 }));
    state.plot.set_y_max_pos(Some(1e16));

    // --- One-shot auto-fit if we've requested a reset ---
    state.plot.set_auto_reset_zoom(state.reset_pending);

    // --- Rebuild every frame ---
    state.plot.clear();

    for curve in collect_background_curves(state, spectrum_manager, settings) {
        add_solid_curve(&mut state.plot, curve, CurveInterpolation::StepPost);
    }
    for curve in collect_primary_curves(state, spectrum_manager, settings) {
        add_solid_curve(&mut state.plot, curve, CurveInterpolation::StepPost);
    }
    for group in collect_roi_curves(state, spectrum_manager, palette) {
        add_solid_curve(&mut state.plot, group.gaussian, CurveInterpolation::Linear);
        if let Some(lin) = group.linear {
            add_dashed_curve(&mut state.plot, lin);
        }
    }

    // --- Markers: nuclide lines, feature lines, cursor ---
    for m in collect_markers(state, spectrum_manager) {
        let mut marker = Marker::vline(m.x)
            .with_line_style(m.line_style)
            .with_line_width(m.width)
            .with_draggable(false);
        marker.color = m.color;
        state.plot.add_marker_data(&marker);
    }

    // --- Show ---
    let response = state.plot.show(ui);
    handle_roi_drag(state, spectrum_manager, settings, &response, ui);

    // --- Read back bounds ---
    let (xmin, xmax) = (response.transform.x.min, response.transform.x.max);
    let (ymin, ymax) = (response.transform.y.min, response.transform.y.max);
    state.x_range = (xmin, xmax);
    state.y_range = (ymin, ymax);

    if state.reset_pending {
        state.plot.set_auto_reset_zoom(false);
        state.reset_pending = false;
    }

    // --- Y-axis lock: re-fit y to the visible x-window each frame ---
    // Runs after `show()` so `state.x_range` reflects this frame's viewport.
    // Writing `plot.limits` directly matches the existing gesture-write
    // pattern (see rsplot's `commit()`), so the change is picked up on the
    // next frame's `show()` with a 1-frame lag invisible at 60 fps.
    if state.y_axis_locked {
        if let Some((lo, hi)) = visible_y_range(state, spectrum_manager) {
            let plot = state.plot.plot_mut();
            let (x0, x1, _, _) = plot.limits;
            plot.limits = (x0, x1, lo, hi);
        }
    }

    // --- Cursor ---
    if state.cursor_enabled {
        if let Some(pointer) = response.response.hover_pos() {
            if response.transform.area.contains(pointer) {
                let (x, _y) = response.transform.pixel_to_data(pointer);
                if x.is_finite() {
                    state.cursor_x = x.clamp(xmin, xmax);
                }
            }
        }
        update_cursor(state, spectrum_manager, settings);
    } else {
        state.cursor_info.clear();
        state.cursor_nuclide = None;
        state.cursor_emission_lines.clear();
    }
}

// ===========================================================================
// Toolbar
// ===========================================================================

fn render_toolbar(
    state: &mut SpectrumPlotState,
    spectrum_manager: &mut SpectrumManager,
    settings: &Settings,
    ui: &mut egui::Ui,
) {
    ui.horizontal(|ui| {
        if ui.button("Reset Zoom").clicked() {
            state.request_reset();
        }

        let lock_label = if state.y_axis_locked {
            "Unlock y-axis"
        } else {
            "Lock y-axis"
        };
        if ui.button(lock_label).clicked() {
            state.y_axis_locked = !state.y_axis_locked;
            if state.y_axis_locked {
                state
                    .plot
                    .plot_mut()
                    .set_scroll_zoom_axes_override(Some((true, false)));
                state
                    .plot
                    .plot_mut()
                    .set_pan_axes_override(Some((true, false)));
            } else {
                state.plot.plot_mut().set_scroll_zoom_axes_override(None);
                state.plot.plot_mut().set_pan_axes_override(None);
            }
        }

        if ui.toggle_value(&mut state.log, "Log").changed() {
            state.request_reset();
        }

        let cps_enabled = state.bkg_mode == BkgDisplayMode::None;
        ui.add_enabled_ui(cps_enabled, |ui| {
            if ui.toggle_value(&mut state.cps, "CPS").changed() {
                state.user_cps_pref = state.cps;
                state.request_reset();
            }
        });

        if ui
            .toggle_value(&mut state.cursor_enabled, "Cursor")
            .changed()
        {
            if !state.cursor_enabled {
                state.cursor_info.clear();
                state.cursor_nuclide = None;
                state.cursor_emission_lines.clear();
            }
        }

        if ui.button("Add ROI").clicked() {
            let (lo, hi) = roi_bounds_from_view(state);
            let mut nr = NewRoi::new(lo, hi);
            nr.owner_spectrum = state.owned_spectrum.clone();
            spectrum_manager.roi_manager.add_roi(nr);
        }

        let mut new_mode = state.bkg_mode;
        egui::ComboBox::from_id_salt("spectrum_bkg_mode")
            .selected_text(state.bkg_mode.label())
            .width(200.0)
            .show_ui(ui, |ui| {
                for m in [
                    BkgDisplayMode::None,
                    BkgDisplayMode::Overlay,
                    BkgDisplayMode::Subtract,
                ] {
                    ui.selectable_value(&mut new_mode, m, m.label());
                }
            });
        if new_mode != state.bkg_mode {
            set_bkg_mode(state, spectrum_manager, new_mode);
        }

        let mut info = state.cursor_info.clone();
        ui.add(
            egui::TextEdit::singleline(&mut info)
                .desired_width(320.0)
                .interactive(false)
                .hint_text("Cursor"),
        );
    });
}

// ===========================================================================
// Curve data (library-agnostic)
// ===========================================================================

#[derive(Clone)]
struct CurveData {
    x: Vec<f64>,
    y: Vec<f64>,
    color: Color32,
    width: f32,
    legend: Option<String>,
}

fn add_solid_curve(plot: &mut Plot1D, c: CurveData, interpolation: CurveInterpolation) {
    if c.x.is_empty() || c.x.len() != c.y.len() {
        return;
    }
    match &c.legend {
        Some(name) => {
            plot.add_curve_with_legend_and_interpolation(&c.x, &c.y, c.color, name, interpolation);
        }
        None => {
            plot.add_curve_with_interpolation(&c.x, &c.y, c.color, interpolation);
        }
    }
    // Width override: rsplot's `add_curve` uses a default width. To set
    // width, re-apply a `CurveSpec`. Skipped here because the default is
    // fine and re-applying doubles the work; add `update_curve_spec` if a
    // specific width is needed.
    let _ = c.width;
}

fn add_dashed_curve(plot: &mut Plot1D, c: CurveData) {
    if c.x.is_empty() || c.x.len() != c.y.len() {
        return;
    }
    let handle = match &c.legend {
        Some(name) => plot.add_curve_with_legend(&c.x, &c.y, c.color, name),
        None => plot.add_curve(&c.x, &c.y, c.color),
    };
    let mut spec = CurveSpec::new(&c.x, &c.y, c.color);
    spec.line_style = LineStyle::Dashed;
    spec.line_width = c.width;
    plot.update_curve_spec(handle, spec);
}

// ---------------------------------------------------------------------------
// Primary / background
// ---------------------------------------------------------------------------

fn collect_primary_curves(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
    settings: &Settings,
) -> Vec<CurveData> {
    let mut out = Vec::new();
    for (name, spectrum) in &spectrum_manager.spectrum_registry {
        if !spectrum.show_in_plot {
            continue;
        }
        if let Some(owned) = &state.owned_spectrum {
            if owned != name {
                continue;
            }
        }
        if state.bkg_mode == BkgDisplayMode::Subtract && spectrum.background.is_none() {
            continue;
        }
        if !settings.appearance.pen {
            continue;
        }

        let y = match state.bkg_mode {
            BkgDisplayMode::Subtract => spectrum.get_bkg_sub(state.log),
            _ => spectrum.get_foreground(state.log, state.cps),
        };
        let Some(y) = y else { continue };
        let Some(x) = spectrum.x_axis.as_slice() else {
            continue;
        };

        out.push(CurveData {
            x: x.to_vec(),
            y: y.to_vec(),
            color: spectrum.color_foreground,
            width: 2.0,
            legend: Some(name.clone()),
        });
    }
    out
}

fn collect_background_curves(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
    settings: &Settings,
) -> Vec<CurveData> {
    if state.bkg_mode != BkgDisplayMode::Overlay || !settings.appearance.pen {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, spectrum) in &spectrum_manager.spectrum_registry {
        if !spectrum.show_in_plot || spectrum.background.is_none() {
            continue;
        }
        if let Some(owned) = &state.owned_spectrum {
            if owned != name {
                continue;
            }
        }
        let Some(y) = spectrum.get_background(state.log, state.cps) else {
            continue;
        };
        let Some(x) = spectrum.x_axis.as_slice() else {
            continue;
        };

        out.push(CurveData {
            x: x.to_vec(),
            y: y.to_vec(),
            color: spectrum.color_background,
            width: 2.0,
            legend: Some(format!("{name} (bkg)")),
        });
    }
    out
}

// ---------------------------------------------------------------------------
// ROI curves
// ---------------------------------------------------------------------------

struct RoiCurveGroup {
    gaussian: CurveData,
    linear: Option<CurveData>,
}

fn collect_roi_curves(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
    palette: &Palette,
) -> Vec<RoiCurveGroup> {
    let mut out = Vec::new();

    for (name, spectrum) in &spectrum_manager.spectrum_registry {
        if !spectrum.show_in_plot {
            continue;
        }
        if let Some(owned) = &state.owned_spectrum {
            if owned != name {
                continue;
            }
        }
        if state.bkg_mode == BkgDisplayMode::Subtract && spectrum.background.is_none() {
            continue;
        }

        for resolved in spectrum.rois.values() {
            let Some(fit) = &resolved.fit else { continue };
            let Some(roi) = spectrum_manager.roi_manager.get_roi(&resolved.tag) else {
                continue;
            };
            if roi.owner_spectrum != state.owned_spectrum {
                continue;
            }

            let (x_min, x_max) = match (fit.region_lower, fit.region_upper) {
                (Some(lo), Some(hi)) => (lo, hi),
                _ => roi.roi_bound,
            };

            let x_slice: Vec<f64> = spectrum
                .x_axis
                .iter()
                .copied()
                .filter(|&x| x > x_min && x < x_max)
                .collect();
            if x_slice.is_empty() {
                continue;
            }

            let x_arr = Array1::from_vec(x_slice.clone());
            let gauss = multi_gaussian(x_arr.view(), fit.params.as_slice().unwrap_or(&[]));
            let lin = if roi.bkg_type != BkgType::None {
                fit.bkg_params
                    .as_ref()
                    .map(|bp| {
                        let coeffs = bp.as_slice().unwrap_or(&[]);
                        x_arr.mapv(|x| polyval(coeffs, x))
                    })
                    .unwrap_or_else(|| Array1::zeros(x_arr.len()))
            } else {
                Array1::zeros(x_arr.len())
            };

            let lt = spectrum
                .foreground
                .as_ref()
                .map(|f| f.live_time)
                .unwrap_or(1.0)
                .max(1e-9);
            let scale = if state.cps { 1.0 / lt } else { 1.0 };

            let gauss_y: Vec<f64> = gauss
                .iter()
                .zip(lin.iter())
                .map(|(g, l)| (g + l) * scale)
                .collect();
            let lin_y: Vec<f64> = lin.iter().map(|l| l * scale).collect();

            out.push(RoiCurveGroup {
                gaussian: CurveData {
                    x: x_slice.clone(),
                    y: gauss_y,
                    color: palette.text,
                    width: 2.0,
                    legend: None,
                },
                linear: if roi.bkg_type != BkgType::None {
                    Some(CurveData {
                        x: x_slice,
                        y: lin_y,
                        color: palette.text,
                        width: 1.0,
                        legend: None,
                    })
                } else {
                    None
                },
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Markers
// ---------------------------------------------------------------------------

struct MarkerData {
    x: f64,
    color: Color32,
    width: f32,
    line_style: LineStyle,
}

fn collect_markers(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
) -> Vec<MarkerData> {
    let mut markers = Vec::new();

    for roi in spectrum_manager.roi_manager.registry().values() {
        if let Some(owned) = &state.owned_spectrum {
            if roi.owner_spectrum.as_deref() != Some(owned) {
                continue;
            }
        }
        let color = Color32::from_rgb(255, 200, 0);
        for x in [roi.roi_bound.0, roi.roi_bound.1] {
            markers.push(MarkerData {
                x,
                color,
                width: 1.5,
                line_style: LineStyle::Dashed,
            });
        }
    }

    for nuclide_id in spectrum_manager.nuclide_library.selected().iter() {
        let Some(nuclide) = spectrum_manager.nuclide_library.get_nuclide(*nuclide_id) else {
            continue;
        };
        for em in &nuclide.emissions {
            let Some(energy) = em.energy_kev.as_ref().map(|q| q.value.mean()) else {
                continue;
            };
            let dashed = matches!(em.r#type, Some(EmissionType::XRay));
            markers.push(MarkerData {
                x: energy,
                color: Color32::LIGHT_BLUE,
                width: 2.0,
                line_style: if dashed {
                    LineStyle::Dashed
                } else {
                    LineStyle::Solid
                },
            });
        }
    }

    // Cursor emission lines — the matched nuclide's emissions, amber.
    // X-rays dashed; everything else solid (mirrors Python's `update_cursor_lines`).
    for &(energy, is_xray) in &state.cursor_emission_lines {
        markers.push(MarkerData {
            x: energy,
            color: Color32::from_rgb(255, 200, 0),
            width: 1.5,
            line_style: if is_xray {
                LineStyle::Dashed
            } else {
                LineStyle::Solid
            },
        });
    }

    for fl in state.feature_lines.values() {
        markers.push(MarkerData {
            x: fl.value,
            color: fl.color,
            width: 2.0,
            line_style: LineStyle::Solid,
        });
    }

    if state.cursor_enabled {
        markers.push(MarkerData {
            x: state.cursor_x,
            color: Color32::GREEN,
            width: 2.0,
            line_style: LineStyle::Solid,
        });
    }

    markers
}

// ===========================================================================
// Cursor text
// ===========================================================================

fn update_cursor(
    state: &mut SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
    settings: &Settings,
) {
    use crate::containers::{Emission, EmissionOrigin};
    use crate::core::MatchOptions;

    let x = state.cursor_x;
    let matched = spectrum_manager.nuclide_library.match_energy_with(
        x,
        MatchOptions {
            match_only_shown: false,
            window: 33.0,
            weight_by_intensity: false,
            filter_by_intensity_percent: 10.0,
        },
    );

    let channel_str = state
        .owned_spectrum
        .as_ref()
        .and_then(|name| spectrum_manager.get_spectrum(name))
        .and_then(|spectrum| {
            let ch = spectrum.get_channel_from_energy(x);
            let y = match state.bkg_mode {
                BkgDisplayMode::Subtract => spectrum.get_bkg_sub(state.log),
                _ => spectrum.get_foreground(state.log, state.cps),
            }?;
            let val = y.get(ch).copied().unwrap_or(0.0);
            Some(if state.cps {
                format!("Chn: {}, #: {:.4} CPS ", ch, val)
            } else {
                format!("Chn: {}, #: {} ", ch, val as i64)
            })
        })
        .unwrap_or_default();

    if let Some(m) = &matched {
        let parent = m.parent_nuclide.map(|id| id.to_name()).unwrap_or_default();
        let e_str = m
            .energy_kev
            .as_ref()
            .map(|q| format!("{:.1} keV", q.value.mean()))
            .unwrap_or_default();
        let is_th = m.parent_nuclide.is_some_and(|id| {
            spectrum_manager
                .nuclide_library
                .decay_chain_ids("Th-232 -- Chain")
                .map(|chain| chain.contains(&id))
                .unwrap_or(false)
        });
        let is_u = m.parent_nuclide.is_some_and(|id| {
            spectrum_manager
                .nuclide_library
                .decay_chain_ids("U-238 -- Chain")
                .map(|chain| chain.contains(&id))
                .unwrap_or(false)
        });
        let suffix = if is_th {
            " (Th-232-Chain)"
        } else if is_u {
            " (U-238-Chain)"
        } else {
            ""
        };
        state.cursor_info = format!(
            "E: {:<4.0} keV, {channel_str}[{} {}{}]",
            x, parent, e_str, suffix
        );
        state.cursor_nuclide = m.parent_nuclide;
    } else {
        state.cursor_info = format!("E: {:<4.0} keV, {channel_str}", x);
        state.cursor_nuclide = None;
    }

    // --- Rebuild the amber emission-lines list (Python: update_cursor_lines) ---
    state.cursor_emission_lines.clear();
    if !settings.temp.spectrum_view_emission_lines_to_cursor {
        return;
    }
    let Some(m) = &matched else {
        return;
    };

    let is_annihilation = matches!(m.origin, Some(EmissionOrigin::Annihilation));

    // Python: annihilation → just the 511 keV line; otherwise every emission
    // of the matched nuclide.
    let emissions: Vec<&Emission> = if is_annihilation {
        vec![m]
    } else if let Some(id) = m.parent_nuclide {
        spectrum_manager
            .nuclide_library
            .get_nuclide(id)
            .map(|n| n.emissions.iter().collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    for em in emissions {
        let intensity = em
            .intensity_percent
            .as_ref()
            .map(|q| q.value.mean())
            .unwrap_or(0.0);
        if intensity < 9.5 {
            continue;
        }
        let Some(energy) = em.energy_kev.as_ref().map(|q| q.value.mean()) else {
            continue;
        };
        let is_xray = matches!(em.r#type, Some(EmissionType::XRay));
        state.cursor_emission_lines.push((energy, is_xray));
    }
}

// ===========================================================================
// Mode transitions
// ===========================================================================

fn set_bkg_mode(
    state: &mut SpectrumPlotState,
    spectrum_manager: &mut SpectrumManager,
    mode: BkgDisplayMode,
) {
    match mode {
        BkgDisplayMode::None => {
            state.cps = state.user_cps_pref;
            spectrum_manager.roi_manager.set_bkg_sub(false);
        }
        BkgDisplayMode::Overlay => {
            state.user_cps_pref = state.cps;
            state.cps = true;
            spectrum_manager.roi_manager.set_bkg_sub(false);
        }
        BkgDisplayMode::Subtract => {
            state.user_cps_pref = state.cps;
            state.cps = true;
            spectrum_manager.roi_manager.set_bkg_sub(true);
        }
    }
    state.bkg_mode = mode;
    state.request_reset();

    let SpectrumManager {
        roi_manager,
        spectrum_registry,
        ..
    } = spectrum_manager;
    roi_manager.update_roi(spectrum_registry, None, None, true);
}

// ===========================================================================
// Small helpers
// ===========================================================================

/// Y-range that fits the visible data in the current x-window.
///
/// Used by the y-axis lock to re-fit y each frame as the user pans or zooms x.
/// Mirrors pyqtgraph's `enableAutoRange(axis='y')` + `setAutoVisible(y=True)`
/// combination: only data whose x falls inside `state.x_range` contributes.
///
/// * Linear mode: `y` from `get_foreground(false, …)` is raw counts, so the
///   fit floor is 0 (matches `set_y_min_pos(Some(0.0))`) and the ceiling is
///   `max * 1.05` for a small margin.
/// * Log mode: `y` from `get_foreground(true, …)` is already `log10(counts)`,
///   so the fit floor is `max - 6` (six decades of headroom below the peak)
///   and the ceiling is `max + 0.5`.
///
/// Returns `None` when the window contains no finite data (empty region,
/// spectrum not loaded, or all-zero data in log mode → all NaN).
fn visible_y_range(
    state: &SpectrumPlotState,
    spectrum_manager: &SpectrumManager,
) -> Option<(f64, f64)> {
    let (xmin, xmax) = state.x_range;
    if !(xmax > xmin) {
        return None;
    }

    let mut y_max = f64::NEG_INFINITY;
    let mut any = false;

    for (name, spectrum) in &spectrum_manager.spectrum_registry {
        if !spectrum.show_in_plot {
            continue;
        }
        if let Some(owned) = &state.owned_spectrum {
            if owned != name {
                continue;
            }
        }
        let y = match state.bkg_mode {
            BkgDisplayMode::Subtract => spectrum.get_bkg_sub(state.log),
            _ => spectrum.get_foreground(state.log, state.cps),
        };
        let Some(y) = y else { continue };
        let Some(x) = spectrum.x_axis.as_slice() else {
            continue;
        };

        for (&xi, &yi) in x.iter().zip(y.iter()) {
            if xi >= xmin && xi <= xmax && yi.is_finite() {
                any = true;
                if yi > y_max {
                    y_max = yi;
                }
            }
        }
    }

    if !any || !y_max.is_finite() {
        return None;
    }

    if state.log {
        Some((y_max - 6.0, y_max + 0.5))
    } else {
        Some((0.0, y_max * 1.05 + 1.0))
    }
}

fn roi_bounds_from_view(state: &SpectrumPlotState) -> (f64, f64) {
    let (x_min, x_max) = state.x_range;
    let diff = (x_max - x_min).min(400.0);
    (x_min + diff * 0.15, x_min + diff * 0.45)
}

fn polyval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().fold(0.0, |acc, &c| acc * x + c)
}
