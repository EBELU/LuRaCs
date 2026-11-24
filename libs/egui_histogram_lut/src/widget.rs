use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Id, IdSalt, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::gradient::{ColorMode, Gradient, GradientPreset};

/// Layout orientation matching `PyQtGraph`'s `HistogramLUTItem(orientation=...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

/// Pre-binned histogram data (`edges.len() == counts.len() + 1`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HistogramData {
    pub edges: Vec<f64>,
    pub counts: Vec<f64>,
}

impl HistogramData {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty() || self.edges.len() < 2
    }

    /// Compute a histogram from raw finite samples into `num_bins` uniform bins.
    #[must_use]
    pub fn from_samples(samples: &[f64], num_bins: usize) -> Self {
        let mut min_v = f64::INFINITY;
        let mut max_v = f64::NEG_INFINITY;
        let mut count_finite = 0usize;
        for &v in samples {
            if v.is_finite() {
                min_v = min_v.min(v);
                max_v = max_v.max(v);
                count_finite += 1;
            }
        }
        if count_finite == 0 || num_bins == 0 {
            return Self::default();
        }
        if (max_v - min_v).abs() <= f64::EPSILON {
            min_v -= 0.5;
            max_v += 0.5;
        }
        let span = max_v - min_v;
        let mut counts = vec![0.0_f64; num_bins];
        for &v in samples {
            if v.is_finite() {
                let norm = ((v - min_v) / span).clamp(0.0, 0.999_999_9);
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                let idx = ((norm * (num_bins as f64)) as usize).min(num_bins - 1);
                counts[idx] += 1.0;
            }
        }
        let edges = (0..=num_bins)
            .map(|i| {
                #[expect(clippy::cast_precision_loss)]
                let f = (i as f64) / (num_bins as f64);
                f.mul_add(span, min_v)
            })
            .collect();
        Self { edges, counts }
    }
}

/// Persistent state for [`HistogramLutWidget`], mirroring `PyQtGraph`'s `HistogramLUTItem`.
#[derive(Debug, Clone, PartialEq)]
pub struct HistogramLutState {
    pub levels: (f64, f64),
    pub view_range: (f64, f64),
    pub data_range: (f64, f64),
    pub gradient: Gradient,
    pub histogram: HistogramData,
    pub log_histogram: bool,
    pub selected_tick: Option<usize>,
    pub editing_tick_color: Option<usize>,
    pub preset_stops: Option<usize>,
}

impl Default for HistogramLutState {
    fn default() -> Self {
        Self {
            levels: (0.0, 1.0),
            view_range: (0.0, 1.0),
            data_range: (0.0, 1.0),
            gradient: Gradient::default(),
            histogram: HistogramData::default(),
            log_histogram: false,
            selected_tick: None,
            editing_tick_color: None,
            preset_stops: None,
        }
    }
}

