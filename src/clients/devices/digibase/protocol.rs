//! Low-level ORTEC/AMETEK digiBase protocol framing and 640-bit status register.
//!
//! Ported from `luracs/clients/digibase_client/digibase.py`.

use std::time::{Duration, Instant};

use crate::clients::driver::DeviceError;
use crate::clients::link::{DeviceLink, LinkError};

/// USB Vendor ID for ORTEC/AMETEK instruments.
pub const VENDOR_ID: u16 = 0x0A2D;
/// USB Product ID for digiBase-RH.
pub const PRODUCT_ID_RH: u16 = 0x001F;
/// Standard USB endpoints for digiBase (non-RH).
pub const STD_OUT_ENDPOINT: u8 = 0x02;
pub const STD_IN_ENDPOINT: u8 = 0x82;
/// Normal-operation USB endpoints for digiBase-RH.
pub const RH_OUT_ENDPOINT: u8 = 0x08;
pub const RH_IN_ENDPOINT: u8 = 0x82;

pub const NUM_CHANNELS: usize = 1024;
pub const STATUS_REG_BYTES: usize = 80;
pub const SPECTRUM_BYTES: usize = NUM_CHANNELS * 4;

/// External gate mode stored in bits `56..64` of the status register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExtGateMode {
    Off = 0,
    Coincidence = 1,
    Enabled = 3,
}

/// 640-bit (80-byte, little-endian) digiBase hardware control/status register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRegister {
    bytes: [u8; STATUS_REG_BYTES],
}

impl Default for StatusRegister {
    fn default() -> Self {
        Self {
            bytes: [0u8; STATUS_REG_BYTES],
        }
    }
}

impl StatusRegister {
    /// Construct from an 80-byte little-endian slice.
    pub fn from_slice(data: &[u8]) -> Result<Self, DeviceError> {
        if data.len() < STATUS_REG_BYTES {
            return Err(DeviceError::Protocol(format!(
                "digiBase status register too short: expected {STATUS_REG_BYTES} bytes, got {}",
                data.len()
            )));
        }
        let mut bytes = [0u8; STATUS_REG_BYTES];
        bytes.copy_from_slice(&data[..STATUS_REG_BYTES]);
        Ok(Self { bytes })
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; STATUS_REG_BYTES] {
        &self.bytes
    }

    /// Read a single bit at `idx` (`0..640`).
    #[must_use]
    pub const fn get_bit(&self, idx: usize) -> bool {
        debug_assert!(idx < 640);
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;
        ((self.bytes[byte_idx] >> bit_idx) & 1) != 0
    }

