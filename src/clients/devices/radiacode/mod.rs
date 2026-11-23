//! Radiacode radiation detector client (101 / 102 / 103).
//!
//! Provides native async USB and BLE connectivity, protocol encoding/decoding,
//! spectrum decompression, and fault-isolated driver execution.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use driver::RadiacodeDriver;
pub use protocol::{
    BLE_NOTIFY_UUID, BLE_WRITE_UUID, Command, DecodedSpectrum, PRODUCT_ID, USB_IN_ENDPOINT,
    USB_INTERFACE, USB_OUT_ENDPOINT, VENDOR_ID, VirtualString,
};

use crate::clients::driver::{DeviceDriver, DeviceError};
use crate::clients::link::{BleLink, DeviceLink, UsbLink};

/// Connect to a RadiaCode device over native async USB.
pub async fn open_usb(serial: Option<&str>) -> Result<RadiacodeDriver, DeviceError> {
    let link = UsbLink::open(
        VENDOR_ID,
        PRODUCT_ID,
        serial,
        USB_INTERFACE,
        USB_IN_ENDPOINT,
        USB_OUT_ENDPOINT,
    )
    .await?;

    let name = serial.map_or_else(
        || "RadiaCode USB".to_string(),
        |s| format!("RadiaCode ({s})"),
    );
    let driver = RadiacodeDriver::new(Arc::new(link), name);
    driver.on_reconnect().await?;
    Ok(driver)
}

/// Create a RadiaCode driver for Bluetooth Low Energy (BLE) communication.
///
/// Connection and reconnection backoff are managed automatically by the underlying `BleLink`.
#[must_use]
pub fn open_ble(target_name: impl Into<String>) -> RadiacodeDriver {
    let name_str = target_name.into();
    let link = BleLink::with_default_policy(&name_str, BLE_WRITE_UUID, BLE_NOTIFY_UUID);
    RadiacodeDriver::new(Arc::new(link), name_str)
}

/// Construct a `RadiacodeDriver` from any existing `DeviceLink` (e.g. `MockLink`).
#[must_use]
pub fn from_link(link: Arc<dyn DeviceLink>, name: impl Into<String>) -> RadiacodeDriver {
    RadiacodeDriver::new(link, name)
}
