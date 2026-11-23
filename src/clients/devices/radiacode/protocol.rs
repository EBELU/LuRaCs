//! Low-level Radiacode protocol framing, virtual string handling, and decoders.
//!
//! Ported and adapted from the official Python Radiacode library.

use std::time::{Duration, Instant};

use crate::clients::device_wrapper_base::{WrappedRealTimePackage, WrappedStatusPackage};
use crate::clients::driver::DeviceError;
use crate::clients::link::{DeviceLink, LinkError};

/// Default hardware IDs for USB and BLE.
pub const VENDOR_ID: u16 = 0x0483;
pub const PRODUCT_ID: u16 = 0xF123;
pub const USB_INTERFACE: u8 = 0;
pub const USB_IN_ENDPOINT: u8 = 0x81;
pub const USB_OUT_ENDPOINT: u8 = 0x01;

pub const BLE_WRITE_UUID: uuid::Uuid = uuid::uuid!("e63215e6-7003-49d8-96b0-b024798fb901");
pub const BLE_NOTIFY_UUID: uuid::Uuid = uuid::uuid!("e63215e7-7003-49d8-96b0-b024798fb901");

/// Radiacode binary command identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Command {
    GetStatus = 0x0005,
    SetExchange = 0x0007,
    GetVersion = 0x000A,
    GetSerial = 0x000B,
    FwImageGetInfo = 0x0012,
    FwSignature = 0x0101,
    RdHwConfig = 0x0807,
    RdVirtSfr = 0x0824,
    WrVirtSfr = 0x0825,
    RdVirtString = 0x0826,
    WrVirtString = 0x0827,
    RdFlash = 0x081C,
    SetTime = 0x0A04,
}

/// Radiacode Virtual String (VS) identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtualString {
    Configuration = 0x0002,
    FwDescriptor = 0x0003,
    SerialNumber = 0x0008,
    TextMessage = 0x000F,
    MemSnapshot = 0x00E0,
    DataBuf = 0x0100,
    SfrFile = 0x0101,
    Spectrum = 0x0200,
    EnergyCalib = 0x0202,
    SpecAccum = 0x0205,
    SpecDiff = 0x0206,
    SpecReset = 0x0207,
}

/// Encode a request frame according to Radiacode packet framing:
/// `[4-byte len] [2-byte cmd] [0x00] [0x80 | (seq % 32)] [args...]`
#[must_use]
pub fn encode_request(cmd: u16, seq: u8, args: &[u8]) -> Vec<u8> {
    let payload_len = 4 + args.len();
    let mut req = Vec::with_capacity(4 + payload_len);
    req.extend_from_slice(&(payload_len as u32).to_le_bytes());
    req.extend_from_slice(&cmd.to_le_bytes());
    req.push(0x00);
    req.push(0x80 | (seq % 32));
    req.extend_from_slice(args);
    req
}

