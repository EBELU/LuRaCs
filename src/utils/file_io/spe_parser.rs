//! Ortec `.Spe` text-format parser.
//!
//! Sections handled: `$DATE_MEA`, `$MEAS_TIM`, `$DATA`, `$ENER_FIT`, `$ROI`.
//! Unknown sections are skipped line by line.
//!
//! Notes on fidelity:
//! - `$ROI` bounds are channel indices (u32 pairs), not keV. Python parsed
//!   them into a local list and then silently dropped them; here they are
//!   stored and reachable via `roi_bounds()`.
//! - If `$ENER_FIT` is absent, `get_spectrum()` returns an uncalibrated
//!   Spectrum (channel axis). Python would have raised TypeError on
//!   `None[::-1]`; we prefer a usable result.
//! - Truncated `$DATA` sections (fewer values than `end - start + 1`) are
//!   accepted with the actual length used as `channels`, matching Python.

use std::path::Path;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use ndarray::Array1;
use thiserror::Error;

use crate::{
    containers::{Spectrum, SpectrumData},
    utils::{ParsedData, SpectrumParser},
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SpeParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing line after {section} in {file}")]
    MissingLine { section: &'static str, file: String },

    #[error("invalid {section} header in {file}: {reason}")]
    InvalidHeader {
        section: &'static str,
        file: String,
        reason: String,
    },

    #[error("invalid date {value:?} in {file} (expected MM/DD/YYYY HH:MM:SS)")]
    InvalidDate { file: String, value: String },
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct SpeParser {
    spectrum: Spectrum,
    roi_bounds: Vec<(u32, u32)>,
}

impl SpeParser {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SpeParseError> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Python opens with errors="ignore"; lossy decode is the closest match.
        let bytes = std::fs::read(path)?;
        let content = String::from_utf8_lossy(&bytes);
        Self::parse(&content, name)
    }

    pub fn parse(content: &str, name: String) -> Result<Self, SpeParseError> {
        let lines: Vec<&str> = content.lines().collect();

        let mut meas_date: Option<DateTime<Local>> = None;
        let mut live_time: Option<f64> = None;
        let mut real_time: Option<f64> = None;
        let mut calib: Option<Vec<f64>> = None;
        let mut data: Vec<u64> = Vec::new();
        let mut roi_bounds: Vec<(u32, u32)> = Vec::new();

        let mut i = 0usize;
        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("$DATE_MEA") {
                let raw = lines
                    .get(i + 1)
                    .copied()
                    .ok_or_else(|| SpeParseError::MissingLine {
                        section: "$DATE_MEA",
                        file: name.clone(),
                    })?;
                meas_date = Some(parse_spe_date(raw.trim(), &name)?);
                i += 2;
                continue;
            }

            if line.starts_with("$MEAS_TIM") {
                let raw = lines
                    .get(i + 1)
                    .copied()
                    .ok_or_else(|| SpeParseError::MissingLine {
                        section: "$MEAS_TIM",
                        file: name.clone(),
                    })?;
                let parts: Vec<&str> = raw.split_whitespace().collect();
                if parts.len() == 2 {
                    if let (Ok(l), Ok(r)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                        live_time = Some(l);
                        real_time = Some(r);
                    }
                }
                i += 2;
                continue;
            }

            if line.starts_with("$DATA") {
                let range =
                    lines
                        .get(i + 1)
                        .copied()
                        .ok_or_else(|| SpeParseError::MissingLine {
                            section: "$DATA",
                            file: name.clone(),
                        })?;
                let parts: Vec<&str> = range.split_whitespace().collect();
                if parts.len() != 2 {
                    return Err(SpeParseError::InvalidHeader {
                        section: "$DATA",
                        file: name.clone(),
                        reason: format!("expected 2 integers, got {:?}", range),
                    });
                }
                let start: i64 = parts[0].parse().map_err(|_| SpeParseError::InvalidHeader {
                    section: "$DATA",
                    file: name.clone(),
                    reason: format!("bad start: {:?}", parts[0]),
                })?;
                let end: i64 = parts[1].parse().map_err(|_| SpeParseError::InvalidHeader {
                    section: "$DATA",
                    file: name.clone(),
                    reason: format!("bad end: {:?}", parts[1]),
                })?;
                let count = (end - start + 1).max(0) as usize;

                i += 2;
                while data.len() < count && i < lines.len() {
                    for token in lines[i].split_whitespace() {
                        if let Ok(v) = token.parse::<u64>() {
                            data.push(v);
                        }
                    }
                    i += 1;
                }
                continue;
            }

            if line.starts_with("$ENER_FIT") {
                let raw = lines
                    .get(i + 1)
                    .copied()
                    .ok_or_else(|| SpeParseError::MissingLine {
                        section: "$ENER_FIT",
                        file: name.clone(),
                    })?;
                let mut coeffs: Vec<f64> = Vec::new();
                for token in raw.split_whitespace() {
                    let v: f64 = token.parse().map_err(|_| SpeParseError::InvalidHeader {
                        section: "$ENER_FIT",
                        file: name.clone(),
                        reason: format!("bad coefficient: {:?}", token),
                    })?;
                    coeffs.push(v);
                }
                // Ortec writes [a0, a1, …] (constant first); reverse for
                // the polyval convention [a_n, …, a0] used by Spectrum.
                coeffs.reverse();
                calib = Some(coeffs);
                i += 2;
                continue;
            }

            if line.starts_with("$ROI") {
                let count_raw =
                    lines
                        .get(i + 1)
                        .copied()
                        .ok_or_else(|| SpeParseError::MissingLine {
                            section: "$ROI",
                            file: name.clone(),
                        })?;
                let n: usize =
                    count_raw
                        .trim()
                        .parse()
                        .map_err(|_| SpeParseError::InvalidHeader {
                            section: "$ROI",
                            file: name.clone(),
                            reason: format!("bad ROI count: {:?}", count_raw),
                        })?;

                for j in 0..n {
                    let roi_line = lines.get(i + 2 + j).copied().ok_or_else(|| {
                        SpeParseError::MissingLine {
                            section: "$ROI",
                            file: name.clone(),
                        }
                    })?;
                    let parts: Vec<&str> = roi_line.split_whitespace().collect();
                    if parts.len() < 2 {
                        return Err(SpeParseError::InvalidHeader {
                            section: "$ROI",
                            file: name.clone(),
                            reason: format!("expected 2 ints, got {:?}", roi_line),
                        });
                    }
                    let lo: u32 = parts[0].parse().map_err(|_| SpeParseError::InvalidHeader {
                        section: "$ROI",
                        file: name.clone(),
                        reason: format!("bad lower bound: {:?}", parts[0]),
                    })?;
                    let hi: u32 = parts[1].parse().map_err(|_| SpeParseError::InvalidHeader {
                        section: "$ROI",
                        file: name.clone(),
                        reason: format!("bad upper bound: {:?}", parts[1]),
                    })?;
                    roi_bounds.push((lo, hi));
                }
                i += 2 + n;
                continue;
            }

            i += 1;
        }

        let channels = data.len();
        let total_counts: u64 = data.iter().sum();
        let y_axis: Array1<f64> = Array1::from_vec(data.iter().map(|&v| v as f64).collect());

        let foreground = SpectrumData {
            y_axis,
            channels,
            total_counts,
            // Python would store None here; we use 0.0, which the cps path
            // in Spectrum treats identically ("unavailable").
            live_time: live_time.unwrap_or(0.0),
            real_time,
            avg_dose_rate: None,
            avg_cps: None,
            // SpectrumData currently stores UTC timestamps.
            start_date: meas_date.map(|dt| dt.with_timezone(&Utc)),
            end_date: None,
            spectrum_name: Some(name.clone()),
            instrument: None,
        };

        let mut spectrum = Spectrum::new(channels, name);
        spectrum.set_foreground(foreground, None);
        if let Some(cal) = calib {
            spectrum.apply_calibration(cal);
        }

        Ok(Self {
            spectrum,
            roi_bounds,
        })
    }

    pub fn into_data(self) -> ParsedData {
        let spec = self.spectrum;
        ParsedData {
            name: spec.name.clone(),
            foreground: spec.foreground.clone(),
            background: spec.background.clone(),
            calibration: spec.calibration_coefficients.clone(),
            peaks: spec.rois.values().cloned().collect(),
            instrument: None,
            remark: Some(spec.remark.clone()),
            instrument_model: spec.instrument_model.clone(),
            instrument_id: spec.instrument_id.clone(),
            peak_data: Vec::new(),
            instrument_class_code: None,
        }
    }

    /// Raw channel-index bounds from `$ROI`. Empty if the section is absent.
    pub fn roi_bounds(&self) -> &[(u32, u32)] {
        &self.roi_bounds
    }
}

impl SpectrumParser for SpeParser {
    type Error = SpeParseError;

    fn get_spectrum(&self) -> Result<Spectrum, Self::Error> {
        Ok(self.spectrum.clone())
    }

    // Spe ROIs are bare channel bounds, not full `Roi` objects — expose
    // them via `roi_bounds()` and keep the trait default (empty) here.
}

fn parse_spe_date(s: &str, file: &str) -> Result<DateTime<Local>, SpeParseError> {
    let naive = NaiveDateTime::parse_from_str(s, "%m/%d/%Y %H:%M:%S").map_err(|_| {
        SpeParseError::InvalidDate {
            file: file.to_string(),
            value: s.to_string(),
        }
    })?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .ok_or_else(|| SpeParseError::InvalidDate {
            file: file.to_string(),
            value: s.to_string(),
        })
}
