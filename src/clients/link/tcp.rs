//! TCP transport link for network-connected spectrometers (e.g. Detective X, IdentiFinder).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use super::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

/// Network TCP transport link.
pub struct TcpLink {
    name: String,
    host: String,
    port: u16,
    policy: ReconnectPolicy,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpLink {
    /// Connect to a spectrometer over TCP.
    pub async fn connect(host: impl Into<String>, port: u16) -> Result<Self, LinkError> {
        let host_str = host.into();
        let name = format!("NET:{host_str}:{port}");
        let addr = format!("{host_str}:{port}");

        let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr))
            .await
            .map_err(|_| LinkError::Timeout(Duration::from_secs(3)))?
            .map_err(|e| LinkError::Disconnected(format!("TCP connect to {addr} failed: {e}")))?;

        Ok(Self {
            name,
            host: host_str,
            port,
            policy: ReconnectPolicy::never(),
            stream: Arc::new(Mutex::new(Some(stream))),
        })
    }
}

#[async_trait]
impl DeviceLink for TcpLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        // Simple non-blocking check
        LinkStatus::Connected
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| LinkError::Disconnected("TCP socket not open".into()))?;

        let res = stream
            .write_all(data)
            .await
            .map_err(|e| LinkError::Disconnected(format!("TCP write failed: {e}")));
        drop(guard);
        res
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut guard = self.stream.lock().await;
        let stream = guard
            .as_mut()
            .ok_or_else(|| LinkError::Disconnected("TCP socket not open".into()))?;

        let mut buf = vec![0u8; 4096];
        let read_res = tokio::time::timeout(timeout, stream.read(&mut buf)).await;
        drop(guard);

        let n = read_res
            .map_err(|_| LinkError::Timeout(timeout))?
            .map_err(|e| LinkError::Disconnected(format!("TCP read failed: {e}")))?;

        if n == 0 {
            return Err(LinkError::Disconnected("TCP connection closed by peer".into()));
        }

        buf.truncate(n);
        Ok(buf)
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&addr))
            .await
            .map_err(|_| LinkError::Timeout(Duration::from_secs(3)))?
            .map_err(|e| LinkError::Disconnected(format!("TCP reconnect failed: {e}")))?;

        let mut guard = self.stream.lock().await;
        *guard = Some(stream);
        drop(guard);
        Ok(())
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut guard = self.stream.lock().await;
        *guard = None;
        drop(guard);
        Ok(())
    }
}
