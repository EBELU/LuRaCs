//! Orchestrates devices, spectrograms, GPS, and Bluetooth scanning.
//!
//! ## Threading model
//!
//! A single `tokio::runtime::Runtime` drives all async work (device poll
//! loops, BLE scans). The `App` owns `RunManager` and calls `poll()` once
//! per frame; `poll()` drains events from the async side, forwards them to
//! spectrograms, and returns the accumulated UI events.
//!
//! ```text
//! tokio::Runtime                        App::update (sync, per frame)
//! ───────────────                       ─────────────────────────────
//! Device poll loop ─┐
//!                   ├─► device_event_tx ──► RunManager::poll()
//! GPS polling ──────┘                              │
//!                                                  ├─► spectrograms
//!                                                  ├─► cps/dr buffers
//!                                                  └─► gui_event_tx ──► App
//! ```
//!
//! ## Not ported
//!
//! - Bluetooth scanning: `btleplug` dependency is stubbed. `find_bluetooth`
//!   and `scan_bluetooth` return `NotImplemented`.
//! - USB device scanning: `rusb`-based `scan_all_usb` returns only vendor/
//!   product IDs; product-name lookup requires opening each device.
//! - `device.use_gps` toggle when connecting a serial GPS: skipped since
//!   the `DeviceWrapper` trait doesn't yet have a per-device flag.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::mpsc;

