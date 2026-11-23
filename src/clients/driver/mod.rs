//! Pure device protocol driver abstraction.
//!
//! Drivers only handle packet encoding/decoding and device-specific state.
//! Transport I/O, timers, reconnection, and polling are owned by the Link and Runner.

pub mod rate_estimator;

use async_trait::async_trait;
use thiserror::Error;

pub use rate_estimator::CpsEstimator;
use crate::clients::device_wrapper_base::{
    ConnectionType, SupportedSetting, WrappedRealTimePackage, WrappedSpectrumPackage,
    WrappedStatusPackage,
};
use crate::clients::gps::GpsData;
use crate::clients::link::LinkError;

/// Errors arising during protocol parsing or command processing.
#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("link I/O failure: {0}")]
    Link(#[from] LinkError),

    #[error("protocol decoding / CRC error: {0}")]
    Protocol(String),

    #[error("not supported by this hardware: {0}")]
    NotSupported(&'static str),

    #[error("hardware reported error: {0}")]
    Hardware(String),

    #[error("device is not initialized")]
    Uninitialized,
}

/// Metadata describing a connected detector.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub model: String,
    pub channels: usize,
    pub connection_type: ConnectionType,
    pub supported_settings: Vec<SupportedSetting>,
}

/// Control interface for instruments with adjustable High Voltage and Analog Front-End
/// (e.g. digiBase, digiDART).
#[async_trait]
pub trait HvControl: Send + Sync {
    async fn set_hv_enabled(&self, enabled: bool) -> Result<(), DeviceError>;
    async fn set_hv(&self, volts: f64) -> Result<(), DeviceError>;
    async fn set_lld(&self, lld: f64) -> Result<(), DeviceError>;
    async fn set_uld(&self, uld: f64) -> Result<(), DeviceError>;
    async fn set_fine_gain(&self, gain: f64) -> Result<(), DeviceError>;
}

/// Clean protocol driver interface.
#[async_trait]
pub trait DeviceDriver: Send + Sync {
    /// Device metadata (name, model, channel count, etc.).
    fn info(&self) -> DeviceInfo;

    // --- Core Telemetry I/O ---

    /// Retrieve the latest real-time measurements (CPS, Dose Rate).
    async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError>;

    /// Retrieve the latest status package (battery, temperature, voltage).
    async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError>;

    /// Retrieve the current spectrum.
    async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError>;

    /// Optional GPS data if the instrument has an onboard GPS receiver
    /// (e.g. Detective X, IdentiFinder).
    async fn read_gps(&self) -> Option<GpsData> {
        None
    }

    // --- State Recovery ---

    /// Called by the `DeviceRunner` after the underlying `DeviceLink` successfully
    /// reconnects. Re-syncs sequence numbers, re-enables acquisition, etc.
    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    // --- Lifecycle and Acquisition Hooks ---

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    async fn set_calibration(&self, _coeffs: &[f64]) -> Result<(), DeviceError> {
        Err(DeviceError::NotSupported("calibration write"))
    }

    /// Access to HV control interface if supported by this instrument.
    fn hv_control(&self) -> Option<&dyn HvControl> {
        None
    }
}
