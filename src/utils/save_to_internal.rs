//! Save spectra, instruments, and ROIs into the internal library directories
//! as N42 XML files.
//!
//! Port of `luracs.utils.save_to_internal`. Each function takes the target
//! library directory explicitly — the Python version read `Settings.Paths`,
//! which we don't want to reintroduce as a global.
//!
//! ## Known limitation
//!
//! `N42Writer` currently writes `UniqueInstrument` only
//! (`Spectrum.instrument: Option<Arc<UniqueInstrument>>`).
//! `save_instrument_to_library` therefore returns
//! `SaveError::GenericInstrumentUnsupported` for `GenericInstrument`.
//! Extend `n42_writer::write_instrument_data` to accept a
//! `GenericInstrument` variant and remove that branch once generic export
//! is needed.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;

use crate::containers::{GenericInstrument, Spectrum, UniqueInstrument};
use crate::utils::file_io::{N42Error, N42Writer, WriteOptions};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("N42 write error: {0}")]
    N42(#[from] N42Error),
    #[error("generic instrument export not yet supported by N42Writer")]
    GenericInstrumentUnsupported,
}

pub type SaveResult<T> = std::result::Result<T, SaveError>;

// ---------------------------------------------------------------------------
// Spectrum
// ---------------------------------------------------------------------------

/// Write `spectrum` to `<library>/<spectrum.name>.xml`.
///
/// The `.xml` extension is appended by `N42Writer::write_out`; the stem is
/// what the caller passes.
pub fn save_spectrum_to_library(spectrum: &Spectrum, library: &Path) -> SaveResult<PathBuf> {
    std::fs::create_dir_all(library)?;
    let stem = library.join(&spectrum.name);
    let new_file = stem.with_extension("xml");

    N42Writer::new(spectrum, &stem, WriteOptions::default())?.write_out()?;

    tracing::debug!(path = %new_file.display(), "spectrum saved to library");
    Ok(new_file)
}

// ---------------------------------------------------------------------------
// Instrument
// ---------------------------------------------------------------------------

/// Either instrument kind. Replaces Python's `UniqueInstrument | GenericInstrument`.
pub enum InstrumentRef<'a> {
    Unique(&'a UniqueInstrument),
    Generic(&'a GenericInstrument),
}

impl<'a> From<&'a UniqueInstrument> for InstrumentRef<'a> {
    fn from(u: &'a UniqueInstrument) -> Self {
        Self::Unique(u)
    }
}

impl<'a> From<&'a GenericInstrument> for InstrumentRef<'a> {
    fn from(g: &'a GenericInstrument) -> Self {
        Self::Generic(g)
    }
}

/// Write an instrument to its library as a stub XML file.
///
/// - Unique instruments go to `unique_library` under `instrument.name`.
/// - Generic instruments go to `generic_library` under `instrument.model`
///   — **not yet implemented** (see module doc).
pub fn save_instrument_to_library(
    instrument: InstrumentRef<'_>,
    unique_library: &Path,
    generic_library: &Path,
) -> SaveResult<PathBuf> {
    match instrument {
        InstrumentRef::Unique(u) => {
            std::fs::create_dir_all(unique_library)?;
            let stem = unique_library.join(&u.name);
            let new_file = stem.with_extension("xml");

            let mut dummy = Spectrum::new(1, "Dummy");
            dummy.instrument = Some(Arc::new(u.clone()));

            N42Writer::new(
                &dummy,
                &stem,
                WriteOptions {
                    export_spectrum: false,
                    export_rois: false,
                    export_instrument: true,
                    comment: None,
                },
            )?
            .write_out()?;

            tracing::debug!(path = %new_file.display(), "instrument saved to library");
            Ok(new_file)
        }
        InstrumentRef::Generic(_) => {
            // Building a stub file would require either a
            // `Option<Arc<GenericInstrument>>` on `Spectrum` or a
            // standalone `write_generic_instrument_xml` helper. Neither
            // exists yet; return an error so the caller notices.
            Err(SaveError::GenericInstrumentUnsupported)
        }
    }
}

// ---------------------------------------------------------------------------
// ROIs
// ---------------------------------------------------------------------------

/// Write the ROIs *of a spectrum* to `<library>/<spectrum.name>.xml`.
///
/// Matches `save_rois_to_internal(dummy_spectrum)` in Python: the spectrum
/// is a carrier for its rois and its name. The file contains only the
/// `<LRC:LuRaCs>/<Peaks>` section (`export_spectrum: false,
/// export_instrument: false`).
pub fn save_rois_to_library(spectrum: &Spectrum, library: &Path) -> SaveResult<PathBuf> {
    std::fs::create_dir_all(library)?;
    let stem = library.join(&spectrum.name);
    let new_file = stem.with_extension("xml");

    N42Writer::new(
        spectrum,
        &stem,
        WriteOptions {
            export_spectrum: false,
            export_rois: true,
            export_instrument: false,
            comment: None,
        },
    )?
    .write_out()?;

    tracing::debug!(path = %new_file.display(), "ROIs saved to library");
    Ok(new_file)
}
