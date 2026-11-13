//! Spectrogram: rolling on-disk log of spectra, spectrogram rows, and
//! device metadata in a per-database SQLite file.
//!
//! Port of `luracs.clients.spectrogram.Spectrogram`.
//!
//! ## Structural changes from Python
//!
//! - **`WrappedSpectrogramData` is a snapshot.** Python's wrapper stored
//!   live *references* to the buffer deques, so a UI reader could observe
//!   a partially-updated buffer. It also mixed live references with
//!   `np.asarray(...)` snapshots (the arrays). We emit fully materialised
//!   snapshots, wrapped in `Arc` so cloning is cheap for the UI.
//! - **`BoundedDeque`** replaces `collections.deque(maxlen=N)` (no std
//!   equivalent; trims on push, supports resize).
//! - **`SpectrogramConfig`** replaces direct reads of `Settings.Advanced`.
//! - **Signals → `VecDeque<SpectrogramEvent>`**, drained per frame by the
//!   GUI (same pattern as Settings / ROI / InstrumentLibrary).
//!
//! ## Bugs fixed vs Python (see inline comments)
//!
//! - `resize_deque` sourced `timedelta_queue` from `timestamp_queue` (copy
//!   paste). Fixed.
//! - `receive_gps` checked `longitude` twice, `latitude` never. Fixed.
//! - `assert not np.any(spectrum_diff < 0)` crashed on counter resets.
//!   Converted to `Err(NegativeDiff)`.
//! - `sigStarted` was declared but never emitted. Now emitted from `open`.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use ndarray::Array1;
use rusqlite::{Connection, params};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::clients::{
    GpsData, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
};
use crate::utils::{CompressionError, compress_spectrum_f64, decompress_spectrum};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SpectrogramError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("compression error: {0}")]
    Compression(#[from] CompressionError),
    #[error("channel count mismatch: expected {expected}, got {got}")]
    ChannelMismatch { expected: usize, got: usize },
    #[error("negative spectrum difference at channel {channel}: {value}")]
    NegativeDiff { channel: usize, value: f64 },
    #[error("concat factor must be one of 1, 2, 4, 8 (got {0})")]
    InvalidConcatFactor(usize),
    #[error("database has no header row")]
    MissingHeader,
}

pub type Result<T> = std::result::Result<T, SpectrogramError>;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectrogramState {
    Active,
    Paused,
    Loaded,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct SpectrogramConfig {
    pub deque_length: usize,
    pub map_save_extra_gps_data: bool,
    pub map_save_extra_measurement_time_data: bool,
}

impl Default for SpectrogramConfig {
    fn default() -> Self {
        Self {
            deque_length: 256,
            map_save_extra_gps_data: false,
            map_save_extra_measurement_time_data: false,
        }
    }
}

// ---------------------------------------------------------------------------
// BoundedDeque
// ---------------------------------------------------------------------------

/// `VecDeque` with a hard capacity. Pushes trim the oldest element when the
/// deque is full; `resize` shrinks from the front.
#[derive(Debug, Clone)]
pub struct BoundedDeque<T> {
    inner: VecDeque<T>,
    capacity: usize,
}

impl BoundedDeque<f64> {
    pub fn filled_nan(capacity: usize) -> Self {
        let mut d = Self::new(capacity);
        for _ in 0..capacity {
            d.push(f64::NAN);
        }
        d
    }
}

impl<T> BoundedDeque<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, value: T) {
        while self.inner.len() >= self.capacity {
            self.inner.pop_front();
        }
        self.inner.push_back(value);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.inner.iter()
    }

    pub fn last(&self) -> Option<&T> {
        self.inner.back()
    }

    pub fn resize(&mut self, new_capacity: usize) {
        self.capacity = new_capacity;
        while self.inner.len() > new_capacity {
            self.inner.pop_front();
        }
    }
}

