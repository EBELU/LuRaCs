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

#[async_trait]
trait UsbInterfaceHandle: Send + Sync {
    async fn bulk_out(&self, endpoint: u8, data: &[u8]) -> Result<(), LinkError>;
    async fn bulk_in(&self, endpoint: u8, max_len: usize, timeout: Duration)
        -> Result<Vec<u8>, LinkError>;
}

#[async_trait]
trait UsbBackend: Send + Sync {
    async fn open_interface(
        &self,
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
        interface_number: u8,
        name: &str,
    ) -> Result<Arc<dyn UsbInterfaceHandle>, LinkError>;
}

struct NusbInterfaceHandle {
    _device: Device,
    interface: Interface,
}

#[async_trait]
impl UsbInterfaceHandle for NusbInterfaceHandle {
    async fn bulk_out(&self, endpoint: u8, data: &[u8]) -> Result<(), LinkError> {
        let mut ep = self
            .interface
            .endpoint::<Bulk, Out>(endpoint)
            .map_err(|e| LinkError::Fatal(format!("failed to open bulk OUT endpoint: {e}")))?;

        let mut buf = ep.allocate(data.len());
        buf.extend_from_slice(data);
        ep.submit(buf);

        let completion = ep.next_complete().await;
        completion
            .status
            .map_err(|e| LinkError::Disconnected(format!("USB write transfer failed: {e}")))?;

        Ok(())
    }

    async fn bulk_in(
        &self,
        endpoint: u8,
        max_len: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, LinkError> {
        let mut ep = self
            .interface
            .endpoint::<Bulk, In>(endpoint)
            .map_err(|e| LinkError::Fatal(format!("failed to open bulk IN endpoint: {e}")))?;

        let buf = ep.allocate(max_len);
        ep.submit(buf);

        let completion = tokio::time::timeout(timeout, ep.next_complete())
            .await
            .map_err(|_| LinkError::Timeout(timeout))?;

        completion
            .status
            .map_err(|e| LinkError::Disconnected(format!("USB read transfer failed: {e}")))?;

        Ok(completion.buffer[..completion.actual_len].to_vec())
    }
}

struct NusbBackend;

#[async_trait]
impl UsbBackend for NusbBackend {
    async fn open_interface(
        &self,
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
        interface_number: u8,
        name: &str,
    ) -> Result<Arc<dyn UsbInterfaceHandle>, LinkError> {
        let dev_info = UsbLink::find_device(vendor_id, product_id, serial).await?;
        let device = dev_info
            .open()
            .await
            .map_err(|e| LinkError::Fatal(format!("failed to open USB device {name}: {e}")))?;

        let interface = device
            .detach_and_claim_interface(interface_number)
            .await
            .map_err(|e| {
                LinkError::PermissionDenied(format!(
                    "failed to claim interface {interface_number}: {e}"
                ))
            })?;

        Ok(Arc::new(NusbInterfaceHandle {
            _device: device,
            interface,
        }))
    }
}

#[must_use]
fn matches_usb_device(
    dev_vendor_id: u16,
    dev_product_id: u16,
    dev_serial: Option<&str>,
    vendor_id: u16,
    product_id: u16,
    serial: Option<&str>,
) -> bool {
    if dev_vendor_id != vendor_id || dev_product_id != product_id {
        return false;
    }
    serial.is_none_or(|expected| dev_serial == Some(expected))
}

/// Native async USB transport link.
pub struct UsbLink {
    name: String,
    in_endpoint: u8,
    out_endpoint: u8,
    policy: ReconnectPolicy,
    inner: Arc<Mutex<Option<Arc<dyn UsbInterfaceHandle>>>>,
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
        Self::open_with_backend(
            &NusbBackend,
            vendor_id,
            product_id,
            serial,
            interface_number,
            in_endpoint,
            out_endpoint,
        )
        .await
    }

