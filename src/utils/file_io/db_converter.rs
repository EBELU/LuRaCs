//! Convert a spectrogram database to XLSX, or extract a `Spectrum` from it.
//!
//! The three helpers below build a `Sheet` per worksheet from the concrete
//! Rust types (`DbHeader`, `DbSummary`, `DbDataColumns`) rather than going
//! through reflection as the Python version did. `Sheet::write` is the
//! `zip_longest(..., fillvalue="")` equivalent: shorter columns are padded
//! with blank cells.

use std::path::Path;

use chrono::{DateTime, Local};
use ndarray::{Array1, Axis};
use rust_xlsxwriter::{Workbook, Worksheet, XlsxError};
use thiserror::Error;

use crate::containers::{Spectrum, SpectrumData};

use super::db_parser::{DbDataColumns, DbHeader, DbParseError, DbParser, DbSummary};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DbWriterError {
    #[error("XLSX error: {0}")]
    Xlsx(#[from] XlsxError),

    #[error("database parse error: {0}")]
    DbParse(#[from] DbParseError),
}

pub type DbWriterResult<T> = std::result::Result<T, DbWriterError>;

// ---------------------------------------------------------------------------
// Cell / Sheet — the write primitive
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Cell {
    Str(String),
    Int(i64),
    Float(f64),
    Empty,
}

impl Cell {
    fn write(&self, ws: &mut Worksheet, row: u32, col: u16) -> std::result::Result<(), XlsxError> {
        match self {
            Cell::Str(s) => {
                ws.write_string(row, col, s)?;
            }
            Cell::Int(i) => {
                ws.write_number(row, col, *i as f64)?;
            }
            Cell::Float(f) => {
                ws.write_number(row, col, *f)?;
            }
            Cell::Empty => {} // leave the cell blank
        }
        Ok(())
    }
}

struct Sheet {
    headers: Vec<String>,
    columns: Vec<Vec<Cell>>,
}

impl Sheet {
    fn new() -> Self {
        Self {
            headers: Vec::new(),
            columns: Vec::new(),
        }
    }

    fn push(&mut self, header: impl Into<String>, cells: Vec<Cell>) {
        self.headers.push(header.into());
        self.columns.push(cells);
    }

    fn scalar(&mut self, header: impl Into<String>, cell: Cell) {
        self.push(header, vec![cell]);
    }

    fn write(self, ws: &mut Worksheet) -> std::result::Result<(), XlsxError> {
        for (col, h) in self.headers.iter().enumerate() {
            ws.write_string(0, col as u16, h)?;
        }
        let n_rows = self.columns.iter().map(Vec::len).max().unwrap_or(0);
        for r in 0..n_rows {
            for (c, col) in self.columns.iter().enumerate() {
                let cell = col.get(r).unwrap_or(&Cell::Empty);
                cell.write(ws, (r + 1) as u32, c as u16)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Per-struct sheet builders
// ---------------------------------------------------------------------------

fn header_to_sheet(h: &DbHeader) -> Sheet {
    let mut s = Sheet::new();
    s.scalar("created", Cell::Str(h.created.to_rfc3339()));
    s.scalar("device_id", Cell::Str(h.device_id.clone()));
    s.scalar("channels", Cell::Int(h.channels as i64));
    s.scalar("concat", Cell::Int(h.concat));
    s.scalar("save_interval", Cell::Int(h.save_interval));
    s.push(
        "calibration",
        h.calibration.iter().map(|c| Cell::Float(*c)).collect(),
    );
    s.scalar("meta", Cell::Str(h.meta.clone()));
    s
}

fn summary_to_sheet(sum: &DbSummary) -> Sheet {
    let mut s = Sheet::new();
    s.scalar("total_duration", Cell::Float(sum.total_duration));
    s.scalar("total_dose", Cell::Float(sum.total_dose));
    s.scalar("last_update", Cell::Str(sum.last_update.to_rfc3339()));
    s.push(
        "total_spectrum",
        sum.total_spectrum.iter().map(|v| Cell::Float(*v)).collect(),
    );
    s
}

fn data_to_sheet(d: &DbDataColumns, include_spectra: bool) -> Sheet {
    let mut s = Sheet::new();

    s.push(
        "timestamps",
        d.timestamps.iter().map(|v| Cell::Float(*v)).collect(),
    );
    s.push(
        "timestamps_datetime",
        d.timestamps_datetime
            .iter()
            .map(|dt| Cell::Str(dt.to_rfc3339()))
            .collect(),
    );
    s.push(
        "temperatures",
        d.temperatures
            .iter()
            .map(|v| Cell::Float(*v as f64))
            .collect(),
    );
    s.push(
        "avg_cps",
        d.avg_cps.iter().map(|v| Cell::Float(*v as f64)).collect(),
    );
    s.push(
        "avg_dr",
        d.avg_dr.iter().map(|v| Cell::Float(*v as f64)).collect(),
    );
    s.push(
        "longitude",
        d.longitude.iter().map(|v| Cell::Float(*v)).collect(),
    );
    s.push(
        "latitude",
        d.latitude.iter().map(|v| Cell::Float(*v)).collect(),
    );

    if include_spectra {
        let (n, ch) = (d.spectra.nrows(), d.spectra.ncols());
        for c in 0..ch {
            let col: Vec<Cell> = (0..n)
                .map(|r| Cell::Int(d.spectra[[r, c]] as i64))
                .collect();
            s.push(format!("Ch{c}"), col);
        }
    }

    s.push(
        "meta",
        d.meta.iter().map(|v| Cell::Str(v.to_string())).collect(),
    );
    s
}

// ---------------------------------------------------------------------------
// db_writer
// ---------------------------------------------------------------------------

pub struct DbWriter;

impl DbWriter {
    /// Export the full database to a workbook with three sheets: `Header`,
    /// `Summary`, `Spectrogram`.
    ///
    /// `include_spectrogram_data = false` omits the per-channel `Ch0..ChN`
    /// columns; all other spectrogram columns are still written.
    pub fn export_full_xlsx(
        parser: &DbParser,
        new_name: impl AsRef<Path>,
        include_spectrogram_data: bool,
    ) -> DbWriterResult<()> {
        let mut workbook = Workbook::new();

        {
            let header = parser.get_header()?;
            let ws = workbook.worksheet_from_index(0)?;
            ws.set_name("Header");
            header_to_sheet(&header).write(ws)?;
        }

        {
            let summary = parser.get_summary()?;
            let ws = workbook.add_worksheet();
            ws.set_name("Summary");
            summary_to_sheet(&summary).write(ws)?;
        }

        {
            let data = parser.get_spectrogram_by_date(None, None)?;
            let ws = workbook.add_worksheet();
            ws.set_name("Spectrogram");
            data_to_sheet(&data, include_spectrogram_data).write(ws)?;
        }

        workbook.save(new_name.as_ref())?;
        Ok(())
    }

    /// Build a `Spectrum` from either the accumulated summary (when both
    /// date limits are `None`) or a summed slice of the spectrogram rows.
    pub fn build_spectrum_from_db(
        parser: &DbParser,
        new_name: impl AsRef<Path>,
        start_time: Option<DateTime<Local>>,
        stop_time: Option<DateTime<Local>>,
    ) -> DbWriterResult<Spectrum> {
        let new_path = new_name.as_ref();
        let stem = new_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("spectrum")
            .to_string();

        let header = parser.get_header()?;
        let summary = parser.get_summary()?;

        let mut new_spectrum = Spectrum::new(header.channels, stem.clone());
        new_spectrum.instrument_id = Some(header.device_id.clone());
        // Python also passed `device_id=`; its own __init__ ignores unknown
        // kwargs, so no field exists for it here.

        let (y_axis, live_time, start_date, end_date) =
            if start_time.is_none() && stop_time.is_none() {
                (
                    summary.total_spectrum.clone(),
                    summary.total_duration,
                    header.created.clone(),
                    summary.last_update.clone(),
                )
            } else {
                let data = parser.get_spectrogram_by_date(start_time, stop_time)?;

                // Sum over timestamps (axis 0) → one value per channel.
                let y_axis: Array1<f64> = data
                    .spectra
                    .map_axis(Axis(0), |col| col.iter().map(|&v| v as f64).sum::<f64>());

                if !data.timestamps_datetime.is_empty() {
                    let sd = data.timestamps_datetime.first().unwrap().clone();
                    let ed = data.timestamps_datetime.last().unwrap().clone();
                    let lt = *data.timestamps.last().unwrap() - *data.timestamps.first().unwrap();
                    (y_axis, lt, sd, ed)
                } else {
                    (y_axis, 0.0, header.created.clone(), header.created.clone())
                }
            };

        let total_counts: u64 = y_axis.sum().max(0.0) as u64;

        let spectrum_data = SpectrumData {
            y_axis: y_axis.clone(),
            channels: header.channels,
            total_counts,
            live_time,
            real_time: Some(live_time),
            avg_dose_rate: None,
            avg_cps: None,
            start_date: Some(start_date.into()),
            end_date: Some(end_date.into()),
            spectrum_name: Some(stem),
            // Python tried to set this to `header.device_id` (a String) on a
            // field typed as `UniqueInstrument` — a bug; leave it empty.
            instrument: None,
        };
        new_spectrum.set_foreground(spectrum_data, None);

        // Scale each coefficient by `concat^i` when the device concatenated
        // multiple physical channels into one logical channel.
        let mut calibration = header.calibration.clone();
        if header.concat != 1 {
            let c = header.concat as f64;
            for (i, coeff) in calibration.iter_mut().enumerate() {
                *coeff *= c.powi(i as i32);
            }
        }
        if !calibration.is_empty() {
            new_spectrum.apply_calibration(calibration);
        }

        Ok(new_spectrum)
    }
}
