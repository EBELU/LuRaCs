//! Top-level spectrum manager.
//!
//! Owns the spectrum registry, the ROI manager, and the instrument and
//! nuclide libraries. GUI elements call into this to mutate state, and the
//! manager emits `SpectrumManagerEvent`s the GUI drains each frame.
//!
//! Replaces `_SpectrumManager` + `EmittedSignals` from the Python.
//! The Python's Qt signals `roiCreated` / `roiUpdated` / `roiRemoved` and
//! `spectrumNameChanged` were declared but never emitted; they are dropped
//! here. The remaining signals map 1:1 to `SpectrumManagerEvent` variants.
//!
//! ## Identity caveat
//!
//! Python's `remove_instrument_from_spectra` uses `is` (object identity).
//! Our `Spectrum.instrument` is `Option<Arc<UniqueInstrument>>` and the
//! `InstrumentLibrary` stores `Instrument::Unique(UniqueInstrument)` by
//! value, so any attach path wraps a fresh `Arc`. We compare instruments by
//! `name` instead — equivalent for a well-behaved library. To restore true
//! identity, change `Spectrum.instrument` to `Option<Arc<Instrument>>` (the
//! enum) and have `InstrumentLibrary` store `Arc<Instrument>`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Duration, Utc};
use eframe::egui::Color32;
use ndarray::Array1;
use thiserror::Error;

