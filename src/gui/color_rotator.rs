//! Rotating colour palette for spectra, plots, and any repeated series.
//!
//! Port of `luracs.utils.color_rotator.ColorRotator`. Uses egui's `Color32`
//! and `Stroke` rather than `QColor` / `QPen` — `Stroke` is the closest egui
//! concept to a "pen" (colour + width), and every drawing API in egui/rsplot
//! takes one.
//!
//! ## Behaviour preserved from Python
//!
//! - Three built-in palettes: Matplotlib (`mpl`), LibreOffice (`lo`), and
//!   Catppuccin Latte (`catppuccin`). Same hex values, same order.
//! - `next_color` advances through the palette round-robin.
//! - `get_color_pair(shift)` returns the next colour plus a hue-shifted
//!   companion, used for foreground/background colouring.
//! - `reset()` returns to the start of the palette.
//!
//! ## Deviations from Python
//!
//! - `QColor.getHsv` returns hue `-1` for achromatic colours and Python has
//!   an explicit early return for that. `Hsva::from(Color32)` returns `h = 0`
//!   with `s = 0` for achromatic colours, so the shift is already a no-op
//!   without the special case. No branch needed.
//! - `next_pen` returns `Stroke` instead of `QPen`. Same callers, same
//!   semantics.

use eframe::egui::ecolor::Hsva;
use eframe::egui::{Color32, Stroke};
use serde::{Deserialize, Serialize};

// ===========================================================================
// Hue shift
// ===========================================================================

/// Shift the hue of `color` by `degrees`, keeping saturation / value / alpha.
///
/// Achromatic colours (saturation 0) are returned unchanged — the shift
/// affects hue only, and hue is irrelevant when saturation is zero.
pub fn shift_hue(color: Color32, degrees: f32) -> Color32 {
    let mut hsva = Hsva::from(color);
    // Wrap into 0.0..1.0, handling negative degrees correctly.
    hsva.h = (hsva.h + degrees / 360.0).rem_euclid(1.0);
    Color32::from(hsva)
}

// ===========================================================================
// ColorScheme
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorScheme {
    Mpl,
    LibreOffice,
    Catppuccin,
}

impl ColorScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mpl => "mpl",
            Self::LibreOffice => "lo",
            Self::Catppuccin => "catppuccin",
        }
    }

    pub fn palette(self) -> &'static [Color32] {
        match self {
            Self::Mpl => &MPL,
            Self::LibreOffice => &LIBREOFFICE,
            Self::Catppuccin => &CATPPUCCIN,
        }
    }

    pub const ALL: &'static [Self] = &[Self::Mpl, Self::LibreOffice, Self::Catppuccin];
}

/// Python's default when `colors is None` is `MPL`.
impl Default for ColorScheme {
    fn default() -> Self {
        Self::Mpl
    }
}

impl std::fmt::Display for ColorScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ColorScheme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "mpl" | "matplotlib" => Ok(Self::Mpl),
            "lo" | "libreoffice" => Ok(Self::LibreOffice),
            "catppuccin" | "catppuccin-latte" | "catppuccin_latte" => Ok(Self::Catppuccin),
            other => Err(format!("invalid color scheme: {other:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Palettes (identical hex values to the Python)
// ---------------------------------------------------------------------------

/// Matplotlib tab10-style — first six.
const MPL: [Color32; 6] = [
    Color32::from_rgb(0x1f, 0x77, 0xb4), // #1f77b4
    Color32::from_rgb(0xff, 0x7f, 0x0e), // #ff7f0e
    Color32::from_rgb(0x2c, 0xa0, 0x2c), // #2ca02c
    Color32::from_rgb(0xd6, 0x27, 0x28), // #d62728
    Color32::from_rgb(0x94, 0x67, 0xbd), // #9467bd
    Color32::from_rgb(0x8c, 0x56, 0x4b), // #8c564b
];

/// LibreOffice chart preset.
const LIBREOFFICE: [Color32; 6] = [
    Color32::from_rgb(0x00, 0x45, 0x86), // #004586
    Color32::from_rgb(0xff, 0x42, 0x0e), // #ff420e
    Color32::from_rgb(0xff, 0xd3, 0x20), // #ffd320
    Color32::from_rgb(0x57, 0x9d, 0x1c), // #579d1c
    Color32::from_rgb(0x7e, 0x00, 0x21), // #7e0021
    Color32::from_rgb(0x83, 0xca, 0xff), // #83caff
];

/// Catppuccin Latte accent palette.
const CATPPUCCIN: [Color32; 7] = [
    Color32::from_rgb(0x1e, 0x66, 0xf5), // #1e66f5 blue
    Color32::from_rgb(0xfe, 0x64, 0x0b), // #fe640b peach
    Color32::from_rgb(0x40, 0xa0, 0x2b), // #40a02b green
    Color32::from_rgb(0xe6, 0x45, 0x53), // #e64553 red
    Color32::from_rgb(0x88, 0x39, 0xef), // #8839ef mauve
    Color32::from_rgb(0x17, 0x92, 0x99), // #179299 teal
    Color32::from_rgb(0xea, 0x76, 0xcb), // #ea76cb pink
];

// ===========================================================================
// ColorRotator
// ===========================================================================

pub struct ColorRotator {
    palette: &'static [Color32],
    width: f32,
    index: usize,
}

impl ColorRotator {
    /// Construct a rotator from a named scheme. `width` is the default
    /// stroke width used by `next_stroke(None)`.
    pub fn new(scheme: ColorScheme) -> Self {
        Self {
            palette: scheme.palette(),
            width: 2.0,
            index: 0,
        }
    }

    /// Construct from a custom palette. Useful for tests or user-supplied
    /// colour lists; the built-in schemes go through `new`.
    pub fn from_palette(palette: &'static [Color32], width: f32) -> Self {
        Self {
            palette,
            width,
            index: 0,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Advance and return the next colour.
    pub fn next_color(&mut self) -> Color32 {
        let c = self.palette[self.index % self.palette.len()];
        self.index += 1;
        c
    }

    /// Advance and return a `Stroke` (colour + width) for direct use in
    /// drawing calls. `width` overrides the rotator's configured width for
    /// this call only.
    pub fn next_stroke(&mut self, width: Option<f32>) -> Stroke {
        Stroke::new(width.unwrap_or(self.width), self.next_color())
    }

    /// Next colour plus a hue-shifted companion (default shift is 15°,
    /// matching Python).
    pub fn get_color_pair(&mut self) -> (Color32, Color32) {
        self.get_color_pair_with(15.0)
    }

    /// Next colour plus a hue-shifted companion with an explicit shift.
    pub fn get_color_pair_with(&mut self, hue_shift_degrees: f32) -> (Color32, Color32) {
        let first = self.next_color();
        let second = shift_hue(first, hue_shift_degrees);
        (first, second)
    }

    /// Return to the start of the palette.
    pub fn reset(&mut self) {
        self.index = 0;
    }

    // ---- Diagnostics ---------------------------------------------------

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn palette_len(&self) -> usize {
        self.palette.len()
    }
}