impl HistogramLutState {
    #[must_use]
    pub fn new(preset: GradientPreset) -> Self {
        Self {
            gradient: Gradient::from_preset(preset),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn new_with_stops(preset: GradientPreset, num_stops: usize) -> Self {
        Self {
            gradient: Gradient::from_preset_with_stops(preset, num_stops),
            preset_stops: Some(num_stops.max(2)),
            ..Self::default()
        }
    }

    /// Populate the histogram from raw samples and optionally reset levels/view range.
    pub fn set_data(&mut self, samples: &[f64], num_bins: usize, reset_levels: bool) {
        let hist = HistogramData::from_samples(samples, num_bins);
        if let (Some(&lo), Some(&hi)) = (hist.edges.first(), hist.edges.last()) {
            self.data_range = (lo, hi);
            if reset_levels {
                self.levels = (lo, hi);
                self.view_range = (lo, hi);
            } else {
                self.view_range = (
                    self.view_range.0.min(lo).min(self.levels.0),
                    self.view_range.1.max(hi).max(self.levels.1),
                );
            }
        }
        self.histogram = hist;
    }

    /// Set a pre-computed histogram (`edges.len() == counts.len() + 1`).
    pub fn set_histogram(&mut self, edges: Vec<f64>, counts: Vec<f64>, reset_levels: bool) {
        if let (Some(&lo), Some(&hi)) = (edges.first(), edges.last()) {
            self.data_range = (lo, hi);
            if reset_levels {
                self.levels = (lo, hi);
                self.view_range = (lo, hi);
            }
        }
        self.histogram = HistogramData { edges, counts };
    }

    /// Set the active `[min, max]` level bounds (mirrors `HistogramLUTItem.setLevels`).
    pub fn set_levels(&mut self, min: f64, max: f64) {
        let lo = min.min(max);
        let hi = max.max(min + 1e-9);
        self.levels = (lo, hi);
    }

    /// Reset `levels` to `data_range`.
    pub fn auto_level(&mut self) {
        self.levels = (
            self.data_range.0,
            self.data_range.1.max(self.data_range.0 + 1e-9),
        );
    }

    /// Reset `view_range` to encompass both `data_range` and `levels`.
    pub fn auto_range(&mut self) {
        let lo = self.data_range.0.min(self.levels.0);
        let hi = self.data_range.1.max(self.levels.1).max(lo + 1e-9);
        self.view_range = (lo, hi);
    }

    /// Generate an `n_pts` lookup table from the active gradient.
    #[must_use]
    pub fn get_lookup_table(&self, n_pts: usize) -> Vec<Color32> {
        self.gradient.get_lookup_table(n_pts)
    }

    /// Map a scalar `value` through `[levels.0, levels.1]` and the active [`Gradient`].
    #[must_use]
    pub fn map_value(&self, value: f64) -> Color32 {
        let span = (self.levels.1 - self.levels.0).max(1e-12);
        #[expect(clippy::cast_possible_truncation)]
        let t = ((value - self.levels.0) / span).clamp(0.0, 1.0) as f32;
        self.gradient.get_color(t)
    }
}

/// Output returned by [`HistogramLutWidget::show`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistogramLutResponse {
    /// True if `levels` changed this frame.
    pub levels_changed: bool,
    /// True if the gradient (preset, mode, or ticks) changed this frame.
    pub gradient_changed: bool,
    /// Current `(min, max)` levels.
    pub levels: (f64, f64),
}

/// PyQtGraph-faithful `HistogramLUTWidget` (`AxisItem` + histogram +
/// `LinearRegionItem` + interactive `GradientEditorItem`).
#[derive(Debug, Clone)]
pub struct HistogramLutWidget {
    id_salt: IdSalt,
    orientation: Orientation,
    size: Vec2,
    fill_color: Color32,
    region_fill: Color32,
    show_axis: bool,
    show_histogram: bool,
    show_gradient: bool,
}

impl HistogramLutWidget {
    #[must_use]
    pub fn new(id_source: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id_salt: IdSalt::new(id_source),
            orientation: Orientation::Vertical,
            size: Vec2::new(128.0, 320.0),
            fill_color: Color32::from_rgba_unmultiplied(200, 200, 210, 110),
            region_fill: Color32::from_rgba_unmultiplied(64, 140, 255, 50),
            show_axis: true,
            show_histogram: true,
            show_gradient: true,
        }
    }

