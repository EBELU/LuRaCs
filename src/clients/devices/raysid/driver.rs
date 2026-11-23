//! High-level `DeviceDriver` implementation for the RaySID BLE gamma spectrometer.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ndarray::Array1;
use tokio::task::JoinHandle;

use super::protocol::{
    self, EnergyRange, SpectrumAccumulator, NUM_CHANNELS,
};
use crate::clients::device_wrapper_base::{
    ConnectionType, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
};
use crate::clients::driver::{DeviceDriver, DeviceError, DeviceInfo};
use crate::clients::link::{DeviceLink, LinkError};

const RX_TIMEOUT: Duration = Duration::from_secs(2);
const PING_INTERVAL: Duration = Duration::from_secs(10);

/// RaySID BLE gamma spectrometer driver.
pub struct RaysidDriver {
    link: Arc<dyn DeviceLink>,
    device_name: String,
    active_tab: Arc<AtomicU8>,
    latest_realtime: Arc<Mutex<WrappedRealTimePackage>>,
    latest_status: Arc<Mutex<WrappedStatusPackage>>,
    spectrum_acc: Arc<Mutex<SpectrumAccumulator>>,
    rx_task: Mutex<Option<JoinHandle<()>>>,
    ping_task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for RaysidDriver {
    fn drop(&mut self) {
        self.stop_workers();
    }
}

impl RaysidDriver {
    /// Construct a new RaySID driver wrapping a `DeviceLink` (typically `BleLink` or `MockLink`).
    pub fn new(link: Arc<dyn DeviceLink>, device_name: impl Into<String>) -> Self {
        Self {
            link,
            device_name: device_name.into(),
            active_tab: Arc::new(AtomicU8::new(1)),
            latest_realtime: Arc::new(Mutex::new(WrappedRealTimePackage {
                cps: 0.0,
                dr: 0.0,
                ..Default::default()
            })),
            latest_status: Arc::new(Mutex::new(WrappedStatusPackage {
                connection_type: Some(ConnectionType::Ble),
                ..Default::default()
            })),
            spectrum_acc: Arc::new(Mutex::new(SpectrumAccumulator::new())),
            rx_task: Mutex::new(None),
            ping_task: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn link(&self) -> &Arc<dyn DeviceLink> {
        &self.link
    }

    fn current_timestamp() -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64()
    }

    fn unix_secs() -> u32 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32
    }

    /// Send a raw RaySID command wrapped in TX2 framing.
    pub async fn send_command(&self, payload: &[u8]) -> Result<(), DeviceError> {
        let pkt = protocol::build_tx2_packet(payload);
        self.link.send(&pkt).await?;
        Ok(())
    }

    /// Switch RaySID energy range and reset the accumulated spectrum.
    pub async fn set_energy_range(&self, range: EnergyRange) -> Result<(), DeviceError> {
        let cmd = protocol::build_reset_packet(range);
        self.send_command(&cmd).await?;
        let mut acc = self.spectrum_acc.lock().unwrap();
        acc.reset();
        drop(acc);
        Ok(())
    }

    /// Notify the device of the active UI tab (affects streaming priority on RaySID).
    pub async fn set_active_tab(&self, tab: u8) -> Result<(), DeviceError> {
        self.active_tab.store(tab, Ordering::Relaxed);
        let ping = protocol::build_ping_packet(tab, Self::unix_secs());
        self.send_command(&ping).await
    }