impl<T: Clone> BoundedDeque<T> {
    pub fn to_vec(&self) -> Vec<T> {
        self.inner.iter().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Buffers {
    pub latest_timestamp: f64,
    pub latest_spectrum: Option<Array1<f64>>,
    pub latest_gps: Option<GpsData>,
    pub accumulated_spectrum: Option<Array1<f64>>,
    pub accumulated_dose_estimate: f64,
    pub spectrum_view_queue: BoundedDeque<Array1<f64>>,
    pub timestamp_queue: BoundedDeque<f64>,
    pub timedelta_queue: BoundedDeque<f64>,
    pub temperature: f64,
    pub cps: f64,
    pub dose_rate: f64,
    pub received_values: u32,
    pub duration: f64,
    pub gps_queue: BoundedDeque<Option<GpsData>>,
    pub count_rate_queue: BoundedDeque<f64>,
    pub dose_rate_queue: BoundedDeque<f64>,
}

impl Buffers {
    pub fn new(deque_len: usize) -> Self {
        Self {
            latest_timestamp: 0.0,
            latest_spectrum: None,
            latest_gps: None,
            accumulated_spectrum: None,
            accumulated_dose_estimate: 0.0,
            spectrum_view_queue: BoundedDeque::new(deque_len),
            timestamp_queue: BoundedDeque::new(deque_len),
            timedelta_queue: BoundedDeque::new(deque_len),
            temperature: -274.0,
            cps: 0.0,
            dose_rate: 0.0,
            received_values: 0,
            duration: 0.0,
            gps_queue: BoundedDeque::new(deque_len),
            count_rate_queue: BoundedDeque::new(deque_len),
            dose_rate_queue: BoundedDeque::new(deque_len),
        }
    }
}

// ---------------------------------------------------------------------------
// WrappedSpectrogramData (snapshot)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WrappedSpectrogramData {
    pub db_name: String,
    pub instrument: String,
    pub calibration_coefficients: Vec<f64>,
    pub start_date: f64,
    pub duration: f64,
    pub concat: usize,
    pub spect_channels: usize,
    pub save_interval: f64,
    pub latest_spectrum: Option<Array1<f64>>,
    pub spectrogram: Vec<Array1<f64>>,
    pub timestamp_deque: Vec<f64>,
    pub time_delta: f64,
    pub estimated_dose: f64,
    pub status: SpectrogramState,
    pub gps_queue: Vec<Option<GpsData>>,
    pub count_rate_queue: Vec<f64>,
    pub dose_rate_queue: Vec<f64>,
}

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SpectrogramEvent {
    /// Emitted once after `open()`. Equivalent to `sigStarted` (which the
    /// Python declared but never emitted).
    Started { db_name: String },
    /// Emitted by `request_data()` or after each persisted sample.
    DataUpdated {
        db_name: String,
        data: Arc<WrappedSpectrogramData>,
    },
}

// ---------------------------------------------------------------------------
// Open mode
// ---------------------------------------------------------------------------

pub enum SpectrogramMode {
    /// Start a fresh database. If the file already exists, drop and re-init.
    New {
        save_interval: f64,
        spect_channels: usize,
        device_id: String,
        calibration_coeff: Vec<f64>,
        concat_factor: usize,
        header_meta: Value,
    },
    /// Reopen an existing database; populate buffers from its recent rows.
    Resume,
}

// ---------------------------------------------------------------------------
// Spectrogram
// ---------------------------------------------------------------------------

pub struct Spectrogram {
    pub db_name: String,
    pub db_path: PathBuf,
    connection: Connection,
    pub buffers: Buffers,
    pub paused: bool,
    pub state: SpectrogramState,

    // Fixed metadata (set at construction or load)
    device_id: String,
    spect_channels: usize,
    concat_factor: usize,
    save_interval: f64,
    calibration_coeff: Vec<f64>,
    header_meta: Value,
    start_date: f64,

