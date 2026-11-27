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
//! - USB device scanning: `rusb`-based `scan_all_usb` returns only vendor/
//!   product IDs; product-name lookup requires opening each device.
//! - `device.use_gps` toggle when connecting a serial GPS: skipped since
//!   the `DeviceWrapper` trait doesn't yet have a per-device flag.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager as BleManager;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
        drop(self.gui_event_tx.send(ev));
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

    pub const fn shared(&self) -> &Arc<RunManagerShared> {
        &self.shared
    }

    pub fn runtime_handle(&self) -> &tokio::runtime::Handle {
        &self.shared.handle
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

    pub const fn spectrogram_manager(&self) -> &SpectrogramManager {
        &self.spectrogram_manager
    }

    pub const fn spectrogram_manager_mut(&mut self) -> &mut SpectrogramManager {
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
        if let Some(gps) = self.gps.as_mut()
            && gps.poll()
        {
            let data = gps.data().clone();
            for sp in self.spectrograms.values_mut() {
                sp.receive_gps(&data);
            }
            drop(
                self.shared
                    .gui_event_tx
                    .send(RunManagerEvent::GpsUpdated(data)),
            );
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
                        drop(self.spectrogram_manager.receive_buffer(&db_name, &data));
                        drop(self.shared.gui_event_tx.send(
                            RunManagerEvent::SpectrogramDataUpdated {
                                name: db_name,
                                data,
                            },
                        ));
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
            drop(self.shared.gui_event_tx.send(rm_ev));
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
                    drop(dr);
                    drop(cps);
                }
                let (cps_vec, dr_vec) = {
                    let cps = self.shared.cps_buffers.lock().unwrap();
                    let dr = self.shared.dr_buffers.lock().unwrap();
                    (
                        cps.get(&name).map(|b| b.to_vec()).unwrap_or_default(),
                        dr.get(&name).map(|b| b.to_vec()).unwrap_or_default(),
                    )
                };
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::RealTimeBuffersUpdated {
                            name: name.clone(),
                            cps: cps_vec,
                            dr: dr_vec,
                        }),
                );

                // Forward to spectrograms matching the device
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        sp.receive_current(&name, &data);
                    }
                }
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::CurrentUpdated { name, data }),
                );
            }
            DeviceEvent::StatusUpdated { name, data } => {
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        sp.receive_status(&name, &data);
                    }
                }
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::StatusUpdated { name, data }),
                );
            }
            DeviceEvent::SpectrumUpdated { name, data } => {
                for sp in self.spectrograms.values_mut() {
                    if sp.device_id() == name {
                        drop(sp.receive_spectrum(&name, &data));
                    }
                }
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::SpectrumUpdated { name, data }),
                );
            }
            DeviceEvent::DeviceStateUpdated { name, state } => {
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::DeviceStateUpdated { name, state }),
                );
            }
            DeviceEvent::PollBehindSchedule { name, behind_secs } => {
                tracing::warn!(device = %name, behind_secs, "poll loop behind schedule");
            }
            DeviceEvent::PollFailed {
                name,
                state,
                reason,
            } => {
                drop(
                    self.shared
                        .gui_event_tx
                        .send(RunManagerEvent::DeviceStateUpdated {
                            name: name.clone(),
                            state,
                        }),
                );
                drop(self.shared.gui_event_tx.send(RunManagerEvent::DeviceError {
                    name,
                    error: reason,
                }));
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

        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::SpectrogramStarted { name }),
        );
        Ok(())
    }

    pub fn close_spectrogram(&mut self, name: &str) {
        if self.spectrograms.remove(name).is_some() {
            self.spectrogram_manager.unregister_logger(name);
            drop(
                self.shared
                    .gui_event_tx
                    .send(RunManagerEvent::SpectrogramClosed {
                        name: name.to_string(),
                    }),
            );
        }
    }

    pub fn resize_spectrogram_deque(&mut self, new_len: usize) {
        for sp in self.spectrograms.values_mut() {
            sp.resize_deque(new_len);
        }
        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::SpectrogramDequeResized),
        );
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
        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::GpsConnection(true)),
        );
        Ok(())
    }

    pub fn connect_mock_gps(&mut self) -> Result<()> {
        self.connect_serial_gps("Mock", 9600)
    }

    pub const fn has_gps(&self) -> bool {
        self.has_gps
    }

    pub fn set_gps(&mut self, state: bool) {
        if self.has_gps
            && let Some(mut gps) = self.gps.take()
        {
            gps.close();
        }
        self.has_gps = state;
        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::GpsConnection(state)),
        );
    }

    // ---- Bluetooth ------------------------------------------------------

    pub fn find_bluetooth(&self, timeout: Duration) -> Result<()> {
        let shared = Arc::clone(&self.shared);
        self.shared.handle.spawn(async move {
            match scan_ble_peripherals(timeout, CancellationToken::new()).await {
                Ok(devices) => shared.emit(RunManagerEvent::BluetoothFound(devices)),
                Err(err) => shared.emit(RunManagerEvent::BluetoothError(err)),
            }
        });
        Ok(())
    }

    pub fn connect_bluetooth_list(&self, targets: &[String], timeout: Duration) -> Result<()> {
        let shared = Arc::clone(&self.shared);
        let targets = targets.to_vec();
        self.shared.handle.spawn(async move {
            match scan_ble_peripherals(timeout, CancellationToken::new()).await {
                Ok(devices) => {
                    for target in targets {
                        let lower_target = target.to_ascii_lowercase();
                        let matched = devices.iter().find(|d| {
                            d.address.eq_ignore_ascii_case(&target)
                                || d.name
                                    .as_deref()
                                    .is_some_and(|n| n.to_ascii_lowercase().contains(&lower_target))
                        });
                        let Some(dev) = matched else {
                            shared.emit(RunManagerEvent::BluetoothError(format!(
                                "Bluetooth device {target:?} not found"
                            )));
                            continue;
                        };
                        let dev_name = dev.name.as_deref().unwrap_or(&dev.address);
                        let lower = dev_name.to_ascii_lowercase();
                        let kind = if lower.contains("raysid") {
                            "raysid"
                        } else {
                            "radiacode"
                        };
                        add_device_async(
                            Arc::clone(&shared),
                            DeviceAddress::Other(dev.address.clone()),
                            kind.to_string(),
                            ConnectionType::Ble,
                        )
                        .await;
                    }
                }
                Err(err) => shared.emit(RunManagerEvent::BluetoothError(err)),
            }
        });
        Ok(())
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
        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::ShutdownStarted),
        );

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
                drop(t.await);
            }
        });

        if let Some(mut gps) = self.gps.take() {
            gps.close();
        }

        drop(
            self.shared
                .gui_event_tx
                .send(RunManagerEvent::ShutdownFinished),
        );
    }

    pub const fn poll_start_time(&self) -> Instant {
        self.poll_start_time
    }
}

