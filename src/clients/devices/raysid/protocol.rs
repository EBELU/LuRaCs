//! Low-level RaySID BLE Transparent UART protocol encoders, decoders, and checksums.
//!
//! Ported from the reversed RaySID Android app protocol (`RaysidClient`).

use uuid::{uuid, Uuid};

pub const SERVICE_UUID: Uuid = uuid!("49535343-fe7d-4ae5-8fa9-9fafd205e455");
pub const RX_UUID: Uuid = uuid!("49535343-1e4d-4bd9-ba61-23c647249616");
pub const TX_UUID: Uuid = uuid!("49535343-8841-43f4-a8d4-ecbe34729bb3");

pub const NUM_CHANNELS: usize = 1800;

/// RaySID spectrum energy range presets for reset (`0x10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EnergyRange {
    /// 25 .. 1000 keV
    Low = 8,
    /// 30 .. 2000 keV
    Mid = 4,
    /// 40 .. 3500 keV
    High = 2,
}

// ---------------------------------------------------------------------------
// Checksums & Packet Encoding (TX2)
// ---------------------------------------------------------------------------

fn calculate_array_crc(data: &[u8]) -> u32 {
    let mut crc = 0u32;
    for chunk in data.chunks(4) {
        let mut word = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            word |= u32::from(b) << (i * 8);
        }
        crc = crc.wrapping_add(word);
    }
    crc
}

fn crc2(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |acc, &b| acc ^ b)
}

/// Build a TX2-framed RaySID command packet:
/// `[0xFF][crc2][0xEE][crc1: u32 BE][data...][total_len: u8]`
#[must_use]
pub fn build_tx2_packet(data: &[u8]) -> Vec<u8> {
    let crc1 = calculate_array_crc(data);
    let inner_len = data.len() + 5;
    let mut inner = Vec::with_capacity(inner_len);
    inner.push(0xEE);
    inner.extend_from_slice(&crc1.to_be_bytes());
    inner.extend_from_slice(data);

    let c2 = crc2(&inner);
    let total_len = inner_len + 3;

    let mut pkt = Vec::with_capacity(total_len);
    pkt.push(0xFF);
    pkt.push(c2);
    pkt.extend_from_slice(&inner);
    pkt.push(total_len as u8);
    pkt
}

/// Build a keepalive/tab ping command (`0x12`).
#[must_use]
pub fn build_ping_packet(active_tab: u8, unix_secs: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(6);
    data.push(0x12);
    data.push(active_tab);
    data.extend_from_slice(&unix_secs.to_be_bytes());
    data
}

/// Build a spectrum range request (`0x3E`, inclusive `start_ch..=end_ch`).
#[must_use]
pub fn build_spectrum_request(start_ch: u16, end_ch: u16) -> Vec<u8> {
    let end_excl = end_ch.saturating_add(1);
    let mut data = Vec::with_capacity(5);
    data.push(0x3E);
    data.extend_from_slice(&start_ch.to_be_bytes());
    data.extend_from_slice(&end_excl.to_be_bytes());
    data
}

/// Build a spectrum reset + energy range command (`0x10`).
#[must_use]
pub const fn build_reset_packet(range: EnergyRange) -> [u8; 2] {
    [0x10, range as u8]
}

// ---------------------------------------------------------------------------
// Checksums & Packet Decoding (RX)
// ---------------------------------------------------------------------------

/// 24-bit XOR checksum over 3-byte big-endian words.
#[must_use]
pub fn checksum_decode(data: &[u8]) -> u32 {
    let mut out = 0u32;
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        out ^= (b0 << 16) | (b1 << 8) | b2;
    }
    out & 0x00FF_FFFF
}

/// Verify 24-bit spectrum packet checksum (last 3 bytes LE).
#[must_use]
pub fn verify_spectrum_checksum(packet: &[u8]) -> bool {
    if packet.len() < 6 {
        return false;
    }
    let payload = &packet[..packet.len() - 3];
    let chk = &packet[packet.len() - 3..];
    let expected = u32::from(chk[0]) | (u32::from(chk[1]) << 8) | (u32::from(chk[2]) << 16);
    checksum_decode(payload) == expected
}

/// Verify CPS packet checksum (`packet[1..len-3]` top 16 bits match `[len-3, len-4]`).
#[must_use]
pub fn verify_cps_checksum(packet: &[u8]) -> bool {
    if packet.len() < 6 {
        return false;
    }
    let payload = &packet[1..packet.len() - 3];
    let calc = checksum_decode(payload);
    let calc_b0 = ((calc >> 16) & 0xFF) as u8;
    let calc_b1 = ((calc >> 8) & 0xFF) as u8;
    let exp_b1 = packet[packet.len() - 4];
    let exp_b0 = packet[packet.len() - 3];
    calc_b0 == exp_b0 && calc_b1 == exp_b1
}

