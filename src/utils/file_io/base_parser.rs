//! Base trait for spectrum-file parsers.
//!
//! Each concrete format (SQLite log, MCA, list-mode, …) implements this
//! trait. Only `get_spectrum` is required; `get_rois` and `get_instrument`
//! have defaults matching the Python ABC.

use std::sync::Arc;

use crate::containers::{GenericInstrument, Roi, Spectrum, UniqueInstrument};

/// Optional instrument payload a parser may attach to a spectrum.
///
/// Python's return type is `UniqueInstrument | GenericInstrument | None`.
/// We keep that union explicit because the two Rust types are distinct
/// (unlike Python, where inheritance blurs the line). `Spectrum.instrument`
/// stores `Option<Arc<UniqueInstrument>>`, so callers must upgrade a
/// `Generic` value with `UniqueInstrument::from_template` if they want to
/// attach it.
#[derive(Debug, Clone)]
pub enum ParsedInstrument {
    Generic(Arc<GenericInstrument>),
    Unique(Arc<UniqueInstrument>),
}

pub trait SpectrumParser {
    /// Concrete parser error type. Keeping it associated (rather than a
    /// shared `dyn Error`) lets each format have its own error enum while
    /// the trait stays zero-cost.
    type Error;

    /// Produce a fully populated `Spectrum`. The only required method.
    fn get_spectrum(&self) -> Result<Spectrum, Self::Error>;

    /// Default: no ROIs. Parsers that extract ROI metadata override this.
    fn get_rois(&self) -> Result<Vec<Roi>, Self::Error> {
        Ok(Vec::new())
    }

    /// Default: no instrument. Matches the Python ABC returning `None`.
    fn get_instrument(&self) -> Option<ParsedInstrument> {
        None
    }
}