// ---------------------------------------------------------------------------
// Async device helpers
// ---------------------------------------------------------------------------

/// Scan for nearby BLE peripherals using `btleplug` for up to `duration` (or until `cancel`).
pub async fn scan_ble_peripherals(
    duration: Duration,
    cancel: CancellationToken,
) -> std::result::Result<Vec<BleDeviceInfo>, String> {
    let manager = BleManager::new()
        .await
        .map_err(|e| format!("BLE manager init failed: {e}"))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| format!("Failed to list BLE adapters: {e}"))?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| "No Bluetooth adapter found".to_string())?;

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| format!("BLE scan start failed: {e}"))?;

    tokio::select! {
        () = tokio::time::sleep(duration) => {}
        () = cancel.cancelled() => {
            drop(adapter.stop_scan().await);
            return Ok(Vec::new());
        }
    }

    drop(adapter.stop_scan().await);

    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|e| format!("Failed to fetch BLE peripherals: {e}"))?;

    let mut out = Vec::with_capacity(peripherals.len());
    for p in peripherals {
        let Ok(Some(props)) = p.properties().await else {
            continue;
        };
        out.push(BleDeviceInfo {
            name: props.local_name,
            address: props.address.to_string(),
            rssi: props.rssi,
        });
    }

    Ok(out)
}

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
        shared
            .cps_buffers
            .lock()
            .unwrap()
            .insert(name.clone(), BoundedDeque::filled_nan(shared.queue_len));
        let mut dr = shared.dr_buffers.lock().unwrap();

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
        drop(handle.stop().await);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::{DummyDevice, register_device};

    #[test]
    fn test_run_manager_devices_gps_spectrogram_and_poll() {
        struct RunningDummy(DummyDevice);
        #[async_trait::async_trait]
        impl DeviceWrapper for RunningDummy {
            fn type_key(&self) -> &'static str { self.0.type_key() }
            fn name(&self) -> &str { self.0.name() }
            fn connection(&self) -> ConnectionType { self.0.connection() }
            fn state(&self) -> DeviceState { self.0.state() }
            fn set_state(&self, s: DeviceState) { self.0.set_state(s); }
            fn channels(&self) -> Option<usize> { self.0.channels() }
            fn is_running(&self) -> bool { true }
            fn is_stopped(&self) -> bool { false }
            fn connected_timestamp(&self) -> f64 { self.0.connected_timestamp() }
            async fn start(&self) -> std::result::Result<(), crate::clients::DeviceError> { Ok(()) }
            async fn stop(&self) -> std::result::Result<(), crate::clients::DeviceError> { Ok(()) }
            async fn get_realtime_data(&self) -> std::result::Result<crate::clients::WrappedRealTimePackage, crate::clients::DeviceError> {
                self.0.get_realtime_data().await
            }
            async fn get_status(&self) -> std::result::Result<crate::clients::WrappedStatusPackage, crate::clients::DeviceError> {
                self.0.get_status().await
            }
            async fn get_spectrum(&self) -> std::result::Result<crate::clients::WrappedSpectrumPackage, crate::clients::DeviceError> {
                self.0.get_spectrum().await
            }
        }
        register_device("RunMgrDummy", |addr, conn| {
            Ok(Box::new(RunningDummy(DummyDevice::new(addr, conn))))
        });

        let mut rm = RunManager::new(16, PollConfig::default()).unwrap();
        let _ = rm.shared();
        let _ = rm.runtime_handle();
        let _ = rm.poll_start_time();
        let _ = rm.spectrogram_manager();
        let _ = rm.spectrogram_manager_mut();

        let cfg = PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.1,
        };
        rm.set_poll_config(cfg);
        assert!((rm.poll_config_get().update_loop_delay - 0.05).abs() < 1e-12);

        // Mock GPS lifecycle
        rm.connect_mock_gps().unwrap();
        assert!(rm.has_gps());
        drop(rm.poll());
        rm.set_gps(false);
        assert!(!rm.has_gps());

        // Add & manage a DummyDevice via the internal runtime
        let jh = rm.add_device(
            DeviceAddress::Other("rm_dummy".into()),
            "RunMgrDummy".into(),
            ConnectionType::Network,
        );
        rm.runtime_handle().block_on(jh).unwrap();
        assert_eq!(rm.list_devices(), vec!["rm_dummy".to_string()]);
        assert!(rm.get_device("rm_dummy").is_some());

        rm.reset_all_spectra();
        rm.start_acquisition_all();
        rm.stop_acquisition_all();

        // Attach a Spectrogram for rm_dummy and let poll() process device + spectrogram events
        let sg_dir = std::env::temp_dir().join("luracs_test_run_mgr_sg");
        drop(std::fs::remove_dir_all(&sg_dir));
        let sp = Spectrogram::open(
            "sg_rm",
            &sg_dir,
            crate::spectogram::SpectrogramConfig::default(),
            crate::spectogram::SpectrogramMode::New {
                save_interval: 1.0,
                spect_channels: 1024,
                device_id: "rm_dummy".into(),
                calibration_coeff: vec![1.0, 0.0],
                concat_factor: 1,
                header_meta: serde_json::Value::Null,
            },
        )
        .unwrap();
        rm.add_spectrogram("sg_rm".into(), sp).unwrap();
        assert!(rm.get_spectrogram("sg_rm").is_some());
        assert!(rm.get_spectrogram_mut("sg_rm").is_some());
        rm.spectrogram_manager_mut().add_roi(
            crate::core::spectogram_manager::NewSpectrogramRoi {
                e_region: (10.0, 50.0),
                alias: Some("sg_r".into()),
                emission: None,
            },
        );

        std::thread::sleep(std::time::Duration::from_millis(80));
        let evs = rm.poll();
        assert!(!evs.is_empty());

        rm.resize_spectrogram_deque(32);
        rm.close_spectrogram("sg_rm");
        assert!(rm.get_spectrogram("sg_rm").is_none());

        rm.remove_device("rm_dummy".into(), true);
        std::thread::sleep(std::time::Duration::from_millis(30));
        drop(rm.poll());
        rm.remove_all_devices();
        rm.shutdown();
        drop(std::fs::remove_dir_all(&sg_dir));
    }
}
