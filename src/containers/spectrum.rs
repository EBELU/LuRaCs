use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use eframe::egui::Color32;
use ndarray::Array1;

use super::instrument::UniqueInstrument;
use super::roi::Roi;

// ---------------------------------------------------------------------------
// SpectrumData
// ---------------------------------------------------------------------------

/// Raw data for one spectrum (foreground or background).
///
/// Deviation from Python: `instrument` is added as a real field. In the
/// Python original, `set_foreground` tries to write `spectrum.instrument = ...`
/// on a `SpectrumData` that has no such field, which would raise
/// `AttributeError` at runtime. Adding it here fixes the bug.
#[derive(Debug, Clone)]
pub struct SpectrumData {
    pub y_axis: Array1<f64>,
    pub channels: usize,
    pub total_counts: u64,
    pub live_time: f64,
    pub real_time: Option<f64>,
    pub avg_dose_rate: Option<f64>,
    pub avg_cps: Option<f64>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub spectrum_name: Option<String>,
    /// Shared instrument, so multiple spectra can point at the same one.
    pub instrument: Option<Arc<UniqueInstrument>>,
}

// ---------------------------------------------------------------------------
// Spectrum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Spectrum {
    // --- Metadata ---
    pub name: String,
    pub channels: usize,
    pub remark: String,

    pub connection: Option<String>,
    pub instrument_id: Option<String>,
    pub instrument_model: Option<String>,

    // --- Axis values ---
    pub foreground: Option<SpectrumData>,
    pub background: Option<SpectrumData>,
    pub x_axis: Array1<f64>,

    // --- ROI storage ---
    /// `tag -> Roi`. Renamed from Python's `ROIs` for Rust convention.
    pub rois: HashMap<String, Roi>,

    // --- Plot colours (egui rather than QColor) ---
    pub color_foreground: Color32,
    pub color_background: Color32,

    // --- Calibration (must be keV) ---
    pub calibration_coefficients: Option<Vec<f64>>,
    pub calibrated: bool,

    // --- State ---
    pub fit_rois: bool,
    pub show_in_plot: bool,

    // --- Instrument ---
    /// Shared rather than owned: multiple spectra can point at the same
    /// instrument, and `UniqueInstrument` is expensive to clone.
    pub instrument: Option<Arc<UniqueInstrument>>,
}

