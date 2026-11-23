//! High-level `DeviceDriver` implementation for the IdentiFINDER R400.
//!
//! Uses `reqwest` for `/virtualPha/api/` commands and a background SSE/SignalR
//! stream task on `/virtualPha/signalR` for live spectrum and telemetry updates.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;
use tokio::task::JoinHandle;

use super::protocol::{
    self, endpoints, SignalRPayload, DEFAULT_CHANNELS,
};
use crate::clients::device_wrapper_base::{
    ConnectionType, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
};
use crate::clients::driver::{DeviceDriver, DeviceError, DeviceInfo};
use crate::clients::link::LinkError;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// IdentiFINDER R400 protocol driver.
pub struct IdentifinderDriver {
    client: reqwest::Client,
    stream_client: reqwest::Client,
    base_url: String,
    device_name: String,
    latest_realtime: Arc<Mutex<WrappedRealTimePackage>>,
    latest_status: Arc<Mutex<WrappedStatusPackage>>,
    latest_spectrum: Arc<Mutex<Option<WrappedSpectrumPackage>>>,
    stream_task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for IdentifinderDriver {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.stream_task.lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
    }
}

impl IdentifinderDriver {
    /// Construct a new IdentiFINDER R400 driver targeting `base_url` (e.g. `"http://192.168.154.141"`).
    pub fn new(
        base_url: impl Into<String>,
        device_name: impl Into<String>,
    ) -> Result<Self, DeviceError> {
        let client = reqwest::Client::builder()
            .timeout(COMMAND_TIMEOUT)
            .build()
            .map_err(|e| {
                DeviceError::Link(LinkError::Fatal(format!("failed to build HTTP client: {e}")))
            })?;

        let stream_client = reqwest::Client::builder()
            .connect_timeout(COMMAND_TIMEOUT)
            .build()
            .map_err(|e| {
                DeviceError::Link(LinkError::Fatal(format!(
                    "failed to build stream HTTP client: {e}"
                )))
            })?;

        Ok(Self {
            client,
            stream_client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            device_name: device_name.into(),
            latest_realtime: Arc::new(Mutex::new(WrappedRealTimePackage {
                cps: 0.0,
                dr: 0.0,
                ..Default::default()
            })),
            latest_status: Arc::new(Mutex::new(WrappedStatusPackage {
                connection_type: Some(ConnectionType::Network),
                ..Default::default()
            })),
            latest_spectrum: Arc::new(Mutex::new(None)),
            stream_task: Mutex::new(None),
        })
    }

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
            DeviceError::Link(LinkError::Timeout(COMMAND_TIMEOUT))
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

    /// Execute a `/virtualPha/api/?action=<action>` command.
    pub async fn command(&self, action: &str) -> Result<serde_json::Value, DeviceError> {
        let url = format!("{}{}", self.base_url, endpoints::API);
        let resp = self
            .client
            .get(&url)
            .query(&[("action", action)])
            .send()
            .await
            .map_err(Self::map_reqwest_err)?
            .error_for_status()
            .map_err(Self::map_reqwest_err)?;

        resp.json::<serde_json::Value>()
            .await
            .map_err(Self::map_reqwest_err)
    }

    pub async fn identify_spectrum(&self) -> Result<serde_json::Value, DeviceError> {
        self.command("Identify").await
    }

    pub async fn save_spectrum(&self) -> Result<serde_json::Value, DeviceError> {
        self.command("Save").await
    }

