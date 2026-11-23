//! ORTEC/AMETEK digiBase USB radiation detector client.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use driver::DigiBaseDriver;
pub use protocol::{
    ExtGateMode, StatusRegister, PRODUCT_ID_RH, RH_IN_ENDPOINT, RH_OUT_ENDPOINT, STD_IN_ENDPOINT,
    STD_OUT_ENDPOINT, VENDOR_ID,
};

use crate::clients::driver::{DeviceDriver, DeviceError};
use crate::clients::link::{DeviceLink, UsbLink};

/// Connect to an ORTEC digiBase or digiBase-RH over native async USB.
pub async fn open_usb(
    product_id: u16,
    serial: Option<&str>,
) -> Result<DigiBaseDriver, DeviceError> {
    let (out_ep, in_ep) = if product_id == PRODUCT_ID_RH {
        (RH_OUT_ENDPOINT, RH_IN_ENDPOINT)
    } else {
        (STD_OUT_ENDPOINT, STD_IN_ENDPOINT)
    };

    let link = UsbLink::open(VENDOR_ID, product_id, serial, 0, in_ep, out_ep).await?;
    let name = serial.map_or_else(|| "digiBase".to_string(), |s| format!("digiBase-{s}"));
    let driver = DigiBaseDriver::new(Arc::new(link), name);
    driver.on_reconnect().await?;
    Ok(driver)
}

/// Construct a `DigiBaseDriver` from any existing `DeviceLink` (e.g. `MockLink`).
#[must_use]
pub fn from_link(link: Arc<dyn DeviceLink>, name: impl Into<String>) -> DigiBaseDriver {
    DigiBaseDriver::new(link, name)
}
