//! Manager for spectrogram-space ROIs and the map/plot data derived from
//! spectrogram updates.
//!
//! Port of `SpectrogramManager` from the Python GUI. Follows the same
//! event-queue pattern as the other managers.
//!
//! ## Deviations from Python
//!
//! - `energy_axes_buffer` must be populated by the caller via
//!   `set_energy_axis`; `receive_buffer` returns early if the axis is
//!   missing (Python would `KeyError`).
//! - Row-wise sums are computed directly from `buffer.spectrogram` instead
//!   of `np.vstack`-ing the whole matrix — same result, no N×C allocation.
//! - `MappingBuffer` is a plain struct with typed fields, not a mixed dict.
//!   GPS stays `Vec<Option<GpsData>>` to preserve full per-point data.
//! - `roi_counts` is shared between the two emitted events via `Arc`.
//! - `movable` is captured at `add_roi` time from `roi_edit_mode`,
//!   matching Python. For interactive dragging in the egui view, prefer
//!   reading `manager.roi_edit_mode()` per frame over the per-ROI flag.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use ndarray::{Array1, s};
use thiserror::Error;

use crate::clients::GpsData;
use crate::containers::Emission;
use crate::containers::Roi;
use crate::spectogram::{Spectrogram, WrappedSpectrogramData};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SpectrogramManagerError {
    #[error("spectrogram ROI not found: {0:?}")]
    RoiNotFound(String),
    #[error("no energy axis registered for logger {0:?}")]
    MissingEnergyAxis(String),
    #[error(
        "spectrogram row length {row_len} does not match energy axis length {axis_len} \
         for logger {logger:?}"
    )]
    AxisLengthMismatch {
        logger: String,
        row_len: usize,
        axis_len: usize,
    },
}

pub type Result<T> = std::result::Result<T, SpectrogramManagerError>;

// ---------------------------------------------------------------------------
// SpectrogramROI (energy-space, distinct from Roi)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SpectrogramRoi {
    pub tag: String,
    pub alias: String,
    /// Energy bounds in keV.
    pub e_region: (f64, f64),
    /// Captured from `roi_edit_mode` at add time (Python parity).
    /// View code should consult `SpectrogramManager::roi_edit_mode` for the
    /// live toggle rather than this field.
    pub movable: bool,
    pub emission: Option<Emission>,
}

/// Parameters for `add_roi`. Replaces Python's `**kwargs`.
#[derive(Debug, Clone, Default)]
pub struct NewSpectrogramRoi {
    /// `(lower, upper)` energy bounds in keV.
    pub e_region: (f64, f64),
    pub alias: Option<String>,
    pub emission: Option<Emission>,
}

