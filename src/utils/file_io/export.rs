//! CSV and XLSX export for spectra and ROIs.
//!
//! Both formats consume the same `Cell`-based rows, so the columns are
//! defined once. Previously the Python code duplicated the ROI table twice
//! and had a `[None] * 7` where it should have been `[None] * 12`; the
//! `roi_row` / `write_roi_table` pair below keeps headers and rows in lockstep.

use ndarray::Array1;
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

use crate::containers::{Emission, EmissionOrigin, Roi, Spectrum};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CsvXlsxError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("XLSX error: {0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),
}

pub type CsvXlsxResult<T> = std::result::Result<T, CsvXlsxError>;

// ---------------------------------------------------------------------------
// Cell — the shared value representation both writers consume
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Cell {
    Text(String),
    Number(f64),
    /// Matches Python's `round(value, decimals)`. `decimals == 0` is
    /// Python's plain `round(value)` (banker's rounding).
    Rounded {
        value: f64,
        decimals: u32,
    },
    Empty,
}

impl Cell {
    fn to_csv_field(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Number(v) => v.to_string(),
            Self::Rounded { value, decimals } => {
                format!("{:.*}", *decimals as usize, value)
            }
            Self::Empty => String::new(),
        }
    }
}

/// Round using `format!`, which matches Python's banker's rounding.
fn round_to(value: f64, decimals: u32) -> f64 {
    if !value.is_finite() {
        return value;
    }
    format!("{:.*}", decimals as usize, value)
        .parse()
        .unwrap_or(value)
}

