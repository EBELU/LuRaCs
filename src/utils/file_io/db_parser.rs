//! SQLite parser for spectrogram / spectrum-log databases.
//!
//! Tables read:
//! - `header`      — device metadata (one row, id = 1)
//! - `summary`     — accumulated spectrum + totals (one row, id = 1)
//! - `spectrogram` — time-series of spectra, cps, dose rate, GPS, meta
//!
//! Bug fix vs. Python: `get_spectrogram_rows` now includes `meta` in its
//! SELECT (the Python omitted it but the row-unpack step expected it).

use crate::utils::{CompressionError, decompress_spectrum};
use chrono::{DateTime, Local, TimeZone};
use flate2::read::ZlibDecoder;
use ndarray::{Array1, Array2};
use rusqlite::{Connection, params, params_from_iter, types::Value as SqlValue};
use serde_json::Value as JsonValue;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DbParseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("decompression error: {0}")]
    Decompress(#[from] CompressionError),

    #[error("no header row found in database")]
    NoHeader,

    #[error("no summary row found in database")]
    NoSummary,

    #[error("invalid unix timestamp: {0}")]
    InvalidTimestamp(i64),

    #[error("spectra buffer is not row-major shape ({rows}, {channels}): {msg}")]
    ShapeMismatch {
        rows: usize,
        channels: usize,
        msg: String,
    },

    #[error("DbParser has been closed")]
    Closed,
}

pub type DbResult<T> = std::result::Result<T, DbParseError>;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DbHeader {
    pub created: DateTime<Local>,
    pub device_id: String,
    pub channels: usize,
    pub concat: i64,
    pub save_interval: i64,
    /// Polynomial coefficients (matches `Spectrum::apply_calibration`).
    pub calibration: Vec<f64>,
    pub meta: String,
}

#[derive(Debug, Clone)]
pub struct DbSummary {
    pub total_duration: f64,
    pub total_dose: f64,
    pub last_update: DateTime<Local>,
    pub total_spectrum: Array1<f64>,
}

#[derive(Debug, Clone)]
pub struct DbDataColumns {
    pub timestamps: Array1<f64>,
    pub timestamps_datetime: Vec<DateTime<Local>>,
    pub temperatures: Array1<f32>,
    pub avg_cps: Array1<f32>,
    pub avg_dr: Array1<f32>,
    pub longitude: Array1<f64>,
    pub latitude: Array1<f64>,
    /// One row per timestamp, one column per channel.
    pub spectra: Array2<i32>,
    pub meta: Vec<JsonValue>,
}

// ---------------------------------------------------------------------------
// Row shape used between rusqlite and the column builder
// ---------------------------------------------------------------------------

struct SpectrogramRow {
    timestamp: i64,
    avg_cps: f64,
    avg_dr: f64,
    temperature: f64,
    latitude: f64,
    longitude: f64,
    spectrum_blob: Vec<u8>,
    meta_blob: Option<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct DbParser {
    /// `Option` so `close()` can drop the underlying handle. Read methods
    /// return `Closed` if the parser was closed.
    connection: Option<Connection>,
    channels: usize,
}

impl DbParser {
    /// Open a database from a file path. Immediately reads the header,
    /// matching Python's `__init__`.
    pub fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Wrap an existing `rusqlite::Connection`.
    pub fn from_connection(conn: Connection) -> DbResult<Self> {
        let mut parser = Self {
            connection: Some(conn),
            channels: 0,
        };
        let header = parser.get_header()?;
        parser.channels = header.channels;
        Ok(parser)
    }

    pub fn close(&mut self) {
        self.connection = None; // Drop closes the underlying handle.
    }

    fn conn(&self) -> DbResult<&Connection> {
        self.connection.as_ref().ok_or(DbParseError::Closed)
    }

    // -----------------------------------------------------------------------
    // Header
    // -----------------------------------------------------------------------