    async fn open_with_backend(
        backend: &dyn UsbBackend,
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

        let interface = backend
            .open_interface(vendor_id, product_id, serial, interface_number, &name)
            .await?;

        Ok(Self {
            name,
            in_endpoint,
            out_endpoint,
            policy: ReconnectPolicy::never(),
            inner: Arc::new(Mutex::new(Some(interface))),
        })
    }

    async fn find_device(
        vendor_id: u16,
        product_id: u16,
        serial: Option<&str>,
    ) -> Result<DeviceInfo, LinkError> {
        let list = nusb::list_devices()
            .await
            .map_err(|e| LinkError::Fatal(format!("failed to enumerate USB devices: {e}")))?;

        for dev in list {
            if matches_usb_device(
                dev.vendor_id(),
                dev.product_id(),
                dev.serial_number(),
                vendor_id,
                product_id,
                serial,
            ) {
                return Ok(dev);
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
        let interface = self
            .inner
            .lock()
            .unwrap()
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| LinkError::Disconnected("USB device not connected".into()))?;

        interface.bulk_out(self.out_endpoint, data).await
    }

    async fn recv(&self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let interface = self
            .inner
            .lock()
            .unwrap()
            .as_ref()
            .map(Arc::clone)
            .ok_or_else(|| LinkError::Disconnected("USB device not connected".into()))?;

        interface.bulk_in(self.in_endpoint, 4096, timeout).await
    }

    async fn reconnect(&self) -> Result<(), LinkError> {
        // USB is fail-fast by design (reconnection policy is for BT/wireless)
        Err(LinkError::Fatal(
            "USB link does not support auto-reconnection".into(),
        ))
    }

    async fn close(&self) -> Result<(), LinkError> {
        let mut guard = self.inner.lock().unwrap();
        *guard = None;
        drop(guard);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Default)]
    struct MockUsbInterface {
        sent: Mutex<Vec<(u8, Vec<u8>)>>,
        rx_queue: Mutex<VecDeque<Result<Vec<u8>, LinkError>>>,
        out_error: Mutex<Option<LinkError>>,
    }

    #[async_trait]
    impl UsbInterfaceHandle for MockUsbInterface {
        async fn bulk_out(&self, endpoint: u8, data: &[u8]) -> Result<(), LinkError> {
            let err = self.out_error.lock().unwrap().clone();
            if let Some(err) = err {
                return Err(err);
            }
            self.sent.lock().unwrap().push((endpoint, data.to_vec()));
            Ok(())
        }

        async fn bulk_in(
            &self,
            _endpoint: u8,
            max_len: usize,
            timeout: Duration,
        ) -> Result<Vec<u8>, LinkError> {
            let Some(next) = self.rx_queue.lock().unwrap().pop_front() else {
                return Err(LinkError::Timeout(timeout));
            };
            next.map(|mut data| {
                data.truncate(max_len);
                data
            })
        }
    }

    struct MockUsbBackend {
        devices: Vec<(u16, u16, Option<String>)>,
        interface: Arc<MockUsbInterface>,
        open_error: Option<LinkError>,
    }

    #[async_trait]
    impl UsbBackend for MockUsbBackend {
        async fn open_interface(
            &self,
            vendor_id: u16,
            product_id: u16,
            serial: Option<&str>,
            _interface_number: u8,
            _name: &str,
        ) -> Result<Arc<dyn UsbInterfaceHandle>, LinkError> {
            if let Some(err) = &self.open_error {
                return Err(err.clone());
            }
            let found = self.devices.iter().any(|(vid, pid, ser)| {
                matches_usb_device(*vid, *pid, ser.as_deref(), vendor_id, product_id, serial)
            });
            if !found {
                return Err(LinkError::NotFound(format!(
                    "USB device {vendor_id:04x}:{product_id:04x} not found"
                )));
            }
            let cloned = Arc::clone(&self.interface);
            let iface: Arc<dyn UsbInterfaceHandle> = cloned;
            Ok(iface)
        }
    }

