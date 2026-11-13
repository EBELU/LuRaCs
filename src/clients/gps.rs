//! NMEA GPS reader.
//!
//! Combines two responsibilities from the Python original:
//! - Pure NMEA parsing (`parse_gps`) and coordinate helpers
//! - A `Gps` handle that owns the serial port and maintains merge state
//!
//! Replaces Qt's `QSerialPort` + `sigUpdated` signal with a `poll()` method
//! the GUI calls once per frame. No threads, no channels; the serial read
//! is non-blocking via `bytes_to_read`.
//!
//! ## Merge behaviour preserved from Python
//!
//! - `Some(x)` fields overwrite; `None` fields keep their old value.
//! - `satellites`, `satellites_in_view`, `fix_type`: value `0` means
//!   "not reported" and keeps the old value (so a GGA with 0 sats doesn't
//!   erase the count from a previous fix).
//! - `valid`: always overwritten by the last sentence (GGA / RMC / GSA
//!   decide in turn; last one wins).
//! - `timestamp`: only advances when a new latitude arrives.
//! - `update_rate`: `round(packets / max(elapsed_secs, 1))`.
//!
//! ## One deliberate fix vs Python
//!
//! `open()` resets `packets_received` and `update_rate`. Python didn't, so
//! reopening the port after a session left the rate spiralling for the
//! first few sentences. The merge state (`data`) is preserved across open /
//! close to match Python's behaviour.

