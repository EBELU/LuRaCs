//! RaySID BLE gamma spectrometer client.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use driver::RaysidDriver;
pub use protocol::{EnergyRange, NUM_CHANNELS, RX_UUID, SERVICE_UUID, TX_UUID};

use crate::clients::link::{BleLink, DeviceLink};

/// Create a RaySID driver over Bluetooth Low Energy (`BleLink` with default exponential backoff).
#[must_use]
pub fn open_ble(target_name: impl Into<String>) -> RaysidDriver {
    let name = target_name.into();
    let link = BleLink::with_default_policy(&name, TX_UUID, RX_UUID);
    RaysidDriver::new(Arc::new(link), name)
}

/// Construct a `RaysidDriver` from any existing `DeviceLink` (e.g. `MockLink`).
#[must_use]
pub fn from_link(link: Arc<dyn DeviceLink>, name: impl Into<String>) -> RaysidDriver {
    RaysidDriver::new(link, name)
}

#[cfg(test)]
mod mod_tests {
    use super::*;
    use crate::clients::driver::DeviceDriver;
    use crate::clients::link::MockLink;

    #[test]
    fn test_raysid_from_link_and_open_ble() {
        let link = Arc::new(MockLink::ble_mock("BLE:RaySID"));
        let drv = from_link(link, "RaySID-test");
        assert_eq!(drv.info().name, "RaySID-test");

        let ble_drv = open_ble("RaySID-1");
        assert_eq!(ble_drv.info().name, "RaySID-1");
    }
}