    pub fn get_header(&self) -> DbResult<DbHeader> {
        let mut stmt = self.conn()?.prepare(
            "SELECT created, device_id, channels, calibration,
                    concat, save_interval, meta
             FROM header WHERE id = 1",
        )?;

        let mut rows = stmt.query([])?;
        let row = rows.next()?.ok_or(DbParseError::NoHeader)?;

        let created_secs: i64 = row.get(0)?;
        let device_id: String = row.get(1)?;
        let channels_i64: i64 = row.get(2)?;
        let calibration_raw: Option<String> = row.get(3)?;
        let concat: i64 = row.get(4)?;
        let save_interval: i64 = row.get(5)?;
        let meta: String = row.get(6)?;

        let calibration: Vec<f64> = match calibration_raw {
            Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
            _ => Vec::new(),
        };

        let created = Local
            .timestamp_opt(created_secs, 0)
            .single()
            .ok_or(DbParseError::InvalidTimestamp(created_secs))?;

        Ok(DbHeader {
            created,
            device_id,
            channels: channels_i64 as usize,
            concat,
            save_interval,
            calibration,
            meta,
        })
    }

    // -----------------------------------------------------------------------
    // Summary
    // -----------------------------------------------------------------------

    pub fn get_summary(&self) -> DbResult<DbSummary> {
        let mut stmt = self.conn()?.prepare(
            "SELECT accumulated_spectrum, total_duration, total_dose, last_update
             FROM summary WHERE id = 1",
        )?;

        let mut rows = stmt.query([])?;
        let row = rows.next()?.ok_or(DbParseError::NoSummary)?;

        let acc_blob: Option<Vec<u8>> = row.get(0)?;
        let total_duration: f64 = row.get(1)?;
        let total_dose: f64 = row.get(2)?;
        let last_update_secs: i64 = row.get(3)?;

        let total_spectrum = match acc_blob {
            Some(blob) if !blob.is_empty() => {
                let counts = decompress_spectrum(&blob, self.channels)?;
                Array1::from_iter(counts.into_iter().map(|v| v as f64))
            }
            _ => Array1::zeros(self.channels),
        };

        let last_update = Local
            .timestamp_opt(last_update_secs, 0)
            .single()
            .ok_or(DbParseError::InvalidTimestamp(last_update_secs))?;

        Ok(DbSummary {
            total_duration,
            total_dose,
            last_update,
            total_spectrum,
        })
    }

    // -----------------------------------------------------------------------
    // Spectrogram queries
    // -----------------------------------------------------------------------

    /// All spectrogram rows whose timestamp falls within the range, ordered
    /// oldest to newest. `start_date` / `end_date` are inclusive.
    pub fn get_spectrogram_by_date(
        &self,
        start_date: Option<DateTime<Local>>,
        end_date: Option<DateTime<Local>>,
    ) -> DbResult<DbDataColumns> {
        let mut query = String::from(
            "SELECT timestamp, avg_cps, avg_dr, temperature,
                    latitude, longitude, spectrum, meta
             FROM spectrogram",
        );

        let mut conditions: Vec<&'static str> = Vec::new();
        let mut params: Vec<SqlValue> = Vec::new();

        if let Some(sd) = start_date {
            conditions.push("timestamp >= ?");
            params.push(SqlValue::Integer(sd.timestamp()));
        }
        if let Some(ed) = end_date {
            conditions.push("timestamp <= ?");
            params.push(SqlValue::Integer(ed.timestamp()));
        }
        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY timestamp ASC");

        let mut stmt = self.conn()?.prepare(&query)?;
        let iter = stmt.query_map(params_from_iter(params), row_to_spectrogram_row)?;

        let mut rows = Vec::new();
        for r in iter {
            rows.push(r?);
        }
        self.rows_to_datacolumns(rows)
    }

