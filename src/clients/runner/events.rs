//! Event and command messages for the isolated device runner.

use crate::clients::device_wrapper_base::{
    DeviceState, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
};
use crate::clients::gps::GpsData;

/// Events emitted by an isolated `DeviceRunner` to `RunManager` or UI.
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    CurrentUpdated {
        name: String,
        data: WrappedRealTimePackage,
    },
    StatusUpdated {
        name: String,
        data: WrappedStatusPackage,
    },
    SpectrumUpdated {
        name: String,
        data: WrappedSpectrumPackage,
    },
    DeviceStateUpdated {
        name: String,
        state: DeviceState,
    },
    GpsUpdated {
        name: String,
        data: GpsData,
    },
    /// Emitted when a device was disconnected and has successfully reconnected.
    Reconnected {
        name: String,
    },
    /// The runner encountered an unrecoverable failure and stopped itself.
    PollFailed {
        name: String,
        state: DeviceState,
        reason: String,
    },
    /// The runner task panicked or encountered an isolated fatal crash.
    TaskPanicked {
        name: String,
        reason: String,
    },
}

/// Commands sent from the application / UI to an isolated `DeviceRunner`.
#[derive(Debug)]
pub enum DeviceCommand {
    ResetSpectrum,
    StartAcquisition,
    StopAcquisition,
    SetCalibration(Vec<f64>),
    Stop,
}
