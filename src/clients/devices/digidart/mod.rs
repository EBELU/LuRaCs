//! ORTEC digiDART USB radiation detector client.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use driver::DigiDartDriver;
pub use protocol::{EP_IN, EP_OUT, PRODUCT_ID, VENDOR_ID};

use crate::clients::driver::{DeviceDriver, DeviceError};
use crate::clients::link::{DeviceLink, UsbLink};

/// Connect to an ORTEC digiDART over native async USB.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn open_usb(serial: Option<&str>) -> Result<DigiDartDriver, DeviceError> {
    let link = UsbLink::open(VENDOR_ID, PRODUCT_ID, serial, 0, EP_IN, EP_OUT).await?;
    let name = serial.map_or_else(|| "digiDART".to_string(), |s| format!("digiDART-{s}"));
    let driver = DigiDartDriver::new(Arc::new(link), name);
    driver.on_reconnect().await?;
    Ok(driver)
}

/// Construct a `DigiDartDriver` from any existing `DeviceLink` (e.g. `MockLink`).
#[must_use]
pub fn from_link(link: Arc<dyn DeviceLink>, name: impl Into<String>) -> DigiDartDriver {
    DigiDartDriver::new(link, name)
}

#[cfg(test)]
mod mod_tests {
    use super::*;
    use crate::clients::link::MockLink;

    #[test]
    fn test_digidart_from_link() {
        let link = Arc::new(MockLink::usb_mock("USB:digiDART"));
        let drv = from_link(link, "digiDART-test");
        assert_eq!(drv.info().name, "digiDART-test");
    }
}