use crate::clients::{
    ConnectionType, DeviceAddress, DeviceError, DeviceEvent, DeviceHandle, DeviceState,
    DeviceWrapper, PollConfig, WrappedRealTimePackage, WrappedSpectrumPackage,
    WrappedStatusPackage, match_model_to_str,
};
use crate::clients::{Gps, GpsData, GpsError};
use crate::core::{
    MappingBuffer, SpectrogramManager, SpectrogramManagerEvent, SpectrogramRoi, energy_axis_for,
};
use crate::spectogram::{
    BoundedDeque, Spectrogram, SpectrogramConfig, SpectrogramError, SpectrogramEvent,
    SpectrogramMode, WrappedSpectrogramData,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum RunManagerError {
    #[error("runtime initialization failed: {0}")]
    Runtime(String),
    #[error("device type {0:?} not registered")]
    UnknownDevice(String),
    #[error("device factory failed: {0}")]
    Factory(#[from] DeviceError),
    #[error("GPS error: {0}")]
    Gps(#[from] GpsError),
    #[error("spectrogram error: {0}")]
    Spectrogram(#[from] SpectrogramError),
    #[error("bluetooth scanning not implemented")]
    BluetoothNotImplemented,
}

pub type Result<T> = std::result::Result<T, RunManagerError>;

// ---------------------------------------------------------------------------
// Public events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BleDeviceInfo {
    pub name: Option<String>,
    pub address: String,
    pub rssi: Option<i16>,
}

#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus: u8,
    pub address: u8,
}

#[derive(Debug, Clone)]
pub enum RunManagerEvent {
    // ---- Device data ---------------------------------------------------
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
    RealTimeBuffersUpdated {
        name: String,
        cps: Vec<f64>,
        dr: Vec<f64>,
    },

    // ---- Device lifecycle ---------------------------------------------
    /// A new device wrapper was constructed (before connecting).
    DeviceWrapped {
        name: String,
    },
    DeviceStateUpdated {
        name: String,
        state: DeviceState,
    },
    DeviceConnected {
        name: String,
    },
    DeviceRemoved {
        name: String,
    },
    DeviceError {
        name: String,
        error: String,
    },

    /// Emitted after a device connects — app should call
    /// `spectrum_manager.create_spectrum(...)` with these args.
    CreateDeviceSpectrum {
        device_name: String,
        channels: usize,
        connection: String,
    },
    /// Emitted on device removal — app should drop the matching spectrum.
    RemoveDeviceSpectrum {
        name: String,
    },

    // ---- Spectrograms --------------------------------------------------
    SpectrogramStarted {
        name: String,
    },
    SpectrogramClosed {
        name: String,
    },
    SpectrogramDequeResized,
    /// Forwarded snapshot from a `Spectrogram`.
    SpectrogramDataUpdated {
        name: String,
        data: Arc<WrappedSpectrogramData>,
    },
    /// Forwarded from the spectrogram manager.
    SpectrogramRoiAdded {
        roi: SpectrogramRoi,
    },
    SpectrogramRoiRemoved {
        roi: SpectrogramRoi,
    },
    SpectrogramRoiCountsUpdated {
        name: String,
        counts: Arc<HashMap<String, Vec<f64>>>,
    },
    SpectrogramMapBufferUpdated {
        name: String,
        buffer: MappingBuffer,
    },

    // ---- GPS -----------------------------------------------------------
    GpsConnection(bool),
    GpsUpdated(GpsData),

    // ---- Shutdown ------------------------------------------------------
    ShutdownStarted,
    ShutdownFinished,

    // ---- Bluetooth -----------------------------------------------------
    BluetoothFound(Vec<BleDeviceInfo>),
    BluetoothError(String),
}

// ---------------------------------------------------------------------------
// Shared state (async + sync)
// ---------------------------------------------------------------------------

pub struct RunManagerShared {
    handle: tokio::runtime::Handle,
    device_event_tx: mpsc::UnboundedSender<DeviceEvent>,
    gui_event_tx: mpsc::UnboundedSender<RunManagerEvent>,

    /// Devices by name. `Arc` so callers can hold a reference.
    devices: RwLock<HashMap<String, Arc<dyn DeviceWrapper>>>,
    /// Poll handles (owns the JoinHandles + CancellationTokens).
    device_handles: Mutex<HashMap<String, DeviceHandle>>,

    /// Rolling CPS / DR buffers per device name.
    cps_buffers: Mutex<HashMap<String, BoundedDeque<f64>>>,
    dr_buffers: Mutex<HashMap<String, BoundedDeque<f64>>>,

    poll_config: Arc<RwLock<PollConfig>>,
    queue_len: usize,
}

impl RunManagerShared {
    fn emit(&self, ev: RunManagerEvent) {
        let _ = self.gui_event_tx.send(ev);
    }
}

// ---------------------------------------------------------------------------
// RunManager
// ---------------------------------------------------------------------------

pub struct RunManager {
    _runtime: tokio::runtime::Runtime,
    shared: Arc<RunManagerShared>,

    // ---- Sync-only state ------------------------------------------------
    device_event_rx: mpsc::UnboundedReceiver<DeviceEvent>,
    gui_event_rx: mpsc::UnboundedReceiver<RunManagerEvent>,
    poll_start_time: Instant,

    has_gps: bool,
    gps: Option<Gps>,

    spectrograms: HashMap<String, Spectrogram>,
    spectrogram_manager: SpectrogramManager,
}

impl RunManager {
    pub fn new(queue_len: usize, poll_config: PollConfig) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| RunManagerError::Runtime(e.to_string()))?;

        let (device_tx, device_rx) = mpsc::unbounded_channel();
        let (gui_tx, gui_rx) = mpsc::unbounded_channel();

        let shared = Arc::new(RunManagerShared {
            handle: runtime.handle().clone(),
            device_event_tx: device_tx,
            gui_event_tx: gui_tx,
            devices: RwLock::new(HashMap::new()),
            device_handles: Mutex::new(HashMap::new()),
            cps_buffers: Mutex::new(HashMap::new()),
            dr_buffers: Mutex::new(HashMap::new()),
            poll_config: Arc::new(RwLock::new(poll_config)),
            queue_len,
        });

        Ok(Self {
            _runtime: runtime,
            shared,
            device_event_rx: device_rx,
            gui_event_rx: gui_rx,
            poll_start_time: Instant::now(),
            has_gps: false,
            gps: None,
            spectrograms: HashMap::new(),
            spectrogram_manager: SpectrogramManager::new(),
        })
    }

    // ---- Shared accessors ----------------------------------------------

    pub fn shared(&self) -> &Arc<RunManagerShared> {
        &self.shared
    }

    pub fn get_device(&self, name: &str) -> Option<Arc<dyn DeviceWrapper>> {
        self.shared.devices.read().unwrap().get(name).cloned()
    }

    pub fn list_devices(&self) -> Vec<String> {
        self.shared
            .devices
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    pub fn get_spectrogram(&self, name: &str) -> Option<&Spectrogram> {
        self.spectrograms.get(name)
    }

    pub fn get_spectrogram_mut(&mut self, name: &str) -> Option<&mut Spectrogram> {
        self.spectrograms.get_mut(name)
    }

    pub fn spectrogram_manager(&self) -> &SpectrogramManager {
        &self.spectrogram_manager
    }

    pub fn spectrogram_manager_mut(&mut self) -> &mut SpectrogramManager {
        &mut self.spectrogram_manager
    }

    pub fn set_poll_config(&self, cfg: PollConfig) {
        *self.shared.poll_config.write().unwrap() = cfg;
    }

    pub fn poll_config_get(&self) -> PollConfig {
        *self.shared.poll_config.read().unwrap()
    }

    // ---- Polling --------------------------------------------------------

    /// Drain device and spectrogram events, update buffers, and return
    /// accumulated UI events. Call once per frame.
    pub fn poll(&mut self) -> Vec<RunManagerEvent> {
        // 1. GPS
        if let Some(gps) = self.gps.as_mut() {
            if gps.poll() {
                let data = gps.data().clone();
                for sp in self.spectrograms.values_mut() {
                    sp.receive_gps(&data);
                }
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::GpsUpdated(data));
            }
        }

        // 2. Device events
        loop {
            let ev = self.device_event_rx.try_recv();
            match ev {
                Ok(ev) => self.handle_device_event(ev),
                Err(_) => break,
            }
        }

        // 3. Forward spectrogram events to the manager
        let db_names: Vec<String> = self.spectrograms.keys().cloned().collect();
        for db_name in &db_names {
            let events = self
                .spectrograms
                .get_mut(db_name)
                .map(|sp| sp.drain_events())
                .unwrap_or_default();
            for ev in events {
                match ev {
                    SpectrogramEvent::Started { .. } => {
                        // Handled in add_spectrogram.
                    }
                    SpectrogramEvent::DataUpdated { db_name, data } => {
                        let _ = self.spectrogram_manager.receive_buffer(&db_name, &data);
                        let _ = self.shared.gui_event_tx.send(
                            RunManagerEvent::SpectrogramDataUpdated {
                                name: db_name,
                                data,
                            },
                        );
                    }
                }
            }
        }

        // 4. Spectrogram manager events
        for ev in self.spectrogram_manager.drain_events() {
            let rm_ev = match ev {
                SpectrogramManagerEvent::RoiAdded { roi } => {
                    RunManagerEvent::SpectrogramRoiAdded { roi }
                }
                SpectrogramManagerEvent::RoiRemoved { roi } => {
                    RunManagerEvent::SpectrogramRoiRemoved { roi }
                }
                SpectrogramManagerEvent::RoiCountsUpdated {
                    logger_name,
                    counts,
                } => RunManagerEvent::SpectrogramRoiCountsUpdated {
                    name: logger_name,
                    counts,
                },
                SpectrogramManagerEvent::MapBufferUpdated {
                    logger_name,
                    buffer,
                } => RunManagerEvent::SpectrogramMapBufferUpdated {
                    name: logger_name,
                    buffer,
                },
            };
            let _ = self.shared.gui_event_tx.send(rm_ev);
        }

        // 5. Drain the outbound queue
        let mut out = Vec::new();
        while let Ok(ev) = self.gui_event_rx.try_recv() {
            out.push(ev);
        }
        out
    }

    fn handle_device_event(&mut self, ev: DeviceEvent) {
        match ev {
            DeviceEvent::CurrentUpdated { name, data } => {
                // Update CPS/DR buffers
                {
                    let mut cps = self.shared.cps_buffers.lock().unwrap();
                    let mut dr = self.shared.dr_buffers.lock().unwrap();
                    if let Some(buf) = cps.get_mut(&name) {
                        buf.push(data.cps);
                    }
                    if let Some(buf) = dr.get_mut(&name) {
                        buf.push(data.dr);
                    }
                }
                let (cps_vec, dr_vec) = {
                    let cps = self.shared.cps_buffers.lock().unwrap();
                    let dr = self.shared.dr_buffers.lock().unwrap();
                    (
                        cps.get(&name).map(|b| b.to_vec()).unwrap_or_default(),
                        dr.get(&name).map(|b| b.to_vec()).unwrap_or_default(),
                    )
                };
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::RealTimeBuffersUpdated {
                        name: name.clone(),
                        cps: cps_vec,
                        dr: dr_vec,
                    });

                // Forward to spectrograms matching the device
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        sp.receive_current(&name, &data);
                    }
                }
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::CurrentUpdated { name, data });
            }
            DeviceEvent::StatusUpdated { name, data } => {
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        sp.receive_status(&name, &data);
                    }
                }
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::StatusUpdated { name, data });
            }
            DeviceEvent::SpectrumUpdated { name, data } => {
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        let _ = sp.receive_spectrum(&name, &data);
                    }
                }
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::SpectrumUpdated { name, data });
            }
            DeviceEvent::DeviceStateUpdated { name, state } => {
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::DeviceStateUpdated { name, state });
            }
            DeviceEvent::PollBehindSchedule { name, behind_secs } => {
                tracing::warn!(device = %name, behind_secs, "poll loop behind schedule");
            }
            DeviceEvent::PollFailed {
                name,
                state,
                reason,
            } => {
                let _ = self
                    .shared
                    .gui_event_tx
                    .send(RunManagerEvent::DeviceStateUpdated {
                        name: name.clone(),
                        state,
                    });
                let _ = self.shared.gui_event_tx.send(RunManagerEvent::DeviceError {
                    name,
                    error: reason,
                });
            }
        }
    }

    // ---- Device management ---------------------------------------------

    /// Fire-and-forget device addition. Returns the JoinHandle the caller
    /// may ignore.
    pub fn add_device(
        &self,
        address: DeviceAddress,
        device_type: String,
        conn_type: ConnectionType,
    ) -> tokio::task::JoinHandle<()> {
        let shared = Arc::clone(&self.shared);
        self.shared.handle.spawn(async move {
            add_device_async(shared, address, device_type, conn_type).await;
        })
    }

    /// Fire-and-forget device removal.
    pub fn remove_device(&self, name: String, remove_spectrum: bool) {
        let shared = Arc::clone(&self.shared);
        self.shared.handle.spawn(async move {
            remove_device_async(shared, name, remove_spectrum).await;
        });
    }

    pub fn remove_all_devices(&self) {
        for name in self.list_devices() {
            self.remove_device(name, false);
        }
    }

    // ---- Global device actions -----------------------------------------

    pub fn reset_all_spectra(&self) {
        let devices = self.shared.devices.read().unwrap();
        for device in devices.values() {
            device.reset_spectrum();
        }
    }

    pub fn start_acquisition_all(&self) {
        let devices = self.shared.devices.read().unwrap();
        for device in devices.values() {
            device.start_acquisition();
        }
    }

    pub fn stop_acquisition_all(&self) {
        let devices = self.shared.devices.read().unwrap();
        for device in devices.values() {
            device.stop_acquisition();
        }
    }

    // ---- Spectrogram management ----------------------------------------

    pub fn add_spectrogram(&mut self, name: String, spectrogram: Spectrogram) -> Result<()> {
        self.spectrogram_manager.register_logger(&name);
        self.spectrogram_manager
            .set_energy_axis(&name, energy_axis_for(&spectrogram));

        self.spectrograms.insert(name.clone(), spectrogram);

        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::SpectrogramStarted { name });
        Ok(())
    }

    pub fn close_spectrogram(&mut self, name: &str) {
        if self.spectrograms.remove(name).is_some() {
            self.spectrogram_manager.unregister_logger(name);
            let _ = self
                .shared
                .gui_event_tx
                .send(RunManagerEvent::SpectrogramClosed {
                    name: name.to_string(),
                });
        }
    }

    pub fn resize_spectrogram_deque(&mut self, new_len: usize) {
        for sp in self.spectrograms.values_mut() {
            sp.resize_deque(new_len);
        }
        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::SpectrogramDequeResized);
    }

    // ---- GPS ------------------------------------------------------------

    pub fn connect_serial_gps(&mut self, port: &str, baud_rate: u32) -> Result<()> {
        // (Follow-up: iterate devices and clear any `use_gps` flag when the
        // DeviceWrapper trait gains that method.)
        if let Some(mut gps) = self.gps.take() {
            gps.close();
        }
        let mut gps = Gps::new();
        gps.open(port, baud_rate)?;
        self.gps = Some(gps);
        self.has_gps = true;
        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::GpsConnection(true));
        Ok(())
    }

    pub fn set_gps(&mut self, state: bool) {
        if self.has_gps {
            if let Some(mut gps) = self.gps.take() {
                gps.close();
            }
        }
        self.has_gps = state;
        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::GpsConnection(state));
    }

    // ---- Bluetooth ------------------------------------------------------

    pub fn find_bluetooth(&mut self, _timeout: Duration) -> Result<()> {
        // Stub: wire up btleplug in a follow-up.
        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::BluetoothError(
                "bluetooth scanning not yet implemented".into(),
            ));
        Err(RunManagerError::BluetoothNotImplemented)
    }

    // ---- USB scanning ---------------------------------------------------

    pub fn scan_all_usb(&self) -> Vec<UsbDeviceInfo> {
        use rusb::{Context, UsbContext};
        let Ok(ctx) = Context::new() else {
            return Vec::new();
        };
        let Ok(devices) = ctx.devices() else {
            return Vec::new();
        };
        devices
            .iter()
            .filter_map(|d| {
                let desc = d.device_descriptor().ok()?;
                Some(UsbDeviceInfo {
                    vendor_id: desc.vendor_id(),
                    product_id: desc.product_id(),
                    bus: d.bus_number(),
                    address: d.address(),
                })
            })
            .collect()
    }

    // ---- Shutdown -------------------------------------------------------

    pub fn shutdown(&mut self) {
        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::ShutdownStarted);

        // Remove all devices gracefully.
        let shared = Arc::clone(&self.shared);
        self._runtime.block_on(async move {
            let device_names: Vec<String> =
                shared.devices.read().unwrap().keys().cloned().collect();
            let mut tasks = Vec::new();
            for name in device_names {
                let s = Arc::clone(&shared);
                tasks.push(tokio::spawn(async move {
                    remove_device_async(s, name, false).await;
                }));
            }
            for t in tasks {
                let _ = t.await;
            }
        });

        if let Some(mut gps) = self.gps.take() {
            gps.close();
        }

        let _ = self
            .shared
            .gui_event_tx
            .send(RunManagerEvent::ShutdownFinished);
    }

    pub fn poll_start_time(&self) -> Instant {
        self.poll_start_time
    }
}