fn unpack_value(v: u16) -> f64 {
    let mult10 = u32::from(v / 6000);
    let res = f64::from(v % 6000);
    res * 10.0_f64.powi(mult10 as i32)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedCps {
    pub cps: f64,
    pub dose_rate: f64,
}

/// Decode a `0x17` CPS / Dose Rate packet.
#[must_use]
pub fn decode_cps_packet(data: &[u8]) -> Option<DecodedCps> {
    if data.len() >= 15 && data[14] > 1 {
        // Overload flag
        return None;
    }

    let mut cps = None;
    let mut dose = None;
    let sets = if data.len() > 20 { 12 } else { 2 };

    for k in 0..sets {
        let base = k * 3 + 2;
        if base + 2 >= data.len() {
            break;
        }
        let dtype = data[base];
        let raw_u16 = u16::from_le_bytes([data[base + 1], data[base + 2]]);
        let value = unpack_value(raw_u16) / 600.0;
        if dtype == 0 {
            cps = Some(value);
        } else if dtype == 1 {
            dose = Some(value * 10.0 / 1000.0);
        }
    }

    Some(DecodedCps {
        cps: cps?,
        dose_rate: dose?,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedStatus {
    pub battery: u8,
    pub temperature: f64,
    pub charging: bool,
    pub temp_ok: bool,
    pub channel_full: bool,
    pub uart_errors: u16,
}

/// Decode a `0x02` status packet.
#[must_use]
pub fn decode_status_packet(data: &[u8]) -> Option<DecodedStatus> {
    if data.len() < 14 {
        return None;
    }
    let temp_raw = u16::from_le_bytes([data[2], data[3]]);
    let temperature = f64::from(temp_raw) / 10.0 - 100.0;
    let battery = (u16::from_le_bytes([data[4], data[5]]) & 0xFF) as u8;
    let charging = (data[6] & 0x01) != 0;
    let state = data[11];
    let temp_ok = (state & 0x01) != 0;
    let channel_full = (state & 0x02) != 0;
    let uart_errors = u16::from_le_bytes([data[12], data[13]]);

    Some(DecodedStatus {
        battery,
        temperature,
        charging,
        temp_ok,
        channel_full,
        uart_errors,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedMeta {
    pub counts: u32,
    pub uptime_secs: f64,
    pub energy: u32,
    pub high_energy_counts: u32,
}

/// Decode a `0x01` spectrum metadata packet.
#[must_use]
pub fn decode_meta_packet(data: &[u8]) -> Option<DecodedMeta> {
    if data.len() < 17 {
        return None;
    }
    let counts = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
    let uptime_raw = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let uptime_secs = f64::from(uptime_raw) / 12.0;
    let energy = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
    let high_energy_counts =
        u32::from(data[14]) | (u32::from(data[15]) << 8) | (u32::from(data[16]) << 16);

    Some(DecodedMeta {
        counts,
        uptime_secs,
        energy,
        high_energy_counts,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSpectrumChunk {
    pub start_ch: usize,
    pub end_ch: usize,
    pub values: Vec<f64>,
}

/// Decode a `0x30`, `0x31`, or `0x32` compressed spectrum chunk packet.
pub fn decode_spectrum_packet(data: &[u8]) -> Option<DecodedSpectrumChunk> {
    if data.len() < 7 {
        return None;
    }
    let total_bytes = if data[0] == 0 { 256 } else { usize::from(data[0]) };
    let pkt = if data.len() > total_bytes + 1 {
        &data[..=total_bytes]
    } else {
        data
    };
    if pkt.len() < 7 {
        return None;
    }

    let channel_div: usize = match pkt[1] {
        0x30 => 1,
        0x31 => 3,
        0x32 => 9,
        _ => return None,
    };

    let start_ch = usize::from(u16::from_le_bytes([pkt[2], pkt[3]]));
    let mut x = start_ch;
    let mut current_value: i32 =
        i32::from(pkt[4]) | (i32::from(pkt[5]) << 8) | (i32::from(pkt[6]) << 16);

    let mut values = Vec::new();
    let push_div = |val: i32, x_pos: &mut usize, out: &mut Vec<f64>| -> bool {
        let scaled = f64::from(val) / (channel_div as f64);
        for _ in 0..channel_div {
            if *x_pos >= NUM_CHANNELS {
                return false;
            }
            out.push(scaled);
            *x_pos += 1;
        }
        true
    };

    if !push_div(current_value, &mut x, &mut values) {
        return None;
    }

    let mut pos = 7usize;
    while pos + 4 < pkt.len() {
        let header = pkt[pos];
        pos += 1;

        let (point_type, point_amount) = if header == 0 {
            (4u8, 1usize)
        } else {
            (header >> 6, usize::from(header & 0x3F))
        };

        match point_type {
            // Two 4-bit signed deltas per byte
            0 => {
                let mut count = 0;
                while count < point_amount {
                    if pos >= pkt.len() {
                        return None;
                    }
                    let byte = pkt[pos];
                    let hi_nibble = i32::from(byte >> 4);
                    let diff1 = if hi_nibble > 7 { hi_nibble - 16 } else { hi_nibble };
                    current_value += diff1;
                    if !push_div(current_value, &mut x, &mut values) {
                        return None;
                    }
                    count += 1;

                    if count < point_amount {
                        let lo_nibble = i32::from(byte & 0x0F);
                        let diff2 = if lo_nibble > 7 { lo_nibble - 16 } else { lo_nibble };
                        current_value += diff2;
                        if !push_div(current_value, &mut x, &mut values) {
                            return None;
                        }
                        count += 1;
                    }
                    pos += 1;
                }
            }
            // One 8-bit signed delta per byte
            1 => {
                for _ in 0..point_amount {
                    if pos >= pkt.len() {
                        return None;
                    }
                    let diff = i32::from(pkt[pos] as i8);
                    current_value += diff;
                    if !push_div(current_value, &mut x, &mut values) {
                        return None;
                    }
                    pos += 1;
                }
            }
            // Two 12-bit signed deltas in 3 bytes
            2 => {
                let mut count = 0;
                while count < point_amount {
                    if pos + 1 >= pkt.len() {
                        return None;
                    }
                    let mut diff1 = (i32::from(pkt[pos]) << 4) | (i32::from(pkt[pos + 1]) >> 4);
                    if diff1 >= 0x800 {
                        diff1 -= 0x1000;
                    }
                    current_value += diff1;
                    if !push_div(current_value, &mut x, &mut values) {
                        return None;
                    }
                    count += 1;
                    pos += 2;

                    if count < point_amount {
                        if pos >= pkt.len() {
                            return None;
                        }
                        let mut diff2 =
                            ((i32::from(pkt[pos - 1]) & 0x0F) << 8) | i32::from(pkt[pos]);
                        if diff2 >= 0x800 {
                            diff2 -= 0x1000;
                        }
                        current_value += diff2;
                        if !push_div(current_value, &mut x, &mut values) {
                            return None;
                        }
                        count += 1;
                        pos += 1;
                    }
                }
            }
            // One 16-bit signed LE delta per 2 bytes
            3 => {
                for _ in 0..point_amount {
                    if pos + 1 >= pkt.len() {
                        return None;
                    }
                    let diff = i32::from(i16::from_le_bytes([pkt[pos], pkt[pos + 1]]));
                    current_value += diff;
                    if !push_div(current_value, &mut x, &mut values) {
                        return None;
                    }
                    pos += 2;
                }
            }
            // One 24-bit signed delta per 3 bytes
            4 => {
                if pos + 2 >= pkt.len() {
                    return None;
                }
                let mut diff =
                    i32::from(pkt[pos + 2]) | (i32::from(pkt[pos + 1]) << 8) | (i32::from(pkt[pos]) << 16);
                if diff > 0x7F_FFFF {
                    diff -= 0x100_0000;
                }
                current_value += diff;
                if !push_div(current_value, &mut x, &mut values) {
                    return None;
                }
                pos += 3;
            }
            _ => return None,
        }
    }

    Some(DecodedSpectrumChunk {
        start_ch,
        end_ch: x,
        values,
    })
}

/// Accumulates 1,800-channel spectrum chunks and metadata from RaySID.
#[derive(Debug, Clone)]
pub struct SpectrumAccumulator {
    pub spectrum: Vec<f64>,
    pub counts: u32,
    pub uptime_secs: f64,
    pub energy: u32,
    pub high_energy_counts: u32,
}

impl Default for SpectrumAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl SpectrumAccumulator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            spectrum: vec![0.0; NUM_CHANNELS],
            counts: 0,
            uptime_secs: 0.0,
            energy: 0,
            high_energy_counts: 0,
        }
    }

    pub fn reset(&mut self) {
        self.spectrum.fill(0.0);
        self.counts = 0;
        self.uptime_secs = 0.0;
        self.energy = 0;
        self.high_energy_counts = 0;
    }

    pub fn insert(&mut self, chunk: &DecodedSpectrumChunk) {
        if chunk.start_ch == 0 && chunk.end_ch == 0 {
            return;
        }
        let end = chunk.end_ch.min(self.spectrum.len());
        let len = end.saturating_sub(chunk.start_ch).min(chunk.values.len());
        self.spectrum[chunk.start_ch..chunk.start_ch + len]
            .copy_from_slice(&chunk.values[..len]);
    }

    pub const fn update_meta(&mut self, meta: &DecodedMeta) {
        self.counts = meta.counts;
        self.uptime_secs = meta.uptime_secs;
        self.energy = meta.energy;
        self.high_energy_counts = meta.high_energy_counts;
    }
}