    #[must_use]
    pub const fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }

    #[must_use]
    pub const fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    #[must_use]
    pub const fn fill_color(mut self, color: Color32) -> Self {
        self.fill_color = color;
        self
    }

    #[must_use]
    pub const fn show_axis(mut self, show: bool) -> Self {
        self.show_axis = show;
        self
    }

    #[must_use]
    pub const fn show_histogram(mut self, show: bool) -> Self {
        self.show_histogram = show;
        self
    }

    #[must_use]
    pub const fn show_gradient(mut self, show: bool) -> Self {
        self.show_gradient = show;
        self
    }

    /// Render the widget and handle all mouse interactions.
    pub fn show(&self, ui: &mut Ui, state: &mut HistogramLutState) -> HistogramLutResponse {
        let mut levels_changed = false;
        let mut gradient_changed = false;

        let (total_rect, _) = ui.allocate_exact_size(self.size, Sense::hover());
        let pad_v = 8.0_f32;
        let plot_rect = Rect::from_min_max(
            Pos2::new(total_rect.left(), total_rect.top() + pad_v),
            Pos2::new(total_rect.right(), total_rect.bottom() - pad_v),
        );

        let (axis_rect, hist_rect, grad_rect, tick_rect) = self.layout_rects(plot_rect);

        // Ensure valid view_range
        let mut vmin = state.view_range.0;
        let mut vmax = state.view_range.1;
        if !vmin.is_finite() || !vmax.is_finite() || vmax <= vmin {
            vmin = 0.0;
            vmax = 1.0;
            state.view_range = (vmin, vmax);
        }
        let vspan = (vmax - vmin).max(1e-12);
        let min_level_span = (vspan * 0.002).max(1e-9);

        let xform = PlotTransform {
            orientation: self.orientation,
            plot_rect,
            vmin,
            vspan,
        };
        let base_id = ui.make_persistent_id(self.id_salt);

        // 1. AxisItem
        if self.show_axis && axis_rect.width() > 0.0 && axis_rect.height() > 0.0 {
            self.draw_axis(ui, axis_rect, vmin, vmax, &xform);
        }

        // 2. Histogram + Pan/Zoom + LinearRegionItem
        if self.show_histogram && hist_rect.width() > 0.0 && hist_rect.height() > 0.0 {
            self.draw_histogram(ui, hist_rect, state, &xform);
            self.interact_hist_pan_zoom(ui, base_id, hist_rect, state, &xform);
            levels_changed |=
                self.interact_linear_region(ui, base_id, hist_rect, state, min_level_span, &xform);
        }

        // 3. GradientEditorItem (colorbar + draggable color ticks + context menu)
        if self.show_gradient && grad_rect.width() > 0.0 && grad_rect.height() > 0.0 {
            self.draw_gradient_bar(ui, grad_rect, state, &xform);
            gradient_changed |= Self::gradient_context_menu(ui, base_id, grad_rect, state);
            gradient_changed |=
                self.interact_gradient_ticks(ui, base_id, tick_rect, state, &xform);
            gradient_changed |= Self::show_tick_color_window(ui, base_id, grad_rect, state);
        }

        HistogramLutResponse {
            levels_changed,
            gradient_changed,
            levels: state.levels,
        }
    }

    fn layout_rects(&self, plot_rect: Rect) -> (Rect, Rect, Rect, Rect) {
        match self.orientation {
            Orientation::Vertical => {
                let axis_w = if self.show_axis { 36.0 } else { 0.0 };
                let tick_w = if self.show_gradient { 14.0 } else { 0.0 };
                let grad_w = if self.show_gradient { 20.0 } else { 0.0 };
                let gap = 3.0;

                let axis_rect = Rect::from_min_max(
                    plot_rect.min,
                    Pos2::new(plot_rect.left() + axis_w, plot_rect.bottom()),
                );
                let tick_rect = Rect::from_min_max(
                    Pos2::new(plot_rect.right() - tick_w, plot_rect.top()),
                    plot_rect.max,
                );
                let grad_rect = Rect::from_min_max(
                    Pos2::new(tick_rect.left() - grad_w, plot_rect.top()),
                    Pos2::new(tick_rect.left(), plot_rect.bottom()),
                );
                let hist_left = if self.show_axis {
                    axis_rect.right() + gap
                } else {
                    plot_rect.left()
                };
                let hist_right = if self.show_gradient {
                    grad_rect.left() - gap
                } else {
                    plot_rect.right()
                };
                let hist_rect = Rect::from_min_max(
                    Pos2::new(hist_left, plot_rect.top()),
                    Pos2::new(hist_right.max(hist_left), plot_rect.bottom()),
                );
                (axis_rect, hist_rect, grad_rect, tick_rect)
            }
            Orientation::Horizontal => {
                let axis_h = if self.show_axis { 22.0 } else { 0.0 };
                let tick_h = if self.show_gradient { 14.0 } else { 0.0 };
                let grad_h = if self.show_gradient { 20.0 } else { 0.0 };
                let gap = 3.0;

                let axis_rect = Rect::from_min_max(
                    Pos2::new(plot_rect.left(), plot_rect.bottom() - axis_h),
                    plot_rect.max,
                );
                let tick_rect = Rect::from_min_max(
                    plot_rect.min,
                    Pos2::new(plot_rect.right(), plot_rect.top() + tick_h),
                );
                let grad_rect = Rect::from_min_max(
                    Pos2::new(plot_rect.left(), tick_rect.bottom()),
                    Pos2::new(plot_rect.right(), tick_rect.bottom() + grad_h),
                );
                let hist_top = if self.show_gradient {
                    grad_rect.bottom() + gap
                } else {
                    plot_rect.top()
                };
                let hist_bot = if self.show_axis {
                    axis_rect.top() - gap
                } else {
                    plot_rect.bottom()
                };
                let hist_rect = Rect::from_min_max(
                    Pos2::new(plot_rect.left(), hist_top),
                    Pos2::new(plot_rect.right(), hist_bot.max(hist_top)),
                );
                (axis_rect, hist_rect, grad_rect, tick_rect)
            }
        }
    }

    fn draw_axis(
        &self,
        ui: &Ui,
        axis_rect: Rect,
        vmin: f64,
        vmax: f64,
        xform: &PlotTransform,
    ) {
        let painter = ui.painter_at(axis_rect.expand(6.0));
        let fg = ui.visuals().text_color();
        let line_color = ui.visuals().widgets.noninteractive.fg_stroke.color;
        let font = FontId::proportional(10.0);

        match self.orientation {
            Orientation::Vertical => {
                painter.line_segment(
                    [axis_rect.right_top(), axis_rect.right_bottom()],
                    Stroke::new(1.0, line_color),
                );
                for tick in nice_ticks(vmin, vmax, 5) {
                    let y = xform.pos_of(tick);
                    if y < axis_rect.top() - 2.0 || y > axis_rect.bottom() + 2.0 {
                        continue;
                    }
                    painter.line_segment(
                        [Pos2::new(axis_rect.right() - 4.0, y), Pos2::new(axis_rect.right(), y)],
                        Stroke::new(1.0, line_color),
                    );
                    painter.text(
                        Pos2::new(axis_rect.right() - 6.0, y),
                        egui::Align2::RIGHT_CENTER,
                        format_axis_val(tick),
                        font.clone(),
                        fg,
                    );
                }
            }
            Orientation::Horizontal => {
                painter.line_segment(
                    [axis_rect.left_top(), axis_rect.right_top()],
                    Stroke::new(1.0, line_color),
                );
                for tick in nice_ticks(vmin, vmax, 5) {
                    let x = xform.pos_of(tick);
                    if x < axis_rect.left() - 2.0 || x > axis_rect.right() + 2.0 {
                        continue;
                    }
                    painter.line_segment(
                        [Pos2::new(x, axis_rect.top()), Pos2::new(x, axis_rect.top() + 4.0)],
                        Stroke::new(1.0, line_color),
                    );
                    painter.text(
                        Pos2::new(x, axis_rect.top() + 5.0),
                        egui::Align2::CENTER_TOP,
                        format_axis_val(tick),
                        font.clone(),
                        fg,
                    );
                }
            }
        }
    }

    fn draw_histogram(
        &self,
        ui: &Ui,
        hist_rect: Rect,
        state: &HistogramLutState,
        xform: &PlotTransform,
    ) {
        let painter = ui.painter_at(hist_rect);
        painter.rect_filled(hist_rect, 0.0, ui.visuals().extreme_bg_color);

        if state.histogram.is_empty() {
            return;
        }

        let map_count = |c: f64| -> f64 {
            if state.log_histogram {
                (1.0 + c.max(0.0)).log10()
            } else {
                c.max(0.0)
            }
        };
        let max_c = state
            .histogram
            .counts
            .iter()
            .copied()
            .map(map_count)
            .fold(0.0_f64, f64::max)
            .max(1e-12);

        let stroke_color = Color32::from_rgba_unmultiplied(
            self.fill_color.r(),
            self.fill_color.g(),
            self.fill_color.b(),
            210,
        );

        for (i, &raw_cnt) in state.histogram.counts.iter().enumerate() {
            if raw_cnt <= 0.0 {
                continue;
            }
            let e0 = state.histogram.edges[i];
            let e1 = state.histogram.edges[i + 1];
            #[expect(clippy::cast_possible_truncation)]
            let frac = (map_count(raw_cnt) / max_c).clamp(0.0, 1.0) as f32;

            let bar = match self.orientation {
                Orientation::Vertical => {
                    let y0 = xform.pos_of(e1);
                    let y1 = xform.pos_of(e0);
                    Rect::from_min_max(
                        Pos2::new(hist_rect.left(), y0),
                        Pos2::new(frac.mul_add(hist_rect.width(), hist_rect.left()), y1),
                    )
                }
                Orientation::Horizontal => {
                    let x0 = xform.pos_of(e0);
                    let x1 = xform.pos_of(e1);
                    Rect::from_min_max(
                        Pos2::new(x0, frac.mul_add(-hist_rect.height(), hist_rect.bottom())),
                        Pos2::new(x1, hist_rect.bottom()),
                    )
                }
            };
            if bar.intersects(hist_rect) {
                painter.rect_filled(bar, 0.0, self.fill_color);
                painter.rect_stroke(bar, 0.0, Stroke::new(0.5, stroke_color), StrokeKind::Inside);
            }
        }
    }

    fn interact_hist_pan_zoom(
        &self,
        ui: &Ui,
        base_id: Id,
        hist_rect: Rect,
        state: &mut HistogramLutState,
        xform: &PlotTransform,
    ) {
        let bg_resp = ui.interact(hist_rect, base_id.with("hist_bg"), Sense::click_and_drag());
        if bg_resp.double_clicked() {
            state.auto_range();
        }
        if bg_resp.dragged_by(egui::PointerButton::Primary) {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            let delta = bg_resp.drag_delta();
            let vspan = state.view_range.1 - state.view_range.0;
            let dv = match self.orientation {
                Orientation::Vertical => f64::from(delta.y / hist_rect.height()) * vspan,
                Orientation::Horizontal => -f64::from(delta.x / hist_rect.width()) * vspan,
            };
            state.view_range.0 += dv;
            state.view_range.1 += dv;
        }
        if ui.rect_contains_pointer(hist_rect) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let factor = f64::from((-scroll * 0.0025).exp());
                let anchor = ui.input(|i| i.pointer.hover_pos()).map_or(
                    f64::midpoint(state.view_range.0, state.view_range.1),
                    |p| xform.val_of_pos(p),
                );
                let lo = anchor - (anchor - state.view_range.0) * factor;
                let hi = anchor + (state.view_range.1 - anchor) * factor;
                if (hi - lo) > 1e-9 && (hi - lo) < 1e12 {
                    state.view_range = (lo, hi);
                }
            }
        }
    }

    fn interact_linear_region(
        &self,
        ui: &Ui,
        base_id: Id,
        hist_rect: Rect,
        state: &mut HistogramLutState,
        min_level_span: f64,
        xform: &PlotTransform,
    ) -> bool {
        let mut levels_changed = false;
        let painter = ui.painter_at(hist_rect);
        let p_lo = xform.pos_of(state.levels.0);
        let p_hi = xform.pos_of(state.levels.1);

        let region_rect = match self.orientation {
            Orientation::Vertical => Rect::from_min_max(
                Pos2::new(hist_rect.left(), p_hi),
                Pos2::new(hist_rect.right(), p_lo),
            ),
            Orientation::Horizontal => Rect::from_min_max(
                Pos2::new(p_lo, hist_rect.top()),
                Pos2::new(p_hi, hist_rect.bottom()),
            ),
        };
        painter.rect_filled(region_rect, 0.0, self.region_fill);

        let hi_handle_rect = boundary_handle_rect(self.orientation, hist_rect, p_hi);
        let lo_handle_rect = boundary_handle_rect(self.orientation, hist_rect, p_lo);

        let hi_resp = ui.interact(hi_handle_rect, base_id.with("level_hi"), Sense::click_and_drag());
        if hi_resp.dragged_by(egui::PointerButton::Primary)
            && let Some(ptr) = hi_resp.interact_pointer_pos()
        {
            state.levels.1 = xform.val_of_pos(ptr).max(state.levels.0 + min_level_span);
            levels_changed = true;
        }

        let lo_resp = ui.interact(lo_handle_rect, base_id.with("level_lo"), Sense::click_and_drag());
        if lo_resp.dragged_by(egui::PointerButton::Primary)
            && let Some(ptr) = lo_resp.interact_pointer_pos()
        {
            state.levels.0 = xform.val_of_pos(ptr).min(state.levels.1 - min_level_span);
            levels_changed = true;
        }

        if hi_resp.hovered() || hi_resp.dragged() || lo_resp.hovered() || lo_resp.dragged() {
            ui.ctx().set_cursor_icon(match self.orientation {
                Orientation::Vertical => CursorIcon::ResizeVertical,
                Orientation::Horizontal => CursorIcon::ResizeHorizontal,
            });
        }

        let draw_bound = |coord: f32, active: bool| {
            let color = if active {
                Color32::from_rgb(255, 220, 90)
            } else {
                Color32::from_rgb(100, 175, 255)
            };
            let seg = match self.orientation {
                Orientation::Vertical => {
                    [Pos2::new(hist_rect.left(), coord), Pos2::new(hist_rect.right(), coord)]
                }
                Orientation::Horizontal => {
                    [Pos2::new(coord, hist_rect.top()), Pos2::new(coord, hist_rect.bottom())]
                }
            };
            painter.line_segment(seg, Stroke::new(2.0, color));
        };
        draw_bound(xform.pos_of(state.levels.1), hi_resp.hovered() || hi_resp.dragged());
        draw_bound(xform.pos_of(state.levels.0), lo_resp.hovered() || lo_resp.dragged());

        levels_changed
    }

    fn draw_gradient_bar(
        &self,
        ui: &Ui,
        grad_rect: Rect,
        state: &HistogramLutState,
        xform: &PlotTransform,
    ) {
        let painter = ui.painter_at(grad_rect);
        let level_span = (state.levels.1 - state.levels.0).max(1e-12);
        let steps = 64usize;
        match self.orientation {
            Orientation::Vertical => {
                let step_h = grad_rect.height() / 64.0;
                for i in 0..steps {
                    #[expect(clippy::cast_precision_loss)]
                    let y0 = (i as f32).mul_add(step_h, grad_rect.top());
                    let v = xform.val_of(y0 + 0.5 * step_h);
                    #[expect(clippy::cast_possible_truncation)]
                    let t = ((v - state.levels.0) / level_span).clamp(0.0, 1.0) as f32;
                    let strip = Rect::from_min_size(
                        Pos2::new(grad_rect.left(), y0),
                        Vec2::new(grad_rect.width(), step_h + 0.5),
                    );
                    painter.rect_filled(strip, 0.0, state.gradient.get_color(t));
                }
            }
            Orientation::Horizontal => {
                let step_w = grad_rect.width() / 64.0;
                for i in 0..steps {
                    #[expect(clippy::cast_precision_loss)]
                    let x0 = (i as f32).mul_add(step_w, grad_rect.left());
                    let v = xform.val_of(x0 + 0.5 * step_w);
                    #[expect(clippy::cast_possible_truncation)]
                    let t = ((v - state.levels.0) / level_span).clamp(0.0, 1.0) as f32;
                    let strip = Rect::from_min_size(
                        Pos2::new(x0, grad_rect.top()),
                        Vec2::new(step_w + 0.5, grad_rect.height()),
                    );
                    painter.rect_filled(strip, 0.0, state.gradient.get_color(t));
                }
            }
        }
        painter.rect_stroke(
            grad_rect,
            0.0,
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
            StrokeKind::Outside,
        );
    }

    fn gradient_context_menu(
        ui: &Ui,
        base_id: Id,
        grad_rect: Rect,
        state: &mut HistogramLutState,
    ) -> bool {
        let mut gradient_changed = false;
        let grad_resp = ui.interact(grad_rect, base_id.with("grad_bar"), Sense::click());
        grad_resp.context_menu(|ui| {
            ui.label("Gradient Presets");
            for preset in GradientPreset::ALL {
                let selected = state.gradient.preset == Some(preset);
                if ui.selectable_label(selected, preset.label()).clicked() {
                    if let Some(stops) = state.preset_stops {
                        state.gradient.load_preset_with_stops(preset, stops);
                    } else {
                        state.gradient.load_preset(preset);
                    }
                    gradient_changed = true;
                    ui.close();
                }
            }
            ui.separator();
            let is_hsv = state.gradient.mode == ColorMode::Hsv;
            if ui.selectable_label(!is_hsv, "RGB Mode").clicked() {
                state.gradient.mode = ColorMode::Rgb;
                gradient_changed = true;
                ui.close();
            }
            if ui.selectable_label(is_hsv, "HSV Mode").clicked() {
                state.gradient.mode = ColorMode::Hsv;
                gradient_changed = true;
                ui.close();
            }
            if ui.button("Reverse Gradient").clicked() {
                state.gradient.reverse();
                gradient_changed = true;
                ui.close();
            }
            if let Some(idx) = state.selected_tick
                && state.gradient.ticks().len() > 2
                && ui.button("Remove Selected Tick").clicked()
            {
                state.gradient.remove_tick(idx);
                state.selected_tick = None;
                state.editing_tick_color = None;
                gradient_changed = true;
                ui.close();
            }
            ui.separator();
            ui.checkbox(&mut state.log_histogram, "Log Histogram");
            if ui.button("Auto Levels").clicked() {
                state.auto_level();
                ui.close();
            }
            if ui.button("Auto Range").clicked() {
                state.auto_range();
                ui.close();
            }
        });
        gradient_changed
    }

    fn interact_gradient_ticks(
        &self,
        ui: &Ui,
        base_id: Id,
        tick_rect: Rect,
        state: &mut HistogramLutState,
        xform: &PlotTransform,
    ) -> bool {
        let mut gradient_changed = false;
        let tick_painter = ui.painter_at(tick_rect.expand(6.0));
        let p_lo = xform.pos_of(state.levels.0);
        let p_hi = xform.pos_of(state.levels.1);
        let tick_span = p_hi - p_lo;

        let tick_track_resp = ui.interact(tick_rect, base_id.with("tick_track"), Sense::click());
        if tick_track_resp.clicked()
            && state.gradient.allow_add
            && let Some(ptr) = tick_track_resp.interact_pointer_pos()
            && tick_span.abs() > 1.0
        {
            let coord = match self.orientation {
                Orientation::Vertical => ptr.y,
                Orientation::Horizontal => ptr.x,
            };
            let t = ((coord - p_lo) / tick_span).clamp(0.0, 1.0);
            let col = state.gradient.get_color(t);
            let idx = state.gradient.add_tick(t, col);
            state.selected_tick = Some(idx);
            gradient_changed = true;
        }

        let tick_count = state.gradient.ticks().len();
        for i in 0..tick_count {
            let tick = state.gradient.ticks()[i];
            let coord = tick.pos.mul_add(tick_span, p_lo);
            let hit_rect = match self.orientation {
                Orientation::Vertical => Rect::from_center_size(
                    Pos2::new(tick_rect.center().x, coord),
                    Vec2::new(tick_rect.width(), 11.0),
                ),
                Orientation::Horizontal => Rect::from_center_size(
                    Pos2::new(coord, tick_rect.center().y),
                    Vec2::new(11.0, tick_rect.height()),
                ),
            };
            let t_resp = ui.interact(
                hit_rect,
                base_id.with(("grad_tick", i)),
                Sense::click_and_drag(),
            );
            if t_resp.clicked() {
                state.selected_tick = Some(i);
            }
            if t_resp.double_clicked() {
                state.selected_tick = Some(i);
                state.editing_tick_color = Some(i);
            }
            if t_resp.dragged_by(egui::PointerButton::Primary)
                && let Some(ptr) = t_resp.interact_pointer_pos()
                && tick_span.abs() > 1.0
            {
                let c = match self.orientation {
                    Orientation::Vertical => ptr.y,
                    Orientation::Horizontal => ptr.x,
                };
                let new_t = ((c - p_lo) / tick_span).clamp(0.0, 1.0);
                let new_idx = state.gradient.set_tick_pos(i, new_t);
                state.selected_tick = Some(new_idx);
                gradient_changed = true;
            }
            if t_resp.hovered() || t_resp.dragged() {
                ui.ctx().set_cursor_icon(match self.orientation {
                    Orientation::Vertical => CursorIcon::ResizeVertical,
                    Orientation::Horizontal => CursorIcon::ResizeHorizontal,
                });
            }

            let border = if state.selected_tick == Some(i) {
                Color32::YELLOW
            } else {
                Color32::WHITE
            };
            let tri = match self.orientation {
                Orientation::Vertical => vec![
                    Pos2::new(tick_rect.left(), coord),
                    Pos2::new(tick_rect.right() - 1.0, coord - 5.0),
                    Pos2::new(tick_rect.right() - 1.0, coord + 5.0),
                ],
                Orientation::Horizontal => vec![
                    Pos2::new(coord, tick_rect.bottom()),
                    Pos2::new(coord - 5.0, tick_rect.top() + 1.0),
                    Pos2::new(coord + 5.0, tick_rect.top() + 1.0),
                ],
            };
            tick_painter.add(Shape::convex_polygon(tri, tick.color, Stroke::new(1.0, border)));
        }
        gradient_changed
    }

    fn show_tick_color_window(
        ui: &Ui,
        base_id: Id,
        grad_rect: Rect,
        state: &mut HistogramLutState,
    ) -> bool {
        let mut gradient_changed = false;
        let _ = CornerRadius::ZERO;
        if let Some(edit_idx) = state.editing_tick_color {
            if let Some(tick) = state.gradient.ticks().get(edit_idx).copied() {
                let mut open = true;
                let mut color = tick.color;
                egui::Window::new("Tick Color")
                    .id(base_id.with("tick_color_win"))
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .default_pos(grad_rect.left_top())
                    .show(ui.ctx(), |ui| {
                        if ui.color_edit_button_srgba(&mut color).changed() {
                            state.gradient.set_tick_color(edit_idx, color);
                            gradient_changed = true;
                        }
                        ui.horizontal(|ui| {
                            if state.gradient.ticks().len() > 2 && ui.button("Remove Tick").clicked()
                            {
                                state.gradient.remove_tick(edit_idx);
                                state.selected_tick = None;
                                state.editing_tick_color = None;
                                gradient_changed = true;
                            }
                            if ui.button("Close").clicked() {
                                state.editing_tick_color = None;
                            }
                        });
                    });
                if !open {
                    state.editing_tick_color = None;
                }
            } else {
                state.editing_tick_color = None;
            }
        }
        gradient_changed
    }
}