use crate::containers::UniqueInstrument;
use crate::containers::{BkgType, FitType, Roi};
use crate::containers::{Spectrum, SpectrumData};
use crate::core::roi_manager::{NewRoi, RoiManager};
use crate::core::{
    Instrument, InstrumentLibrary, InstrumentLibraryError, LibraryKind, NuclideLibrary,
    NuclideLibraryError,
};
use crate::gui::{ColorRotator, ColorScheme};
use crate::utils::{InstrumentData, N42Error, ParsedData};
use crate::utils::{N42Writer, WriteOptions};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SpectrumManagerError {
    #[error("spectrum not found: {0:?}")]
    NotFound(String),

    #[error("invalid foreground data")]
    InvalidForegroundData,

    #[error("nuclide library error: {0}")]
    Nuclide(#[from] NuclideLibraryError),

    #[error("instrument library error: {0}")]
    Instrument(#[from] InstrumentLibraryError),

    #[error("XML write error: {0}")]
    Xml(#[from] N42Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SpectrumManagerError>;

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum SpectrumManagerEvent {
    SpectrumCreated(String),
    SpectrumUpdated(String),
    SpectrumRemoved(String),
    /// Emitted on the new name after a rename.
    SpectrumRenamed(String),
    /// `(spectrum_name, "bkg")` — the second field is a Python quirk preserved
    /// for parity with the original signal signature.
    BackgroundRemoved(String, String),
    VisibilityChanged(String, bool),
    /// Emitted when an unknown-instrument import had to be saved to disk.
    NewInstrumentLoaded(PathBuf),
    ColorUpdated(String),
}

// ---------------------------------------------------------------------------
// Wrapped spectrum (device output)
// ---------------------------------------------------------------------------

/// Device-side spectrum payload. Corresponds to the Python
/// `WrappedSpectrumPackage`.
#[derive(Debug, Clone)]
pub struct WrappedSpectrumPackage {
    pub y_axis: Array1<f64>,
    pub live_time: f64,
    pub real_time: Option<f64>,
    pub calib_coeff: Option<Vec<f64>>,
}

/// What `set_foreground_spectrum` accepts.
pub enum ForegroundInput {
    Wrapped(WrappedSpectrumPackage),
    Raw(SpectrumData),
}

// ---------------------------------------------------------------------------
// Import bundle
// ---------------------------------------------------------------------------

/// Parameter bundle for `import_spectrum`, replacing Python's loosely-typed
/// `data_dict`. The `XmlParser::ParsedData` struct can be converted into one
/// of these (see the note at the end of the module).
#[derive(Debug, Clone, Default)]
pub struct SpectrumImportBundle {
    pub name: String,
    pub foreground: Option<SpectrumData>,
    pub background: Option<SpectrumData>,
    pub calibration: Option<Vec<f64>>,
    pub peaks: Vec<Roi>,
    pub instrument: Option<UniqueInstrument>,
    pub remark: Option<String>,
    pub instrument_model: Option<String>,
    pub instrument_id: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    pub external_import: bool,
    pub load_rois_on_import: bool,
    pub load_instrument_on_import: bool,
    pub tabbed_spectrum_view: bool,
    pub load_calibration_from_instrument: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            external_import: false,
            load_rois_on_import: true,
            load_instrument_on_import: true,
            tabbed_spectrum_view: false,
            load_calibration_from_instrument: true,
        }
    }
}

// ---------------------------------------------------------------------------
// SpectrumManager
// ---------------------------------------------------------------------------

pub struct SpectrumManager {
    pub color_rotation: ColorRotator,
    pub spectrum_registry: HashMap<String, Spectrum>,
    pub roi_manager: RoiManager,
    pub unique_instrument_library: InstrumentLibrary,
    pub generic_instrument_library: InstrumentLibrary,
    pub nuclide_library: NuclideLibrary,
    pub unique_instrument_library_dir: PathBuf,
    events: VecDeque<SpectrumManagerEvent>,
}

impl SpectrumManager {
    pub fn new(
        color_scheme: ColorScheme,
        unique_instrument_library_dir: PathBuf,
        nuclide_data_dir: &Path,
    ) -> Result<Self> {
        Ok(Self {
            color_rotation: ColorRotator::new(color_scheme),
            spectrum_registry: HashMap::new(),
            roi_manager: RoiManager::new(),
            unique_instrument_library: InstrumentLibrary::new(LibraryKind::Unique),
            generic_instrument_library: InstrumentLibrary::new(LibraryKind::Generic),
            nuclide_library: NuclideLibrary::load_from_dir(nuclide_data_dir)?,
            unique_instrument_library_dir,
            events: VecDeque::new(),
        })
    }

    // ---- Event queue ----------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<SpectrumManagerEvent> {
        self.events.drain(..).collect()
    }

    fn emit(&mut self, ev: SpectrumManagerEvent) {
        self.events.push_back(ev);
    }

    // ---- Spectrum creation / replacement -------------------------------

    /// Create a new spectrum under `name`. Returns `true` on success,
    /// `false` if the name already exists (matching Python's return value).
    pub fn create_spectrum(
        &mut self,
        name: &str,
        channels: usize,
        device: Option<&str>,
        load_calibration_from_instrument: bool,
    ) -> Result<bool> {
        if self.spectrum_registry.contains_key(name) {
            return Ok(false);
        }

        let mut new_spect = Spectrum::new(channels, name.to_string());
        new_spect.connection = device.map(str::to_string);

        // Attach a matching library instrument, if any.
        if let Some(dev) = device {
            let lookup = self
                .unique_instrument_library
                .get_instrument_by_name(dev)
                .and_then(|i| i.as_unique())
                .cloned();
            if let Some(u) = lookup {
                new_spect.instrument = Some(Arc::new(u));
            }
        }

        // Assign foreground and background colors.
        let (fg_clr, bkg_clr) = self.color_rotation.get_color_pair();
        new_spect.color_foreground = fg_clr;
        new_spect.color_background = bkg_clr;

        self.spectrum_registry.insert(name.to_string(), new_spect);
        self.emit(SpectrumManagerEvent::SpectrumCreated(name.to_string()));

        // Attach instrument-provided calibration if desired.
        if load_calibration_from_instrument {
            let coeffs_opt = self
                .spectrum_registry
                .get(name)
                .and_then(|s| s.instrument.as_ref())
                .and_then(|u| u.calibration_coefficients.clone());
            if let Some(coeffs) = coeffs_opt {
                self.calibrate_spectrum(name, coeffs)?;
            }
        }
        Ok(true)
    }

    /// Replace the spectrum under `name`, preserving its connection,
    /// instrument, and colors from the existing entry.
    pub fn set_spectrum(&mut self, name: &str, mut spectrum: Spectrum) -> Result<()> {
        let Some(old) = self.spectrum_registry.get(name) else {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        };
        spectrum.connection = old.connection.clone();
        spectrum.instrument = old.instrument.clone();
        spectrum.color_foreground = old.color_foreground;
        spectrum.color_background = old.color_background;
        self.spectrum_registry.insert(name.to_string(), spectrum);
        self.emit(SpectrumManagerEvent::SpectrumUpdated(name.to_string()));
        Ok(())
    }

    // ---- Foreground / background ---------------------------------------

    pub fn set_foreground_spectrum(&mut self, name: &str, data: ForegroundInput) -> Result<()> {
        if !self.spectrum_registry.contains_key(name) {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        }

        let (new_spectrum, calib_coeff) = match data {
            ForegroundInput::Wrapped(WrappedSpectrumPackage {
                y_axis,
                live_time,
                real_time,
                calib_coeff,
            }) => {
                let duration = real_time.unwrap_or(live_time);
                let end = Utc::now();
                let start = end - Duration::milliseconds((duration * 1000.0).round() as i64);
                let total: f64 = y_axis.iter().sum();
                let channels = y_axis.len();
                let avg_cps = Some(total / live_time.max(1.0));
                (
                    SpectrumData {
                        y_axis,
                        channels,
                        total_counts: total.max(0.0) as u64,
                        live_time,
                        real_time,
                        avg_dose_rate: None,
                        avg_cps,
                        start_date: Some(start),
                        end_date: Some(end),
                        spectrum_name: Some(name.to_string()),
                        instrument: None,
                    },
                    calib_coeff,
                )
            }
            ForegroundInput::Raw(sd) => (sd, None),
        };

        let needs_calibration = {
            let spec = self.spectrum_registry.get(name).unwrap();
            !spec.calibrated && calib_coeff.is_some()
        };

        if let Some(spec) = self.spectrum_registry.get_mut(name) {
            spec.set_foreground(new_spectrum, None);
        }

        if needs_calibration {
            if let Some(cal) = calib_coeff {
                self.calibrate_spectrum(name, cal)?;
            }
        }

        self.emit(SpectrumManagerEvent::SpectrumUpdated(name.to_string()));
        Ok(())
    }

    pub fn set_background_spectrum(
        &mut self,
        foreground_name: &str,
        spectrum_data: SpectrumData,
    ) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(foreground_name) else {
            return Err(SpectrumManagerError::NotFound(foreground_name.to_string()));
        };
        spec.set_background(spectrum_data, None);
        self.emit(SpectrumManagerEvent::SpectrumUpdated(
            foreground_name.to_string(),
        ));
        Ok(())
    }

    pub fn clear_background(&mut self, name: &str) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(name) else {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        };
        spec.background = None;
        self.emit(SpectrumManagerEvent::BackgroundRemoved(
            name.to_string(),
            "bkg".to_string(),
        ));
        Ok(())
    }

    // ---- Removal / rename ----------------------------------------------

    pub fn remove_spectrum(&mut self, name: &str) {
        if self.spectrum_registry.remove(name).is_some() {
            self.emit(SpectrumManagerEvent::SpectrumRemoved(name.to_string()));
        }
    }

    pub fn rename_spectrum(&mut self, current_name: &str, new_name: &str) -> Result<()> {
        let Some(mut spectrum) = self.spectrum_registry.remove(current_name) else {
            return Err(SpectrumManagerError::NotFound(current_name.to_string()));
        };
        spectrum.name = new_name.to_string();
        self.spectrum_registry
            .insert(new_name.to_string(), spectrum);

        // Update ROI owner references.
        self.roi_manager
            .rename_owner_spectrum(current_name, new_name);

        // Match Python's post-rename signal sequence:
        //   removed(current) → created(new) → updated(new) → renamed(new)
        self.emit(SpectrumManagerEvent::SpectrumRemoved(
            current_name.to_string(),
        ));
        self.emit(SpectrumManagerEvent::SpectrumCreated(new_name.to_string()));
        self.emit(SpectrumManagerEvent::SpectrumUpdated(new_name.to_string()));
        self.emit(SpectrumManagerEvent::SpectrumRenamed(new_name.to_string()));
        Ok(())
    }

    // ---- Calibration / display -----------------------------------------

    pub fn calibrate_spectrum(&mut self, name: &str, coeffs: Vec<f64>) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(name) else {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        };
        spec.apply_calibration(coeffs);
        self.emit(SpectrumManagerEvent::SpectrumUpdated(name.to_string()));
        Ok(())
    }

    pub fn set_color(&mut self, name: &str, fg_bkg: &str, color: Color32) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(name) else {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        };
        if fg_bkg.eq_ignore_ascii_case("foreground") {
            spec.color_foreground = color;
        } else {
            spec.color_background = color;
        }
        self.emit(SpectrumManagerEvent::ColorUpdated(name.to_string()));
        Ok(())
    }

    pub fn update_visibility(&mut self, name: &str, state: bool) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(name) else {
            return Err(SpectrumManagerError::NotFound(name.to_string()));
        };
        spec.show_in_plot = state;
        self.emit(SpectrumManagerEvent::VisibilityChanged(
            name.to_string(),
            state,
        ));
        // Re-evaluate ROIs on this spectrum.
        self.roi_manager
            .update_roi(&mut self.spectrum_registry, Some(name), None, true);
        Ok(())
    }

    // ---- Getters --------------------------------------------------------

    pub fn get_spectrum(&self, name: &str) -> Option<&Spectrum> {
        self.spectrum_registry.get(name)
    }

    pub fn get_spectra_dict(&self) -> &HashMap<String, Spectrum> {
        &self.spectrum_registry
    }

    pub fn get_spectra_dict_mut(&mut self) -> &mut HashMap<String, Spectrum> {
        &mut self.spectrum_registry
    }

    // ---- Instrument management -----------------------------------------

    pub fn set_spectrum_instrument(
        &mut self,
        spectrum_name: &str,
        instrument: UniqueInstrument,
    ) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(spectrum_name) else {
            return Err(SpectrumManagerError::NotFound(spectrum_name.to_string()));
        };
        spec.instrument = Some(Arc::new(instrument));
        self.emit(SpectrumManagerEvent::SpectrumUpdated(
            spectrum_name.to_string(),
        ));
        Ok(())
    }

    pub fn clear_spectrum_instrument(&mut self, spectrum_name: &str) -> Result<()> {
        let Some(spec) = self.spectrum_registry.get_mut(spectrum_name) else {
            return Err(SpectrumManagerError::NotFound(spectrum_name.to_string()));
        };
        spec.instrument = None;
        self.emit(SpectrumManagerEvent::SpectrumUpdated(
            spectrum_name.to_string(),
        ));
        Ok(())
    }

    /// Clear `instrument` from every spectrum that currently references the
    /// removed instrument. Compares by `name` (see module-level caveat).
    pub fn remove_instrument_from_spectra(&mut self, removed_name: &str) {
        let to_update: Vec<String> = self
            .spectrum_registry
            .iter()
            .filter(|(_, s)| {
                s.instrument
                    .as_ref()
                    .map(|u| u.name == removed_name)
                    .unwrap_or(false)
            })
            .map(|(k, _)| k.clone())
            .collect();

        for name in to_update {
            if let Some(spec) = self.spectrum_registry.get_mut(&name) {
                spec.instrument = None;
            }
            self.emit(SpectrumManagerEvent::SpectrumUpdated(name));
        }
    }

    // ---- Import ---------------------------------------------------------

    pub fn import_spectrum(
        &mut self,
        bundle: SpectrumImportBundle,
        opts: ImportOptions,
    ) -> Result<()> {
        let Some(fg) = bundle.foreground.clone() else {
            // Python: warn and return. Nothing to import.
            return Ok(());
        };
        let channels = fg.channels;

        self.create_spectrum(
            &bundle.name,
            channels,
            None,
            opts.load_calibration_from_instrument,
        )?;
        self.set_foreground_spectrum(&bundle.name, ForegroundInput::Raw(fg))?;

        if let Some(bg) = bundle.background {
            self.set_background_spectrum(&bundle.name, bg)?;
        }

        if let Some(cal) = bundle.calibration {
            self.calibrate_spectrum(&bundle.name, cal)?;
        }

        if !bundle.peaks.is_empty() && (!opts.external_import || opts.load_rois_on_import) {
            for peak in &bundle.peaks {
                let owner = if opts.tabbed_spectrum_view {
                    Some(bundle.name.clone())
                } else {
                    None
                };
                let mut spec = NewRoi::new(peak.roi_bound.0, peak.roi_bound.1);
                spec.alias = Some(peak.alias.clone());
                spec.fit_type = peak.fit_type;
                spec.bkg_type = peak.bkg_type;
                spec.emission = peak.emission.clone();
                spec.owner_spectrum = owner;
                spec.meta = peak.meta.clone();
                self.roi_manager.add_roi(spec);
            }
        }

        if let Some(instr) = bundle.instrument {
            if !opts.external_import || opts.load_instrument_on_import {
                self.attach_or_store_instrument(&bundle.name, instr)?;
            }
        }

        if let Some(spec) = self.spectrum_registry.get_mut(&bundle.name) {
            if let Some(r) = bundle.remark {
                spec.remark = r;
            }
            if let Some(m) = bundle.instrument_model {
                spec.instrument_model = Some(m);
            }
            if let Some(id) = bundle.instrument_id {
                spec.instrument_id = Some(id);
            }
        }

        self.emit(SpectrumManagerEvent::SpectrumUpdated(bundle.name));
        Ok(())
    }

    pub fn import_spectrum_as_background(
        &mut self,
        spectrum_name: &str,
        bundle: SpectrumImportBundle,
    ) -> Result<()> {
        let Some(fg) = bundle.foreground else {
            return Ok(());
        };
        self.set_background_spectrum(spectrum_name, fg)
    }

    /// Internal helper: attach a parsed instrument to a spectrum, persisting
    /// it to the library if it isn't known yet.
    fn attach_or_store_instrument(
        &mut self,
        spectrum_name: &str,
        instrument: UniqueInstrument,
    ) -> Result<()> {
        // One lookup: is this instrument already in the library?
        if let Some(existing) = self
            .unique_instrument_library
            .get_instrument_by_name(&instrument.name)
            .and_then(|i| i.as_unique())
            .cloned()
        {
            // Known instrument — attach a copy of the stored version so future
            // library updates propagate to this spectrum.
            self.set_spectrum_instrument(spectrum_name, existing)?;
            return Ok(());
        }

        // Unknown instrument — persist a stub XML file for it, register in the
        // library, then attach the newly stored version.
        std::fs::create_dir_all(&self.unique_instrument_library_dir)?;
        let file_path = self
            .unique_instrument_library_dir
            .join(&instrument.name)
            .with_extension("xml");

        let mut dummy = Spectrum::new(1, "Dummy");
        dummy.instrument = Some(Arc::new(instrument.clone()));
        N42Writer::new(
            &dummy,
            &file_path,
            WriteOptions {
                export_spectrum: false,
                export_rois: false,
                export_instrument: true,
                comment: None,
            },
        )?
        .write_out()?;

        // Reuse the *stored copy* (not the incoming `instrument`) so the
        // spectrum and library point at consistent data if `set_raw` ever
        // starts normalising fields (e.g. deduping names, filling defaults).
        self.unique_instrument_library.set_raw(
            file_path.to_string_lossy().into_owned(),
            Instrument::Unique(instrument.clone()),
        )?;

        let stored = self
            .unique_instrument_library
            .get_instrument_by_name(&instrument.name)
            .and_then(|i| i.as_unique())
            .cloned()
            .unwrap_or(instrument);

        self.set_spectrum_instrument(spectrum_name, stored)?;
        self.emit(SpectrumManagerEvent::NewInstrumentLoaded(file_path));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

impl From<ParsedData> for SpectrumImportBundle {
    fn from(p: ParsedData) -> Self {
        let instrument = match p.instrument {
            Some(InstrumentData::Unique(u)) => Some(u),
            _ => None,
        };
        Self {
            name: p.name,
            foreground: p.foreground,
            background: p.background,
            calibration: p.calibration,
            peaks: p.peaks,
            instrument,
            remark: p.remark,
            instrument_model: p.instrument_model,
            instrument_id: p.instrument_id,
        }
    }
}