fn write_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &Cell,
) -> std::result::Result<(), XlsxError> {
    match cell {
        Cell::Text(s) => {
            ws.write_string(row, col, s)?;
        }
        Cell::Number(v) => {
            ws.write_number(row, col, *v)?;
        }
        Cell::Rounded { value, decimals } => {
            let rounded = round_to(*value, *decimals);
            if *decimals == 0 {
                // Excel writes integer-looking values without a decimal
                // point when the number is whole.
                ws.write_number(row, col, rounded.round())?;
            } else {
                let fmt =
                    Format::new().set_num_format(format!("0.{}", "0".repeat(*decimals as usize)));
                ws.write_number_with_format(row, col, rounded, &fmt)?;
            }
        }
        Cell::Empty => { /* leave the cell blank */ }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Headers — single source of truth
// ---------------------------------------------------------------------------

const SPECTRUM_HEADERS: [&str; 3] = ["Energy/Channel [keV]", "Counts", "CPS"];

const ROI_HEADERS: [&str; 20] = [
    "Alias",
    "Lower Bound [keV]",
    "Upper Bound [keV]",
    "ROI Counts",
    "Live Time [s]",
    "Centroid [keV]",
    "FWHM [keV]",
    "FWHM Std [keV]",
    "Amplitude",
    "Peak Counts",
    "Peak Counts Std",
    "G",
    "G std",
    "B",
    "B std",
    "N",
    "N std",
    "Nuclide",
    "Photopeak [keV]",
    "Intensity [%]",
];

const ROI_COLS: usize = ROI_HEADERS.len();

// ---------------------------------------------------------------------------
// Row builders — the only place column data is produced
// ---------------------------------------------------------------------------

fn spectrum_rows(spectrum: &Spectrum) -> Vec<[Cell; 3]> {
    let acc = spectrum
        .get_foreground(false, false)
        .unwrap_or_else(|| Array1::zeros(spectrum.channels));
    let cps = spectrum
        .get_foreground(false, true)
        .unwrap_or_else(|| Array1::zeros(spectrum.channels));

    spectrum
        .x_axis
        .iter()
        .zip(acc.iter())
        .zip(cps.iter())
        .map(|((&e, &c), &cp)| [Cell::Number(e), Cell::Number(c), Cell::Number(cp)])
        .collect()
}

fn background_rows(spectrum: &Spectrum) -> Option<Vec<[Cell; 3]>> {
    spectrum.background.as_ref()?;
    let acc = spectrum.get_background(false, false)?;
    let cps = spectrum
        .get_background(false, true)
        .unwrap_or_else(|| Array1::zeros(spectrum.channels));

    Some(
        spectrum
            .x_axis
            .iter()
            .zip(acc.iter())
            .zip(cps.iter())
            .map(|((&e, &c), &cp)| [Cell::Number(e), Cell::Number(c), Cell::Number(cp)])
            .collect(),
    )
}

fn emission_nuclide_name(em: &Emission) -> String {
    if let Some(n) = &em.parent_nuclide {
        return n.to_name();
    }
    match em.origin {
        Some(EmissionOrigin::Annihilation) => "Annihilation".into(),
        _ => String::new(),
    }
}

fn roi_row(roi: &Roi) -> [Cell; ROI_COLS] {
    let mut row: Vec<Cell> = Vec::with_capacity(ROI_COLS);

    row.push(Cell::Text(roi.alias.clone()));
    row.push(Cell::Rounded {
        value: roi.roi_bound.0,
        decimals: 0,
    });
    row.push(Cell::Rounded {
        value: roi.roi_bound.1,
        decimals: 0,
    });
    row.push(Cell::Number(roi.roi_counts));
    row.push(Cell::Number(roi.live_time));

    match &roi.fit {
        Some(fit) => {
            row.extend([
                Cell::Rounded {
                    value: fit.mu(),
                    decimals: 4,
                },
                Cell::Rounded {
                    value: fit.fwhm(),
                    decimals: 4,
                },
                Cell::Rounded {
                    value: fit.fwhm_err(),
                    decimals: 4,
                },
                Cell::Rounded {
                    value: fit.a(),
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.peak_area,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.peak_area_err,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.g,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.g_err,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.b,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.b_err,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.n,
                    decimals: 0,
                },
                Cell::Rounded {
                    value: fit.n_err,
                    decimals: 0,
                },
            ]);
        }
        None => {
            // 12 fit columns, matching the header — the Python code used 7,
            // which misaligned every subsequent column in rows without a fit.
            row.extend(std::iter::repeat_n(Cell::Empty, 12));
        }
    }

    match &roi.emission {
        Some(em) => {
            row.push(Cell::Text(emission_nuclide_name(em)));
            row.push(
                em.energy_kev
                    .as_ref()
                    .map(|q| Cell::Number(q.value.mean()))
                    .unwrap_or(Cell::Empty),
            );
            row.push(
                em.intensity_percent
                    .as_ref()
                    .map(|q| Cell::Number(q.value.mean()))
                    .unwrap_or(Cell::Empty),
            );
        }
        None => {
            row.extend(std::iter::repeat_n(Cell::Empty, 3));
        }
    }

    debug_assert_eq!(row.len(), ROI_COLS);
    row.try_into()
        .unwrap_or_else(|_| unreachable!("roi_row must produce exactly ROI_COLS cells"))
}

fn with_extension(path: impl AsRef<Path>, ext: &str) -> PathBuf {
    path.as_ref().with_extension(ext)
}

// ---------------------------------------------------------------------------
// CSV writer
// ---------------------------------------------------------------------------

pub mod csv_export {
    use super::*;

    pub fn spectrum(spectrum: &Spectrum, path: impl AsRef<Path>) -> CsvXlsxResult<()> {
        let mut wtr = csv::Writer::from_path(with_extension(path, "csv"))?;
        wtr.write_record(SPECTRUM_HEADERS)?;
        for row in spectrum_rows(spectrum) {
            wtr.write_record(row.iter().map(Cell::to_csv_field))?;
        }
        wtr.flush()?;
        Ok(())
    }

    pub fn rois<'a>(
        rois: impl IntoIterator<Item = &'a Roi>,
        path: impl AsRef<Path>,
    ) -> CsvXlsxResult<()> {
        let mut wtr = csv::Writer::from_path(with_extension(path, "csv"))?;
        wtr.write_record(ROI_HEADERS)?;
        for roi in rois {
            wtr.write_record(roi_row(roi).iter().map(Cell::to_csv_field))?;
        }
        wtr.flush()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// XLSX writer
// ---------------------------------------------------------------------------
pub mod xlsx_export {
    use super::*;

    /// Options for XLSX export. `instrument` is accepted for API stability
    /// but not written yet — the Python version stubbed it out with
    /// `if export_instrument and False`, so this preserves that behaviour.
    #[derive(Debug, Clone, Copy)]
    pub struct Options {
        pub spectra: bool,
        pub rois: bool,
        pub instrument: bool,
    }

    impl Default for Options {
        fn default() -> Self {
            Self {
                spectra: true,
                rois: true,
                instrument: false,
            }
        }
    }

    fn write_table<const N: usize>(
        ws: &mut Worksheet,
        headers: &[&str; N],
        rows: &[[Cell; N]],
    ) -> CsvXlsxResult<()> {
        for (col, h) in headers.iter().enumerate() {
            ws.write_string(0, col as u16, *h)?;
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                write_cell(ws, (r + 1) as u32, c as u16, cell)?;
            }
        }
        Ok(())
    }

    fn write_roi_table(ws: &mut Worksheet, rows: &[[Cell; ROI_COLS]]) -> CsvXlsxResult<()> {
        for (col, h) in ROI_HEADERS.iter().enumerate() {
            ws.write_string(0, col as u16, *h)?;
        }
        for (r, row) in rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                write_cell(ws, (r + 1) as u32, c as u16, cell)?;
            }
        }
        Ok(())
    }

    /// Export with the default option set (foreground + background + ROIs).
    pub fn spectrum(spectrum: &Spectrum, path: impl AsRef<Path>) -> CsvXlsxResult<()> {
        spectrum_with(spectrum, path, Options::default())
    }

    /// Export with explicit options.
    pub fn spectrum_with(
        spectrum: &Spectrum,
        path: impl AsRef<Path>,
        opts: Options,
    ) -> CsvXlsxResult<()> {
        let mut workbook = Workbook::new();
        let mut sheets_written = 0usize;

        // `worksheet_from_index` returns `Result` in current rust_xlsxwriter;
        // `?` is the right way to unwrap it.
        macro_rules! next_sheet {
            ($name:expr) => {{
                let ws = workbook.add_worksheet();
                sheets_written += 1;
                ws.set_name($name);
                ws
            }};
        }

        if opts.spectra {
            let ws = next_sheet!("Spectrum Foreground");
            let rows = spectrum_rows(spectrum);
            write_table(ws, &SPECTRUM_HEADERS, &rows)?;
        }

        if opts.spectra
            && let Some(rows) = background_rows(spectrum)
        {
            let ws = next_sheet!("Spectrum Background");
            write_table(ws, &SPECTRUM_HEADERS, &rows)?;
        }

        if opts.rois && !spectrum.rois.is_empty() {
            let ws = next_sheet!("ROIs");
            let rows: Vec<[Cell; ROI_COLS]> = spectrum.rois.values().map(roi_row).collect();
            write_roi_table(ws, &rows)?;
        }

        // If nothing was written, keep the workbook valid by renaming the
        // default sheet instead of removing it.
        if sheets_written == 0 {
            let ws = workbook.add_worksheet();
            ws.set_name("Empty");
        }

        workbook.save(with_extension(path, "xlsx"))?;
        Ok(())
    }

    /// ROI-only XLSX export (single "ROIs" sheet).
    pub fn rois<'a>(
        rois: impl IntoIterator<Item = &'a Roi>,
        path: impl AsRef<Path>,
    ) -> CsvXlsxResult<()> {
        let mut workbook = Workbook::new();
        let ws = workbook.add_worksheet();
        ws.set_name("ROIs");
        let rows: Vec<[Cell; ROI_COLS]> = rois.into_iter().map(roi_row).collect();
        write_roi_table(ws, &rows)?;
        workbook.save(with_extension(path, "xlsx"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::{BkgType, Fit, FitType, SpectrumData};

    #[test]
    fn test_csv_and_xlsx_export_spectrum_and_rois() {
        let dir = std::env::temp_dir().join("luracs_test_export");
        std::fs::create_dir_all(&dir).unwrap();

        let mut spec = Spectrum::new(3, "ExpSpec");
        let make_sd = |counts: Vec<f64>| SpectrumData {
            y_axis: Array1::from_vec(counts),
            channels: 3,
            total_counts: 6,
            live_time: 2.0,
            real_time: Some(2.0),
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: None,
            instrument: None,
        };
        spec.set_foreground(make_sd(vec![1.0, 2.0, 3.0]), None);
        spec.set_background(make_sd(vec![0.5, 1.0, 1.5]), None);

        let fit = Fit {
            region_lower: Some(0.0),
            region_upper: Some(3.0),
            lower: 0.5,
            upper: 2.5,
            params: Array1::from_vec(vec![10.0, 1.5, 1.0]),
            param_errs: Array1::from_vec(vec![1.0, 0.1, 0.2]),
            bkg_params: None,
            bkg_est_channels: 1,
            g: 6.0,
            b: 1.0,
            n: 5.0,
            peak_area: 5.0,
            g_err: 2.4,
            b_err: 1.0,
            n_err: 2.6,
            peak_area_err: 2.6,
        };
        let roi = Roi {
            tag: "r1".to_string(),
            alias: "Peak1".to_string(),
            spectrum: "ExpSpec".to_string(),
            roi_bound: (0.5, 2.5),
            region_bound: (0.0, 3.0),
            fit_type: FitType::Gaussian,
            bkg_type: BkgType::Linear,
            bkg_est_channels: 1,
            fit: Some(fit),
            roi_counts: 6.0,
            live_time: 2.0,
            emission: Some(Emission::annihilation()),
            meta: serde_json::Map::new(),
            merge: false,
            movable: true,
            poisson_weights: true,
            owner_spectrum: None,
        };
        spec.set_roi(roi);

        // CSV
        let csv_spec = dir.join("spec.csv");
        let csv_rois = dir.join("rois.csv");
        csv_export::spectrum(&spec, &csv_spec).unwrap();
        csv_export::rois(spec.rois.values(), &csv_rois).unwrap();
        assert!(csv_spec.exists());
        assert!(csv_rois.exists());

        // XLSX
        let xlsx_spec = dir.join("spec.xlsx");
        let xlsx_empty = dir.join("empty.xlsx");
        let xlsx_rois = dir.join("rois.xlsx");
        xlsx_export::spectrum(&spec, &xlsx_spec).unwrap();
        xlsx_export::spectrum_with(
            &spec,
            &xlsx_empty,
            xlsx_export::Options {
                spectra: false,
                rois: false,
                instrument: false,
            },
        )
        .unwrap();
        xlsx_export::rois(spec.rois.values(), &xlsx_rois).unwrap();
        assert!(xlsx_spec.exists());
        assert!(xlsx_empty.exists());
        assert!(xlsx_rois.exists());

        drop(std::fs::remove_dir_all(&dir));
    }
}
