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
    let minutes = (degrees as f64).mul_add(-100.0, v);
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

#[derive(Default)]
struct GpsState {
    data: GpsData,
    packets_received: u64,
    connection_time: Option<Instant>,
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
    mock: Option<super::mock_gps::MockGps>,
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
            mock: None,
            buffer: Vec::new(),
            state: GpsState::default(),
        }
    }

    /// Open a mock GPS simulation.
    pub fn new_mock() -> Self {
        let mut gps = Self::new();
        gps.open_mock();
        gps
    }

    /// Open the serial port (or a simulated GPS if `port_name` is `"Mock"`).
    /// A short timeout lets each `poll()` return promptly even if no data arrives.
    pub fn open(&mut self, port_name: &str, baud_rate: u32) -> Result<()> {
        if port_name.eq_ignore_ascii_case("mock") {
            self.open_mock();
            return Ok(());
        }

        self.mock = None;
        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(2))
            .open()?;
        self.port = Some(port);
        // Preserve data across reopen (matching Python), but reset the
        // rate counter — the Python bug was rate inflation across sessions.
        self.state.reset_rate();
        Ok(())
    }

    /// Open a simulated GPS client directly.
    pub fn open_mock(&mut self) {
        let mut mock = super::mock_gps::MockGps::new();
        mock.open();
        self.mock = Some(mock);
        self.port = None;
        self.buffer.clear();
    }

    pub fn close(&mut self) {
        self.port = None; // Drop closes the underlying handle.
        if let Some(m) = self.mock.as_mut() {
            m.close();
        }
        self.mock = None;
        self.buffer.clear();
    }

    pub fn is_open(&self) -> bool {
        self.port.is_some() || self.mock.as_ref().is_some_and(super::mock_gps::MockGps::is_open)
    }

    pub const fn is_mock(&self) -> bool {
        self.mock.is_some()
    }

    pub fn data(&self) -> &GpsData {
        self.mock
            .as_ref()
            .map_or(&self.state.data, super::mock_gps::MockGps::data)
    }

    pub fn packets_received(&self) -> u64 {
        self.mock
            .as_ref()
            .map_or(self.state.packets_received, super::mock_gps::MockGps::packets_received)
    }

    /// Drain available bytes from the port, parse complete lines, and merge
    /// them into the current state (or advance the mock GPS simulation).
    ///
    /// Returns `true` if at least one sentence was parsed or a new mock fix was generated.
    pub fn poll(&mut self) -> bool {
        if let Some(mock) = self.mock.as_mut() {
            return mock.poll();
        }

        let Some(port) = self.port.as_mut() else {
            return false;
        };

        // Fast path: nothing to read. `bytes_to_read` is a hint, not a
        // guarantee — if it fails or lies, the fallback `read` below picks
        // up the data with its 2 ms timeout.
        if matches!(port.bytes_to_read(), Ok(0)) {
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
    v.map_or_else(
        || format!("{:>1$}", "---", width),
        |x| format!("{:>1$.2$}", x, width, decimals),
    )
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gps_mock_integration() {
        let mut gps = Gps::new();
        assert!(!gps.is_open());
        assert!(!gps.is_mock());

        assert!(gps.open("Mock", 9600).is_ok());
        assert!(gps.is_open());
        assert!(gps.is_mock());

        let data = gps.data();
        assert_eq!(data.source, "Mock GPS");
        assert!(data.valid);

        gps.close();
        assert!(!gps.is_open());
        assert!(!gps.is_mock());
    }

    #[test]
    fn test_parse_gps_sentences_and_merge() {
        let gga = parse_gps("$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47");
        assert!(gga.valid);
        assert!(gga.latitude.is_some());
        assert!(gga.longitude.is_some());
        assert_eq!(gga.satellites, 8);

        let rmc = parse_gps("$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A");
        assert!(rmc.valid);
        assert!(rmc.speed.is_some());

        let gsa = parse_gps("$GPGSA,A,3,04,05,,09,12,,,24,,,,,2.5,1.3,2.1*39");
        assert!(gsa.valid);
        assert_eq!(gsa.fix_type, 3);

        let gsv = parse_gps("$GPGSV,2,1,08,01,40,083,46,02,17,308,41,12,07,344,39,14,22,228,45*75");
        assert_eq!(gsv.satellites_in_view, 8);

        let bad = parse_gps("NOT_NMEA\n$GPGGA,short\n$GPRMC,short\n$GPGSA,short\n$GPGSV,1");
        assert!(!bad.valid);

        let mut state = GpsState::default();
        state.merge(gga);
        assert!(state.data.valid);
        state.merge(rmc);
        state.merge(gsa);
        state.merge(gsv);
        assert_eq!(state.data.satellites_in_view, 8);
    }

    #[test]
    fn test_gps_new_mock_poll_and_format() {
        let mut closed = Gps::new();
        assert!(!closed.poll());
        assert_eq!(closed.packets_received(), 0);

        let mut gps = Gps::new_mock();
        assert!(gps.is_open());
        assert!(gps.is_mock());
        let _ = gps.poll();
        let data = gps.data();
        let formatted = format_gps(data);
        assert!(!formatted.is_empty());
        gps.close();

        // Default, reset_rate, find_crlf, fmt_opt(None)
        let mut def_gps = Gps::default();
        assert!(!def_gps.is_open());
        let mut st = GpsState::default();
        st.data.update_rate = 5.0;
        st.reset_rate();
        assert_eq!(find_crlf(b"abc\r\ndef"), Some(3));
        assert_eq!(find_crlf(b"abcdef"), None);
        let empty_fmt = format_gps(&GpsData::default());
        assert!(empty_fmt.contains("---"));
    }
}
