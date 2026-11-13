//! Device wrapper base: trait + registry + poll loop.
//!
//! Port of `luracs.clients.device_wrapper_base`.
//!
//! ## Structural changes from Python
//!
//! - **ABC → trait.** `DeviceWrapper` requires `get_realtime_data`,
//!   `get_status`, `get_spectrum`, `is_running`, `is_stopped`. Optional
//!   methods (`reset_spectrum`, `set_calibration`, `start_acquisition`,
//!   `stop_acquisition`) get default bodies; `set_calibration` returns
//!   `Err(DeviceError::NotSupported)` where Python raised
//!   `NotImplementedError`.
//! - **`__init_subclass__` → explicit `register_device`.** Call
//!   `register_device("Raysid", factory)` once per concrete driver at
//!   startup (a `ctor` crate or a manual init function works).
//! - **Poll lifecycle on a handle.** `DeviceHandle` owns the `JoinHandle`
//!   and `CancellationToken`. `dyn DeviceWrapper` stays `&self`-only.
//! - **Signals → channel.** The poll loop sends `DeviceEvent`s; the run
//!   manager owns the receiver.
//! - **`Settings.Advanced` reads → `Arc<RwLock<PollConfig>>`.** Re-read
//!   each loop iteration (same as Python).
//! - **`state: AtomicU8`.** Required because `dyn DeviceWrapper` cannot
//!   expose `&mut self`.
//! - **`battery: Option<u8>`.** Python's `int = np.nan` collapses to
//!   "unknown"; `Option<u8>` says so. Same for `WrappedRealTimePackage`'s
//!   `dead_time`, `cps_error`, `dr_error` — those keep `f64::NAN` as
//!   default, matching Python's numeric behaviour.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ndarray::Array1;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Connection / state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    Usb,
    Ble,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceState {
    Uninitialized = 0,
    Connecting,
    Connected,
    ConnectionFailed,
    ConnectionLost,
    Stopping,
    Stopped,
    Error,
}

impl DeviceState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Connecting,
            2 => Self::Connected,
            3 => Self::ConnectionFailed,
            4 => Self::ConnectionLost,
            5 => Self::Stopping,
            6 => Self::Stopped,
            7 => Self::Error,
            _ => Self::Uninitialized,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportedSetting {
    Calibration,
    HvAndAmp,
}

// ---------------------------------------------------------------------------
// Data packages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WrappedRealTimePackage {
    pub cps: f64,
    pub dr: f64,
    pub dead_time: f64,
    pub cps_error: f64,
    pub dr_error: f64,
    pub timestamp: f64,
}