impl Spectrum {
    pub fn new(channels: usize, name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            channels,
            remark: String::new(),
            connection: None,
            instrument_id: None,
            instrument_model: None,
            foreground: None,
            background: None,
            x_axis: Array1::from_iter((0..channels).map(|i| i as f64)),
            rois: HashMap::new(),
            color_foreground: Color32::WHITE,
            color_background: Color32::WHITE,
            calibration_coefficients: None,
            calibrated: false,
            fit_rois: true,
            show_in_plot: true,
            instrument: None,
        }
    }

    // -----------------------------------------------------------------------
    // Foreground / background
    // -----------------------------------------------------------------------

    pub fn set_foreground(&mut self, mut spectrum: SpectrumData, color: Option<Color32>) {
        assert_eq!(
            spectrum.channels, self.channels,
            "Number of channels in foreground does not match: {} vs {}",
            spectrum.channels, self.channels
        );

        // Python's intent (buggy as written): if the incoming data has no
        // instrument, adopt the spectrum's instrument. We do that here.
        if spectrum.instrument.is_none() {
            if let Some(inst) = &self.instrument {
                spectrum.instrument = Some(Arc::clone(inst));
            }
        }

        // Python assigned `color_background` here; that is a bug — it should
        // be the foreground colour. Fixed.
        if let Some(c) = color {
            self.color_foreground = c;
        }

        self.foreground = Some(spectrum);
    }

    pub fn set_background(&mut self, spectrum: SpectrumData, color: Option<Color32>) {
        assert_eq!(
            spectrum.channels, self.channels,
            "Number of channels in foreground does not match the number of \
             channels in attempted background! Foreground: {} channels, \
             Background: {} channels",
            self.channels, spectrum.channels
        );

        if let Some(c) = color {
            self.color_background = c;
        }

        self.background = Some(spectrum);
    }

    // -----------------------------------------------------------------------
    // Calibration
    // -----------------------------------------------------------------------

    /// `coefficients` are `[a_n, a_{n-1}, …, a_0]` (highest degree first),
    /// matching NumPy's `polyval` convention.
    pub fn apply_calibration(&mut self, coefficients: Vec<f64>) {
        self.x_axis =
            Array1::from_iter((0..self.channels).map(|i| polyval(&coefficients, i as f64)));

        let len = coefficients.len();
        self.calibrated =
            !(len >= 2 && coefficients[len - 1] == 0.0 && coefficients[len - 2] == 1.0);
        self.calibration_coefficients = Some(coefficients);
    }

    // -----------------------------------------------------------------------
    // Data accessors
    // -----------------------------------------------------------------------

    /// Returns `None` only when `cps == true` and live time is not positive.
    /// If no foreground is loaded, returns an array of zeros (matching Python).
    pub fn get_foreground(&self, log: bool, cps: bool) -> Option<Array1<f64>> {
        let Some(fg) = self.foreground.as_ref() else {
            return Some(Array1::zeros(self.channels));
        };

        let y_data = if cps {
            if fg.live_time > 0.0 {
                fg.y_axis.mapv(|v| v / fg.live_time)
            } else {
                return None;
            }
        } else {
            fg.y_axis.clone()
        };

        if log {
            Some(y_data.mapv(|v| if v > 0.0 { v.log10() } else { f64::NAN }))
        } else {
            Some(y_data)
        }
    }

    /// Returns `None` if no background is loaded, or if `cps == true` and
    /// live time is not positive.
    pub fn get_background(&self, log: bool, cps: bool) -> Option<Array1<f64>> {
        let bg = self.background.as_ref()?;

        let bkg_y_data = if cps {
            if bg.live_time > 0.0 {
                bg.y_axis.mapv(|v| v / bg.live_time)
            } else {
                return None;
            }
        } else {
            bg.y_axis.clone()
        };

        if log {
            Some(bkg_y_data.mapv(|v| if v > 0.0 { v.log10() } else { f64::NAN }))
        } else {
            Some(bkg_y_data)
        }
    }

    /// Background-subtracted spectrum, in cps. Falls back to foreground cps
    /// when there is no background.
    ///
    /// Note: the Python original ignores `log` in the fallback branch (returns
    /// the foreground *without* log). That behaviour is preserved here for
    /// compatibility; if you'd rather have the log applied uniformly, replace
    /// the early return with `.map(|a| log_if(a, log))`.
    pub fn get_bkg_sub(&self, log: bool) -> Option<Array1<f64>> {
        if self.foreground.is_none() {
            return None;
        }

        if self.background.is_none() {
            return self.get_foreground(false, true);
        }

        let fg = self.get_foreground(false, true)?;
        let bg = self.get_background(false, true)?;
        let data = fg - bg;

        if log {
            Some(data.mapv(|v| if v > 0.0 { v.log10() } else { f64::NAN }))
        } else {
            Some(data)
        }
    }

    // -----------------------------------------------------------------------
    // ROI management
    // -----------------------------------------------------------------------

    pub fn set_roi(&mut self, roi: Roi) {
        self.rois.insert(roi.tag.clone(), roi);
    }

    pub fn remove_roi(&mut self, roi_tag: &str) {
        self.rois.remove(roi_tag);
    }

    // -----------------------------------------------------------------------
    // Instrument management
    // -----------------------------------------------------------------------

    pub fn set_instrument(&mut self, instrument: UniqueInstrument) {
        // No runtime type check needed: the type system enforces it.
        self.instrument = Some(Arc::new(instrument));
    }

    pub fn get_instrument(&self) -> Option<&Arc<UniqueInstrument>> {
        self.instrument.as_ref()
    }

    pub fn clear_instrument(&mut self) {
        self.instrument = None;
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn get_channel_from_energy(&self, e_kev: f64) -> usize {
        let Some(coeffs) = self.calibration_coefficients.as_ref() else {
            return e_kev.round() as usize;
        };

        if !self.calibrated {
            return e_kev.round() as usize;
        }

        let channels: Vec<f64> = (0..self.channels).map(|i| i as f64).collect();
        let energies: Vec<f64> = channels.iter().map(|&ch| polyval(coeffs, ch)).collect();

        interp(e_kev, &energies, &channels) as usize
    }
}

// ---------------------------------------------------------------------------
// Numeric helpers (NumPy equivalents)
// ---------------------------------------------------------------------------

/// Horner evaluation of `[a_n, a_{n-1}, …, a_0]` at `x`.
fn polyval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().fold(0.0, |acc, &c| acc * x + c)
}

/// Linear interpolation, matching `np.interp`. Assumes `xp` is increasing.
fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    if xp.is_empty() {
        return f64::NAN;
    }
    if x <= xp[0] {
        return fp[0];
    }
    if x >= xp[xp.len() - 1] {
        return fp[fp.len() - 1];
    }
    for i in 0..xp.len() - 1 {
        if x >= xp[i] && x <= xp[i + 1] {
            let t = (x - xp[i]) / (xp[i + 1] - xp[i]);
            return fp[i] + t * (fp[i + 1] - fp[i]);
        }
    }
    f64::NAN
}