// ---------------------------------------------------------------------------
// Mapping buffer (replaces the mixed dict in Python)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MappingBuffer {
    pub timestamps: Vec<f64>,
    pub gps: Vec<Option<GpsData>>,
    pub count_rate: Vec<f64>,
    pub dose_rate: Vec<f64>,
    /// Per-ROI count rate history, keyed by `SpectrogramRoi::tag`.
    /// Shared via `Arc` with the paired `RoiCountsUpdated` event so the
    /// same arrays aren't duplicated.
    pub roi_counts: Arc<HashMap<String, Vec<f64>>>,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SpectrogramManagerEvent {
    RoiAdded {
        roi: SpectrogramRoi,
    },
    RoiRemoved {
        roi: SpectrogramRoi,
    },
    RoiCountsUpdated {
        logger_name: String,
        counts: Arc<HashMap<String, Vec<f64>>>,
    },
    MapBufferUpdated {
        logger_name: String,
        buffer: MappingBuffer,
    },
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct SpectrogramManager {
    /// Logger names this manager has been told about. External callers use
    /// this to enumerate; this manager doesn't own `Spectrogram` objects
    /// (their updates arrive as `receive_buffer(...)`).
    pub spectrogram_registry: HashSet<String>,

    pub roi_registry: HashMap<String, SpectrogramRoi>,
    pub energy_axes_buffer: HashMap<String, Array1<f64>>,
    pub roi_counter: usize,
    pub roi_edit_mode: bool,

    events: VecDeque<SpectrogramManagerEvent>,
}

impl Default for SpectrogramManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrogramManager {
    pub fn new() -> Self {
        Self {
            spectrogram_registry: HashSet::new(),
            roi_registry: HashMap::new(),
            energy_axes_buffer: HashMap::new(),
            roi_counter: 0,
            roi_edit_mode: false,
            events: VecDeque::new(),
        }
    }

    // ---- Event queue ----------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<SpectrogramManagerEvent> {
        self.events.drain(..).collect()
    }

    fn emit(&mut self, ev: SpectrogramManagerEvent) {
        self.events.push_back(ev);
    }

    // ---- Registration / config ------------------------------------------

    /// Register a logger by name. Does not create the `Spectrogram`; the
    /// caller owns it. Purely bookkeeping so external code can enumerate.
    pub fn register_logger(&mut self, name: impl Into<String>) {
        self.spectrogram_registry.insert(name.into());
    }

    pub fn unregister_logger(&mut self, name: &str) {
        self.spectrogram_registry.remove(name);
        self.energy_axes_buffer.remove(name);
    }

    pub fn set_roi_edit_mode(&mut self, state: bool) {
        self.roi_edit_mode = state;
    }

    pub fn roi_edit_mode(&self) -> bool {
        self.roi_edit_mode
    }

    /// Cache the calibrated energy axis for a logger. Required before
    /// `receive_buffer` can compute ROI counts for that logger.
    ///
    /// Typically called from the handler for `SpectrogramEvent::Started`,
    /// using `x_axis = polyval(calibration, arange(spect_channels))`.
    pub fn set_energy_axis(&mut self, logger_name: impl Into<String>, axis: Array1<f64>) {
        self.energy_axes_buffer.insert(logger_name.into(), axis);
    }

    pub fn remove_energy_axis(&mut self, logger_name: &str) {
        self.energy_axes_buffer.remove(logger_name);
    }

    // ---- ROI CRUD -------------------------------------------------------

    /// Create a new energy-space ROI. Returns the generated tag.
    pub fn add_roi(&mut self, spec: NewSpectrogramRoi) -> String {
        let tag = format!("SG_ROI_{}", self.roi_counter);
        self.roi_counter += 1;

        let alias = spec.alias.unwrap_or_else(|| tag.clone());
        let roi = SpectrogramRoi {
            tag: tag.clone(),
            alias,
            e_region: spec.e_region,
            movable: self.roi_edit_mode,
            emission: spec.emission,
        };
        self.roi_registry.insert(tag.clone(), roi.clone());
        self.emit(SpectrogramManagerEvent::RoiAdded { roi });
        tag
    }

    /// Update the energy bounds of an existing ROI. Returns `false` if the
    /// tag was unknown.
    pub fn update_roi_bounds(&mut self, tag: &str, e_region: (f64, f64)) -> bool {
        if let Some(roi) = self.roi_registry.get_mut(tag) {
            roi.e_region = e_region;
            true
        } else {
            false
        }
    }

    /// Remove a single ROI by tag. No-op if unknown.
    pub fn remove_roi(&mut self, tag: &str) {
        if let Some(roi) = self.roi_registry.remove(tag) {
            self.emit(SpectrogramManagerEvent::RoiRemoved { roi });
        }
    }

    /// Remove every ROI, emitting one `RoiRemoved` per tag.
    pub fn clear_rois(&mut self) {
        let tags: Vec<String> = self.roi_registry.keys().cloned().collect();
        for tag in tags {
            self.remove_roi(&tag);
        }
    }

    /// Populate the ROI registry from parsed N42 ROIs. Each input's
    /// `roi_bound` becomes the new ROI's `e_region`; `alias` and
    /// `emission` are carried over. `Roi::tag` is discarded (matching
    /// Python, which regenerates tags on import).
    pub fn load_rois(&mut self, rois: &[Roi]) {
        for peak in rois {
            self.add_roi(NewSpectrogramRoi {
                e_region: peak.roi_bound,
                alias: Some(peak.alias.clone()),
                emission: peak.emission.clone(),
            });
        }
    }

    pub fn get_roi_alias_from_tag(&self, tag: &str) -> Option<&str> {
        self.roi_registry.get(tag).map(|r| r.alias.as_str())
    }

    pub fn get_roi(&self, tag: &str) -> Option<&SpectrogramRoi> {
        self.roi_registry.get(tag)
    }

    // ---- Data extraction ------------------------------------------------

    /// Consume one spectrogram snapshot:
    /// - compute per-ROI count-rate histories from the spectra window,
    /// - build the map buffer with GPS / rate / ROI series.
    ///
    /// Emits `RoiCountsUpdated` (if any ROIs exist) followed by
    /// `MapBufferUpdated`. Returns early on missing energy axis or
    /// shape mismatch — Python would `KeyError` in the first case.
    pub fn receive_buffer(
        &mut self,
        logger_name: &str,
        buffer: &WrappedSpectrogramData,
    ) -> Result<()> {
        if buffer.spectrogram.is_empty() {
            return Ok(());
        }

        let eaxis = self
            .energy_axes_buffer
            .get(logger_name)
            .ok_or_else(|| SpectrogramManagerError::MissingEnergyAxis(logger_name.to_string()))?
            .clone();

        // Validate shapes once.
        let expected_cols = eaxis.len();
        for row in &buffer.spectrogram {
            if row.len() != expected_cols {
                return Err(SpectrogramManagerError::AxisLengthMismatch {
                    logger: logger_name.to_string(),
                    row_len: row.len(),
                    axis_len: expected_cols,
                });
            }
        }
        let n_rows = buffer.spectrogram.len();

        // ---- Per-ROI counts (only if ROIs exist) ------------------------
        let counts: Arc<HashMap<String, Vec<f64>>> = if !self.roi_registry.is_empty() {
            let mut results: HashMap<String, Vec<f64>> =
                HashMap::with_capacity(self.roi_registry.len());

            for roi in self.roi_registry.values() {
                let (emin, emax) = roi.e_region;
                let i0 = searchsorted_left(&eaxis, emin);
                let i1 = searchsorted_left(&eaxis, emax);

                let mut series = Vec::with_capacity(n_rows);
                if i0 >= i1 {
                    // Empty energy window — Python gives zeros, not an error.
                    series.resize(n_rows, 0.0);
                } else {
                    for row in &buffer.spectrogram {
                        let s_row: f64 = row.slice(s![i0..i1]).sum();
                        series.push(s_row / buffer.save_interval);
                    }
                }
                results.insert(roi.tag.clone(), series);
            }
            let shared = Arc::new(results);
            self.emit(SpectrogramManagerEvent::RoiCountsUpdated {
                logger_name: logger_name.to_string(),
                counts: shared.clone(),
            });
            shared
        } else {
            Arc::new(HashMap::new())
        };

        // ---- Map buffer ------------------------------------------------
        let mapping_buffer = MappingBuffer {
            timestamps: buffer.timestamp_deque.clone(),
            gps: buffer.gps_queue.clone(),
            count_rate: buffer.count_rate_queue.clone(),
            dose_rate: buffer.dose_rate_queue.clone(),
            roi_counts: counts,
        };
        self.emit(SpectrogramManagerEvent::MapBufferUpdated {
            logger_name: logger_name.to_string(),
            buffer: mapping_buffer,
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Left-biased binary search: the first index where `arr[idx] >= v`.
/// Matches `numpy.searchsorted(arr, v)` (default `side='left'`).
fn searchsorted_left(arr: &Array1<f64>, v: f64) -> usize {
    let mut lo = 0usize;
    let mut hi = arr.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if arr[mid] < v {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

// ---------------------------------------------------------------------------
// Convenience: build an energy axis from a Spectrogram's calibration
// ---------------------------------------------------------------------------

/// Build the energy axis that `set_energy_axis` expects from a
/// `Spectrogram`'s calibration coefficients and channel count.
///
/// Uses the same `[a_n, …, a_0]` Horner convention as `Spectrum::apply_calibration`
/// and `calibrate_x_axis`.
pub fn energy_axis_from_calibration(coeffs: &[f64], channels: usize) -> Array1<f64> {
    if coeffs.is_empty() {
        return Array1::from_iter((0..channels).map(|i| i as f64));
    }
    Array1::from_iter(
        (0..channels).map(|i| coeffs.iter().fold(0.0, |acc, &c| acc * (i as f64) + c)),
    )
}

/// Convenience: derive the axis directly from a spectrogram's header.
pub fn energy_axis_for(spectrogram: &Spectrogram) -> Array1<f64> {
    energy_axis_from_calibration(
        spectrogram.calibration_coeff(),
        spectrogram.spect_channels(),
    )
}