impl Default for WrappedRealTimePackage {
    fn default() -> Self {
        Self {
            cps: 0.0,
            dr: 0.0,
            dead_time: f64::NAN,
            cps_error: f64::NAN,
            dr_error: f64::NAN,
            timestamp: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WrappedStatusPackage {
    /// `None` when the device doesn't report a battery percentage
    /// (Python's `int = np.nan` default collapses to "unknown").
    pub battery: Option<u8>,
    pub temperature: f64,
    pub charging: bool,
    pub total_dose: f64,
    pub total_uptime: f64,
    pub voltage: f64,
    pub desired_voltage: f64,
    pub lower_level_discriminator: f64,
    pub upper_level_discriminator: f64,
    pub fine_gain: f64,
    pub device_state: Option<DeviceState>,
    pub connection_type: Option<ConnectionType>,
    pub timestamp: f64,
}

impl Default for WrappedStatusPackage {
    fn default() -> Self {
        Self {
            battery: None,
            temperature: f64::NAN,
            charging: false,
            total_dose: f64::NAN,
            total_uptime: f64::NAN,
            voltage: f64::NAN,
            desired_voltage: f64::NAN,
            lower_level_discriminator: f64::NAN,
            upper_level_discriminator: f64::NAN,
            fine_gain: f64::NAN,
            device_state: None,
            connection_type: None,
            timestamp: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WrappedSpectrumPackage {
    pub y_axis: Array1<f64>,
    pub live_time: f64,
    pub real_time: Option<f64>,
    pub calib_coeff: Option<Vec<f64>>,
    pub timestamp: f64,
}

impl Default for WrappedSpectrumPackage {
    fn default() -> Self {
        Self {
            y_axis: Array1::zeros(0),
            live_time: 1.0,
            real_time: None,
            calib_coeff: None,
            timestamp: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("not supported by this device")]
    NotSupported,
    #[error("device disconnected: {0}")]
    Disconnected(String),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("initialization: {0}")]
    Init(String),
}

// ---------------------------------------------------------------------------
// Address (per-connection-type payload)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum DeviceAddress {
    Usb {
        vendor_id: u16,
        product_id: u16,
        serial: Option<String>,
    },
    Ble {
        name: String,
    },
    Network {
        host: String,
        port: u16,
    },
    /// Free-form address for devices that auto-discover or take a
    /// non-standard locator (e.g. a file path, a COM number).
    Other(String),
}

impl DeviceAddress {
    pub fn display_name(&self) -> String {
        match self {
            DeviceAddress::Usb {
                vendor_id,
                product_id,
                serial,
            } => match serial {
                Some(s) => format!("USB:{vendor_id:04x}:{product_id:04x}:{s}"),
                None => format!("USB:{vendor_id:04x}:{product_id:04x}"),
            },
            DeviceAddress::Ble { name } => format!("BLE:{name}"),
            DeviceAddress::Network { host, port } => format!("NET:{host}:{port}"),
            DeviceAddress::Other(s) => s.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Poll config (was Settings.Advanced)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct PollConfig {
    pub update_loop_delay: f64,
    pub spectrum_update_delay: f64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            update_loop_delay: 1.0,
            spectrum_update_delay: 1.0,
        }
    }
}

impl PollConfig {
    /// Number of update iterations between spectrum samples.
    fn spectrum_interval(&self) -> usize {
        ((self.spectrum_update_delay / self.update_loop_delay).ceil() as usize).max(1)
    }
}

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

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
    /// Emitted when the poll loop can't keep up; the original logged a
    /// warning. The UI can display it in a status bar if desired.
    PollBehindSchedule {
        name: String,
        behind_secs: f64,
    },
    /// Unrecoverable failure in the loop; the loop exits after emitting this.
    PollFailed {
        name: String,
        state: DeviceState,
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Device trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait DeviceWrapper: Send + Sync + 'static {
    /// Stable key used for registry lookup (was `type` on the Python class).
    fn type_key(&self) -> &'static str;
    fn name(&self) -> &str;
    fn connection(&self) -> ConnectionType;
    fn channels(&self) -> Option<usize>;

    fn state(&self) -> DeviceState;
    fn set_state(&self, state: DeviceState);
    fn connected_timestamp(&self) -> f64;

    fn is_running(&self) -> bool;
    fn is_stopped(&self) -> bool;

    // --- Required device I/O -------------------------------------------
    async fn get_realtime_data(&self) -> Result<WrappedRealTimePackage, DeviceError>;
    async fn get_status(&self) -> Result<WrappedStatusPackage, DeviceError>;
    async fn get_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError>;

    // --- Optional lifecycle -------------------------------------------
    async fn start(&self) -> Result<(), DeviceError> {
        Ok(())
    }
    async fn stop(&self) -> Result<(), DeviceError> {
        Ok(())
    }

    // --- Optional feature hooks ---------------------------------------
    fn supported_settings(&self) -> &'static [SupportedSetting] {
        &[]
    }

    fn reset_spectrum(&self) {}

    fn set_calibration(&self, _coeffs: &[f64]) -> Result<(), DeviceError> {
        Err(DeviceError::NotSupported)
    }

    fn start_acquisition(&self) {}
    fn stop_acquisition(&self) {}
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

type DeviceFactory = dyn Fn(DeviceAddress, ConnectionType) -> Result<Box<dyn DeviceWrapper>, DeviceError>
    + Send
    + Sync;

fn registry() -> &'static Mutex<HashMap<&'static str, Arc<DeviceFactory>>> {
    static R: OnceLock<Mutex<HashMap<&'static str, Arc<DeviceFactory>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a concrete device type. Idempotent — re-registering the same
/// key replaces the previous factory.
pub fn register_device<F>(type_key: &'static str, factory: F)
where
    F: Fn(DeviceAddress, ConnectionType) -> Result<Box<dyn DeviceWrapper>, DeviceError>
        + Send
        + Sync
        + 'static,
{
    registry()
        .lock()
        .unwrap()
        .insert(type_key, Arc::new(factory));
}

/// Clone the current registry snapshot.
pub fn get_registry() -> HashMap<&'static str, Arc<DeviceFactory>> {
    registry().lock().unwrap().clone()
}

/// Match a free-form name (e.g. `"Raysid #1234"`) against a registered key
/// using `contains`. Same semantics as the Python `match_model_to_str`.
pub fn match_model_to_str(name: &str) -> Option<Arc<DeviceFactory>> {
    let reg = registry().lock().unwrap();
    for (key, factory) in reg.iter() {
        if name.contains(key) {
            return Some(factory.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Poll loop
// ---------------------------------------------------------------------------

async fn run_poll_loop(
    wrapper: Arc<dyn DeviceWrapper>,
    events: mpsc::UnboundedSender<DeviceEvent>,
    settings: Arc<RwLock<PollConfig>>,
    cancel: CancellationToken,
) {
    let name = wrapper.name().to_string();
    let start = Instant::now();

    let initial_cfg = *settings.read().unwrap();
    let mut cfg = initial_cfg;
    let mut loops_since_spectrum: usize = 0;

    let send = |ev: DeviceEvent| {
        // Unbounded: only fails if the receiver has been dropped (manager
        // shutting down); ignoring the error is correct here.
        let _ = events.send(ev);
    };

    while wrapper.is_running() && !cancel.is_cancelled() {
        // Re-read config each iteration; if it changed, reset the
        // spectrum-sampling counter (matches Python).
        let now_cfg = *settings.read().unwrap();
        if now_cfg.update_loop_delay != cfg.update_loop_delay
            || now_cfg.spectrum_update_delay != cfg.spectrum_update_delay
        {
            cfg = now_cfg;
            loops_since_spectrum = 0;
        }

        // Real-time
        match wrapper.get_realtime_data().await {
            Ok(data) => send(DeviceEvent::CurrentUpdated {
                name: name.clone(),
                data,
            }),
            Err(DeviceError::Timeout(msg)) => {
                tracing::warn!(device = %name, "poll timeout: {msg}");
            }
            Err(e) => {
                tracing::error!(device = %name, "realtime failed: {e}");
                wrapper.set_state(DeviceState::Error);
                send(DeviceEvent::DeviceStateUpdated {
                    name: name.clone(),
                    state: DeviceState::Error,
                });
                send(DeviceEvent::PollFailed {
                    name: name.clone(),
                    state: DeviceState::Error,
                    reason: e.to_string(),
                });
                break;
            }
        }

        // Status
        match wrapper.get_status().await {
            Ok(data) => send(DeviceEvent::StatusUpdated {
                name: name.clone(),
                data,
            }),
            Err(DeviceError::Timeout(msg)) => {
                tracing::warn!(device = %name, "status timeout: {msg}");
            }
            Err(e) => {
                tracing::error!(device = %name, "status failed: {e}");
                wrapper.set_state(DeviceState::Error);
                send(DeviceEvent::DeviceStateUpdated {
                    name: name.clone(),
                    state: DeviceState::Error,
                });
                send(DeviceEvent::PollFailed {
                    name: name.clone(),
                    state: DeviceState::Error,
                    reason: e.to_string(),
                });
                break;
            }
        }

        // Spectrum (every N loops)
        loops_since_spectrum += 1;
        if loops_since_spectrum >= cfg.spectrum_interval() {
            match wrapper.get_spectrum().await {
                Ok(data) => send(DeviceEvent::SpectrumUpdated {
                    name: name.clone(),
                    data,
                }),
                Err(DeviceError::Timeout(msg)) => {
                    tracing::warn!(device = %name, "spectrum timeout: {msg}");
                }
                Err(e) => {
                    tracing::error!(device = %name, "spectrum failed: {e}");
                    wrapper.set_state(DeviceState::Error);
                    send(DeviceEvent::DeviceStateUpdated {
                        name: name.clone(),
                        state: DeviceState::Error,
                    });
                    send(DeviceEvent::PollFailed {
                        name: name.clone(),
                        state: DeviceState::Error,
                        reason: e.to_string(),
                    });
                    break;
                }
            }
            loops_since_spectrum = 0;
        }

        // Schedule next iteration
        let elapsed = start.elapsed().as_secs_f64();
        let cycle = (elapsed / cfg.update_loop_delay).floor() + 1.0;
        let next_at = cycle * cfg.update_loop_delay;
        let now = start.elapsed().as_secs_f64();
        let sleep_for = next_at - now;

        if sleep_for > 0.0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs_f64(sleep_for)) => {}
                _ = cancel.cancelled() => break,
            }
        } else if sleep_for < 0.0 {
            send(DeviceEvent::PollBehindSchedule {
                name: name.clone(),
                behind_secs: -sleep_for,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// DeviceHandle (owns the poll lifecycle)
// ---------------------------------------------------------------------------

struct PollTask {
    cancel: CancellationToken,
    join: JoinHandle<()>,
}

pub struct DeviceHandle {
    pub wrapper: Arc<dyn DeviceWrapper>,
    poll_task: tokio::sync::Mutex<Option<PollTask>>,
}

impl DeviceHandle {
    pub fn new(wrapper: Box<dyn DeviceWrapper>) -> Self {
        Self {
            wrapper: Arc::from(wrapper),
            poll_task: tokio::sync::Mutex::new(None),
        }
    }

    pub fn new_arc(wrapper: Arc<dyn DeviceWrapper>) -> Self {
        Self {
            wrapper,
            poll_task: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn start_polling(
        &self,
        events: mpsc::UnboundedSender<DeviceEvent>,
        settings: Arc<RwLock<PollConfig>>,
    ) {
        let mut lock = self.poll_task.lock().await;
        if let Some(task) = lock.as_ref() {
            if !task.join.is_finished() {
                return; // already running
            }
        }
        let cancel = CancellationToken::new();
        let join = tokio::spawn(run_poll_loop(
            self.wrapper.clone(),
            events,
            settings,
            cancel.clone(),
        ));
        *lock = Some(PollTask { cancel, join });
    }

    pub async fn stop_polling(&self) {
        let task = self.poll_task.lock().await.take();
        if let Some(task) = task {
            task.cancel.cancel();
            let _ = task.join.await;
        }
    }

    /// Start the device and begin polling. Combines Python's
    /// `start() = client.start() + start_polling()`.
    pub async fn start(
        &self,
        events: mpsc::UnboundedSender<DeviceEvent>,
        settings: Arc<RwLock<PollConfig>>,
    ) -> Result<(), DeviceError> {
        self.wrapper.start().await?;
        self.start_polling(events, settings).await;
        Ok(())
    }

    /// Stop polling then the device. Combines Python's `stop()`.
    pub async fn stop(&self) -> Result<(), DeviceError> {
        self.stop_polling().await;
        self.wrapper.stop().await
    }
}

// ---------------------------------------------------------------------------
// Example concrete device (skeleton)
// ---------------------------------------------------------------------------

/// Bare-minimum concrete wrapper, useful as a template for real drivers.
///
/// A real device would:
/// - hold a `Mutex<Client>` for the device handle,
/// - implement `get_*` by calling into it,
/// - manage `running` based on a user start/stop command (or, for
///   always-on devices, set it once at construction).
pub struct DummyDevice {
    name: String,
    connection: ConnectionType,
    state: AtomicU8,
    running: std::sync::atomic::AtomicBool,
    connected_timestamp: f64,
}

impl DummyDevice {
    pub fn new(address: DeviceAddress, connection: ConnectionType) -> Self {
        Self {
            name: address.display_name(),
            connection,
            state: AtomicU8::new(DeviceState::Uninitialized as u8),
            running: std::sync::atomic::AtomicBool::new(false),
            connected_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0),
        }
    }
}

#[async_trait]
impl DeviceWrapper for DummyDevice {
    fn type_key(&self) -> &'static str {
        "Dummy"
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn connection(&self) -> ConnectionType {
        self.connection
    }
    fn channels(&self) -> Option<usize> {
        Some(1024)
    }

    fn state(&self) -> DeviceState {
        DeviceState::from_u8(self.state.load(Ordering::Relaxed))
    }
    fn set_state(&self, s: DeviceState) {
        self.state.store(s as u8, Ordering::Relaxed);
    }
    fn connected_timestamp(&self) -> f64 {
        self.connected_timestamp
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    fn is_stopped(&self) -> bool {
        !self.is_running()
    }

    async fn get_realtime_data(&self) -> Result<WrappedRealTimePackage, DeviceError> {
        Ok(WrappedRealTimePackage {
            cps: 12.3,
            dr: 0.045,
            timestamp: self.connected_timestamp,
            ..Default::default()
        })
    }
    async fn get_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
        Ok(WrappedStatusPackage {
            timestamp: self.connected_timestamp,
            ..Default::default()
        })
    }
    async fn get_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
        Ok(WrappedSpectrumPackage {
            y_axis: Array1::zeros(1024),
            timestamp: self.connected_timestamp,
            ..Default::default()
        })
    }
}

// Example registration — call once at app startup.
//
// register_device("Dummy", |addr, conn| Ok(Box::new(DummyDevice::new(addr, conn))));
