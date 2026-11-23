//! High-level `DeviceDriver` implementation for the ORTEC Detective X.
//!
//! Communicates with the Detective X REST API (`/remote/v1/...`) using `reqwest`.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;

use super::protocol::{
    self, endpoints, CalibrationResponse, DoseCountRateResponse, InstrumentStatusResponse,
    SpectrumResponse,
};
use crate::clients::device_wrapper_base::{
    ConnectionType, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
};
use crate::clients::driver::{DeviceDriver, DeviceError, DeviceInfo};
use crate::clients::gps::GpsData;
use crate::clients::link::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const DETECTIVE_X_CHANNELS: usize = 1 << 14; // 16384 channels

/// Lightweight `DeviceLink` adapter for `reqwest`-backed network devices supervised by `DeviceRunner`.
pub struct HttpLink {
    name: String,
    policy: ReconnectPolicy,
}

impl HttpLink {
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            name: format!("NET:{base_url}"),
            policy: ReconnectPolicy::never(),
        }
    }
}

#[async_trait]
impl DeviceLink for HttpLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        LinkStatus::Connected
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, _data: &[u8]) -> Result<(), LinkError> {
        Ok(())
    }

    async fn recv(&self, _timeout: Duration) -> Result<Vec<u8>, LinkError> {
        Ok(Vec::new())
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        Ok(())
    }

    async fn close(&self) -> Result<(), LinkError> {
        Ok(())
    }
}

/// ORTEC Detective X protocol driver backed by `reqwest::Client`.
pub struct DetectiveXDriver {
    client: reqwest::Client,
    base_url: String,
    device_name: String,
    use_gps: bool,
    calib_coeff: Mutex<Option<Vec<f64>>>,
    latest_gps: Mutex<Option<GpsData>>,
}

impl DetectiveXDriver {
    /// Construct a new Detective X driver targeting `base_url` (e.g. `"http://192.168.1.50"`).
    pub fn new(
        base_url: impl Into<String>,
        device_name: impl Into<String>,
        use_gps: bool,
    ) -> Result<Self, DeviceError> {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .map_err(|e| DeviceError::Link(LinkError::Fatal(format!("failed to build HTTP client: {e}"))))?;

        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            device_name: device_name.into(),
            use_gps,
            calib_coeff: Mutex::new(None),
            latest_gps: Mutex::new(None),
        })
    }

    /// Base URL of the instrument REST API.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn current_timestamp() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    fn map_reqwest_err(err: reqwest::Error) -> DeviceError {
        if err.is_timeout() {
            DeviceError::Link(LinkError::Timeout(DEFAULT_TIMEOUT))
        } else if err.is_connect() {
            DeviceError::Link(LinkError::Disconnected(err.to_string()))
        } else if let Some(status) = err.status() {
            DeviceError::Hardware(format!("HTTP {status}: {err}"))
        } else if err.is_decode() {
            DeviceError::Protocol(format!("JSON decode error: {err}"))
        } else {
            DeviceError::Link(LinkError::Transient(err.to_string()))
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, DeviceError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(Self::map_reqwest_err)?
            .error_for_status()
            .map_err(Self::map_reqwest_err)?;

        resp.json::<T>().await.map_err(Self::map_reqwest_err)
    }

    async fn put(&self, path: &str) -> Result<(), DeviceError> {
        let url = format!("{}{}", self.base_url, path);
        let _resp = self
            .client
            .put(&url)
            .send()
            .await
            .map_err(Self::map_reqwest_err)?
            .error_for_status()
            .map_err(Self::map_reqwest_err)?;
        Ok(())
    }

    /// Query and cache polynomial energy calibration coefficients (reversed to descending order).
    pub async fn fetch_calibration(&self) -> Result<Vec<f64>, DeviceError> {
        let resp: CalibrationResponse = self.get_json(endpoints::CALIBRATION).await?;
        let model = &resp.calibration_type.equation_model.model;
        if model != "Polynomial" {
            return Err(DeviceError::Protocol(format!(
                "unsupported calibration model: {model:?}"
            )));
        }

        let mut coeffs = resp.calibration_type.equation_model.coefficients;
        // Reverse to descending polynomial order [a_n, ..., a1, a0] for SpectrumManager.
        coeffs.reverse();

        let mut guard = self.calib_coeff.lock().unwrap();
        *guard = Some(coeffs.clone());
        drop(guard);

        Ok(coeffs)
    }
}

#[async_trait]
impl DeviceDriver for DetectiveXDriver {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            name: self.device_name.clone(),
            model: "DetectiveX".into(),
            channels: DETECTIVE_X_CHANNELS,
            connection_type: ConnectionType::Network,
            supported_settings: Vec::new(),
        }
    }

    async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        let resp: DoseCountRateResponse = self.get_json(endpoints::DOSE_COUNT_RATE).await?;
        let (cps, dr) = protocol::extract_gamma_current(&resp)?;

        Ok(WrappedRealTimePackage {
            cps,
            dr,
            timestamp: Self::current_timestamp(),
            ..Default::default()
        })
    }

    async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        let resp: InstrumentStatusResponse = self.get_json(endpoints::INSTRUMENT_STATUS).await?;
        let status = resp.instrument_status;
        let now = Self::current_timestamp();

        if self.use_gps {
            let coords = &status.coordinates;
            let gps = if coords.len() >= 2 && coords[0] != 0.0 && coords[1] != 0.0 {
                GpsData {
                    source: "DetectiveX".into(),
                    latitude: Some(coords[0]),
                    longitude: Some(coords[1]),
                    valid: true,
                    timestamp: now as u64,
                    ..Default::default()
                }
            } else {
                GpsData {
                    source: "DetectiveX".into(),
                    latitude: Some(0.0),
                    longitude: Some(0.0),
                    valid: false,
                    timestamp: now as u64,
                    ..Default::default()
                }
            };

            let mut guard = self.latest_gps.lock().unwrap();
            *guard = Some(gps);
            drop(guard);
        }

        let battery_pct = status.battery.round().clamp(0.0, 100.0) as u8;
        let temp_c = status.crystal_temperature - 273.0;

        Ok(WrappedStatusPackage {
            battery: Some(battery_pct),
            temperature: temp_c,
            connection_type: Some(ConnectionType::Network),
            timestamp: now,
            ..Default::default()
        })
    }

    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        let resp: SpectrumResponse = self.get_json(endpoints::SPECTRUM).await?;
        let spec = resp.measurement.spectrum;

        let live_time = protocol::parse_iso_duration(&spec.live_time)?;
        let real_time = protocol::parse_iso_duration(&spec.real_time)?;
        let calib_coeff = self.calib_coeff.lock().unwrap().clone();

        Ok(WrappedSpectrumPackage {
            y_axis: Array1::from_vec(spec.channel_data),
            live_time,
            real_time: Some(real_time),
            calib_coeff,
            timestamp: Self::current_timestamp(),
        })
    }

    async fn read_gps(&self) -> Option<GpsData> {
        if !self.use_gps {
            return None;
        }
        let guard = self.latest_gps.lock().unwrap();
        guard.clone()
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        // Verify connectivity, fetch calibration, and start acquisition
        let _ = self.read_status().await?;
        drop(self.fetch_calibration().await?);
        self.start_acquisition().await?;
        Ok(())
    }

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        self.put(endpoints::ACQ_START).await
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        self.put(endpoints::ACQ_STOP).await
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        self.put(endpoints::ACQ_CLEAR).await
    }
}
