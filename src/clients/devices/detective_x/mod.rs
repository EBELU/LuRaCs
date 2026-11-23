//! ORTEC Detective X network detector client.
//!
//! Communicates via the Detective X HTTP/JSON REST API using `reqwest`.

pub mod driver;
pub mod protocol;
#[cfg(test)]
mod tests;

pub use driver::{DetectiveXDriver, HttpLink};
pub use protocol::DEFAULT_PORT;

use crate::clients::driver::{DeviceDriver, DeviceError};

/// Normalize a user-supplied address (e.g. `"192.168.1.10"`, `"localhost:8000"`,
/// or `"http://192.168.1.10:8080"`) into a base URL (`"http://..."`).
#[must_use]
pub fn normalize_base_url(address: &str) -> String {
    let trimmed = address.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

/// Connect to an ORTEC Detective X instrument over HTTP.
pub async fn connect_network(
    address: &str,
    use_gps: bool,
) -> Result<DetectiveXDriver, DeviceError> {
    let base_url = normalize_base_url(address);
    let name = format!("DetectiveX_{address}");
    let driver = DetectiveXDriver::new(base_url, name, use_gps)?;
    driver.on_reconnect().await?;
    Ok(driver)
}