    /// Start the background SSE/SignalR stream task if not already running.
    pub fn start_stream(&self) {
        let mut guard = self.stream_task.lock().unwrap();
        if let Some(existing) = guard.as_ref()
            && !existing.is_finished()
        {
            return;
        }

        let client = self.stream_client.clone();
        let url = format!("{}{}", self.base_url, endpoints::SIGNALR);
        let rt_cache = self.latest_realtime.clone();
        let status_cache = self.latest_status.clone();
        let spec_cache = self.latest_spectrum.clone();

        let handle = tokio::spawn(async move {
            let Ok(mut resp) = client
                .get(&url)
                .header("Accept", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .send()
                .await
            else {
                return;
            };

            if resp.error_for_status_ref().is_err() {
                return;
            }

            let mut line_buf = Vec::new();
            while let Ok(Some(chunk)) = resp.chunk().await {
                line_buf.extend_from_slice(&chunk);
                while let Some(newline_pos) = line_buf.iter().position(|&b| b == b'\n') {
                    let raw_line: Vec<u8> = line_buf.drain(..=newline_pos).collect();
                    let Ok(line) = std::str::from_utf8(&raw_line) else {
                        continue;
                    };
                    if let Some(payload) = protocol::parse_stream_line(line) {
                        apply_payload(&payload, &rt_cache, &status_cache, &spec_cache);
                    }
                }
            }
        });

        *guard = Some(handle);
        drop(guard);
    }

    /// Stop the background SSE/SignalR stream task.
    pub fn stop_stream(&self) {
        let mut guard = self.stream_task.lock().unwrap();
        if let Some(handle) = guard.take() {
            handle.abort();
        }
        drop(guard);
    }
}

fn apply_payload(
    payload: &SignalRPayload,
    rt_cache: &Mutex<WrappedRealTimePackage>,
    status_cache: &Mutex<WrappedStatusPackage>,
    spec_cache: &Mutex<Option<WrappedSpectrumPackage>>,
) {
    let now = IdentifinderDriver::current_timestamp();

    {
        let mut guard = status_cache.lock().unwrap();
        guard.connection_type = Some(ConnectionType::Network);
        guard.timestamp = now;
        drop(guard);
    }

    let Some(counts) = payload.spectrum.as_ref() else {
        return;
    };

    let misc = payload.misc.as_ref();
    let live_time = protocol::parse_seconds(misc.and_then(|m| m.live_time.as_ref()));
    let real_time = protocol::parse_seconds(misc.and_then(|m| m.real_time.as_ref()));
    let dr = protocol::parse_leading_number(misc.and_then(|m| m.dose_rate.as_ref()));
    let cps = protocol::parse_leading_number(misc.and_then(|m| m.gamma_rate.as_ref()));

    {
        let mut guard = rt_cache.lock().unwrap();
        *guard = WrappedRealTimePackage {
            cps,
            dr,
            timestamp: now,
            ..Default::default()
        };
        drop(guard);
    }

    {
        let mut guard = spec_cache.lock().unwrap();
        *guard = Some(WrappedSpectrumPackage {
            y_axis: Array1::from_vec(counts.clone()),
            live_time,
            real_time: Some(real_time),
            calib_coeff: None,
            timestamp: now,
        });
        drop(guard);
    }
}

#[async_trait]
impl DeviceDriver for IdentifinderDriver {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            name: self.device_name.clone(),
            model: "IdentiFINDER R400".into(),
            channels: DEFAULT_CHANNELS,
            connection_type: ConnectionType::Network,
            supported_settings: Vec::new(),
        }
    }

    async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        let guard = self.latest_realtime.lock().unwrap();
        Ok(guard.clone())
    }

    async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        let guard = self.latest_status.lock().unwrap();
        Ok(guard.clone())
    }

    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        let guard = self.latest_spectrum.lock().unwrap();
        Ok(guard.clone().unwrap_or_else(|| WrappedSpectrumPackage {
            y_axis: Array1::zeros(DEFAULT_CHANNELS),
            live_time: 0.0,
            real_time: Some(0.0),
            calib_coeff: None,
            timestamp: Self::current_timestamp(),
        }))
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        self.start_stream();
        drop(self.command("Start").await?);
        Ok(())
    }

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        drop(self.command("Start").await?);
        Ok(())
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        let res = self.command("Stop").await;
        self.stop_stream();
        drop(res?);
        Ok(())
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        drop(self.command("Clear").await?);
        Ok(())
    }
}
