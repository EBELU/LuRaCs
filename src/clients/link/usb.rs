//! USB transport link implemented using native async `nusb`.
//!
//! Fail-fast without reconnection loops (as requested: reconnection policy is
//! specifically for Bluetooth / wireless carriers).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nusb::transfer::{Bulk, In, Out};
use nusb::{Device, DeviceInfo, Interface, MaybeFuture};

use super::{DeviceLink, LinkError, LinkStatus, ReconnectPolicy};

/// Native async USB transport link.
pub struct UsbLink {
    name: String,
    in_endpoint: u8,
    out_endpoint: u8,
    policy: ReconnectPolicy,
    inner: Arc<Mutex<Option<UsbLinkInner>>>,
}

struct UsbLinkInner {
    _device: Device,
    interface: Interface,
}

impl UsbLink {
    /// Open a connection to a matching USB device asynchronously.
    pub async fn open(
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
        interface_number: u8,
        in_endpoint: u8,
        out_endpoint: u8,
    ) -> Result<Self, LinkError> {
        let name = serial.map_or_else(
            || format!("USB:{vendor_id:04x}:{product_id:04x}"),
            |s| format!("USB:{vendor_id:04x}:{product_id:04x}:{s}"),
        );

        let dev_info = Self::find_device(vendor_id, product_id, serial).await?;
        let device = dev_info.open().await.map_err(|e| {
            LinkError::Fatal(format!("failed to open USB device {name}: {e}"))
        })?;

        let interface = device
            .detach_and_claim_interface(interface_number)
            .await
            .map_err(|e| {
                LinkError::PermissionDenied(format!(
                    "failed to claim interface {interface_number}: {e}"
                ))
            })?;

        Ok(Self {
            name,
            in_endpoint,
            out_endpoint,
            policy: ReconnectPolicy::never(),
            inner: Arc::new(Mutex::new(Some(UsbLinkInner {
                _device: device,
                interface,
            }))),
        })
    }

    async fn find_device(
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
    ) -> Result<DeviceInfo, LinkError> {
        let list = nusb::list_devices().await.map_err(|e| {
            LinkError::Fatal(format!("failed to enumerate USB devices: {e}"))
        })?;

        for dev in list {
            if dev.vendor_id() == vendor_id && dev.product_id() == product_id {
                if let Some(expected_serial) = serial {
                    if dev.serial_number() == Some(expected_serial) {
                        return Ok(dev);
                    }
                } else {
                    return Ok(dev);
                }
            }
        }

        Err(LinkError::NotFound(format!(
            "USB device {vendor_id:04x}:{product_id:04x} not found"
        )))
    }
}

#[async_trait]
impl DeviceLink for UsbLink {
    fn address_name(&self) -> &str {
        &self.name
    }

    fn status(&self) -> LinkStatus {
        let guard = self.inner.lock().unwrap();
        if guard.is_some() {
            LinkStatus::Connected
        } else {
            LinkStatus::Disconnected
        }
    }

    fn reconnect_policy(&self) -> &ReconnectPolicy {
        &self.policy
    }

    async fn send(&self, data: &[u8]) -> Result<(), LinkError> {
        let interface = {
            let guard = self.inner.lock().unwrap();
            let Some(inner) = guard.as_ref() else {
                return Err(LinkError::Disconnected("USB device not connected".into()));
            };
            let iface = inner.interface.clone();
            drop(guard);
            iface
        };

        let mut ep = interface
            .endpoint::<Bulk, Out>(self.out_endpoint)
            .map_err(|e| LinkError::Fatal(format!("failed to open bulk OUT endpoint: {e}")))?;

        let mut buf = ep.allocate(data.len());
        buf.extend_from_slice(data);
        ep.submit(buf);

        let completion = ep.next_complete().await;
        completion.status.map_err(|e| {
            LinkError::Disconnected(format!("USB write transfer failed: {e}"))
        })?;

        Ok(())
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let interface = {
            let guard = self.inner.lock().unwrap();
            let Some(inner) = guard.as_ref() else {
                return Err(LinkError::Disconnected("USB device not connected".into()));
            };
            let iface = inner.interface.clone();
            drop(guard);
            iface
        };

        let mut ep = interface
            .endpoint::<Bulk, In>(self.in_endpoint)
            .map_err(|e| LinkError::Fatal(format!("failed to open bulk IN endpoint: {e}")))?;

        let buf = ep.allocate(4096);
        ep.submit(buf);

        let completion = tokio::time::timeout(timeout, ep.next_complete())
            .await
            .map_err(|_| LinkError::Timeout(timeout))?;

        completion.status.map_err(|e| {
            LinkError::Disconnected(format!("USB read transfer failed: {e}"))
        })?;

        Ok(completion.buffer[..completion.actual_len].to_vec())
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        // USB is fail-fast by design (reconnection policy is for BT/wireless)
        Err(LinkError::Fatal("USB link does not support auto-reconnection".into()))
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut guard = self.inner.lock().unwrap();
        *guard = None;
        drop(guard);
        Ok(())
    }
}
