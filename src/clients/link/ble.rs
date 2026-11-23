//! Bluetooth Low Energy (BLE) link implemented using native async `btleplug`.
//!
//! Applies composable reconnection policies (`ReconnectPolicy`) with backoff
//! and termination limits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use super::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

/// Native async Bluetooth Low Energy (BLE) transport link.
pub struct BleLink {
    name: String,
    target_name: String,
    write_uuid: Uuid,
    notify_uuid: Uuid,
    policy: ReconnectPolicy,
    inner: Arc<Mutex<Option<BleLinkInner>>>,
}

struct BleLinkInner {
    _adapter: Adapter,
    peripheral: Peripheral,
    write_char: Characteristic,
    notify_rx: mpsc::Receiver<Vec<u8>>,
    status: LinkStatus,
}

impl BleLink {
    /// Construct a new BLE link with a composable reconnection policy.
    #[must_use]
    pub fn new(
        target_name: impl Into<String>,
        write_uuid: Uuid,
        notify_uuid: Uuid,
        policy: ReconnectPolicy,
    ) -> Self {
        let name_str = target_name.into();
        Self {
            name: format!("BLE:{name_str}"),
            target_name: name_str,
            write_uuid,
            notify_uuid,
            policy,
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Construct a BLE link with default composable policy (exponential backoff, 10 retries, 2m max).
    #[must_use]
    pub fn with_default_policy(
        target_name: impl Into<String>,
        write_uuid: Uuid,
        notify_uuid: Uuid,
    ) -> Self {
        Self::new(target_name, write_uuid, notify_uuid, ReconnectPolicy::ble_default())
    }

    async fn connect_internal(&self) -> Result<BleLinkInner, LinkError> {
        let manager = Manager::new().await.map_err(|e| {
            LinkError::Fatal(format!("BLE Manager initialization failed: {e}"))
        })?;

        let adapters = manager.adapters().await.map_err(|e| {
            LinkError::Fatal(format!("failed to fetch BLE adapters: {e}"))
        })?;

        let adapter = adapters.into_iter().next().ok_or_else(|| {
            LinkError::NotFound("no Bluetooth adapters found on system".into())
        })?;

        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| LinkError::Transient(format!("BLE scan start failed: {e}")))?;

        // Scan for target peripheral with a 3s discovery window
        let start_scan = Instant::now();
        let scan_limit = Duration::from_secs(3);
        let mut target_peripheral: Option<Peripheral> = None;

        while start_scan.elapsed() < scan_limit {
            let peripherals = adapter.peripherals().await.map_err(|e| {
                LinkError::Transient(format!("failed to list peripherals: {e}"))
            })?;

            for p in peripherals {
                if let Ok(Some(props)) = p.properties().await
                    && let Some(local_name) = props.local_name
                    && local_name.contains(&self.target_name)
                {
                    target_peripheral = Some(p);
                    break;
                }
            }
            if target_peripheral.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        let peripheral = target_peripheral.ok_or_else(|| {
            LinkError::NotFound(format!("BLE peripheral {:?} not found", self.target_name))
        })?;

        peripheral.connect().await.map_err(|e| {
            LinkError::Disconnected(format!("BLE connect failed: {e}"))
        })?;

        peripheral.discover_services().await.map_err(|e| {
            LinkError::Fatal(format!("BLE service discovery failed: {e}"))
        })?;

        let chars = peripheral.characteristics();
        let write_char = chars
            .iter()
            .find(|c| c.uuid == self.write_uuid)
            .cloned()
            .ok_or_else(|| {
                LinkError::NotFound(format!("write characteristic {} not found", self.write_uuid))
            })?;

        let notify_char = chars
            .iter()
            .find(|c| c.uuid == self.notify_uuid)
            .cloned()
            .ok_or_else(|| {
                LinkError::NotFound(format!("notify characteristic {} not found", self.notify_uuid))
            })?;

        peripheral
            .subscribe(&notify_char)
            .await
            .map_err(|e| LinkError::Fatal(format!("failed to subscribe to notification: {e}")))?;

        let mut stream = peripheral
            .notifications()
            .await
            .map_err(|e| LinkError::Fatal(format!("failed to get notification stream: {e}")))?;

        let (tx, rx) = mpsc::channel(128);
        let notify_uuid = self.notify_uuid;

        tokio::spawn(async move {
            while let Some(data) = stream.next().await {
                if data.uuid == notify_uuid && tx.send(data.value).await.is_err() {
                    break;
                }
            }
        });

        Ok(BleLinkInner {
            _adapter: adapter,
            peripheral,
            write_char,
            notify_rx: rx,
            status: LinkStatus::Connected,
        })
    }
}

#[async_trait]
impl DeviceLink for BleLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        let guard = self.inner.try_lock();
        guard.map_or(LinkStatus::Connecting, |g| {
            g.as_ref().map_or(LinkStatus::Disconnected, |i| i.status.clone())
        })
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let (peripheral, write_char) = {
            let guard = self.inner.lock().await;
            let Some(inner) = guard.as_ref() else {
                return Err(LinkError::Disconnected("BLE peripheral not connected".into()));
            };
            let p = inner.peripheral.clone();
            let wc = inner.write_char.clone();
            drop(guard);
            (p, wc)
        };

        for chunk in data.chunks(18) {
            peripheral
                .write(&write_char, chunk, WriteType::WithoutResponse)
                .await
                .map_err(|e| LinkError::Disconnected(format!("BLE write failed: {e}")))?;
        }

        Ok(())
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut guard = self.inner.lock().await;
        let Some(inner) = guard.as_mut() else {
            return Err(LinkError::Disconnected("BLE peripheral not connected".into()));
        };

        let opt_pkt = tokio::time::timeout(timeout, inner.notify_rx.recv()).await;
        drop(guard);

        let opt_pkt = opt_pkt.map_err(|_| LinkError::Timeout(timeout))?;
        opt_pkt.ok_or_else(|| LinkError::Disconnected("BLE notification stream closed".into()))
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        let start_time = Instant::now();
        let mut attempt = 0;

        loop {
            let elapsed = start_time.elapsed();
            let Some(delay) = self.policy.next_attempt(attempt, elapsed) else {
                return Err(LinkError::Disconnected(format!(
                    "BLE reconnect exhausted policy limits after {attempt} attempts ({elapsed:?})"
                )));
            };

            {
                let mut guard = self.inner.lock().await;
                if let Some(inner) = guard.as_mut() {
                    inner.status = LinkStatus::Reconnecting {
                        attempt,
                        next_retry: delay,
                    };
                }
            }

            tokio::time::sleep(delay).await;

            match self.connect_internal().await {
                Ok(new_inner) => {
                    let mut guard = self.inner.lock().await;
                    *guard = Some(new_inner);
                    drop(guard);
                    return Ok(());
                }
                Err(e) if e.is_recoverable() => {
                    attempt += 1;
                    continue;
                }
                Err(fatal) => {
                    let mut guard = self.inner.lock().await;
                    if let Some(inner) = guard.as_mut() {
                        inner.status = LinkStatus::Failed(fatal.to_string());
                    }
                    drop(guard);
                    return Err(fatal);
                }
            }
        }
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut guard = self.inner.lock().await;
        let inner = guard.take();
        drop(guard);

        if let Some(inner) = inner {
            drop(inner.peripheral.disconnect().await);
        }
        Ok(())
    }
}
