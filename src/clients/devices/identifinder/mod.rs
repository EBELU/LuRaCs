//! IdentiFINDER R400 network detector client.
//!
//! Communicates via `/virtualPha/api/` HTTP commands and `/virtualPha/signalR`
//! SSE streaming using `reqwest`.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

pub use driver::IdentifinderDriver;

use crate::clients::devices::detective_x::normalize_base_url;
use crate::clients::driver::{DeviceDriver, DeviceError};

/// Connect to an IdentiFINDER R400 over HTTP, starting the live SignalR stream and acquisition.
pub async fn connect_network(address: &str) -> Result<IdentifinderDriver, DeviceError> {
    let base_url = normalize_base_url(address);
    let name = format!("R400_{address}");
    let driver = IdentifinderDriver::new(base_url, name)?;
    driver.on_reconnect().await?;
    Ok(driver)
}
