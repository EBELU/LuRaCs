use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use eframe::egui::Color32;
use ndarray::Array1;

use super::instrument::UniqueInstrument;
use super::roi::Roi;
use crate::utils::numerics::{interp, polyval};

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
        if spectrum.instrument.is_none()
            && let Some(inst) = &self.instrument
        {
            spectrum.instrument = Some(Arc::clone(inst));
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
        self.foreground.as_ref()?;

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

    pub const fn get_instrument(&self) -> Option<&Arc<UniqueInstrument>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::instrument::GenericInstrument;
    use crate::containers::{BkgType, FitType};

    fn make_data(y: Vec<f64>, live_time: f64) -> SpectrumData {
        let channels = y.len();
        let total_counts = y.iter().sum::<f64>() as u64;
        SpectrumData {
            y_axis: Array1::from_vec(y),
            channels,
            total_counts,
            live_time,
            real_time: Some(live_time),
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: None,
            instrument: None,
        }
    }

    #[test]
    fn test_spectrum_lifecycle_calibration_and_data_accessors() {
        let mut spec = Spectrum::new(4, "TestSpec");
        assert_eq!(spec.channels, 4);
        assert_eq!(spec.x_axis.as_slice().unwrap(), &[0.0, 1.0, 2.0, 3.0]);

        // No foreground -> zeros
        assert_eq!(spec.get_foreground(false, false).unwrap().as_slice().unwrap(), &[0.0; 4]);
        assert!(spec.get_background(false, false).is_none());
        assert!(spec.get_bkg_sub(false).is_none());

        // Instrument management
        let inst = UniqueInstrument::from_template("Det-1", &GenericInstrument::default());
        spec.set_instrument(inst);
        assert!(spec.get_instrument().is_some());

        // Set foreground (adopts spectrum's instrument)
        spec.set_foreground(make_data(vec![10.0, 20.0, 0.0, 40.0], 2.0), Some(Color32::RED));
        assert_eq!(spec.color_foreground, Color32::RED);
        assert!(spec.foreground.as_ref().unwrap().instrument.is_some());

        // Bkg-sub falls back to foreground cps when no background
        assert_eq!(spec.get_bkg_sub(false).unwrap().as_slice().unwrap(), &[5.0, 10.0, 0.0, 20.0]);

        // Log foreground
        let fg_log = spec.get_foreground(true, false).unwrap();
        assert!((fg_log[0] - 1.0).abs() < 1e-12);
        assert!(fg_log[2].is_nan());

        // Set background
        spec.set_background(make_data(vec![2.0, 4.0, 0.0, 10.0], 2.0), Some(Color32::BLUE));
        assert_eq!(spec.color_background, Color32::BLUE);
        assert_eq!(spec.get_background(false, true).unwrap().as_slice().unwrap(), &[1.0, 2.0, 0.0, 5.0]);
        let bg_log = spec.get_background(true, false).unwrap();
        assert!(bg_log[2].is_nan());

        // Bkg-sub with background (linear & log)
        assert_eq!(spec.get_bkg_sub(false).unwrap().as_slice().unwrap(), &[4.0, 8.0, 0.0, 15.0]);
        let sub_log = spec.get_bkg_sub(true).unwrap();
        assert!(sub_log[2].is_nan());

        // Zero live_time -> None when cps == true
        spec.set_foreground(make_data(vec![1.0; 4], 0.0), None);
        assert!(spec.get_foreground(false, true).is_none());
        spec.set_background(make_data(vec![1.0; 4], 0.0), None);
        assert!(spec.get_background(false, true).is_none());

        // Calibration & get_channel_from_energy
        assert_eq!(spec.get_channel_from_energy(2.4), 2);
        spec.apply_calibration(vec![1.0, 0.0]); // identity: [1, 0] -> calibrated = false
        assert!(!spec.calibrated);
        assert_eq!(spec.get_channel_from_energy(2.4), 2);

        spec.apply_calibration(vec![10.0, 0.0]); // E = 10 * ch -> calibrated = true
        assert!(spec.calibrated);
        assert_eq!(spec.get_channel_from_energy(20.0), 2);

        // ROI management
        let roi = Roi {
            tag: "r1".to_string(),
            alias: "Peak".to_string(),
            spectrum: "TestSpec".to_string(),
            roi_bound: (1.0, 3.0),
            region_bound: (0.0, 4.0),
            fit_type: FitType::None,
            bkg_type: BkgType::None,
            bkg_est_channels: 2,
            fit: None,
            roi_counts: 0.0,
            live_time: 1.0,
            emission: None,
            meta: serde_json::Map::new(),
            merge: false,
            movable: true,
            poisson_weights: false,
            owner_spectrum: None,
        };
        spec.set_roi(roi);
        assert!(spec.rois.contains_key("r1"));
        spec.remove_roi("r1");
        assert!(!spec.rois.contains_key("r1"));

        spec.clear_instrument();
        assert!(spec.get_instrument().is_none());
    }
}


