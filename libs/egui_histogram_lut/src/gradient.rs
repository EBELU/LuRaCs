use egui::Color32;

/// Color interpolation space, mirroring `PyQtGraph`'s `GradientEditorItem.colorMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Rgb,
    Hsv,
}

/// Built-in gradient presets matching `PyQtGraph`'s `GradientPresets` plus
/// standard perceptually-uniform colormaps (`colorous`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientPreset {
    #[default]
    Thermal,
    Flame,
    Yellowy,
    Bipolar,
    Spectrum,
    Cyclic,
    Greyclip,
    Grey,
    Viridis,
    Plasma,
    Inferno,
    Magma,
    Cividis,
    Turbo,
}

impl GradientPreset {
    pub const ALL: [Self; 14] = [
        Self::Thermal,
        Self::Flame,
        Self::Yellowy,
        Self::Bipolar,
        Self::Spectrum,
        Self::Cyclic,
        Self::Greyclip,
        Self::Grey,
        Self::Viridis,
        Self::Plasma,
        Self::Inferno,
        Self::Magma,
        Self::Cividis,
        Self::Turbo,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Thermal => "thermal",
            Self::Flame => "flame",
            Self::Yellowy => "yellowy",
            Self::Bipolar => "bipolar",
            Self::Spectrum => "spectrum",
            Self::Cyclic => "cyclic",
            Self::Greyclip => "greyclip",
            Self::Grey => "grey",
            Self::Viridis => "viridis",
            Self::Plasma => "plasma",
            Self::Inferno => "inferno",
            Self::Magma => "magma",
            Self::Cividis => "cividis",
            Self::Turbo => "turbo",
        }
    }
}

/// A single color stop on a [`Gradient`], matching a `GradientEditorItem` tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientTick {
    /// Normalized position along the gradient in `[0.0, 1.0]`.
    pub pos: f32,
    pub color: Color32,
}

impl GradientTick {
    #[must_use]
    pub fn new(pos: f32, color: Color32) -> Self {
        Self {
            pos: pos.clamp(0.0, 1.0),
            color,
        }
    }
}

/// Editable multi-stop color gradient matching `PyQtGraph`'s `GradientEditorItem` model.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub mode: ColorMode,
    /// Sorted by `pos` ascending. Always contains at least one tick when non-empty.
    ticks: Vec<GradientTick>,
    pub allow_add: bool,
    pub preset: Option<GradientPreset>,
}

impl Default for Gradient {
    fn default() -> Self {
        Self::from_preset(GradientPreset::default())
    }
}

impl Gradient {
    /// Construct a gradient from one of the built-in presets with its default stop count.
    #[must_use]
    pub fn from_preset(preset: GradientPreset) -> Self {
        let mut g = Self {
            mode: ColorMode::Rgb,
            ticks: Vec::new(),
            allow_add: true,
            preset: Some(preset),
        };
        g.load_preset(preset);
        g
    }

    /// Construct a gradient from one of the built-in presets resampled to `num_stops`
    /// color knobs (clamped to at least 2).
    #[must_use]
    pub fn from_preset_with_stops(preset: GradientPreset, num_stops: usize) -> Self {
        let mut g = Self {
            mode: ColorMode::Rgb,
            ticks: Vec::new(),
            allow_add: true,
            preset: Some(preset),
        };
        g.load_preset_with_stops(preset, num_stops);
        g
    }

    /// Construct a custom gradient from explicit ticks.
    #[must_use]
    pub fn from_ticks(mode: ColorMode, mut ticks: Vec<GradientTick>) -> Self {
        ticks.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        for t in &mut ticks {
            t.pos = t.pos.clamp(0.0, 1.0);
        }
        Self {
            mode,
            ticks,
            allow_add: true,
            preset: None,
        }
    }