    /// Spawn background RX frame decoder and keepalive ping workers.
    pub fn start_workers(&self) {
        self.stop_workers();

        // 1. RX notification frame reassembly & decode worker
        let link_rx = self.link.clone();
        let rt_cache = self.latest_realtime.clone();
        let status_cache = self.latest_status.clone();
        let spec_acc = self.spectrum_acc.clone();

        let rx_handle = tokio::spawn(async move {
            let mut buf: Vec<u8> = Vec::new();
            loop {
                match link_rx.recv(RX_TIMEOUT).await {
                    Ok(chunk) => {
                        if chunk.is_empty() {
                            continue;
                        }
                        buf.extend_from_slice(&chunk);
                        process_rx_buffer(&mut buf, &rt_cache, &status_cache, &spec_acc);
                    }
                    Err(LinkError::Timeout(_)) => {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Err(_) => break,
                }
            }
        });

        // 2. Keepalive ping worker
        let link_tx = self.link.clone();
        let active_tab = self.active_tab.clone();
        let ping_handle = tokio::spawn(async move {
            // Initial spectrum request + ping
            let spec_req = protocol::build_tx2_packet(&protocol::build_spectrum_request(
                0,
                (NUM_CHANNELS - 1) as u16,
            ));
            drop(link_tx.send(&spec_req).await);

            loop {
                let tab = active_tab.load(Ordering::Relaxed);
                let ping = protocol::build_tx2_packet(&protocol::build_ping_packet(
                    tab,
                    Self::unix_secs(),
                ));
                if link_tx.send(&ping).await.is_err() {
                    break;
                }
                tokio::time::sleep(PING_INTERVAL).await;
            }
        });

        *self.rx_task.lock().unwrap() = Some(rx_handle);
        *self.ping_task.lock().unwrap() = Some(ping_handle);
    }

    pub fn stop_workers(&self) {
        if let Ok(mut guard) = self.rx_task.lock()
            && let Some(h) = guard.take()
        {
            h.abort();
        }
        if let Ok(mut guard) = self.ping_task.lock()
            && let Some(h) = guard.take()
        {
            h.abort();
        }
    }
}

/// Extract and dispatch all complete `[len][type][...]` packets from `buf`.
pub fn process_rx_buffer(
    buf: &mut Vec<u8>,
    rt_cache: &Mutex<WrappedRealTimePackage>,
    status_cache: &Mutex<WrappedStatusPackage>,
    spec_acc: &Mutex<SpectrumAccumulator>,
) {
    while buf.len() >= 2 {
        let expected_len = if buf[0] == 0 {
            256
        } else {
            usize::from(buf[0])
        };

        // Full packet is `expected_len + 1` bytes (length byte + payload/checksum)
        if buf.len() < expected_len + 1 {
            break;
        }

        let packet: Vec<u8> = buf.drain(..=expected_len).collect();
        let pkt_type = packet[1];
        let now = RaysidDriver::current_timestamp();

        match pkt_type {
            0x01 => {
                if let Some(meta) = protocol::decode_meta_packet(&packet) {
                    let mut acc = spec_acc.lock().unwrap();
                    acc.update_meta(&meta);
                    drop(acc);
                }
            }
            0x02 => {
                if let Some(st) = protocol::decode_status_packet(&packet)
                    && st.uart_errors == 0
                {
                    let mut guard = status_cache.lock().unwrap();
                    *guard = WrappedStatusPackage {
                        battery: Some(st.battery.min(100)),
                        temperature: st.temperature,
                        charging: st.charging,
                        connection_type: Some(ConnectionType::Ble),
                        timestamp: now,
                        ..Default::default()
                    };
                    drop(guard);
                }
            }
            0x17 => {
                if protocol::verify_cps_checksum(&packet)
                    && let Some(cps) = protocol::decode_cps_packet(&packet)
                    && cps.cps > 0.0
                    && cps.dose_rate > 0.0
                {
                    let mut guard = rt_cache.lock().unwrap();
                    *guard = WrappedRealTimePackage {
                        cps: cps.cps,
                        dr: cps.dose_rate,
                        timestamp: now,
                        ..Default::default()
                    };
                    drop(guard);
                }
            }
            0x30..=0x32 => {
                if protocol::verify_spectrum_checksum(&packet)
                    && let Some(chunk) = protocol::decode_spectrum_packet(&packet)
                {
                    let mut acc = spec_acc.lock().unwrap();
                    acc.insert(&chunk);
                    drop(acc);
                }
            }
            0x03 | 0x06 => {
                // Known keepalive/ack packets
            }
            _ => {
                // Unknown packet type — clear buffer to resync
                buf.clear();
                break;
            }
        }
    }
}

#[async_trait]
impl DeviceDriver for RaysidDriver {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            name: self.device_name.clone(),
            model: "RaySID".into(),
            channels: NUM_CHANNELS,
            connection_type: ConnectionType::Ble,
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
        let acc = self.spectrum_acc.lock().unwrap();
        let pkg = WrappedSpectrumPackage {
            y_axis: Array1::from_vec(acc.spectrum.clone()),
            live_time: acc.uptime_secs,
            real_time: Some(acc.uptime_secs),
            calib_coeff: None,
            timestamp: Self::current_timestamp(),
        };
        drop(acc);
        Ok(pkg)
    }

    async fn on_reconnect(&self) -> Result<(), DeviceError> {
        self.start_workers();
        Ok(())
    }

    async fn start_acquisition(&self) -> Result<(), DeviceError> {
        self.start_workers();
        Ok(())
    }

    async fn stop_acquisition(&self) -> Result<(), DeviceError> {
        self.stop_workers();
        Ok(())
    }

    async fn reset_spectrum(&self) -> Result<(), DeviceError> {
        self.set_energy_range(EnergyRange::Mid).await
    }
}