/// Execute a synchronous request-response transaction over any `DeviceLink`.
pub async fn execute_transaction(
    link: &dyn DeviceLink,
    cmd: u16,
    seq: u8,
    args: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, DeviceError> {
    let req = encode_request(cmd, seq, args);
    link.send(&req).await?;

    let mut buf = Vec::new();
    let expected_seq = 0x80 | (seq % 32);
    let start = Instant::now();

    // Read until we have the 4-byte length prefix
    let resp_len = loop {
        if start.elapsed() > timeout {
            return Err(DeviceError::Link(LinkError::Timeout(timeout)));
        }
        let chunk = link.recv(timeout.saturating_sub(start.elapsed())).await?;
        buf.extend_from_slice(&chunk);
        if buf.len() >= 4 {
            let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
            break len;
        }
    };

    // Read remaining bytes of the packet
    while buf.len() < 4 + resp_len {
        if start.elapsed() > timeout {
            return Err(DeviceError::Link(LinkError::Timeout(timeout)));
        }
        let chunk = link.recv(timeout.saturating_sub(start.elapsed())).await?;
        buf.extend_from_slice(&chunk);
    }

    let payload = &buf[4..4 + resp_len];
    if payload.len() < 4 {
        return Err(DeviceError::Protocol(format!(
            "response too short: expected >= 4 header bytes, got {}",
            payload.len()
        )));
    }

    let resp_cmd = u16::from_le_bytes(payload[0..2].try_into().unwrap());
    let resp_seq = payload[3];

    if resp_cmd != cmd || resp_seq != expected_seq {
        return Err(DeviceError::Protocol(format!(
            "header mismatch: sent cmd=0x{cmd:04x} seq=0x{expected_seq:02x}, got cmd=0x{resp_cmd:04x} seq=0x{resp_seq:02x}"
        )));
    }

    Ok(payload[4..].to_vec())
}

/// Read a Virtual String (RD_VIRT_STRING) from the device.
pub async fn read_virtual_string(
    link: &dyn DeviceLink,
    seq: u8,
    vs_id: u32,
    timeout: Duration,
) -> Result<Vec<u8>, DeviceError> {
    let args = vs_id.to_le_bytes();
    let resp =
        execute_transaction(link, Command::RdVirtString as u16, seq, &args, timeout).await?;
    if resp.len() < 8 {
        return Err(DeviceError::Protocol(format!(
            "RD_VIRT_STRING response too short: got {} bytes",
            resp.len()
        )));
    }

    let retcode = u32::from_le_bytes(resp[0..4].try_into().unwrap());
    let flen = u32::from_le_bytes(resp[4..8].try_into().unwrap()) as usize;

    if retcode != 1 {
        return Err(DeviceError::Hardware(format!(
            "RD_VIRT_STRING for 0x{vs_id:04x} returned error code {retcode}"
        )));
    }

    let mut data = resp[8..].to_vec();
    if data.len() == flen + 1 && data.last() == Some(&0) {
        data.pop();
    }

    if data.len() != flen {
        return Err(DeviceError::Protocol(format!(
            "RD_VIRT_STRING length mismatch for 0x{vs_id:04x}: declared {flen}, got {}",
            data.len()
        )));
    }

    Ok(data)
}

/// Write data to a Virtual String (WR_VIRT_STRING) on the device.
pub async fn write_virtual_string(
    link: &dyn DeviceLink,
    seq: u8,
    vs_id: u32,
    data: &[u8],
    timeout: Duration,
) -> Result<(), DeviceError> {
    let mut args = Vec::with_capacity(8 + data.len());
    args.extend_from_slice(&vs_id.to_le_bytes());
    args.extend_from_slice(&(data.len() as u32).to_le_bytes());
    args.extend_from_slice(data);

    let resp =
        execute_transaction(link, Command::WrVirtString as u16, seq, &args, timeout).await?;
    if resp.len() < 4 {
        return Err(DeviceError::Protocol(
            "WR_VIRT_STRING response missing retcode".into(),
        ));
    }

    let retcode = u32::from_le_bytes(resp[0..4].try_into().unwrap());
    if retcode != 1 {
        return Err(DeviceError::Hardware(format!(
            "WR_VIRT_STRING for 0x{vs_id:04x} returned error code {retcode}"
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Spectrum Decoding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DecodedSpectrum {
    pub duration_secs: u32,
    pub a0: f32,
    pub a1: f32,
    pub a2: f32,
    pub counts: Vec<u32>,
}

/// Decodes VS::SPECTRUM raw binary payload supporting format version 0 (raw u32)
/// and format version 1 (run-length encoded deltas).
pub fn decode_spectrum(data: &[u8], format_version: u32) -> Result<DecodedSpectrum, DeviceError> {
    if data.len() < 16 {
        return Err(DeviceError::Protocol(format!(
            "spectrum data too short: got {} bytes, minimum header is 16",
            data.len()
        )));
    }

    let duration_secs = u32::from_le_bytes(data[0..4].try_into().unwrap());
    let a0 = f32::from_le_bytes(data[4..8].try_into().unwrap());
    let a1 = f32::from_le_bytes(data[8..12].try_into().unwrap());
    let a2 = f32::from_le_bytes(data[12..16].try_into().unwrap());

    let payload = &data[16..];
    let counts = match format_version {
        0 => decode_counts_v0(payload)?,
        1 => decode_counts_v1(payload)?,
        other => {
            return Err(DeviceError::Protocol(format!(
                "unsupported spectrum format version {other}"
            )))
        }
    };

    Ok(DecodedSpectrum {
        duration_secs,
        a0,
        a1,
        a2,
        counts,
    })
}

fn decode_counts_v0(mut data: &[u8]) -> Result<Vec<u32>, DeviceError> {
    let mut counts = Vec::with_capacity(data.len() / 4);
    while data.len() >= 4 {
        counts.push(u32::from_le_bytes(data[0..4].try_into().unwrap()));
        data = &data[4..];
    }
    Ok(counts)
}

fn decode_counts_v1(mut data: &[u8]) -> Result<Vec<u32>, DeviceError> {
    let mut counts = Vec::new();
    let mut last: i32 = 0;

    while data.len() >= 2 {
        let u16_val = u16::from_le_bytes(data[0..2].try_into().unwrap());
        data = &data[2..];

        let cnt = ((u16_val >> 4) & 0x0FFF) as usize;
        let vlen = u16_val & 0x0F;

        for _ in 0..cnt {
            let v = match vlen {
                0 => 0,
                1 => {
                    if data.is_empty() {
                        return Err(DeviceError::Protocol(
                            "unexpected EOF in v1 spectrum (vlen 1)".into(),
                        ));
                    }
                    let val = i32::from(data[0]);
                    data = &data[1..];
                    val
                }
                2 => {
                    if data.is_empty() {
                        return Err(DeviceError::Protocol(
                            "unexpected EOF in v1 spectrum (vlen 2)".into(),
                        ));
                    }
                    let delta = i32::from(data[0] as i8);
                    data = &data[1..];
                    last + delta
                }
                3 => {
                    if data.len() < 2 {
                        return Err(DeviceError::Protocol(
                            "unexpected EOF in v1 spectrum (vlen 3)".into(),
                        ));
                    }
                    let delta = i32::from(i16::from_le_bytes(data[0..2].try_into().unwrap()));
                    data = &data[2..];
                    last + delta
                }
                4 => {
                    if data.len() < 3 {
                        return Err(DeviceError::Protocol(
                            "unexpected EOF in v1 spectrum (vlen 4)".into(),
                        ));
                    }
                    let a = i32::from(data[0]);
                    let b = i32::from(data[1]);
                    let c = i32::from(data[2] as i8); // sign-extended 24-bit
                    data = &data[3..];
                    let delta = (c << 16) | (b << 8) | a;
                    last + delta
                }
                5 => {
                    if data.len() < 4 {
                        return Err(DeviceError::Protocol(
                            "unexpected EOF in v1 spectrum (vlen 5)".into(),
                        ));
                    }
                    let delta = i32::from_le_bytes(data[0..4].try_into().unwrap());
                    data = &data[4..];
                    last + delta
                }
                other => {
                    return Err(DeviceError::Protocol(format!(
                        "unsupported vlen={other} in spectrum v1 compression"
                    )));
                }
            };
            last = v;
            counts.push(v.max(0) as u32);
        }
    }

    Ok(counts)
}

// ---------------------------------------------------------------------------
// DataBuf Decoding
// ---------------------------------------------------------------------------

/// Decodes VS::DATA_BUF extracting the latest real-time package and status package.
#[must_use]
pub fn decode_data_buf(
    mut data: &[u8],
    now_timestamp: f64,
) -> (Option<WrappedRealTimePackage>, Option<WrappedStatusPackage>) {
    let mut latest_rt: Option<WrappedRealTimePackage> = None;
    let mut latest_status: Option<WrappedStatusPackage> = None;

    while data.len() >= 7 {
        let _seq = data[0];
        let eid = data[1];
        let gid = data[2];
        let ts_offset_10ms = i32::from_le_bytes(data[3..7].try_into().unwrap());
        data = &data[7..];

        let ts = f64::mul_add(f64::from(ts_offset_10ms), 0.010, now_timestamp);

        if eid == 0 && gid == 0 {
            // RealTimeData: count_rate (f32), dose_rate (f32), count_rate_err (u16), dose_rate_err (u16), flags (u16), rt_flags (u8) = 15 bytes
            if data.len() < 15 {
                break;
            }
            let count_rate = f32::from_le_bytes(data[0..4].try_into().unwrap());
            let dose_rate = f32::from_le_bytes(data[4..8].try_into().unwrap());
            let count_rate_err = u16::from_le_bytes(data[8..10].try_into().unwrap());
            let dose_rate_err = u16::from_le_bytes(data[10..12].try_into().unwrap());
            let _flags = u16::from_le_bytes(data[12..14].try_into().unwrap());
            let _rt_flags = data[14];
            data = &data[15..];

            latest_rt = Some(WrappedRealTimePackage {
                cps: f64::from(count_rate),
                dr: f64::from(dose_rate) * 1e4,
                cps_error: f64::from(count_rate_err) / 10.0,
                dr_error: f64::from(dose_rate_err) / 10.0,
                dead_time: f64::NAN,
                timestamp: ts,
            });
        } else if eid == 0 && gid == 3 {
            // RareData: duration (u32), dose (f32), temperature (u16), charge_level (u16), flags (u16) = 14 bytes
            if data.len() < 14 {
                break;
            }
            let duration = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let dose = f32::from_le_bytes(data[4..8].try_into().unwrap());
            let temperature = u16::from_le_bytes(data[8..10].try_into().unwrap());
            let charge_level = u16::from_le_bytes(data[10..12].try_into().unwrap());
            let _flags = u16::from_le_bytes(data[12..14].try_into().unwrap());
            data = &data[14..];

            let temp_c = (f64::from(temperature) - 2000.0) / 100.0;
            let battery_pct = ((charge_level / 100) as u8).min(100);

            latest_status = Some(WrappedStatusPackage {
                battery: Some(battery_pct),
                temperature: temp_c,
                charging: false,
                total_dose: f64::from(dose),
                total_uptime: f64::from(duration),
                timestamp: ts,
                ..Default::default()
            });
        } else if eid == 0 && gid == 1 {
            // RawData: 8 bytes
            if data.len() < 8 {
                break;
            }
            data = &data[8..];
        } else if eid == 0 && (gid == 2 || gid == 4 || gid == 5) {
            // DoseRateDB / UserData / ScheduleData: 16 bytes
            if data.len() < 16 {
                break;
            }
            data = &data[16..];
        } else if eid == 0 && gid == 6 {
            // AccelData: 6 bytes
            if data.len() < 6 {
                break;
            }
            data = &data[6..];
        } else if eid == 0 && gid == 7 {
            // Event: 4 bytes
            if data.len() < 4 {
                break;
            }
            data = &data[4..];
        } else if eid == 0 && (gid == 8 || gid == 9) {
            // RawCountRate / RawDoseRate: 6 bytes
            if data.len() < 6 {
                break;
            }
            data = &data[6..];
        } else if eid == 1 && (gid == 1 || gid == 2 || gid == 3) {
            // Variable samples: samples_num (u16), smpl_time_ms (u32)
            if data.len() < 6 {
                break;
            }
            let samples = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;
            let sample_size = match gid {
                1 => 8,
                2 => 16,
                3 => 14,
                _ => break,
            };
            let total_skip = 6 + samples * sample_size;
            if data.len() < total_skip {
                break;
            }
            data = &data[total_skip..];
        } else {
            // Unknown record type - safely stop parsing
            break;
        }
    }

    (latest_rt, latest_status)
}
