//! Canberra `.TKA` text-format parser.
//!
//! Layout: line 1 = live time (integer), line 2 = real time (integer),
//! each subsequent line = one channel count.
//!
//! Bug fix vs. Python: `get_spectrum()` in the original never called
//! `set_foreground`, so the returned Spectrum was always empty. This port
//! returns the fully-populated spectrum.

use std::path::Path;

use ndarray::Array1;
use thiserror::Error;

use crate::{
    containers::{Spectrum, SpectrumData},
    utils::{ParsedData, SpectrumParser},
};

#[derive(Debug, Error)]
pub enum TkaParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing {what} in {file}")]
    MissingLine { file: String, what: &'static str },

    #[error("invalid number on line {line} of {file}: {value:?}")]
    InvalidNumber {
        file: String,
        line: usize,
        value: String,
    },
}

pub struct TkaParser {
    spectrum: Spectrum,
}

impl TkaParser {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TkaParseError> {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let bytes = std::fs::read(path)?;
        let content = String::from_utf8_lossy(&bytes);
        Self::parse(&content, name)
    }

    pub fn parse(content: &str, name: String) -> Result<Self, TkaParseError> {
        let mut lines = content.lines();

        let live_raw = lines.next().ok_or_else(|| TkaParseError::MissingLine {
            file: name.clone(),
            what: "live time (line 1)",
        })?;
        let live_time: f64 = live_raw
            .trim()
            .parse()
            .map_err(|_| TkaParseError::InvalidNumber {
                file: name.clone(),
                line: 1,
                value: live_raw.to_string(),
            })?;

        let real_raw = lines.next().ok_or_else(|| TkaParseError::MissingLine {
            file: name.clone(),
            what: "real time (line 2)",
        })?;
        let real_time: f64 = real_raw
            .trim()
            .parse()
            .map_err(|_| TkaParseError::InvalidNumber {
                file: name.clone(),
                line: 2,
                value: real_raw.to_string(),
            })?;

        let mut data: Vec<u64> = Vec::new();
        for (idx, line) in lines.enumerate() {
            let v: u64 = line
                .trim()
                .parse()
                .map_err(|_| TkaParseError::InvalidNumber {
                    file: name.clone(),
                    line: idx + 3,
                    value: line.to_string(),
                })?;
            data.push(v);
        }

        let channels = data.len();
        let total_counts: u64 = data.iter().sum();
        let y_axis: Array1<f64> = Array1::from_vec(data.iter().map(|&v| v as f64).collect());

        let foreground = SpectrumData {
            y_axis,
            channels,
            total_counts,
            live_time,
            real_time: Some(real_time),
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: Some(name.clone()),
            instrument: None,
        };

        let mut spectrum = Spectrum::new(channels, name);
        spectrum.set_foreground(foreground, None);

        Ok(Self { spectrum })
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
}

impl SpectrumParser for TkaParser {
    type Error = TkaParseError;

    fn get_spectrum(&self) -> Result<Spectrum, Self::Error> {
        Ok(self.spectrum.clone())
    }
}
