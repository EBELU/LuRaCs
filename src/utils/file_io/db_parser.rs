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
    pub save_interval: f64,
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
    timestamp: f64,
    avg_cps: f64,
    avg_dr: Option<f64>,
    temperature: Option<f64>,
    latitude: Option<f64>,
    longitude: Option<f64>,
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

        let created_secs: f64 = row.get(0)?;
        let device_id: String = row.get(1)?;
        let channels_i64: i64 = row.get(2)?;
        let calibration_raw: Option<String> = row.get(3)?;
        let concat: i64 = row.get(4)?;
        let save_interval: f64 = row.get(5)?;
        let meta: String = row.get(6)?;

        let calibration: Vec<f64> = match calibration_raw {
            Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
            _ => Vec::new(),
        };

        let created = Local
            .timestamp_opt(created_secs as i64, 0)
            .single()
            .ok_or(DbParseError::InvalidTimestamp(created_secs as i64))?;

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
        let last_update_secs: f64 = row.get(3)?;

        let total_spectrum = match acc_blob {
            Some(blob) if !blob.is_empty() => {
                let counts = decompress_spectrum(&blob, self.channels)?;
                Array1::from_iter(counts.into_iter().map(|v| v as f64))
            }
            _ => Array1::zeros(self.channels),
        };

        let last_update = Local
            .timestamp_opt(last_update_secs as i64, 0)
            .single()
            .ok_or(DbParseError::InvalidTimestamp(last_update_secs as i64))?;

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
                .timestamp_opt(r.timestamp as i64, 0)
                .single()
                .ok_or(DbParseError::InvalidTimestamp(r.timestamp as i64))?;

            timestamps.push(r.timestamp);
            timestamps_datetime.push(dt);

            // Preserve Python's "0 means unknown → NaN" semantics.
            temperatures.push(match r.temperature {
                Some(t) if t != 0.0 => (t / 1000.0) as f32,
                _ => f32::NAN,
            });
            avg_cps.push((r.avg_cps / 1000.0) as f32);
            avg_dr.push(match r.avg_dr {
                Some(d) if d != 0.0 => (d / 1000.0) as f32,
                _ => f32::NAN,
            });

            latitudes.push(r.latitude.unwrap_or(f64::NAN));
            longitudes.push(r.longitude.unwrap_or(f64::NAN));

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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::utils::compress_spectrum;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    pub fn build_test_db_connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE header (
                id INTEGER PRIMARY KEY,
                created REAL,
                device_id TEXT,
                channels INTEGER,
                calibration TEXT,
                concat INTEGER,
                save_interval REAL,
                meta TEXT
            );
            CREATE TABLE summary (
                id INTEGER PRIMARY KEY,
                accumulated_spectrum BLOB,
                total_duration REAL,
                total_dose REAL,
                last_update REAL
            );
            CREATE TABLE spectrogram (
                timestamp REAL,
                avg_cps REAL,
                avg_dr REAL,
                temperature REAL,
                latitude REAL,
                longitude REAL,
                spectrum BLOB,
                meta BLOB
            );",
        )
        .unwrap();

        let spec_blob = compress_spectrum(&[10, 20, 30, 40]).unwrap();
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(br#"{"note":"ok"}"#).unwrap();
        let meta_blob = enc.finish().unwrap();

        conn.execute(
            "INSERT INTO header VALUES (1, 1700000000.0, 'RC-102', 4, '[0.0, 2.0]', 2, 1.0, '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO summary VALUES (1, ?1, 20.0, 0.5, 1700000020.0)",
            [&spec_blob],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spectrogram VALUES (1700000005.0, 5000.0, 120.0, 25000.0, 48.1, 11.5, ?1, ?2)",
            rusqlite::params![&spec_blob, &meta_blob],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spectrogram VALUES (1700000010.0, 6000.0, NULL, 0.0, NULL, NULL, ?1, NULL)",
            rusqlite::params![&spec_blob],
        )
        .unwrap();

        conn
    }

    #[test]
    fn test_db_parser_queries_and_close() {
        let conn = build_test_db_connection();
        let mut parser = DbParser::from_connection(conn).unwrap();

        let header = parser.get_header().unwrap();
        assert_eq!(header.device_id, "RC-102");
        assert_eq!(header.channels, 4);
        assert_eq!(header.calibration, vec![0.0, 2.0]);

        let summary = parser.get_summary().unwrap();
        assert!((summary.total_duration - 20.0).abs() < 1e-12);
        assert_eq!(summary.total_spectrum.as_slice().unwrap(), &[10.0, 20.0, 30.0, 40.0]);

        let cols = parser.get_spectrogram_by_date(None, None).unwrap();
        assert_eq!(cols.timestamps.len(), 2);
        assert_eq!(cols.spectra.dim(), (2, 4));
        assert!(cols.temperatures[1].is_nan());

        let t0 = Local.timestamp_opt(1_700_000_000, 0).unwrap();
        let t1 = Local.timestamp_opt(1_700_000_008, 0).unwrap();
        let filtered = parser.get_spectrogram_by_date(Some(t0), Some(t1)).unwrap();
        assert_eq!(filtered.timestamps.len(), 1);

        parser.close();
        assert!(matches!(parser.get_header(), Err(DbParseError::Closed)));
    }
}
