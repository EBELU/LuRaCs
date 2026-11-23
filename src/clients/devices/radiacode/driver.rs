//! High-level `DeviceDriver` implementation for RadiaCode 101/102/103.
//!
//! Provides structured telemetry polling, spectrum acquisition, and calibration control.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{Datelike, Local, Timelike};
use ndarray::Array1;

use super::protocol::{
    self, Command, DecodedSpectrum, VirtualString,
};
use crate::clients::device_wrapper_base::{
    ConnectionType, SupportedSetting, WrappedRealTimePackage, WrappedSpectrumPackage,
    WrappedStatusPackage,
};
use crate::clients::driver::{DeviceDriver, DeviceError, DeviceInfo};
use crate::clients::link::DeviceLink;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2500);

/// RadiaCode device protocol driver.
pub struct RadiacodeDriver {
    link: Arc<dyn DeviceLink>,
    seq: AtomicU8,
    spec_format_version: AtomicU32,
    device_name: String,
    channels: usize,
    latest_realtime: Mutex<WrappedRealTimePackage>,
    latest_status: Mutex<WrappedStatusPackage>,
    latest_spectrum: Mutex<Option<WrappedSpectrumPackage>>,
    calib_coeff: Mutex<[f64; 3]>,
}

impl RadiacodeDriver {
    /// Construct a new driver wrapping an active communication link.
    pub fn new(link: Arc<dyn DeviceLink>, name: impl Into<String>) -> Self {
        Self {
            link,
            seq: AtomicU8::new(0),
            spec_format_version: AtomicU32::new(0),
            device_name: name.into(),
            channels: 1024,
            latest_realtime: Mutex::new(WrappedRealTimePackage::default()),
            latest_status: Mutex::new(WrappedStatusPackage::default()),
            latest_spectrum: Mutex::new(None),
            calib_coeff: Mutex::new([0.0, 1.0, 0.0]),
        }
    }

    /// Link reference.
    #[must_use]
    pub fn link(&self) -> &Arc<dyn DeviceLink> {
        &self.link
    }

    fn next_seq(&self) -> u8 {
        self.seq.fetch_add(1, Ordering::SeqCst) % 32
    }

    fn current_timestamp() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    /// Read raw string configuration (CP1251 / UTF-8) from instrument.
    pub async fn read_configuration(&self) -> Result<String, DeviceError> {
        let seq = self.next_seq();
        let bytes = protocol::read_virtual_string(
            &*self.link,
            seq,
            VirtualString::Configuration as u32,
            DEFAULT_TIMEOUT,
        )
        .await?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    /// Read hardware serial number string.
    pub async fn read_serial_number(&self) -> Result<String, DeviceError> {
        let seq = self.next_seq();
        let bytes = protocol::read_virtual_string(
            &*self.link,
            seq,
            VirtualString::SerialNumber as u32,
            DEFAULT_TIMEOUT,
        )
        .await?;
        Ok(String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string())
    }
}

#[async_trait]
impl DeviceDriver for RadiacodeDriver {
    fn info(&self) -> DeviceInfo {
        let conn_type = if self.link.address_name().starts_with("USB") {
            ConnectionType::Usb
        } else if self.link.address_name().starts_with("BLE") {
            ConnectionType::Ble
        } else {
            ConnectionType::Usb
        };

        DeviceInfo {
            name: self.device_name.clone(),
            model: "RadiaCode".into(),
            channels: self.channels,
            connection_type: conn_type,
            supported_settings: vec![SupportedSetting::Calibration],
        }
    }

    async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        let seq = self.next_seq();
        let data = protocol::read_virtual_string(
            &*self.link,
            seq,
            VirtualString::DataBuf as u32,
            DEFAULT_TIMEOUT,
        )
        .await?;

        let now = Self::current_timestamp();
        let (opt_rt, opt_status) = protocol::decode_data_buf(&data, now);

        if let Some(status) = opt_status {
            let mut guard = self.latest_status.lock().unwrap();
            *guard = status;
            drop(guard);
        }

        opt_rt.map_or_else(
            || {
                let guard = self.latest_realtime.lock().unwrap();
                Ok(guard.clone())
            },
            |rt| {
                let mut guard = self.latest_realtime.lock().unwrap();
                *guard = rt.clone();
                drop(guard);
                Ok(rt)
            },
        )
    }

