//! Low-level ORTEC digiDART USB protocol framing and command helpers.
//!
//! Ported from `luracs/clients/digidart_client/digidart.py`.

use std::time::{Duration, Instant};

use crate::clients::driver::DeviceError;
use crate::clients::link::{DeviceLink, LinkError};

pub const VENDOR_ID: u16 = 0x0A2D;
pub const PRODUCT_ID: u16 = 0x0005;
pub const EP_OUT: u8 = 0x02;
pub const EP_IN: u8 = 0x82;

pub const SPECTRUM_CHUNK_SIZE: usize = 0x2000; // 8192 channels max per USB transfer
pub const DEFAULT_CHANNELS: usize = 8192;

const FINE_GAIN_STEP: f64 = 1.0 / 196_608.0;
const FINE_GAIN_MIN: f64 = 0.45;
const FINE_GAIN_RAW_MIN: u32 = 88_474;
const FINE_GAIN_RAW_MAX: u32 = 196_607;

/// digiDART command opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Opcode {
    Clear = 0x0002,
    ClearCounters = 0x0004,
    ClearData = 0x0005,
    DisableHv = 0x000C,
    EnableHv = 0x0015,
    SetGainCoarse = 0x0034,
    SetGainConversion = 0x0035,
    SetGainFine = 0x0036,
    SetHv = 0x003B,
    SetLld = 0x0040,
    SetUld = 0x0058,
    ShowActive = 0x005D,
    ShowGainCoarse = 0x006F,
    ShowGainConversion = 0x0070,
    ShowGainFine = 0x0071,
    ShowHvActual = 0x0077,
    ShowHvTarget = 0x0078,
    ShowLive = 0x007D,
    ShowLld = 0x0081,
    ShowTrue = 0x009A,
    Start = 0x00A5,
    Stop = 0x00A9,
    Spectrum = 0x00BB,
}

/// Build a standard command packet (`[n_cmd: u16 LE][reply_len: u16 LE][params: u32 BE...]`).
#[must_use]
pub fn encode_command(opcode: u16, reply_len: u16, params: &[u32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + params.len() * 4);
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&reply_len.to_le_bytes());
    for &p in params {
        buf.extend_from_slice(&p.to_be_bytes());
    }
    buf
}

/// Send a command and read `reply_len` bytes across one or more `recv` chunks.
pub async fn send_recv(
    link: &dyn DeviceLink,
    cmd: &[u8],
    reply_len: usize,
    timeout: Duration,
) -> Result<Vec<u8>, DeviceError> {
    link.send(cmd).await?;

    let mut buf = Vec::with_capacity(reply_len);
    let start = Instant::now();

    while buf.len() < reply_len {
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

    if buf.len() < reply_len {
        buf.resize(reply_len, 0);
    }

    Ok(buf)
}

fn check_status(name: &str, macro_err: u8, micro_err: u8) -> Result<(), DeviceError> {
    if macro_err != 0 || micro_err != 0 {
        return Err(DeviceError::Hardware(format!(
            "digiDART {name} error macro=0x{macro_err:02x} micro=0x{micro_err:02x}"
        )));
    }
    Ok(())
}

/// Execute a command returning `(macro_err, micro_err)` (2-byte reply).
pub async fn cmd(
    link: &dyn DeviceLink,
    opcode: Opcode,
    params: &[u32],
    timeout: Duration,
) -> Result<(u8, u8), DeviceError> {
    let pkt = encode_command(opcode as u16, 2, params);
    let r = send_recv(link, &pkt, 2, timeout).await?;
    Ok((r[0], r[1]))
}

/// Execute a command and fail if `macro_err != 0 || micro_err != 0`.
pub async fn cmd_checked(
    link: &dyn DeviceLink,
    name: &str,
    opcode: Opcode,
    params: &[u32],
    timeout: Duration,
) -> Result<(), DeviceError> {
    let (e3, e4) = cmd(link, opcode, params, timeout).await?;
    check_status(name, e3, e4)
}

/// Query a single `u32` (big-endian) value (6-byte reply).
pub async fn query_u32_1(
    link: &dyn DeviceLink,
    name: &str,
    opcode: Opcode,
    timeout: Duration,
) -> Result<u32, DeviceError> {
    let pkt = encode_command(opcode as u16, 6, &[]);
    let r = send_recv(link, &pkt, 6, timeout).await?;
    check_status(name, r[0], r[1])?;
    Ok(u32::from_be_bytes(r[2..6].try_into().unwrap()))
}

/// Query two `u32` (big-endian) values (10-byte reply).
pub async fn query_u32_2(
    link: &dyn DeviceLink,
    name: &str,
    opcode: Opcode,
    timeout: Duration,
) -> Result<(u32, u32), DeviceError> {
    let pkt = encode_command(opcode as u16, 10, &[]);
    let r = send_recv(link, &pkt, 10, timeout).await?;
    check_status(name, r[0], r[1])?;
    let a = u32::from_be_bytes(r[2..6].try_into().unwrap());
    let b = u32::from_be_bytes(r[6..10].try_into().unwrap());
    Ok((a, b))
}

/// Convert fine gain float `[0.45, 1.0]` to raw hardware u32.
pub fn encode_fine_gain(val: f64) -> Result<u32, DeviceError> {
    if !(FINE_GAIN_MIN..=1.0).contains(&val) {
        return Err(DeviceError::Protocol(format!(
            "digiDART fine gain {val} out of range [{FINE_GAIN_MIN}, 1.0]"
        )));
    }
    let raw = (val / FINE_GAIN_STEP + 0.5) as u32;
    Ok(raw.clamp(FINE_GAIN_RAW_MIN, FINE_GAIN_RAW_MAX))
}

/// Convert raw hardware u32 to fine gain float.
#[must_use]
pub fn decode_fine_gain(raw: u32) -> f64 {
    f64::from(raw) * FINE_GAIN_STEP
}

/// Read `count` spectrum channels starting at `start`, chunked by `SPECTRUM_CHUNK_SIZE`.
pub async fn read_spectrum(
    link: &dyn DeviceLink,
    start: usize,
    count: usize,
    timeout: Duration,
) -> Result<Vec<f64>, DeviceError> {
    let mut out = Vec::with_capacity(count);
    let mut chan = start;
    let mut remaining = count;

    while remaining > 0 {
        let n = remaining.min(SPECTRUM_CHUNK_SIZE);
        let reply_len = n * 4 + 4;
        let pkt = encode_command(
            Opcode::Spectrum as u16,
            reply_len as u16,
            &[chan as u32, n as u32],
        );
        let r = send_recv(link, &pkt, reply_len, timeout).await?;

        let status = u32::from_be_bytes(r[0..4].try_into().unwrap());
        if status != 0 {
            return Err(DeviceError::Hardware(format!(
                "digiDART SPECTRUM status=0x{status:08x} on chunk at {chan}"
            )));
        }

        for &chunk in r[4..4 + n * 4].as_chunks::<4>().0 {
            out.push(f64::from(u32::from_le_bytes(chunk)));
        }

        chan += n;
        remaining -= n;
    }

    Ok(out)
}