    config: SpectrogramConfig,
    events: VecDeque<SpectrogramEvent>,
}

impl Spectrogram {
    /// Open (or create) a spectrogram database.
    ///
    /// `db_dir` is typically `Settings.Paths.spectrogram_library`;
    /// `db_name` is the file stem (caller strips colons for Windows
    /// portability, matching `start_spectrogram`).
    pub fn open(
        db_name: impl Into<String>,
        db_dir: &Path,
        config: SpectrogramConfig,
        mode: SpectrogramMode,
    ) -> Result<Self> {
        let db_name = db_name.into();
        std::fs::create_dir_all(db_dir)?;
        let db_path = db_dir.join(&db_name).with_extension("db");
        let connection = Connection::open(&db_path)?;

        let mut sp = Self {
            db_name: db_name.clone(),
            db_path,
            connection,
            buffers: Buffers::new(config.deque_length),
            paused: false,
            state: SpectrogramState::Loaded,
            device_id: String::new(),
            spect_channels: 0,
            concat_factor: 1,
            save_interval: 1.0,
            calibration_coeff: Vec::new(),
            header_meta: Value::Null,
            start_date: now_secs(),
            config,
            events: VecDeque::new(),
        };

        match mode {
            SpectrogramMode::Resume => {
                sp.load_from_db()?;
                // Preserve Python behaviour: resumed spectrograms start
                // paused until the caller explicitly unpauses them.
                sp.paused = true;
                sp.state = SpectrogramState::Loaded;
            }
            SpectrogramMode::New {
                save_interval,
                spect_channels,
                device_id,
                calibration_coeff,
                concat_factor,
                header_meta,
            } => {
                if !matches!(concat_factor, 1 | 2 | 4 | 8) {
                    return Err(SpectrogramError::InvalidConcatFactor(concat_factor));
                }
                sp.save_interval = save_interval;
                sp.concat_factor = concat_factor;
                sp.spect_channels = (spect_channels as f64 / concat_factor as f64).round() as usize;
                sp.device_id = device_id;
                sp.calibration_coeff = calibration_coeff;
                sp.header_meta = header_meta;
                sp.start_date = now_secs();

                if sp.db_path.exists() {
                    sp.clean_db()?;
                } else {
                    sp.init_database()?;
                }

                sp.state = SpectrogramState::Active;
            }
        }

        sp.events.push_back(SpectrogramEvent::Started {
            db_name: sp.db_name.clone(),
        });
        Ok(sp)
    }

    // ---- Event queue ----------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<SpectrogramEvent> {
        self.events.drain(..).collect()
    }

    // ---- Accessors ------------------------------------------------------

    pub fn device_id(&self) -> &str {
        &self.device_id
    }
    pub fn spect_channels(&self) -> usize {
        self.spect_channels
    }
    pub fn concat_factor(&self) -> usize {
        self.concat_factor
    }
    pub fn save_interval(&self) -> f64 {
        self.save_interval
    }
    pub fn calibration_coeff(&self) -> &[f64] {
        &self.calibration_coeff
    }
    pub fn start_date(&self) -> f64 {
        self.start_date
    }

    // ---- Snapshot -------------------------------------------------------

    fn build_snapshot(
        &self,
        latest_spectrum: Option<Array1<f64>>,
        time_delta: Option<f64>,
    ) -> Arc<WrappedSpectrogramData> {
        Arc::new(WrappedSpectrogramData {
            db_name: self.db_name.clone(),
            instrument: self.device_id.clone(),
            calibration_coefficients: self.calibration_coeff.clone(),
            start_date: self.start_date,
            duration: self.buffers.duration,
            concat: self.concat_factor,
            spect_channels: self.spect_channels,
            save_interval: self.save_interval,
            latest_spectrum: latest_spectrum.or_else(|| self.buffers.accumulated_spectrum.clone()),
            spectrogram: self.buffers.spectrum_view_queue.to_vec(),
            timestamp_deque: self.buffers.timestamp_queue.to_vec(),
            // Python's fallback is 0 (new mode) or 1 (resume). Both are
            // sentinels meaning "no recent delta"; we use 0 for both and
            // document the deviation.
            time_delta: time_delta.unwrap_or(0.0),
            estimated_dose: self.buffers.accumulated_dose_estimate,
            status: self.state,
            gps_queue: self.buffers.gps_queue.to_vec(),
            count_rate_queue: self.buffers.count_rate_queue.to_vec(),
            dose_rate_queue: self.buffers.dose_rate_queue.to_vec(),
        })
    }