struct PlotTransform {
    orientation: Orientation,
    plot_rect: Rect,
    vmin: f64,
    vspan: f64,
}

impl PlotTransform {
    fn pos_of(&self, val: f64) -> f32 {
        #[expect(clippy::cast_possible_truncation)]
        let norm = ((val - self.vmin) / self.vspan) as f32;
        match self.orientation {
            Orientation::Vertical => norm.mul_add(-self.plot_rect.height(), self.plot_rect.bottom()),
            Orientation::Horizontal => norm.mul_add(self.plot_rect.width(), self.plot_rect.left()),
        }
    }

    fn val_of(&self, coord: f32) -> f64 {
        let norm = match self.orientation {
            Orientation::Vertical => {
                f64::from((self.plot_rect.bottom() - coord) / self.plot_rect.height())
            }
            Orientation::Horizontal => {
                f64::from((coord - self.plot_rect.left()) / self.plot_rect.width())
            }
        };
        norm.mul_add(self.vspan, self.vmin)
    }

    fn val_of_pos(&self, pos: Pos2) -> f64 {
        match self.orientation {
            Orientation::Vertical => self.val_of(pos.y),
            Orientation::Horizontal => self.val_of(pos.x),
        }
    }
}

fn boundary_handle_rect(orientation: Orientation, hist_rect: Rect, coord: f32) -> Rect {
    match orientation {
        Orientation::Vertical => Rect::from_center_size(
            Pos2::new(hist_rect.center().x, coord),
            Vec2::new(hist_rect.width(), 10.0),
        ),
        Orientation::Horizontal => Rect::from_center_size(
            Pos2::new(coord, hist_rect.center().y),
            Vec2::new(10.0, hist_rect.height()),
        ),
    }
}

fn nice_ticks(min: f64, max: f64, approx_count: usize) -> Vec<f64> {
    let span = max - min;
    if !span.is_finite() || span <= 0.0 || approx_count == 0 {
        return Vec::new();
    }
    #[expect(clippy::cast_precision_loss)]
    let raw_step = span / (approx_count as f64);
    let mag = 10.0_f64.powf(raw_step.log10().floor());
    let norm = raw_step / mag;
    let step = if norm <= 1.5 {
        1.0 * mag
    } else if norm <= 3.0 {
        2.0 * mag
    } else if norm <= 7.0 {
        5.0 * mag
    } else {
        10.0 * mag
    };
    let start = (min / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = start;
    while v <= max + step * 1e-6 && ticks.len() < 16 {
        ticks.push(if v.abs() < step * 1e-9 { 0.0 } else { v });
        v += step;
    }
    ticks
}

fn format_axis_val(v: f64) -> String {
    let av = v.abs();
    if av == 0.0 {
        "0".into()
    } else if av >= 10_000.0 || av < 0.01 {
        format!("{v:.1e}")
    } else if av >= 100.0 {
        format!("{v:.0}")
    } else if av >= 1.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}
