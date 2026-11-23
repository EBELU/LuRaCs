use egui::{Color32, Context, RawInput, Vec2};
use egui_histogram_lut::{
    ColorMode, Gradient, GradientPreset, GradientTick, HistogramData, HistogramLutState,
    HistogramLutWidget, Orientation,
};

#[test]
fn test_presets_and_lookup_table() {
    for preset in GradientPreset::ALL {
        let grad = Gradient::from_preset(preset);
        assert!(grad.ticks().len() >= 2);
        let lut = grad.get_lookup_table(256);
        assert_eq!(lut.len(), 256);
        assert_eq!(lut[0], grad.get_color(0.0));
        assert_eq!(lut[255], grad.get_color(1.0));
    }

    let thermal = Gradient::from_preset(GradientPreset::Thermal);
    assert_eq!(thermal.get_color(0.0), Color32::BLACK);
    assert_eq!(thermal.get_color(1.0), Color32::WHITE);
}

#[test]
fn test_gradient_tick_editing_and_reverse() {
    let mut grad = Gradient::from_ticks(
        ColorMode::Rgb,
        vec![
            GradientTick::new(0.0, Color32::BLACK),
            GradientTick::new(1.0, Color32::WHITE),
        ],
    );
    // Cannot remove when only 2 ticks remain
    assert!(!grad.remove_tick(0));

    let mid = grad.add_tick(0.5, Color32::RED);
    assert_eq!(mid, 1);
    assert_eq!(grad.ticks().len(), 3);

    // Move past upper tick
    let moved = grad.set_tick_pos(1, 0.9);
    assert_eq!(moved, 1);
    assert!((grad.ticks()[1].pos - 0.9).abs() < 1e-6);

    grad.reverse();
    assert!((grad.ticks()[0].pos - 0.0).abs() < 1e-6);
    assert_eq!(grad.ticks()[0].color, Color32::WHITE);

    assert!(grad.remove_tick(1));
    assert_eq!(grad.ticks().len(), 2);
}

#[test]
fn test_hsv_interpolation() {
    let spec = Gradient::from_preset(GradientPreset::Spectrum);
    assert_eq!(spec.mode, ColorMode::Hsv);
    let mid = spec.get_color(0.5);
    // Midpoint between 0° (red) and 300° (magenta) is ~150° (spring green)
    assert!(mid.g() > 200);
}

#[test]
fn test_histogram_and_state_mapping() {
    let samples = [0.0, 1.0, 2.0, f64::NAN, f64::INFINITY, 10.0];
    let hist = HistogramData::from_samples(&samples, 10);
    assert_eq!(hist.edges.len(), 11);
    assert_eq!(hist.counts.len(), 10);
    let total: f64 = hist.counts.iter().sum();
    assert!((total - 4.0).abs() < 1e-9);

    let mut state = HistogramLutState::new(GradientPreset::Grey);
    state.set_data(&samples, 10, true);
    assert_eq!(state.levels, (0.0, 10.0));

    state.set_levels(2.0, 8.0);
    assert_eq!(state.map_value(0.0), Color32::BLACK);
    assert_eq!(state.map_value(10.0), Color32::WHITE);
    let mid = state.map_value(5.0);
    assert!((i16::from(mid.r()) - 128).abs() <= 1);
}

#[test]
fn test_egui_widget_renders_vertical_and_horizontal() {
    let ctx = Context::default();
    let mut state = HistogramLutState::new(GradientPreset::Thermal);
    state.set_data(&[1.0, 2.0, 2.5, 3.0, 5.0, 8.0], 16, true);

    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        let resp_v = HistogramLutWidget::new("lut_v")
            .orientation(Orientation::Vertical)
            .size(Vec2::new(130.0, 280.0))
            .show(ui, &mut state);
        assert_eq!(resp_v.levels, (1.0, 8.0));

        let resp_h = HistogramLutWidget::new("lut_h")
            .orientation(Orientation::Horizontal)
            .size(Vec2::new(280.0, 110.0))
            .show(ui, &mut state);
        assert_eq!(resp_h.levels, (1.0, 8.0));
    });
    out.textures_delta.clear();
}