    /// Replace the current ticks and mode with `preset` (mirrors `loadPreset`).
    pub fn load_preset(&mut self, preset: GradientPreset) {
        self.preset = Some(preset);
        match preset {
            GradientPreset::Thermal => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(0.3333, Color32::from_rgb(185, 0, 0)),
                    GradientTick::new(0.6666, Color32::from_rgb(255, 220, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 255, 255)),
                ];
            }
            GradientPreset::Flame => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(0.2, Color32::from_rgb(7, 0, 220)),
                    GradientTick::new(0.5, Color32::from_rgb(236, 0, 134)),
                    GradientTick::new(0.8, Color32::from_rgb(246, 246, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 255, 255)),
                ];
            }
            GradientPreset::Yellowy => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(0.2329, Color32::from_rgb(32, 0, 129)),
                    GradientTick::new(0.5258, Color32::from_rgb(115, 15, 255)),
                    GradientTick::new(0.8363, Color32::from_rgb(255, 255, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 255, 255)),
                ];
            }
            GradientPreset::Bipolar => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 255, 255)),
                    GradientTick::new(0.25, Color32::from_rgb(0, 0, 255)),
                    GradientTick::new(0.5, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(0.75, Color32::from_rgb(255, 0, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 255, 0)),
                ];
            }
            GradientPreset::Spectrum => {
                self.mode = ColorMode::Hsv;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(255, 0, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 0, 255)),
                ];
            }
            GradientPreset::Cyclic => {
                self.mode = ColorMode::Hsv;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(255, 0, 4)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 0, 0)),
                ];
            }
            GradientPreset::Greyclip => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(0.99, Color32::from_rgb(255, 255, 255)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 0, 0)),
                ];
            }
            GradientPreset::Grey => {
                self.mode = ColorMode::Rgb;
                self.ticks = vec![
                    GradientTick::new(0.0, Color32::from_rgb(0, 0, 0)),
                    GradientTick::new(1.0, Color32::from_rgb(255, 255, 255)),
                ];
            }
            GradientPreset::Viridis => self.load_colorous(colorous::VIRIDIS, 16),
            GradientPreset::Plasma => self.load_colorous(colorous::PLASMA, 16),
            GradientPreset::Inferno => self.load_colorous(colorous::INFERNO, 16),
            GradientPreset::Magma => self.load_colorous(colorous::MAGMA, 16),
            GradientPreset::Cividis => self.load_colorous(colorous::CIVIDIS, 16),
            GradientPreset::Turbo => self.load_colorous(colorous::TURBO, 16),
        }
    }

    /// Replace the current ticks and mode with `preset` resampled to `num_stops`
    /// color knobs (clamped to at least 2).
    pub fn load_preset_with_stops(&mut self, preset: GradientPreset, num_stops: usize) {
        let stops = num_stops.max(2);
        match preset {
            GradientPreset::Viridis => self.load_colorous(colorous::VIRIDIS, stops),
            GradientPreset::Plasma => self.load_colorous(colorous::PLASMA, stops),
            GradientPreset::Inferno => self.load_colorous(colorous::INFERNO, stops),
            GradientPreset::Magma => self.load_colorous(colorous::MAGMA, stops),
            GradientPreset::Cividis => self.load_colorous(colorous::CIVIDIS, stops),
            GradientPreset::Turbo => self.load_colorous(colorous::TURBO, stops),
            _ => {
                self.load_preset(preset);
                let resampled = (0..stops)
                    .map(|i| {
                        #[expect(clippy::cast_precision_loss)]
                        let pos = (i as f32) / ((stops - 1) as f32);
                        GradientTick::new(pos, self.get_color(pos))
                    })
                    .collect();
                self.ticks = resampled;
            }
        }
        self.preset = Some(preset);
    }

    fn load_colorous(&mut self, grad: colorous::Gradient, num_stops: usize) {
        let stops = num_stops.max(2);
        self.mode = ColorMode::Rgb;
        self.ticks = (0..stops)
            .map(|i| {
                #[expect(clippy::cast_precision_loss)]
                let pos = (i as f32) / ((stops - 1) as f32);
                let c = grad.eval_continuous(f64::from(pos));
                GradientTick::new(pos, Color32::from_rgb(c.r, c.g, c.b))
            })
            .collect();
    }

    #[must_use]
    pub fn ticks(&self) -> &[GradientTick] {
        &self.ticks
    }

    /// Add a new tick at `pos` (clamped to `[0, 1]`) and return its sorted index.
    pub fn add_tick(&mut self, pos: f32, color: Color32) -> usize {
        self.preset = None;
        let pos = pos.clamp(0.0, 1.0);
        let idx = self.ticks.partition_point(|t| t.pos <= pos);
        self.ticks.insert(idx, GradientTick::new(pos, color));
        idx
    }

    /// Remove tick `idx` if at least 2 ticks would remain. Returns `true` if removed.
    pub fn remove_tick(&mut self, idx: usize) -> bool {
        if self.ticks.len() <= 2 || idx >= self.ticks.len() {
            return false;
        }
        self.preset = None;
        self.ticks.remove(idx);
        true
    }

    /// Move tick `idx` to `new_pos` (clamped to `[0, 1]`) and re-sort, returning its new index.
    pub fn set_tick_pos(&mut self, idx: usize, new_pos: f32) -> usize {
        if idx >= self.ticks.len() {
            return idx;
        }
        self.preset = None;
        let target = self.ticks[idx];
        let new_pos = new_pos.clamp(0.0, 1.0);
        self.ticks[idx].pos = new_pos;

        // Keep ticks sorted while tracking the moved tick by pointer/identity.
        let mut new_idx = idx;
        while new_idx > 0 && self.ticks[new_idx].pos < self.ticks[new_idx - 1].pos {
            self.ticks.swap(new_idx, new_idx - 1);
            new_idx -= 1;
        }
        while new_idx + 1 < self.ticks.len()
            && self.ticks[new_idx].pos > self.ticks[new_idx + 1].pos
        {
            self.ticks.swap(new_idx, new_idx + 1);
            new_idx += 1;
        }
        let _ = target;
        new_idx
    }

    pub fn set_tick_color(&mut self, idx: usize, color: Color32) {
        if let Some(t) = self.ticks.get_mut(idx) {
            self.preset = None;
            t.color = color;
        }
    }

    /// Reverse all tick positions (`pos -> 1.0 - pos`).
    pub fn reverse(&mut self) {
        self.preset = None;
        for t in &mut self.ticks {
            t.pos = (1.0 - t.pos).clamp(0.0, 1.0);
        }
        self.ticks.reverse();
    }

    /// Sample the gradient at normalized position `x` in `[0.0, 1.0]`
    /// (mirrors `GradientEditorItem.getColor`).
    #[must_use]
    pub fn get_color(&self, x: f32) -> Color32 {
        if self.ticks.is_empty() {
            return Color32::BLACK;
        }
        let x = x.clamp(0.0, 1.0);
        if x <= self.ticks[0].pos {
            return self.ticks[0].color;
        }
        let last = self.ticks[self.ticks.len() - 1];
        if x >= last.pos {
            return last.color;
        }
        for win in self.ticks.windows(2) {
            let a = win[0];
            let b = win[1];
            if x <= b.pos {
                let span = b.pos - a.pos;
                let frac = if span <= 1e-6 {
                    0.0
                } else {
                    ((x - a.pos) / span).clamp(0.0, 1.0)
                };
                return match self.mode {
                    ColorMode::Rgb => lerp_rgb(a.color, b.color, frac),
                    ColorMode::Hsv => lerp_hsv(a.color, b.color, frac),
                };
            }
        }
        last.color
    }

    /// Generate an `n_pts` RGBA lookup table (mirrors `GradientEditorItem.getLookupTable`).
    #[must_use]
    pub fn get_lookup_table(&self, n_pts: usize) -> Vec<Color32> {
        if n_pts == 0 {
            return Vec::new();
        }
        if n_pts == 1 {
            return vec![self.get_color(0.0)];
        }
        (0..n_pts)
            .map(|i| {
                #[expect(clippy::cast_precision_loss)]
                let x = (i as f32) / ((n_pts - 1) as f32);
                self.get_color(x)
            })
            .collect()
    }
}