    /// The `nr_of_rows` most recent spectrogram rows, ordered oldest to
    /// newest (matches Python's `list(reversed(...))`).
    ///
    /// Note: unlike the Python original, this query includes `meta`. The
    /// Python omitted it while the row builder expected it, which raised
    /// `ValueError` at runtime.
    pub fn get_spectrogram_rows(&self, nr_of_rows: usize) -> DbResult<DbDataColumns> {
        let mut stmt = self.conn()?.prepare(
            "SELECT timestamp, avg_cps, avg_dr, temperature,
                    latitude, longitude, spectrum, meta
             FROM spectrogram
             ORDER BY timestamp DESC
             LIMIT ?",
        )?;

        let iter = stmt.query_map(params![nr_of_rows as i64], row_to_spectrogram_row)?;

        let mut rows: Vec<SpectrogramRow> = Vec::with_capacity(nr_of_rows);
        for r in iter {
            rows.push(r?);
        }
        rows.reverse();
        self.rows_to_datacolumns(rows)
    }

    // -----------------------------------------------------------------------
    // Column builder
    // -----------------------------------------------------------------------

    fn rows_to_datacolumns(&self, rows: Vec<SpectrogramRow>) -> DbResult<DbDataColumns> {
        let n = rows.len();
        let ch = self.channels;

        let mut timestamps = Vec::with_capacity(n);
        let mut timestamps_datetime = Vec::with_capacity(n);
        let mut temperatures = Vec::with_capacity(n);
        let mut avg_cps = Vec::with_capacity(n);
        let mut avg_dr = Vec::with_capacity(n);
        let mut longitudes = Vec::with_capacity(n);
        let mut latitudes = Vec::with_capacity(n);
        let mut spectra_flat: Vec<i32> = Vec::with_capacity(n * ch);
        let mut meta = Vec::with_capacity(n);

        for r in rows {
            let dt = Local
                .timestamp_opt(r.timestamp, 0)
                .single()
                .ok_or(DbParseError::InvalidTimestamp(r.timestamp))?;

            timestamps.push(r.timestamp as f64);
            timestamps_datetime.push(dt);

            // Preserve Python's "0 means unknown → NaN" semantics.
            temperatures.push(if r.temperature != 0.0 {
                (r.temperature / 1000.0) as f32
            } else {
                f32::NAN
            });
            avg_cps.push((r.avg_cps / 1000.0) as f32);
            avg_dr.push(if r.avg_dr != 0.0 {
                (r.avg_dr / 1000.0) as f32
            } else {
                f32::NAN
            });

            latitudes.push(r.latitude);
            longitudes.push(r.longitude);

            let counts = decompress_spectrum(&r.spectrum_blob, ch)?;
            spectra_flat.extend(counts.into_iter().map(|v| v as i32));

            let meta_value = match r.meta_blob {
                Some(blob) if !blob.is_empty() => {
                    let mut decoder = ZlibDecoder::new(&blob[..]);
                    let mut json_str = String::new();
                    decoder.read_to_string(&mut json_str)?;
                    serde_json::from_str(&json_str)?
                }
                _ => JsonValue::Object(serde_json::Map::new()),
            };
            meta.push(meta_value);
        }

        let spectra = Array2::from_shape_vec((n, ch), spectra_flat).map_err(|e| {
            DbParseError::ShapeMismatch {
                rows: n,
                channels: ch,
                msg: e.to_string(),
            }
        })?;

        Ok(DbDataColumns {
            timestamps: Array1::from_vec(timestamps),
            timestamps_datetime,
            temperatures: Array1::from_vec(temperatures),
            avg_cps: Array1::from_vec(avg_cps),
            avg_dr: Array1::from_vec(avg_dr),
            longitude: Array1::from_vec(longitudes),
            latitude: Array1::from_vec(latitudes),
            spectra,
            meta,
        })
    }
}

fn row_to_spectrogram_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SpectrogramRow> {
    Ok(SpectrogramRow {
        timestamp: row.get(0)?,
        avg_cps: row.get(1)?,
        avg_dr: row.get(2)?,
        temperature: row.get(3)?,
        latitude: row.get(4)?,
        longitude: row.get(5)?,
        spectrum_blob: row.get(6)?,
        meta_blob: row.get(7)?,
    })
}