    /// Emit a snapshot for the UI. Mirrors `request_data()`.
    pub fn request_data(&mut self) {
        let snapshot = self.build_snapshot(None, None);
        self.events.push_back(SpectrogramEvent::DataUpdated {
            db_name: self.db_name.clone(),
            data: snapshot,
        });
    }

    // ---- Pause / close --------------------------------------------------

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.state = SpectrogramState::Paused;
        } else {
            self.state = SpectrogramState::Active;
            // Force the next received spectrum to be treated as the first
            // diff baseline (avoids a huge bogus diff after a pause).
            self.buffers.latest_spectrum = None;
        }
    }

    pub fn close(self) {
        // Drop closes the SQLite handle.
    }

    // ---- Database setup -------------------------------------------------

    fn init_database(&self) -> Result<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS header (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                created INTEGER NOT NULL,
                device_id TEXT NOT NULL,
                channels INTEGER NOT NULL CHECK(channels > 0),
                calibration TEXT,
                concat INTEGER NOT NULL,
                save_interval REAL NOT NULL,
                meta TEXT
            );
            CREATE TABLE IF NOT EXISTS summary (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                accumulated_spectrum BLOB,
                total_duration REAL NOT NULL,
                total_dose REAL,
                last_update INTEGER
            );
            CREATE TABLE IF NOT EXISTS spectrogram (
                id INTEGER PRIMARY KEY,
                timestamp REAL NOT NULL,
                avg_cps INTEGER NOT NULL,
                avg_dr INTEGER,
                temperature INTEGER,
                latitude REAL,
                longitude REAL,
                spectrum BLOB NOT NULL,
                meta BLOB
            );
            CREATE INDEX IF NOT EXISTS idx_spectrogram_timestamp
                ON spectrogram(timestamp);",
        )?;

        self.connection.execute(
            "INSERT OR IGNORE INTO summary
             (id, accumulated_spectrum, total_duration, total_dose, last_update)
             VALUES (1, NULL, 0, 0, ?1)",
            params![now_secs()],
        )?;

        let cal_json = serde_json::to_string(&self.calibration_coeff)?;
        let meta_json = serde_json::to_string(&self.header_meta)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO header
             (id, created, device_id, channels, calibration, concat, save_interval, meta)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                self.start_date,
                self.device_id,
                self.spect_channels as i64,
                cal_json,
                self.concat_factor as i64,
                self.save_interval,
                meta_json,
            ],
        )?;

        Ok(())
    }

    fn clean_db(&self) -> Result<()> {
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);

        for table in tables {
            // Table names come from sqlite_master; safe to interpolate after
            // quoting — but table names are schema-controlled, so simple
            // identifier quoting is enough.
            self.connection
                .execute(&format!("DROP TABLE IF EXISTS \"{table}\""), [])?;
        }
        self.init_database()
    }

    // ---- Load -----------------------------------------------------------

    fn load_from_db(&mut self) -> Result<()> {
        // Header
        let (created, device_id, channels, calibration, concat, save_interval, _meta): (
            f64,
            String,
            i64,
            Option<String>,
            i64,
            f64,
            Option<String>,
        ) = self
            .connection
            .query_row(
                "SELECT created, device_id, channels, calibration,
                        concat, save_interval, meta
                 FROM header WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => SpectrogramError::MissingHeader,
                other => SpectrogramError::Sqlite(other),
            })?;

        self.start_date = created;
        self.device_id = device_id;
        self.spect_channels = channels as usize;
        self.concat_factor = concat as usize;
        self.save_interval = save_interval;
        self.calibration_coeff = match calibration {
            Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
            _ => Vec::new(),
        };

        // Summary
        let summary: Option<(Option<Vec<u8>>, f64, Option<f64>, Option<f64>)> = self
            .connection
            .query_row(
                "SELECT accumulated_spectrum, total_duration, total_dose, last_update
                 FROM summary WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();

        if let Some((acc_blob, total_duration, total_dose, last_update)) = summary {
            if let Some(blob) = acc_blob {
                let counts = decompress_spectrum(&blob, self.spect_channels)?;
                self.buffers.accumulated_spectrum =
                    Some(Array1::from_iter(counts.into_iter().map(|v| v as f64)));
            }
            self.buffers.duration = total_duration;
            self.buffers.accumulated_dose_estimate = total_dose.unwrap_or(0.0);
            self.buffers.latest_timestamp = last_update.unwrap_or(0.0);
        }

        // Recent rows for the view buffer.
        let limit = self.buffers.spectrum_view_queue.capacity() as i64;
        let mut stmt = self.connection.prepare(
            "SELECT timestamp, avg_cps, avg_dr, latitude, longitude, spectrum, meta
             FROM spectrogram
             ORDER BY timestamp DESC
             LIMIT ?1",
        )?;
        let rows: Vec<(
            f64,
            i64,
            Option<i64>,
            Option<f64>,
            Option<f64>,
            Vec<u8>,
            Option<Vec<u8>>,
        )> = stmt
            .query_map(params![limit], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;

        // Python iterates `reversed(rows)` — restore oldest-to-newest so
        // pushes preserve chronological order.
        for (ts, avg_cps, avg_dr, lat, lon, spec_blob, _meta) in rows.into_iter().rev() {
            let counts = decompress_spectrum(&spec_blob, self.spect_channels)?;
            let spectrum: Array1<f64> = Array1::from_iter(counts.into_iter().map(|v| v as f64));
            self.buffers.spectrum_view_queue.push(spectrum);
            self.buffers.timestamp_queue.push(ts);
            self.buffers.count_rate_queue.push(avg_cps as f64 / 1000.0);
            let dr = avg_dr.map(|v| v as f64 / 1000.0).unwrap_or(f64::NAN);
            self.buffers.dose_rate_queue.push(dr);
            let gps = match (lat, lon) {
                (Some(la), Some(lo)) => {
                    let mut g = GpsData::default();
                    g.latitude = Some(la);
                    g.longitude = Some(lo);
                    Some(g)
                }
                _ => None,
            };
            self.buffers.gps_queue.push(gps);
        }

        Ok(())
    }

    // ---- Receive methods ------------------------------------------------

    pub fn receive_current(&mut self, name: &str, current: &WrappedRealTimePackage) {
        if name != self.device_id || self.paused {
            return;
        }
        self.buffers.cps += current.cps;
        self.buffers.dose_rate += current.dr;
        self.buffers.received_values += 1;
    }

    pub fn receive_status(&mut self, name: &str, status: &WrappedStatusPackage) {
        if name != self.device_id || self.paused {
            return;
        }
        self.buffers.temperature = status.temperature;
    }

    pub fn receive_gps(&mut self, data: &GpsData) {
        // BUG FIX vs Python: the original checked `longitude is not None`
        // twice and never checked `latitude`.
        if data.valid && data.latitude.is_some() && data.longitude.is_some() {
            self.buffers.latest_gps = Some(data.clone());
        }
    }

    pub fn receive_spectrum(&mut self, name: &str, package: &WrappedSpectrumPackage) -> Result<()> {
        if name != self.device_id || self.paused {
            return Ok(());
        }

        let spectrum = package.y_axis.clone();

        // First sample establishes the diff baseline.
        if self.buffers.latest_spectrum.is_none() {
            self.buffers.latest_spectrum = Some(spectrum);
            self.buffers.latest_timestamp = package.timestamp;
            return Ok(());
        }

        let new_ts = package.timestamp;

        // Gate on rounded timestamp difference (Python's banker's rounding).
        let gate = (self.buffers.latest_timestamp + self.save_interval).round_ties_even();
        if new_ts.round_ties_even() < gate {
            return Ok(());
        }

        let dt = new_ts - self.buffers.latest_timestamp;
        self.buffers.latest_timestamp = new_ts;
        self.buffers.timestamp_queue.push(new_ts);

        let processed = self.process_spectrum(&spectrum)?;

        // Accumulate.
        match &mut self.buffers.accumulated_spectrum {
            Some(acc) => {
                if acc.len() != processed.len() {
                    return Err(SpectrogramError::ChannelMismatch {
                        expected: acc.len(),
                        got: processed.len(),
                    });
                }
                *acc += &processed;
            }
            None => {
                self.buffers.accumulated_spectrum = Some(processed.clone());
            }
        }

        self.buffers.spectrum_view_queue.push(processed.clone());

        let avg_cps = self.buffers.cps / self.buffers.received_values.max(1) as f64;
        let avg_dr = self.buffers.dose_rate / self.buffers.received_values.max(1) as f64;

        self.buffers.timedelta_queue.push(dt);
        self.buffers.gps_queue.push(self.buffers.latest_gps.clone());
        self.buffers.count_rate_queue.push(avg_cps);
        self.buffers.dose_rate_queue.push(avg_dr);
        self.buffers.duration += dt;
        if self.buffers.dose_rate != 0.0 {
            self.buffers.accumulated_dose_estimate += avg_dr / 3600.0 * dt;
        }

        // Emit snapshot BEFORE persistence so the UI updates promptly.
        let snapshot = self.build_snapshot(Some(processed.clone()), Some(dt));
        self.events.push_back(SpectrogramEvent::DataUpdated {
            db_name: self.db_name.clone(),
            data: snapshot,
        });

        // Persist row + refresh summary.
        let meta = self.build_meta(package);
        self.insert_spectrogram(new_ts, avg_cps, avg_dr, &processed, meta)?;
        self.update_summary(new_ts)?;

        // Reset per-interval accumulators.
        self.buffers.received_values = 0;
        self.buffers.cps = 0.0;
        self.buffers.dose_rate = 0.0;

        Ok(())
    }

    // ---- Spectrum processing -------------------------------------------

    fn process_spectrum(&mut self, spectrum: &Array1<f64>) -> Result<Array1<f64>> {
        let prev =
            self.buffers
                .latest_spectrum
                .as_ref()
                .ok_or(SpectrogramError::ChannelMismatch {
                    expected: 0,
                    got: spectrum.len(),
                })?;

        if spectrum.len() != prev.len() {
            return Err(SpectrogramError::ChannelMismatch {
                expected: prev.len(),
                got: spectrum.len(),
            });
        }

        let mut diff = Array1::<f64>::zeros(spectrum.len());
        for i in 0..spectrum.len() {
            let d = spectrum[i] - prev[i];
            if d < 0.0 {
                return Err(SpectrogramError::NegativeDiff {
                    channel: i,
                    value: d,
                });
            }
            diff[i] = d;
        }

        self.buffers.latest_spectrum = Some(spectrum.clone());

        // Concat: pad to a multiple of concat_factor, reshape, sum groups.
        let cf = self.concat_factor;
        let pad = (cf - (diff.len() % cf)) % cf;
        let padded_len = diff.len() + pad;
        let n_out = padded_len / cf;
        let mut result = Array1::<f64>::zeros(n_out);
        for i in 0..n_out {
            let mut acc = 0.0;
            for k in 0..cf {
                let idx = i * cf + k;
                if idx < diff.len() {
                    acc += diff[idx];
                }
            }
            result[i] = acc;
        }
        Ok(result)
    }

    // ---- Persistence ---------------------------------------------------

    fn build_meta(&self, package: &WrappedSpectrumPackage) -> Value {
        let mut map = Map::new();
        if let Some(gps) = &self.buffers.latest_gps {
            if self.config.map_save_extra_gps_data {
                map.insert(
                    "extra_gps".into(),
                    json!({
                        "alt": gps.altitude,
                        "course": gps.course,
                        "speed": gps.speed,
                        "hdop": gps.hdop,
                        "vdop": gps.vdop,
                        "ts": gps.timestamp,
                    }),
                );
            }
        }
        if self.config.map_save_extra_measurement_time_data {
            map.insert(
                "extra_measurement_time".into(),
                json!({
                    "live_time": package.live_time,
                    "real_time": package.real_time,
                }),
            );
        }
        Value::Object(map)
    }

    fn insert_spectrogram(
        &self,
        timestamp: f64,
        avg_cps: f64,
        avg_dr: f64,
        processed: &Array1<f64>,
        meta: Value,
    ) -> Result<()> {
        let avg_cps_int: i64 = (avg_cps * 1000.0).round() as i64;
        let avg_dr_int: Option<i64> = if avg_dr.is_nan() {
            None
        } else {
            Some((avg_dr * 1000.0).round() as i64)
        };
        let temp_int: Option<i64> = if self.buffers.temperature.is_nan() {
            None
        } else {
            Some((self.buffers.temperature * 1000.0).round() as i64)
        };
        let (lat, lon) = match &self.buffers.latest_gps {
            Some(g) => (g.latitude, g.longitude),
            None => (None, None),
        };

        let spectrum_bytes = compress_spectrum_f64(&processed.to_vec())?;

        let meta_bytes: Option<Vec<u8>> = if meta.as_object().map(|m| m.is_empty()).unwrap_or(true)
        {
            None
        } else {
            let raw = serde_json::to_vec(&meta)?;
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(6));
            use std::io::Write;
            enc.write_all(&raw)?;
            Some(enc.finish()?)
        };

        self.connection.execute(
            "INSERT INTO spectrogram
             (timestamp, avg_cps, avg_dr, temperature,
              latitude, longitude, spectrum, meta)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                timestamp,
                avg_cps_int,
                avg_dr_int,
                temp_int,
                lat,
                lon,
                spectrum_bytes,
                meta_bytes,
            ],
        )?;
        Ok(())
    }

    fn update_summary(&self, ts: f64) -> Result<()> {
        let acc_bytes = match &self.buffers.accumulated_spectrum {
            Some(acc) => compress_spectrum_f64(&acc.to_vec())?,
            None => Vec::new(),
        };
        let dose = if self.buffers.accumulated_dose_estimate != 0.0 {
            self.buffers.accumulated_dose_estimate
        } else {
            0.0
        };
        self.connection.execute(
            "UPDATE summary
             SET total_duration = ?1,
                 total_dose = ?2,
                 accumulated_spectrum = ?3,
                 last_update = ?4
             WHERE id = 1",
            params![self.buffers.duration, dose, acc_bytes, ts],
        )?;
        Ok(())
    }

    // ---- Deque management ----------------------------------------------

    /// Resize the rolling buffers. Python's `resize_deque` also had a bug
    /// (used `timestamp_queue` for the `timedelta_queue` source) — fixed.
    pub fn resize_deque(&mut self, new_len: usize) {
        self.buffers.spectrum_view_queue.resize(new_len);
        self.buffers.timestamp_queue.resize(new_len);
        self.buffers.timedelta_queue.resize(new_len);
        self.buffers.gps_queue.resize(new_len);
        self.buffers.count_rate_queue.resize(new_len);
        self.buffers.dose_rate_queue.resize(new_len);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// Silence the unused warning when `meta` is always empty.
#[allow(dead_code)]
fn zlib_decompress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut dec = ZlibDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}
