//! Device link layer: hardware transport abstraction and reconnection engine.
//!
//! Separates low-level protocol encoders from physical I/O over USB, BLE, and Network.

pub mod ble;
pub mod mock;
pub mod policy;
pub mod tcp;
pub mod usb;

use std::time::Duration;
use async_trait::async_trait;
use thiserror::Error;

pub use policy::{BackoffStrategy, ReconnectLimit, ReconnectPolicy};
pub use mock::MockLink;
pub use usb::UsbLink;
pub use ble::BleLink;
pub use tcp::TcpLink;

/// Current state of the physical communication link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    /// Actively connected and ready for I/O.
    Connected,
    /// Establishing initial connection.
    Connecting,
    /// Actively attempting reconnection after link drop.
    Reconnecting {
        attempt: u32,
        next_retry: Duration,
    },
    /// Link cleanly closed / stopped.
    Disconnected,
    /// Unrecoverable failure.
    Failed(String),
}

/// Errors occurring at the link / transport level.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("timeout waiting for response after {0:?}")]
    Timeout(Duration),

    #[error("transient transport I/O error: {0}")]
    Transient(String),

    #[error("connection lost / disconnected: {0}")]
    Disconnected(String),

    #[error("device not found on bus: {0}")]
    NotFound(String),

    #[error("permission denied accessing device: {0}")]
    PermissionDenied(String),

    #[error("fatal unrecoverable transport failure: {0}")]
    Fatal(String),
}

impl LinkError {
    /// True if an automatic reconnect or retry can potentially resolve the problem.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Timeout(_) | Self::Transient(_) | Self::Disconnected(_))
    }
}

/// Unified interface for physical communication channels (USB, BLE, Network, Mock).
#[async_trait]
pub trait DeviceLink: Send + Sync {
    /// Human-readable descriptor of the link (e.g. `"USB:0483:f123:12345"`, `"BLE:Radiacode-1234"`).
    fn address_name(&self) -> &str;

    /// Current connection status.
    fn status(&self) -> LinkStatus;

    /// Transport's reconnection policy.
    fn reconnect_policy(&self) -> &ReconnectPolicy;

    /// Send raw bytes to the device.
    async fn send(&self, data: &[u8]) -> Result<(), LinkError>;

    /// Receive raw bytes from the device with a timeout.
    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError>;

    /// Half-duplex transaction: send request and await response.
    async fn transaction(&self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, LinkError> {
        self.send(request).await?;
        self.recv(timeout).await
    }

    /// Explicitly reconnect the link according to policy.
    async fn reconnect(&self) -> Result<(), LinkError>;

    /// Close and release the underlying device resources.
    async fn close(&self) -> Result<(), LinkError>;
}