fn lerp_rgb(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp_u8 = |u: u8, v: u8| -> u8 {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            f32::from(u)
                .mul_add(1.0 - t, f32::from(v) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        }
    };
    Color32::from_rgba_unmultiplied(
        lerp_u8(a.r(), b.r()),
        lerp_u8(a.g(), b.g()),
        lerp_u8(a.b(), b.b()),
        lerp_u8(a.a(), b.a()),
    )
}

fn lerp_hsv(first: Color32, second: Color32, frac: f32) -> Color32 {
    let (h0, s0, v0) = rgb_to_hsv(first);
    let (h1, s1, v1) = rgb_to_hsv(second);
    let hue = (h1 - h0).mul_add(frac, h0);
    let sat = (s1 - s0).mul_add(frac, s0);
    let val = (v1 - v0).mul_add(frac, v0);
    let [red, green, blue] = hsv_to_rgb(hue, sat, val);
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let alpha = f32::from(first.a())
        .mul_add(1.0 - frac, f32::from(second.a()) * frac)
        .round()
        .clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
}

fn rgb_to_hsv(color: Color32) -> (f32, f32, f32) {
    let red = f32::from(color.r()) / 255.0;
    let green = f32::from(color.g()) / 255.0;
    let blue = f32::from(color.b()) / 255.0;
    let max_c = red.max(green).max(blue);
    let min_c = red.min(green).min(blue);
    let delta = max_c - min_c;
    let hue = if delta <= 1e-6 {
        0.0
    } else if (max_c - red).abs() <= 1e-6 {
        60.0 * (((green - blue) / delta).rem_euclid(6.0))
    } else if (max_c - green).abs() <= 1e-6 {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    let sat = if max_c <= 1e-6 { 0.0 } else { delta / max_c };
    (hue, sat, max_c)
}

fn hsv_to_rgb(h_deg: f32, sat: f32, val: f32) -> [u8; 3] {
    let h_deg = h_deg.rem_euclid(360.0);
    let chroma = val * sat;
    let sector = h_deg / 60.0;
    let sec = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if sector < 1.0 {
        (chroma, sec, 0.0)
    } else if sector < 2.0 {
        (sec, chroma, 0.0)
    } else if sector < 3.0 {
        (0.0, chroma, sec)
    } else if sector < 4.0 {
        (0.0, sec, chroma)
    } else if sector < 5.0 {
        (sec, 0.0, chroma)
    } else {
        (chroma, 0.0, sec)
    };
    let match_val = val - chroma;
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    [
        ((r1 + match_val) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g1 + match_val) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b1 + match_val) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}