use std::io::Read;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serialport::SerialPort;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum GpsError {
    #[error("serial port: {0}")]
    Serial(#[from] serialport::Error),
}

pub type Result<T> = std::result::Result<T, GpsError>;

// ---------------------------------------------------------------------------
// GPSData
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GpsData {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub altitude: Option<f64>,
    pub speed: Option<f64>,
    pub course: Option<f64>,

    pub satellites: u32,
    pub satellites_in_view: u32,

    pub hdop: Option<f64>,
    pub vdop: Option<f64>,
    pub pdop: Option<f64>,

    pub fix_type: u32,
    pub valid: bool,
    pub update_rate: f64,

    pub source: String,
    pub timestamp: u64,
}

impl Default for GpsData {
    fn default() -> Self {
        Self {
            latitude: None,
            longitude: None,
            altitude: None,
            speed: None,
            course: None,
            satellites: 0,
            satellites_in_view: 0,
            hdop: None,
            vdop: None,
            pdop: None,
            fix_type: 0,
            valid: false,
            update_rate: 0.0,
            source: "USB GPS".to_string(),
            timestamp: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// NMEA parsing (pure)
// ---------------------------------------------------------------------------

/// Convert NMEA `DDMM.MMMM` / `DDDMM.MMMM` to decimal degrees.
fn nmea_coord(value: &str, hemisphere: &str) -> Option<f64> {
    if value.is_empty() || hemisphere.is_empty() {
        return None;
    }
    let v: f64 = value.parse().ok()?;
    let degrees = (v / 100.0) as i32;
    let minutes = v - (degrees as f64) * 100.0;
    if !(0.0..60.0).contains(&minutes) {
        return None;
    }
    let mut result = degrees as f64 + minutes / 60.0;
    if hemisphere == "S" || hemisphere == "W" {
        result = -result;
    }
    Some(result)
}

/// Parse one or more NMEA sentences into a `GpsData`.
///
/// Supported: GGA, RMC, GSA, GSV. Multiple sentences in `nmea` are merged
/// into the returned value (later fields override earlier ones, with the
/// specific exceptions documented at the top of this module).
pub fn parse_gps(nmea: &str) -> GpsData {
    let mut lat = None;
    let mut lon = None;
    let mut alt = None;
    let mut speed = None;
    let mut course = None;
    let mut satellites: u32 = 0;
    let mut satellites_in_view: u32 = 0;
    let mut hdop = None;
    let mut vdop = None;
    let mut pdop = None;
    let mut fix_type: u32 = 0;
    let mut valid = false;

    for line in nmea.lines() {
        let line = line.trim();
        if !line.starts_with('$') {
            continue;
        }
        let sentence = line.split_once('*').map(|(s, _)| s).unwrap_or(line);
        let fields: Vec<&str> = sentence.split(',').collect();
        let Some(&stype) = fields.first() else {
            continue;
        };

        match stype {
            "$GPGGA" | "$GNGGA" => {
                if fields.len() < 10 {
                    continue;
                }
                lat = nmea_coord(fields[2], fields[3]);
                lon = nmea_coord(fields[4], fields[5]);
                let fix: i32 = fields[6].parse().unwrap_or(0);
                satellites = fields[7].parse().unwrap_or(0);
                hdop = fields[8].parse().ok();
                alt = fields[9].parse().ok();
                valid = fix > 0;
            }
            "$GPRMC" | "$GNRMC" => {
                if fields.len() < 9 {
                    continue;
                }
                valid = fields[2] == "A";
                let rmc_lat = nmea_coord(fields[3], fields[4]);
                let rmc_lon = nmea_coord(fields[5], fields[6]);
                if lat.is_none() {
                    lat = rmc_lat;
                    lon = rmc_lon;
                }
                // RMC speed is in knots; convert to m/s.
                speed = fields[7].parse::<f64>().ok().map(|v| v * 0.514_444);
                course = fields[8].parse().ok();
            }
            "$GPGSA" | "$GNGSA" => {
                if fields.len() < 18 {
                    continue;
                }
                fix_type = fields[2].parse().unwrap_or(0);
                satellites = fields[3..15].iter().filter(|s| !s.is_empty()).count() as u32;
                pdop = fields[15].parse().ok();
                hdop = fields[16].parse().ok();
                vdop = fields[17].parse().ok();
                valid = matches!(fix_type, 2 | 3);
            }
            "$GPGSV" | "$GNGSV" => {
                if fields.len() < 4 {
                    continue;
                }
                satellites_in_view = fields[3].parse().unwrap_or(0);
            }
            _ => {}
        }
    }

    GpsData {
        latitude: lat,
        longitude: lon,
        altitude: alt,
        speed,
        course,
        satellites,
        satellites_in_view,
        hdop,
        vdop,
        pdop,
        fix_type,
        valid,
        update_rate: 0.0,
        source: "USB GPS".to_string(),
        timestamp: 0,
    }
}

// ---------------------------------------------------------------------------
// Merge state
// ---------------------------------------------------------------------------

struct GpsState {
    data: GpsData,
    packets_received: u64,
    connection_time: Option<Instant>,
}

impl Default for GpsState {
    fn default() -> Self {
        Self {
            data: GpsData::default(),
            packets_received: 0,
            connection_time: None,
        }
    }
}

impl GpsState {
    fn reset_rate(&mut self) {
        self.packets_received = 0;
        self.connection_time = Some(Instant::now());
    }

    fn merge(&mut self, new: GpsData) {
        self.packets_received += 1;

        let elapsed_secs = self
            .connection_time
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
            .max(1.0);
        let update_rate = (self.packets_received as f64 / elapsed_secs).round_ties_even();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        self.data = GpsData {
            latitude: new.latitude.or(self.data.latitude),
            longitude: new.longitude.or(self.data.longitude),
            altitude: new.altitude.or(self.data.altitude),
            speed: new.speed.or(self.data.speed),
            course: new.course.or(self.data.course),
            satellites: if new.satellites != 0 {
                new.satellites
            } else {
                self.data.satellites
            },
            satellites_in_view: if new.satellites_in_view != 0 {
                new.satellites_in_view
            } else {
                self.data.satellites_in_view
            },
            hdop: new.hdop.or(self.data.hdop),
            vdop: new.vdop.or(self.data.vdop),
            pdop: new.pdop.or(self.data.pdop),
            fix_type: if new.fix_type != 0 {
                new.fix_type
            } else {
                self.data.fix_type
            },
            valid: new.valid,
            update_rate,
            source: self.data.source.clone(),
            timestamp: if new.latitude.is_some() {
                now
            } else {
                self.data.timestamp
            },
        };
    }
}

// ---------------------------------------------------------------------------
// Gps handle
// ---------------------------------------------------------------------------

pub struct Gps {
    port: Option<Box<dyn SerialPort>>,
    buffer: Vec<u8>,
    state: GpsState,
}

impl Default for Gps {
    fn default() -> Self {
        Self::new()
    }
}

impl Gps {
    pub fn new() -> Self {
        Self {
            port: None,
            buffer: Vec::new(),
            state: GpsState::default(),
        }
    }

    /// Open the serial port. A short timeout lets each `poll()` return
    /// promptly even if no data arrives.
    pub fn open(&mut self, port_name: &str, baud_rate: u32) -> Result<()> {
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(2))
            .open()?;
        self.port = Some(port);
        // Preserve data across reopen (matching Python), but reset the
        // rate counter — the Python bug was rate inflation across sessions.
        self.state.reset_rate();
        Ok(())
    }

    pub fn close(&mut self) {
        self.port = None; // Drop closes the underlying handle.
        self.buffer.clear();
    }

    pub fn is_open(&self) -> bool {
        self.port.is_some()
    }

    pub fn data(&self) -> &GpsData {
        &self.state.data
    }

    pub fn packets_received(&self) -> u64 {
        self.state.packets_received
    }

    /// Drain available bytes from the port, parse complete lines, and merge
    /// them into the current state.
    ///
    /// Returns `true` if at least one sentence was parsed. Callers can use
    /// this to gate UI updates, or ignore it and always render `data()`.
    pub fn poll(&mut self) -> bool {
        let Some(port) = self.port.as_mut() else {
            return false;
        };

        // Fast path: nothing to read. `bytes_to_read` is a hint, not a
        // guarantee — if it fails or lies, the fallback `read` below picks
        // up the data with its 2 ms timeout.
        if let Ok(0) = port.bytes_to_read() {
            return false;
        }

        let mut buf = [0u8; 512];
        let n = match port.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => n,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => return false,
            Err(_) => return false,
        };
        self.buffer.extend_from_slice(&buf[..n]);

        let mut updated = false;
        while let Some(pos) = find_crlf(&self.buffer) {
            let line_bytes: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            let Ok(sentence) = std::str::from_utf8(&line_bytes[..pos]) else {
                continue;
            };
            let sentence = sentence.trim();
            if sentence.is_empty() || !sentence.starts_with('$') {
                continue;
            }
            self.state.merge(parse_gps(sentence));
            updated = true;
        }
        updated
    }
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn fmt_opt(v: Option<f64>, width: usize, decimals: usize) -> String {
    match v {
        None => format!("{:>1$}", "---", width),
        Some(x) => format!("{:>1$.2$}", x, width, decimals),
    }
}

/// Human-readable one-line GPS status. Matches the Python `format_gps`.
pub fn format_gps(data: &GpsData) -> String {
    format!(
        "Source: {} | Lat: {} | Lon: {} | Alt: {} m | Speed: {} m/s | Course: {}° | Sats: {:2}/{:2} | HDOP: {} | Rate: {} Hz | Fix: {}",
        data.source,
        fmt_opt(data.latitude, 10, 6),
        fmt_opt(data.longitude, 10, 6),
        fmt_opt(data.altitude, 7, 1),
        fmt_opt(data.speed, 6, 2),
        fmt_opt(data.course, 6, 1),
        data.satellites,
        data.satellites_in_view,
        fmt_opt(data.hdop, 4, 1),
        fmt_opt(Some(data.update_rate), 3, 1),
        if data.valid { "YES" } else { "NO " },
    )
}
