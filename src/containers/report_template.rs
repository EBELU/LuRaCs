//! Report data types consumed by `n42_report_writer`.
//!
//! Ported from the Python `luracs.containers.report_template` module. The
//! shape is inferred from how the report writer accesses these fields.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GroundMeasurement {
    pub detector_name: String,

    /// `None` when the ground has not been measured. The report writer skips
    /// the XML section entirely in that case, but the text report still
    /// includes the block (with `None` for `Start`).
    pub start_time: Option<DateTime<Utc>>,

    pub measurement_time: f64,
    pub accumulated_counts: f64,
    pub accumulated_dose: f64,
    pub mean_count_rate: f64,
    pub mean_dose_rate: f64,

    pub stop_timestamps: Vec<DateTime<Utc>>,
    pub count_rate_data: Vec<f64>,
    pub dose_rate_data: Vec<f64>,
    pub rate_axis: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeasurementReport {
    pub date: DateTime<Utc>,
    pub author_name: String,
    pub location: String,
    pub longitude: f64,
    pub latitude: f64,

    pub weather: String,
    pub wind_direction: String,
    pub wind_speed: f64,
    pub air_temperature: f64,

    pub remark: String,

    pub fg_measurement: GroundMeasurement,
    pub bg_measurement: GroundMeasurement,
}

impl Default for MeasurementReport {
    fn default() -> Self {
        Self {
            date: Utc::now(),
            author_name: String::new(),
            location: String::new(),
            longitude: 0.0,
            latitude: 0.0,
            weather: String::new(),
            wind_direction: String::new(),
            wind_speed: 0.0,
            air_temperature: 0.0,
            remark: String::new(),
            fg_measurement: GroundMeasurement::default(),
            bg_measurement: GroundMeasurement::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measurement_report_default_and_serde() {
        let mut rep = MeasurementReport {
            author_name: "Tester".to_string(),
            ..Default::default()
        };
        rep.fg_measurement.detector_name = "Det-1".to_string();
        rep.fg_measurement.start_time = Some(Utc::now());

        let json = serde_json::to_string(&rep).unwrap();
        let decoded: MeasurementReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.author_name, "Tester");
        assert_eq!(decoded.fg_measurement.detector_name, "Det-1");
        assert!(decoded.bg_measurement.start_time.is_none());
    }
}