    /// Set a single bit at `idx` (`0..640`).
    pub const fn set_bit(&mut self, idx: usize, val: bool) {
        debug_assert!(idx < 640);
        let byte_idx = idx / 8;
        let bit_idx = idx % 8;
        if val {
            self.bytes[byte_idx] |= 1 << bit_idx;
        } else {
            self.bytes[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Read an unsigned integer from bit range `[start..stop)` (up to 32 bits).
    #[must_use]
    pub fn get_bits(&self, start: usize, stop: usize) -> u32 {
        debug_assert!(start < stop && stop <= 640 && (stop - start) <= 32);
        let mut out = 0u32;
        for (offset, idx) in (start..stop).enumerate() {
            if self.get_bit(idx) {
                out |= 1u32 << offset;
            }
        }
        out
    }

    /// Write an unsigned integer into bit range `[start..stop)` (up to 32 bits).
    pub fn set_bits(&mut self, start: usize, stop: usize, val: u32) {
        debug_assert!(start < stop && stop <= 640 && (stop - start) <= 32);
        for (offset, idx) in (start..stop).enumerate() {
            self.set_bit(idx, ((val >> offset) & 1) != 0);
        }
    }

    // ---- Typed register fields ----

    #[must_use]
    pub const fn acq_running(&self) -> bool {
        self.get_bit(1)
    }

    pub const fn set_acq_running(&mut self, running: bool) {
        self.set_bit(1, running);
    }

    #[must_use]
    pub const fn hv_enabled(&self) -> bool {
        self.get_bit(6)
    }

    pub const fn set_hv_enabled(&mut self, enabled: bool) {
        self.set_bit(6, enabled);
    }

    /// Acquisition livetime in seconds (20 ms / 50 Hz ticks).
    #[must_use]
    pub fn livetime_secs(&self) -> f64 {
        f64::from(self.get_bits(224, 256)) / 50.0
    }

    /// Acquisition realtime in seconds (20 ms / 50 Hz ticks).
    #[must_use]
    pub fn realtime_secs(&self) -> f64 {
        f64::from(self.get_bits(288, 320)) / 50.0
    }

    /// HV setpoint in Volts (1.25 V increments).
    #[must_use]
    pub fn hv_target(&self) -> f64 {
        f64::from(self.get_bits(336, 352)) * 1.25
    }

    /// Set HV target in Volts (must be `< 1200.0`).
    pub fn set_hv_target(&mut self, volts: f64) -> Result<(), DeviceError> {
        if !(0.0..1200.0).contains(&volts) {
            return Err(DeviceError::Protocol(format!(
                "digiBase HV {volts} V out of range [0, 1200)"
            )));
        }
        let raw = ((volts as u32) * 4) / 5;
        self.set_bits(336, 352, raw);
        Ok(())
    }

    /// HV ADC readback in Volts (call after toggling bit 610).
    #[must_use]
    pub fn hv_readback(&self) -> f64 {
        let low = self.get_bits(24, 32);
        let high = self.get_bits(13, 15);
        f64::from(low | (high << 8)) * 1.25
    }

    #[must_use]
    pub fn lld(&self) -> u32 {
        self.get_bits(170, 180)
    }

    pub fn set_lld(&mut self, val: u32) {
        self.set_bits(170, 180, val & 0x3FF);
    }

    #[must_use]
    pub fn uld(&self) -> u32 {
        self.get_bits(176, 192)
    }

    pub fn set_uld(&mut self, val: u32) {
        self.set_bits(176, 192, val & 0xFFFF);
    }

    #[must_use]
    pub fn fine_gain(&self) -> f64 {
        f64::from(self.get_bits(96, 128)) / f64::from(0x0040_0000u32)
    }

    pub fn set_fine_gain(&mut self, val: f64) -> Result<(), DeviceError> {
        if !(0.25..2.0).contains(&val) {
            return Err(DeviceError::Protocol(format!(
                "digiBase fine gain {val} out of range [0.25, 2.0)"
            )));
        }
        let raw = (val * f64::from(0x0040_0000u32)) as u32;
        self.set_bits(128, 152, raw | 0x0080_0000);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// USB Command Helpers
// ---------------------------------------------------------------------------

/// Send a command and read at least `expected_len` bytes across one or more `recv` calls.
pub async fn send_and_read(
    link: &dyn DeviceLink,
    cmd: &[u8],
    expected_len: usize,
    timeout: Duration,
) -> Result<Vec<u8>, DeviceError> {
    link.send(cmd).await?;
    let mut buf = Vec::with_capacity(expected_len);
    let start = Instant::now();

    while buf.len() < expected_len {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(DeviceError::Link(LinkError::Timeout(timeout)));
        }
        let chunk = link.recv(remaining).await?;
        if chunk.is_empty() {
            break;
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(buf)
}

/// Read the 80-byte status register (`0x01`).
pub async fn read_status_register(
    link: &dyn DeviceLink,
    timeout: Duration,
) -> Result<StatusRegister, DeviceError> {
    let data = send_and_read(link, &[0x01], STATUS_REG_BYTES, timeout).await?;
    StatusRegister::from_slice(&data)
}

/// Write the 80-byte status register (`0x00` + 80 bytes).
pub async fn write_status_register(
    link: &dyn DeviceLink,
    reg: &StatusRegister,
    timeout: Duration,
) -> Result<(), DeviceError> {
    let mut pkt = Vec::with_capacity(1 + STATUS_REG_BYTES);
    pkt.push(0x00);
    pkt.extend_from_slice(reg.as_bytes());
    link.send(&pkt).await?;
    // Read acknowledgment byte(s) if returned by device; ignore timeout on empty ack
    drop(link.recv(timeout).await);
    Ok(())
}

/// Clear the 1024-channel spectrum (`0x02` + 4096 zero bytes).
pub async fn clear_spectrum(link: &dyn DeviceLink, timeout: Duration) -> Result<(), DeviceError> {
    let mut pkt = vec![0u8; 1 + SPECTRUM_BYTES];
    pkt[0] = 0x02;
    link.send(&pkt).await?;
    drop(link.recv(timeout).await);
    Ok(())
}

/// Read the 1024-channel spectrum (`0x80`), returning counts as `Vec<f64>`.
pub async fn read_spectrum_counts(
    link: &dyn DeviceLink,
    timeout: Duration,
) -> Result<Vec<f64>, DeviceError> {
    let data = send_and_read(link, &[0x80], SPECTRUM_BYTES, timeout).await?;
    if data.len() < SPECTRUM_BYTES {
        return Err(DeviceError::Protocol(format!(
            "digiBase spectrum too short: expected {SPECTRUM_BYTES} bytes, got {}",
            data.len()
        )));
    }
    let counts = data[..SPECTRUM_BYTES]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&c| f64::from(u32::from_le_bytes(c)))
        .collect();
    Ok(counts)
}