    #[test]
    fn test_matches_usb_device() {
        assert!(matches_usb_device(0x0483, 0xf123, Some("ABC"), 0x0483, 0xf123, None));
        assert!(matches_usb_device(0x0483, 0xf123, Some("ABC"), 0x0483, 0xf123, Some("ABC")));
        assert!(!matches_usb_device(0x0483, 0xf123, Some("ABC"), 0x0483, 0xf123, Some("XYZ")));
        assert!(!matches_usb_device(0x0483, 0xf123, None, 0x0483, 0xf123, Some("ABC")));
        assert!(!matches_usb_device(0x1111, 0xf123, None, 0x0483, 0xf123, None));
        assert!(!matches_usb_device(0x0483, 0x2222, None, 0x0483, 0xf123, None));
    }

    #[tokio::test]
    async fn test_usb_link_mock_lifecycle_and_errors() {
        let iface = Arc::new(MockUsbInterface::default());
        iface
            .rx_queue
            .lock()
            .unwrap()
            .push_back(Ok(vec![1, 2, 3, 4]));
        iface
            .rx_queue
            .lock()
            .unwrap()
            .push_back(Err(LinkError::Disconnected("unplugged".into())));

        let backend = MockUsbBackend {
            devices: vec![(0x0483, 0xf123, Some("RC-01".into()))],
            interface: Arc::clone(&iface),
            open_error: None,
        };

        // Missing device -> NotFound
        let Err(err) =
            UsbLink::open_with_backend(&backend, 0x0483, 0xf123, Some("WRONG"), 0, 0x81, 0x01)
                .await
        else {
            panic!("expected NotFound");
        };
        assert!(matches!(err, LinkError::NotFound(_)));

        // Open with serial
        let link = UsbLink::open_with_backend(&backend, 0x0483, 0xf123, Some("RC-01"), 0, 0x81, 0x01)
            .await
            .unwrap();
        assert_eq!(link.address_name(), "USB:0483:f123:RC-01");
        assert_eq!(link.status(), LinkStatus::Connected);
        assert_eq!(link.reconnect_policy(), &ReconnectPolicy::never());

        // Send and recv
        link.send(b"hello").await.unwrap();
        assert_eq!(*iface.sent.lock().unwrap(), vec![(0x01, b"hello".to_vec())]);
        assert_eq!(
            link.recv(Duration::from_millis(50)).await.unwrap(),
            vec![1, 2, 3, 4]
        );

        // Recv error and timeout
        assert!(matches!(
            link.recv(Duration::from_millis(50)).await,
            Err(LinkError::Disconnected(_))
        ));
        assert!(matches!(
            link.recv(Duration::from_millis(10)).await,
            Err(LinkError::Timeout(_))
        ));

        // Send error
        *iface.out_error.lock().unwrap() = Some(LinkError::Disconnected("pipe broken".into()));
        assert!(matches!(link.send(b"x").await, Err(LinkError::Disconnected(_))));

        // Reconnect is fail-fast Fatal
        assert!(matches!(link.reconnect().await, Err(LinkError::Fatal(_))));

        // Close transitions to Disconnected and rejects send/recv
        link.close().await.unwrap();
        assert_eq!(link.status(), LinkStatus::Disconnected);
        assert!(matches!(link.send(b"x").await, Err(LinkError::Disconnected(_))));
        assert!(matches!(
            link.recv(Duration::from_millis(10)).await,
            Err(LinkError::Disconnected(_))
        ));

        // PermissionDenied passthrough from backend
        let denied_backend = MockUsbBackend {
            devices: vec![],
            interface: Arc::clone(&iface),
            open_error: Some(LinkError::PermissionDenied("udev".into())),
        };
        let Err(link_err) =
            UsbLink::open_with_backend(&denied_backend, 0x0483, 0xf123, None, 0, 0x81, 0x01).await
        else {
            panic!("expected PermissionDenied");
        };
        assert!(matches!(link_err, LinkError::PermissionDenied(_)));
    }
}