// ---------------------------------------------------------------------------
// Async device helpers
// ---------------------------------------------------------------------------

async fn add_device_async(
    shared: Arc<RunManagerShared>,
    address: DeviceAddress,
    device_type: String,
    conn_type: ConnectionType,
) {
    let Some(factory) = match_model_to_str(&device_type) else {
        shared.emit(RunManagerEvent::DeviceError {
            name: device_type.clone(),
            error: format!("unknown device type: {device_type}"),
        });
        return;
    };

    let device_box = match factory(address, conn_type) {
        Ok(d) => d,
        Err(e) => {
            shared.emit(RunManagerEvent::DeviceError {
                name: device_type.clone(),
                error: e.to_string(),
            });
            return;
        }
    };

    let device: Arc<dyn DeviceWrapper> = Arc::from(device_box);
    let name = device.name().to_string();

    if shared.devices.read().unwrap().contains_key(&name) {
        return;
    }

    device.set_state(DeviceState::Connecting);
    shared.emit(RunManagerEvent::DeviceStateUpdated {
        name: name.clone(),
        state: DeviceState::Connecting,
    });
    shared.emit(RunManagerEvent::DeviceWrapped { name: name.clone() });

    let handle = DeviceHandle::new_arc(Arc::clone(&device));

    if let Err(e) = handle
        .start(
            shared.device_event_tx.clone(),
            Arc::clone(&shared.poll_config),
        )
        .await
    {
        device.set_state(DeviceState::ConnectionFailed);
        shared.emit(RunManagerEvent::DeviceStateUpdated {
            name: name.clone(),
            state: DeviceState::ConnectionFailed,
        });
        shared.emit(RunManagerEvent::DeviceError {
            name,
            error: e.to_string(),
        });
        return;
    }

    if !device.is_running() {
        device.set_state(DeviceState::ConnectionFailed);
        shared.emit(RunManagerEvent::DeviceStateUpdated {
            name: name.clone(),
            state: DeviceState::ConnectionFailed,
        });
        return;
    }

    // Register
    shared
        .devices
        .write()
        .unwrap()
        .insert(name.clone(), Arc::clone(&device));
    shared
        .device_handles
        .lock()
        .unwrap()
        .insert(name.clone(), handle);

    // Buffers
    {
        let mut cps = shared.cps_buffers.lock().unwrap();
        let mut dr = shared.dr_buffers.lock().unwrap();
        cps.insert(name.clone(), BoundedDeque::filled_nan(shared.queue_len));
        dr.insert(name.clone(), BoundedDeque::filled_nan(shared.queue_len));
    }

    device.set_state(DeviceState::Connected);
    shared.emit(RunManagerEvent::DeviceStateUpdated {
        name: name.clone(),
        state: DeviceState::Connected,
    });
    shared.emit(RunManagerEvent::DeviceConnected { name: name.clone() });
    shared.emit(RunManagerEvent::CreateDeviceSpectrum {
        device_name: name.clone(),
        channels: device.channels().unwrap_or(0),
        connection: name,
    });
}

async fn remove_device_async(shared: Arc<RunManagerShared>, name: String, remove_spectrum: bool) {
    let device = shared.devices.write().unwrap().remove(&name);
    let handle = shared.device_handles.lock().unwrap().remove(&name);

    if let Some(device) = &device {
        device.set_state(DeviceState::Stopping);
        shared.emit(RunManagerEvent::DeviceStateUpdated {
            name: name.clone(),
            state: DeviceState::Stopping,
        });
    }

    if let Some(handle) = handle {
        let _ = handle.stop().await;
    }

    if let Some(device) = &device {
        if let Err(e) = device.stop().await {
            shared.emit(RunManagerEvent::DeviceError {
                name: name.clone(),
                error: e.to_string(),
            });
        }
        device.set_state(DeviceState::Stopped);
        shared.emit(RunManagerEvent::DeviceStateUpdated {
            name: name.clone(),
            state: DeviceState::Stopped,
        });
    }

    shared.emit(RunManagerEvent::DeviceRemoved { name: name.clone() });

    if remove_spectrum {
        shared.emit(RunManagerEvent::RemoveDeviceSpectrum { name });
    }
}
