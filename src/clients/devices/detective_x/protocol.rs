//! Detective X JSON DTOs, endpoints, and ISO-8601 duration parsing.
//!
//! Ported from `luracs/clients/detective_x_client/detective_x.py`.

use serde::Deserialize;

use crate::clients::driver::DeviceError;

/// Default HTTP port for Detective X REST API.
pub const DEFAULT_PORT: u16 = 80;

/// REST API endpoint paths.
pub mod endpoints {
    pub const INSTRUMENT_STATUS: &str = "/remote/v1/instrumentStatus";
    pub const SPECTRUM: &str = "/remote/v1/measurement/spectrum";
    pub const DOSE_COUNT_RATE: &str = "/remote/v1/measurement/doseCountRate";
    pub const CALIBRATION: &str = "/remote/v1/calibration";
    pub const ACQ_START: &str = "/remote/v1/remote/acq/start";
    pub const ACQ_STOP: &str = "/remote/v1/remote/acq/stop";
    pub const ACQ_CLEAR: &str = "/remote/v1/remote/acq/clear";
}

// ---------------------------------------------------------------------------
// JSON DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstrumentStatusResponse {
    pub instrument_status: InstrumentStatusDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InstrumentStatusDto {
    pub battery: f64,
    pub crystal_temperature: f64,
    #[serde(default)]
    pub coordinates: Vec<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SpectrumResponse {
    pub measurement: SpectrumMeasurementDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SpectrumMeasurementDto {
    pub spectrum: SpectrumDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SpectrumDto {
    pub channel_data: Vec<f64>,
    pub live_time: String,
    pub real_time: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DoseCountRateResponse {
    pub measurement: DoseCountMeasurementDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DoseCountMeasurementDto {
    pub count_dose_data: Vec<CountDoseItemDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CountDoseItemDto {
    pub detector_type: String,
    pub remark: String,
    pub dose_rate: Vec<DoseRateEntryDto>,
    pub count_rate: CountRateEntryDto,
}

#[derive(Debug, Deserialize)]
pub struct DoseRateEntryDto {
    pub unit: String,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct CountRateEntryDto {
    pub unit: String,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CalibrationResponse {
    pub calibration_type: CalibrationTypeDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CalibrationTypeDto {
    pub equation_model: EquationModelDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct EquationModelDto {
    pub model: String,
    pub coefficients: Vec<f64>,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Parse ISO 8601 duration strings produced by Detective X (e.g. `"PT12.34s"`, `"PT1M30.5S"`)
/// using `jiff::SignedDuration`.
pub fn parse_iso_duration(raw: &str) -> Result<f64, DeviceError> {
    let dur: jiff::SignedDuration = raw
        .trim()
        .parse()
        .map_err(|e| DeviceError::Protocol(format!("invalid ISO duration {raw:?}: {e}")))?;
    Ok(dur.as_secs_f64())
}

/// Extract `(cps, dose_rate_uSv_h)` from a `DoseCountRateResponse`.
pub fn extract_gamma_current(resp: &DoseCountRateResponse) -> Result<(f64, f64), DeviceError> {
    let gamma_current = resp
        .measurement
        .count_dose_data
        .iter()
        .find(|item| item.detector_type == "Gamma" && item.remark == "Current")
        .ok_or_else(|| {
            DeviceError::Protocol("missing Gamma/Current entry in doseCountRate response".into())
        })?;

    if gamma_current.count_rate.unit != "cps" {
        return Err(DeviceError::Protocol(format!(
            "unexpected count rate unit: {:?}",
            gamma_current.count_rate.unit
        )));
    }

    let dose_rate = gamma_current
        .dose_rate
        .iter()
        .find(|entry| is_micro_sv_per_hour(&entry.unit))
        .ok_or_else(|| {
            DeviceError::Protocol("missing µSv/h dose rate entry in doseCountRate response".into())
        })?;

    Ok((gamma_current.count_rate.value, dose_rate.value))
}

fn is_micro_sv_per_hour(unit: &str) -> bool {
    let cleaned = unit.replace('Â', "");
    matches!(cleaned.as_str(), "µSv/h" | "μSv/h" | "uSv/h")
}
