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

#[async_trait]
trait BlePeripheralHandle: Send + Sync {
    async fn write_without_response(&self, chunk: &[u8]) -> Result<(), LinkError>;
    async fn disconnect(&self);
}

struct BleConnected {
    peripheral: Arc<dyn BlePeripheralHandle>,
    notify_rx: mpsc::Receiver<Vec<u8>>,
}

#[async_trait]
trait BleBackend: Send + Sync {
    async fn connect(
        &self,
        target_name: &str,
        write_uuid: Uuid,
        notify_uuid: Uuid,
    ) -> Result<BleConnected, LinkError>;
}

struct BtleplugPeripheral {
    _adapter: Adapter,
    peripheral: Peripheral,
    write_char: Characteristic,
}

#[async_trait]
impl BlePeripheralHandle for BtleplugPeripheral {
    async fn write_without_response(&self, chunk: &[u8]) -> Result<(), LinkError> {
        self.peripheral
            .write(&self.write_char, chunk, WriteType::WithoutResponse)
            .await
            .map_err(|e| LinkError::Disconnected(format!("BLE write failed: {e}")))
    }

    async fn disconnect(&self) {
        drop(self.peripheral.disconnect().await);
    }
}

struct BtleplugBackend;

#[async_trait]
impl BleBackend for BtleplugBackend {
    async fn connect(
        &self,
        target_name: &str,
        write_uuid: Uuid,
        notify_uuid: Uuid,
    ) -> Result<BleConnected, LinkError> {
        let manager = Manager::new()
            .await
            .map_err(|e| LinkError::Fatal(format!("BLE Manager initialization failed: {e}")))?;

        let adapters = manager
            .adapters()
            .await
            .map_err(|e| LinkError::Fatal(format!("failed to fetch BLE adapters: {e}")))?;

        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| LinkError::NotFound("no Bluetooth adapters found on system".into()))?;

        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| LinkError::Transient(format!("BLE scan start failed: {e}")))?;

        // Scan for target peripheral with a 3s discovery window
        let start_scan = Instant::now();
        let scan_limit = Duration::from_secs(3);
        let mut target_peripheral: Option<Peripheral> = None;

        while start_scan.elapsed() < scan_limit {
            let peripherals = adapter
                .peripherals()
                .await
                .map_err(|e| LinkError::Transient(format!("failed to list peripherals: {e}")))?;

            for p in peripherals {
                if let Ok(Some(props)) = p.properties().await
                    && let Some(local_name) = props.local_name
                    && local_name.contains(target_name)
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

        let peripheral = target_peripheral
            .ok_or_else(|| LinkError::NotFound(format!("BLE peripheral {target_name:?} not found")))?;

        peripheral
            .connect()
            .await
            .map_err(|e| LinkError::Disconnected(format!("BLE connect failed: {e}")))?;

        peripheral
            .discover_services()
            .await
            .map_err(|e| LinkError::Fatal(format!("BLE service discovery failed: {e}")))?;

        let chars = peripheral.characteristics();
        let write_char = chars
            .iter()
            .find(|c| c.uuid == write_uuid)
            .cloned()
            .ok_or_else(|| {
                LinkError::NotFound(format!("write characteristic {write_uuid} not found"))
            })?;

        let notify_char = chars
            .iter()
            .find(|c| c.uuid == notify_uuid)
            .cloned()
            .ok_or_else(|| {
                LinkError::NotFound(format!("notify characteristic {notify_uuid} not found"))
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

        tokio::spawn(async move {
            while let Some(data) = stream.next().await {
                if data.uuid == notify_uuid && tx.send(data.value).await.is_err() {
                    break;
                }
            }
        });

        Ok(BleConnected {
            peripheral: Arc::new(BtleplugPeripheral {
                _adapter: adapter,
                peripheral,
                write_char,
            }),
            notify_rx: rx,
        })
    }
}

/// Native async Bluetooth Low Energy (BLE) transport link.
pub struct BleLink {
    name: String,
    target_name: String,
    write_uuid: Uuid,
    notify_uuid: Uuid,
    policy: ReconnectPolicy,
    backend: Arc<dyn BleBackend>,
    inner: Arc<Mutex<Option<BleLinkInner>>>,
}

struct BleLinkInner {
    peripheral: Arc<dyn BlePeripheralHandle>,
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
        Self::with_backend(
            target_name,
            write_uuid,
            notify_uuid,
            policy,
            Arc::new(BtleplugBackend),
        )
    }

    fn with_backend(
        target_name: impl Into<String>,
        write_uuid: Uuid,
        notify_uuid: Uuid,
        policy: ReconnectPolicy,
        backend: Arc<dyn BleBackend>,
    ) -> Self {
        let name_str = target_name.into();
        Self {
            name: format!("BLE:{name_str}"),
            target_name: name_str,
            write_uuid,
            notify_uuid,
            policy,
            backend,
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
        Self::new(
            target_name,
            write_uuid,
            notify_uuid,
            ReconnectPolicy::ble_default(),
        )
    }

    async fn connect_internal(&self) -> Result<BleLinkInner, LinkError> {
        let connected = self
            .backend
            .connect(&self.target_name, self.write_uuid, self.notify_uuid)
            .await?;

        Ok(BleLinkInner {
            peripheral: connected.peripheral,
            notify_rx: connected.notify_rx,
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
            g.as_ref()
                .map_or(LinkStatus::Disconnected, |i| i.status.clone())
        })
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let peripheral = self
            .inner
            .lock()
            .await
            .as_ref()
            .map(|inner| Arc::clone(&inner.peripheral))
            .ok_or_else(|| LinkError::Disconnected("BLE peripheral not connected".into()))?;

        for chunk in data.chunks(18) {
            peripheral.write_without_response(chunk).await?;
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
            inner.peripheral.disconnect().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::clients::link::{BackoffStrategy, ReconnectLimit};

    #[derive(Default)]
    struct MockBlePeripheral {
        chunks: StdMutex<Vec<Vec<u8>>>,
        write_error: StdMutex<Option<LinkError>>,
        disconnected: AtomicBool,
    }

    #[async_trait]
    impl BlePeripheralHandle for MockBlePeripheral {
        async fn write_without_response(&self, chunk: &[u8]) -> Result<(), LinkError> {
            let err = self.write_error.lock().unwrap().clone();
            if let Some(err) = err {
                return Err(err);
            }
            self.chunks.lock().unwrap().push(chunk.to_vec());
            Ok(())
        }

        async fn disconnect(&self) {
            self.disconnected.store(true, Ordering::SeqCst);
        }
    }

    type MockConnectResult = Result<(Arc<MockBlePeripheral>, mpsc::Receiver<Vec<u8>>), LinkError>;

    struct MockBleBackend {
        results: StdMutex<VecDeque<MockConnectResult>>,
    }

    #[async_trait]
    impl BleBackend for MockBleBackend {
        async fn connect(
            &self,
            _target_name: &str,
            _write_uuid: Uuid,
            _notify_uuid: Uuid,
        ) -> Result<BleConnected, LinkError> {
            let next = self
                .results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(LinkError::NotFound("no mock peripheral".into())));
            next.map(|(peripheral, notify_rx)| BleConnected {
                peripheral,
                notify_rx,
            })
        }
    }

    fn fast_policy(max_attempts: u32) -> ReconnectPolicy {
        ReconnectPolicy {
            strategy: BackoffStrategy::Constant(Duration::from_millis(1)),
            limits: vec![ReconnectLimit::MaxAttempts(max_attempts)],
        }
    }

    #[tokio::test]
    async fn test_ble_link_chunking_recv_retry_and_close() {
        let peripheral = Arc::new(MockBlePeripheral::default());
        let (tx, rx) = mpsc::channel(8);

        let backend = Arc::new(MockBleBackend {
            results: StdMutex::new(VecDeque::from([
                Err(LinkError::Transient("scan busy".into())),
                Ok((Arc::clone(&peripheral), rx)),
            ])),
        });

        let link = BleLink::with_backend(
            "RadiaCode-102",
            Uuid::nil(),
            Uuid::nil(),
            fast_policy(3),
            backend,
        );

        assert_eq!(link.address_name(), "BLE:RadiaCode-102");
        assert_eq!(link.status(), LinkStatus::Disconnected);
        assert_eq!(link.reconnect_policy(), &fast_policy(3));

        // Disconnected send/recv fail
        assert!(matches!(link.send(b"abc").await, Err(LinkError::Disconnected(_))));
        assert!(matches!(
            link.recv(Duration::from_millis(10)).await,
            Err(LinkError::Disconnected(_))
        ));

        // Reconnect retries transient failure and succeeds
        link.reconnect().await.unwrap();
        assert_eq!(link.status(), LinkStatus::Connected);

        // Send >18 bytes -> splits into 18-byte BLE GATT chunks
        let payload: Vec<u8> = (0..40).collect();
        link.send(&payload).await.unwrap();
        let chunks = peripheral.chunks.lock().unwrap().clone();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 18);
        assert_eq!(chunks[1].len(), 18);
        assert_eq!(chunks[2].len(), 4);

        // Recv notification, timeout, and closed stream
        tx.send(vec![9, 8, 7]).await.unwrap();
        assert_eq!(
            link.recv(Duration::from_millis(50)).await.unwrap(),
            vec![9, 8, 7]
        );
        assert!(matches!(
            link.recv(Duration::from_millis(10)).await,
            Err(LinkError::Timeout(_))
        ));
        drop(tx);
        assert!(matches!(
            link.recv(Duration::from_millis(50)).await,
            Err(LinkError::Disconnected(_))
        ));

        // Write error propagates
        *peripheral.write_error.lock().unwrap() = Some(LinkError::Disconnected("gatt drop".into()));
        assert!(matches!(link.send(b"x").await, Err(LinkError::Disconnected(_))));

        // Close disconnects peripheral
        link.close().await.unwrap();
        assert!(peripheral.disconnected.load(Ordering::SeqCst));
        assert_eq!(link.status(), LinkStatus::Disconnected);
    }

    #[tokio::test]
    async fn test_ble_link_reconnect_fatal_and_exhaustion() {
        // 1. Fatal error stops immediately and records Failed status when inner exists
        let peripheral = Arc::new(MockBlePeripheral::default());
        let (_tx, rx) = mpsc::channel(4);
        let backend = Arc::new(MockBleBackend {
            results: StdMutex::new(VecDeque::from([
                Ok((peripheral, rx)),
                Err(LinkError::Fatal("no permissions".into())),
            ])),
        });

        let link = BleLink::with_backend(
            "RaySID",
            Uuid::nil(),
            Uuid::nil(),
            fast_policy(3),
            backend,
        );
        link.reconnect().await.unwrap();
        let fatal = link.reconnect().await.expect_err("expected Fatal");
        assert!(matches!(fatal, LinkError::Fatal(_)));
        assert!(matches!(link.status(), LinkStatus::Failed(_)));

        // 2. Policy exhaustion returns Disconnected
        let exhausted_backend = Arc::new(MockBleBackend {
            results: StdMutex::new(VecDeque::from([
                Err(LinkError::Transient("t1".into())),
                Err(LinkError::Transient("t2".into())),
            ])),
        });
        let exhausted_link = BleLink::with_backend(
            "RaySID",
            Uuid::nil(),
            Uuid::nil(),
            fast_policy(1),
            exhausted_backend,
        );
        let err = exhausted_link
            .reconnect()
            .await
            .expect_err("expected Disconnected");
        assert!(matches!(err, LinkError::Disconnected(_)));
    }
}