    async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        let guard = self.latest_status.lock().unwrap();
        Ok(guard.clone())
    }

    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        let seq = self.next_seq();
        let data = protocol::read_virtual_string(
            &*self.link,
            seq,
            VirtualString::Spectrum as u32,
            DEFAULT_TIMEOUT,
        )
        .await?;

        let format_ver = self.spec_format_version.load(Ordering::Relaxed);
        let DecodedSpectrum {
            duration_secs,
            a0,
            a1,
            a2,
            counts,
        } = protocol::decode_spectrum(&data, format_ver)?;

        // Update polynomial calibration coefficients in [a2, a1, a0] order
        // (descending order used by SpectrumManager polynomial evaluation)
        let calib = [f64::from(a2), f64::from(a1), f64::from(a0)];
        {
            let mut guard = self.calib_coeff.lock().unwrap();
            *guard = calib;
        }

        let now = Self::current_timestamp();
        let pkg = WrappedSpectrumPackage {
            y_axis: Array1::from_vec(counts.into_iter().map(f64::from).collect()),
            live_time: f64::from(duration_secs),
            real_time: Some(f64::from(duration_secs)),
            calib_coeff: Some(calib.to_vec()),
            timestamp: now,
        };

        {
            let mut guard = self.latest_spectrum.lock().unwrap();
            *guard = Some(pkg.clone());
        }

        Ok(pkg)
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        let seq = self.next_seq();
        protocol::write_virtual_string(
            &*self.link,
            seq,
            VirtualString::Spectrum as u32,
            &[],
            DEFAULT_TIMEOUT,
        )
        .await
    }

    async fn set_calibration(&self, coeffs: &[f64]) -> Result<(), DeviceError> {
        if coeffs.len() != 3 {
            return Err(DeviceError::Protocol(format!(
                "expected 3 calibration coefficients, got {}",
                coeffs.len()
            )));
        }

        // GUI / SpectrumManager supplies descending polynomial [a2, a1, a0].
        // RadiaCode hardware registers expect ascending order [a0, a1, a2].
        let a0 = coeffs[2] as f32;
        let a1 = coeffs[1] as f32;
        let a2 = coeffs[0] as f32;

        let mut data = Vec::with_capacity(12);
        data.extend_from_slice(&a0.to_le_bytes());
        data.extend_from_slice(&a1.to_le_bytes());
        data.extend_from_slice(&a2.to_le_bytes());

        let seq = self.next_seq();
        protocol::write_virtual_string(
            &*self.link,
            seq,
            VirtualString::EnergyCalib as u32,
            &data,
            DEFAULT_TIMEOUT,
        )
        .await?;

        let mut guard = self.calib_coeff.lock().unwrap();
        *guard = [coeffs[0], coeffs[1], coeffs[2]];
        drop(guard);

        Ok(())
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        self.seq.store(0, Ordering::Relaxed);

        // 1. Initial handshake
        let seq = self.next_seq();
        protocol::execute_transaction(
            &*self.link,
            Command::SetExchange as u16,
            seq,
            &[0x01, 0xff, 0x12, 0xff],
            DEFAULT_TIMEOUT,
        )
        .await?;

        // 2. Synchronize device clock
        let seq = self.next_seq();
        let now = Local::now();
        let year_offset = now.year().saturating_sub(2000).clamp(0, 255) as u8;
        let time_payload = [
            now.day() as u8,
            now.month() as u8,
            year_offset,
            0u8,
            now.second() as u8,
            now.minute() as u8,
            now.hour() as u8,
            0u8,
        ];
        protocol::execute_transaction(
            &*self.link,
            Command::SetTime as u16,
            seq,
            &time_payload,
            DEFAULT_TIMEOUT,
        )
        .await?;

        // 3. Detect spectrum format version from configuration string
        if let Ok(cfg) = self.read_configuration().await {
            for line in cfg.lines() {
                if let Some(rest) = line.strip_prefix("SpecFormatVersion=")
                    && let Ok(ver) = rest.trim().parse::<u32>()
                {
                    self.spec_format_version.store(ver, Ordering::Relaxed);
                    break;
                }
            }
        }

        Ok(())
    }
}
