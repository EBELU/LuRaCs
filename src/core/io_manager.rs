//! IOManager: file indexing, import, and export.
//!
//! Port of `luracs.core.io_manager._IOManager`.
//!
//! ## Structure
//!
//! - `FileIndex` — five `Indexer`s tracking saved files under the app-data
//!   library directories. Emits `FileIndexEvent`s the `App` wires into the
//!   instrument libraries (replacing Python's global `sigItemAdded.connect`).
//! - `Importer` — file dialogs + dispatch, returns `ImportOutcome`s the app
//!   forwards to the spectrum manager.
//! - `Exporter` — dialogs + dispatch. Where Python wrote directly, this
//!   returns an `ExportOutcome` so the app can call into `SpectrumManager`
//!   and log consistently.
//!
//! ## Bugs fixed vs Python
//!
//! - `export_spectrogram_to_spectrum` referenced `current_spectrogram` in the
//!   log message even when `spectrogram_is_loaded == false` (would
//!   `UnboundLocalError`). Guarded.
//! - `Indexer::update_file` deleted the registry key but never the file on
//!   disk, so `run_index` immediately re-added it. Now we remove the on-disk
//!   file *after* a successful save when the new path differs from the old.
//! - `Indexer::get_key_from_attr` raised on the first item missing the
//!   attribute (Python `KeyError`). Now skips items that don't have it.
//! - `csv_writer.export_rois` combined ROIs from every spectrum into one
//!   list; the caller (`Exporter::export_rois`) now exposes this explicitly.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rfd::FileDialog;
use serde_json::Value;
use thiserror::Error;

use crate::containers::{GenericInstrument, Roi, Spectrum, UniqueInstrument};
use crate::utils::{DbParseError, DbParser};
use crate::utils::{InstrumentData, ParsedData, XmlParseError, XmlParser};
use crate::utils::{LibraryZipError, unzip_library, zip_library};
use crate::utils::{SpeParseError, SpeParser};
use crate::utils::{TkaParseError, TkaParser};

use crate::utils::{
    InstrumentRef, SaveError, save_instrument_to_library, save_rois_to_library,
    save_spectrum_to_library,
};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum IoManagerError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("glob: {0}")]
    Glob(#[from] glob::PatternError),
    #[error("XML parse: {0}")]
    Xml(#[from] XmlParseError),
    #[error("DB parse: {0}")]
    Db(#[from] DbParseError),
    #[error("Spe parse: {0}")]
    Spe(#[from] SpeParseError),
    #[error("Tka parse: {0}")]
    Tka(#[from] TkaParseError),
    #[error("zip: {0}")]
    Zip(#[from] LibraryZipError),
    #[error("save: {0}")]
    Save(String),
    #[error("unknown file kind: {0}")]
    UnknownKind(PathBuf),
    #[error("no save function set for this indexer")]
    NoSaveFn,
    #[error("key not found in index: {0}")]
    MissingKey(PathBuf),
}

pub type Result<T> = std::result::Result<T, IoManagerError>;

// ---------------------------------------------------------------------------
// IndexedFile
// ---------------------------------------------------------------------------

pub enum IndexedFile {
    Xml(XmlParser),
    Db(DbParser),
}

impl IndexedFile {
    pub fn as_xml(&self) -> Option<&XmlParser> {
        if let Self::Xml(p) = self {
            Some(p)
        } else {
            None
        }
    }
    pub fn as_xml_mut(&mut self) -> Option<&mut XmlParser> {
        if let Self::Xml(p) = self {
            Some(p)
        } else {
            None
        }
    }
    pub fn as_db(&self) -> Option<&DbParser> {
        if let Self::Db(p) = self {
            Some(p)
        } else {
            None
        }
    }

    /// Snapshot of attributes as JSON, for `get_key_from_attr`-style queries.
    /// `None` for kinds that don't expose uniform metadata (matches Python,
    /// where DbParser would `KeyError`).
    pub fn metadata(&self) -> Option<Value> {
        match self {
            // ParsedData has no serde derive; convert a small subset.
            Self::Xml(p) => {
                let h = p.get_header();
                Some(serde_json::json!({
                    "name": h.name,
                    "instrument_model": h.instrument_model,
                    "instrument_id": h.instrument_id,
                    "instrument_class_code": h.instrument_class_code,
                    "remark": h.remark,
                }))
            }
            Self::Db(_) => None,
        }
    }

    pub fn close(&mut self) {
        if let Self::Db(p) = self {
            p.close();
        }
    }
}

// ---------------------------------------------------------------------------
// Save input / callbacks
// ---------------------------------------------------------------------------

/// What the caller can ask an `Indexer` to persist. The app injects a save
/// function per index that knows how to write each of these to the library.
pub enum SaveInput<'a> {
    Spectrum(&'a Spectrum),
    Rois { rois: &'a [Roi], name: &'a str },
    UniqueInstrument(&'a UniqueInstrument),
    GenericInstrument(&'a GenericInstrument),
}

pub type SaveFn = Box<dyn Fn(SaveInput<'_>) -> std::result::Result<PathBuf, String> + Send + Sync>;

// ---------------------------------------------------------------------------
// Indexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum ParserKind {
    Xml { meta_only: bool },
    Db,
}

#[derive(Debug, Clone)]
pub enum IndexerEvent {
    ItemAdded { path: PathBuf },
    IndexUpdated,
}

pub struct Indexer {
    parser_kind: ParserKind,
    /// Glob pattern (e.g. `/path/*.xml`).
    path_pattern: String,
    save_fn: Option<SaveFn>,
    registry: HashMap<PathBuf, IndexedFile>,
    events: VecDeque<IndexerEvent>,
}

impl Indexer {
    pub fn new(parser_kind: ParserKind, path_pattern: impl Into<String>) -> Self {
        Self {
            parser_kind,
            path_pattern: path_pattern.into(),
            save_fn: None,
            registry: HashMap::new(),
            events: VecDeque::new(),
        }
    }

    pub fn with_save_fn(mut self, f: SaveFn) -> Self {
        self.save_fn = Some(f);
        self
    }

    pub fn drain_events(&mut self) -> Vec<IndexerEvent> {
        self.events.drain(..).collect()
    }

    pub fn get(&self, path: &Path) -> Option<&IndexedFile> {
        self.registry.get(path)
    }
    pub fn get_mut(&mut self, path: &Path) -> Option<&mut IndexedFile> {
        self.registry.get_mut(path)
    }
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.registry.keys()
    }
    pub fn len(&self) -> usize {
        self.registry.len()
    }

    /// Scan the glob and load any new files. Emits `ItemAdded` per new file
    /// and `IndexUpdated` once at the end if anything changed.
    pub fn run_index(&mut self) {
        let pattern = match glob::glob(&self.path_pattern) {
            Ok(p) => p,
            Err(_) => return,
        };

        let mut changed = false;
        for entry in pattern.flatten() {
            if self.registry.contains_key(&entry) {
                continue;
            }
            match self.load(&entry) {
                Ok(item) => {
                    self.registry.insert(entry.clone(), item);
                    self.events
                        .push_back(IndexerEvent::ItemAdded { path: entry });
                    changed = true;
                }
                Err(e) => {
                    tracing::warn!(path = %entry.display(), "indexer: {e}");
                }
            }
        }
        if changed {
            self.events.push_back(IndexerEvent::IndexUpdated);
        }
    }

    fn load(&self, path: &Path) -> Result<IndexedFile> {
        Ok(match &self.parser_kind {
            ParserKind::Xml { meta_only } => IndexedFile::Xml(XmlParser::open(path, *meta_only)?),
            ParserKind::Db => IndexedFile::Db(DbParser::open(path)?),
        })
    }

    /// Persist a new or updated item. Updates the registry, then re-runs the
    /// index. Returns the new path.
    pub fn save_file(&mut self, input: SaveInput) -> Result<PathBuf> {
        let save_fn = self.save_fn.as_ref().ok_or(IoManagerError::NoSaveFn)?;
        let new_path = save_fn(input).map_err(IoManagerError::Save)?;
        // Remove any stale registry entry for the destination so a rewrite
        // is treated as fresh.
        self.registry.remove(&new_path);
        self.run_index();
        Ok(new_path)
    }

    /// Rename on disk. Removes both the old key and, if present, the target
    /// key (matching Python, which overwrites).
    pub fn rename_file(&mut self, old_name: &Path, new_name: &Path) -> Result<()> {
        self.registry.remove(old_name);
        if self.registry.contains_key(new_name) {
            self.registry.remove(new_name);
            if new_name.exists() {
                std::fs::remove_file(new_name)?;
            }
        }
        std::fs::rename(old_name, new_name)?;
        self.run_index();
        Ok(())
    }

    /// Update an existing item: save it, then remove the old on-disk file
    /// if the save wrote somewhere else.
    ///
    /// BUG FIX vs Python: the original deleted the registry key but never
    /// the file, so `run_index` immediately re-added it. We now remove the
    /// stale file when the new path differs.
    pub fn update_file(&mut self, old_key: &Path, input: SaveInput) -> Result<PathBuf> {
        self.registry.remove(old_key);
        let new_path = self.save_file(input)?;
        if new_path != old_key && old_key.exists() {
            std::fs::remove_file(old_key)?;
        }
        self.events.push_back(IndexerEvent::IndexUpdated);
        Ok(new_path)
    }

    /// Delete a file by key. Closes any open handle first.
    pub fn delete_file(&mut self, key: &Path) -> Result<()> {
        if let Some(mut item) = self.registry.remove(key) {
            item.close();
        }
        if key.exists() {
            std::fs::remove_file(key)?;
        }
        self.events.push_back(IndexerEvent::IndexUpdated);
        Ok(())
    }

    /// Return the key of the first file whose `metadata()[attr] == value`.
    /// Skips entries that don't expose metadata (DBs), matching Python's
    /// effective behaviour of only working on XMLs.
    pub fn get_key_from_attr(&self, attr: &str, value: &Value) -> Option<&PathBuf> {
        self.registry
            .iter()
            .find(|(_, item)| {
                item.metadata().and_then(|m| m.get(attr).cloned()).as_ref() == Some(value)
            })
            .map(|(k, _)| k)
    }

    pub fn get_item_from_attr(&self, attr: &str, value: &Value) -> Option<&IndexedFile> {
        self.registry
            .values()
            .find(|item| item.metadata().and_then(|m| m.get(attr).cloned()).as_ref() == Some(value))
    }
}

// ---------------------------------------------------------------------------
// FileIndex
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexKind {
    Spectrum,
    Spectrogram,
    Roi,
    UniqueInstrument,
    GenericInstrument,
}

#[derive(Debug, Clone)]
pub enum FileIndexEvent {
    ItemAdded { which: IndexKind, path: PathBuf },
    IndexUpdated { which: IndexKind },
}

pub struct FileIndex {
    pub spectrum: Indexer,
    pub spectrogram: Indexer,
    pub roi: Indexer,
    pub unique_instrument: Indexer,
    pub generic_instrument: Indexer,
    events: VecDeque<FileIndexEvent>,
}

impl FileIndex {
    /// Build the five indexes. The three that persist items
    /// (spectrum, roi, unique/generic instrument) require a `SaveFn` each;
    /// the spectrogram index has none.
    pub fn new(
        appdata: &Path,
        spectrum_save: SaveFn,
        roi_save: SaveFn,
        unique_instrument_save: SaveFn,
        generic_instrument_save: SaveFn,
    ) -> Self {
        let pat = |subdir: &str, ext: &str| {
            appdata
                .join(subdir)
                .join(format!("*.{ext}"))
                .to_string_lossy()
                .into_owned()
        };

        Self {
            spectrum: Indexer::new(
                ParserKind::Xml { meta_only: true },
                pat("spectrum_library", "xml"),
            )
            .with_save_fn(spectrum_save),
            spectrogram: Indexer::new(ParserKind::Db, pat("spectrogram_library", "db")),
            roi: Indexer::new(
                ParserKind::Xml { meta_only: true },
                pat("roi_library", "xml"),
            )
            .with_save_fn(roi_save),
            unique_instrument: Indexer::new(
                ParserKind::Xml { meta_only: true },
                pat("unique_instrument_library", "xml"),
            )
            .with_save_fn(unique_instrument_save),
            generic_instrument: Indexer::new(
                ParserKind::Xml { meta_only: true },
                pat("generic_instrument_library", "xml"),
            )
            .with_save_fn(generic_instrument_save),
            events: VecDeque::new(),
        }
    }

    pub fn indexer(&self, kind: IndexKind) -> &Indexer {
        match kind {
            IndexKind::Spectrum => &self.spectrum,
            IndexKind::Spectrogram => &self.spectrogram,
            IndexKind::Roi => &self.roi,
            IndexKind::UniqueInstrument => &self.unique_instrument,
            IndexKind::GenericInstrument => &self.generic_instrument,
        }
    }

    pub fn indexer_mut(&mut self, kind: IndexKind) -> &mut Indexer {
        match kind {
            IndexKind::Spectrum => &mut self.spectrum,
            IndexKind::Spectrogram => &mut self.spectrogram,
            IndexKind::Roi => &mut self.roi,
            IndexKind::UniqueInstrument => &mut self.unique_instrument,
            IndexKind::GenericInstrument => &mut self.generic_instrument,
        }
    }

    pub fn drain_events(&mut self) -> Vec<FileIndexEvent> {
        let mut out = Vec::new();
        for kind in [
            IndexKind::Spectrum,
            IndexKind::Spectrogram,
            IndexKind::Roi,
            IndexKind::UniqueInstrument,
            IndexKind::GenericInstrument,
        ] {
            for ev in self.indexer_mut(kind).drain_events() {
                out.push(match ev {
                    IndexerEvent::ItemAdded { path } => {
                        FileIndexEvent::ItemAdded { which: kind, path }
                    }
                    IndexerEvent::IndexUpdated => FileIndexEvent::IndexUpdated { which: kind },
                });
            }
        }
        out
    }

    pub fn run_index_all(&mut self) {
        for kind in [
            IndexKind::Spectrum,
            IndexKind::Spectrogram,
            IndexKind::Roi,
            IndexKind::UniqueInstrument,
            IndexKind::GenericInstrument,
        ] {
            self.indexer_mut(kind).run_index();
        }
    }
}

// ---------------------------------------------------------------------------
// Importer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFilter {
    All,
    Spectrum,
    Spectrogram,
    Rois,
    Instrument,
}

impl ImportFilter {
    fn rfd_name(self) -> &'static str {
        match self {
            Self::All => "All supported files",
            Self::Spectrum => "Spectrum files",
            Self::Spectrogram => "LuRaCs spectrogram database",
            Self::Rois => "LuRaCs ROIs file",
            Self::Instrument => "LuRaCs instrument file",
        }
    }
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::All => &["xml", "n42", "TKA", "Spe", "spe", "db"],
            Self::Spectrum => &["xml", "n42", "TKA", "Spe", "spe"],
            Self::Spectrogram => &["db"],
            Self::Rois => &["xml"],
            Self::Instrument => &["xml"],
        }
    }
}

/// What the app should do with the picked file(s).
#[derive(Debug)]
pub enum ImportOutcome {
    /// A parsed spectrum bundle, ready for `SpectrumManager::import_spectrum`.
    Spectrum(ParsedData),
    /// A parsed spectrum bundle to attach as background to an existing
    /// spectrum (paired with that spectrum's name by the caller).
    SpectrumAsBackground(ParsedData),
    /// A spectrogram DB has been copied into the library directory.
    Spectrogram(String),
    /// ROIs from a parsed XML file.
    Rois(Vec<Roi>),
    /// An instrument to be stored in its library.
    Instrument(InstrumentData),
}

pub struct Importer {
    /// Last directory the user opened. Persist this back into `Settings`.
    pub last_opened_dir: PathBuf,
    /// When true, imported ROIs get `owner_spectrum` set to the source
    /// file's stem (matches Python's `tabbed_spectrum_view` behaviour).
    pub tabbed_spectrum_view: bool,
}

impl Importer {
    pub fn new(last_opened_dir: PathBuf, tabbed_spectrum_view: bool) -> Self {
        Self {
            last_opened_dir,
            tabbed_spectrum_view,
        }
    }

    /// Open a file dialog and return the picked paths + filter.
    pub fn pick_files(&mut self, filter: ImportFilter) -> Option<(Vec<PathBuf>, ImportFilter)> {
        let mut dlg = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .add_filter(filter.rfd_name(), filter.extensions());
        if filter == ImportFilter::All {
            // Add each named filter too, so the user sees them individually.
            for f in [
                ImportFilter::Spectrum,
                ImportFilter::Spectrogram,
                ImportFilter::Rois,
                ImportFilter::Instrument,
            ] {
                dlg = dlg.add_filter(f.rfd_name(), f.extensions());
            }
        }
        let paths = dlg.pick_files()?;
        if paths.is_empty() {
            return None;
        }
        self.last_opened_dir = paths[0].parent().unwrap_or(Path::new("/")).to_path_buf();
        Some((paths, filter))
    }

    pub fn pick_file(&mut self, filter: ImportFilter) -> Option<(PathBuf, ImportFilter)> {
        let dlg = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .add_filter(filter.rfd_name(), filter.extensions());
        let path = dlg.pick_file()?;
        self.last_opened_dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        Some((path, filter))
    }

    /// Dispatch a set of already-picked paths into `ImportOutcome`s.
    /// The app drains the returned list and calls `SpectrumManager` etc.
    pub fn import_paths(
        &mut self,
        paths: &[PathBuf],
        filter: ImportFilter,
        appdata_spectrogram_library: &Path,
    ) -> Vec<ImportOutcome> {
        let mut out = Vec::new();
        for path in paths {
            match filter {
                ImportFilter::Spectrum | ImportFilter::All => {
                    if let Some(parsed) = self.parse_spectrum(path) {
                        out.push(ImportOutcome::Spectrum(parsed));
                    }
                }
                ImportFilter::Spectrogram => {
                    if let Some(name) = self.import_spectrogram(path, appdata_spectrogram_library) {
                        out.push(ImportOutcome::Spectrogram(name));
                    }
                }
                ImportFilter::Rois => {
                    if let Some(rois) = self.import_rois(path) {
                        out.push(ImportOutcome::Rois(rois));
                    }
                }
                ImportFilter::Instrument => {
                    if let Some(inst) = self.import_instrument(path) {
                        out.push(ImportOutcome::Instrument(inst));
                    }
                }
            }
        }
        out
    }

    pub fn parse_spectrum(&self, path: &Path) -> Option<ParsedData> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())?
            .to_ascii_lowercase();
        match ext.as_str() {
            "xml" | "n42" => XmlParser::open(path, false).ok().map(|p| p.into_data()),
            "spe" => SpeParser::open(path).ok().map(|p| p.into_data()),
            "tka" => TkaParser::open(path).ok().map(|p| p.into_data()),
            _ => None,
        }
    }

    fn import_spectrogram(&self, path: &Path, library_dir: &Path) -> Option<String> {
        std::fs::create_dir_all(library_dir).ok()?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        let dest = library_dir.join(name).with_extension("db");
        std::fs::copy(path, &dest).ok()?;
        Some(dest.file_stem()?.to_string_lossy().into_owned())
    }

    fn import_rois(&mut self, path: &Path) -> Option<Vec<Roi>> {
        let parser = XmlParser::open(path, false).ok()?;
        let name = parser.get_header().name;
        let mut out = Vec::new();
        for peak in parser.get_rois().iter() {
            let mut roi = peak.clone();
            if self.tabbed_spectrum_view {
                roi.owner_spectrum = Some(name.clone());
            }
            out.push(roi);
        }
        Some(out)
    }

    fn import_instrument(&self, path: &Path) -> Option<InstrumentData> {
        XmlParser::open(path, true).ok()?.get_instrument_owned()
    }

    /// Convenience: dialog → outcomes in one step.
    pub fn import_generic(
        &mut self,
        filter: ImportFilter,
        appdata_spectrogram_library: &Path,
    ) -> Vec<ImportOutcome> {
        let Some((paths, filter)) = self.pick_files(filter) else {
            return Vec::new();
        };
        self.import_paths(&paths, filter, appdata_spectrogram_library)
    }

    pub fn import_library(&mut self, appdata: &Path) -> Result<()> {
        let Some(zip_path) = FileDialog::new()
            .set_title("Import zipped data library")
            .add_filter("Zip file", &["zip"])
            .pick_file()
        else {
            return Ok(());
        };
        unzip_library(&zip_path, appdata)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Exporter
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFilter {
    Spectrum,
    Spectrogram,
    Rois,
    Instrument,
}

impl ExportFilter {
    fn rfd_name(self) -> &'static str {
        match self {
            Self::Spectrum => "XML / CSV / Excel",
            Self::Spectrogram => "Spectrogram DB / Excel",
            Self::Rois => "XML / Excel",
            Self::Instrument => "XML",
        }
    }
    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Spectrum => &["xml", "csv", "xlsx"],
            Self::Spectrogram => &["db", "xlsx"],
            Self::Rois => &["xml", "xlsx"],
            Self::Instrument => &["xml"],
        }
    }
}

/// What the app should do with the export target the user picked.
pub enum ExportOutcome {
    /// Write the named spectrum to `path` using the selected format.
    Spectrum {
        spectrum_name: String,
        path: PathBuf,
        format: ExportFormat,
    },
    /// Create a spectrum from a spectrogram DB and store it in the library.
    SpectrogramToSpectrum {
        db_path: PathBuf,
        spectrum_name: String,
        remark: String,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        stop_time: Option<chrono::DateTime<chrono::Utc>>,
    },
    /// Export ROIs (already gathered by the caller) to `path`.
    Rois { rois: Vec<Roi>, path: PathBuf },
    /// Zip the entire appdata library to `path`.
    Library(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Xml,
    Csv,
    Xlsx,
}

pub struct Exporter {
    pub last_opened_dir: PathBuf,
}

impl Exporter {
    pub fn new(last_opened_dir: PathBuf) -> Self {
        Self { last_opened_dir }
    }

    pub fn export_spectrum_dialog(&mut self, spectrum_name: &str) -> Option<ExportOutcome> {
        let path = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .set_title("Export spectrum")
            .add_filter("XML", &["xml"])
            .add_filter("CSV", &["csv"])
            .add_filter("Excel", &["xlsx"])
            .save_file()?;
        self.last_opened_dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        let format = match path.extension().and_then(|s| s.to_str()) {
            Some("csv") => ExportFormat::Csv,
            Some("xlsx") => ExportFormat::Xlsx,
            _ => ExportFormat::Xml,
        };
        Some(ExportOutcome::Spectrum {
            spectrum_name: spectrum_name.to_string(),
            path: path.with_extension(""),
            format,
        })
    }

    /// Ask for a source DB (if not currently loaded) and a target spectrum
    /// name. Returns the outcome for the caller to fulfil via
    /// `DbWriter::build_spectrum_from_db` + `FileIndex::save_file`.
    pub fn export_spectrogram_to_spectrum_dialog(
        &mut self,
        spectrogram_name: Option<&str>,
    ) -> Option<ExportOutcome> {
        // If a name is provided the caller already has the DB loaded in the
        // run manager; use it directly. Otherwise prompt for a file.
        let db_path = match spectrogram_name {
            Some(_) => PathBuf::new(), // sentinel; caller knows
            None => {
                let p = FileDialog::new()
                    .set_directory(&self.last_opened_dir)
                    .set_title("Select spectrogram database")
                    .add_filter("Spectrogram DB", &["db"])
                    .pick_file()?;
                self.last_opened_dir = p.parent().unwrap_or(Path::new("/")).to_path_buf();
                p
            }
        };

        let spectrum_name = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .set_title("Save as spectrum name…")
            .save_file()?;
        let spectrum_name = spectrum_name
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "exported_spectrum".into());

        Some(ExportOutcome::SpectrogramToSpectrum {
            db_path,
            spectrum_name,
            remark: String::new(),
            start_time: None,
            stop_time: None,
        })
    }

    pub fn export_rois_dialog(&mut self, rois: Vec<Roi>) -> Option<ExportOutcome> {
        if rois.is_empty() {
            return None;
        }
        let path = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .set_title("Export ROIs")
            .add_filter("CSV", &["csv"])
            .save_file()?;
        self.last_opened_dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        Some(ExportOutcome::Rois {
            rois,
            path: path.with_extension("csv"),
        })
    }

    pub fn export_library_dialog(&mut self) -> Option<ExportOutcome> {
        let path = FileDialog::new()
            .set_directory(&self.last_opened_dir)
            .set_title("Export zipped data library")
            .add_filter("Zip file", &["zip"])
            .save_file()?;
        self.last_opened_dir = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        Some(ExportOutcome::Library(path))
    }

    /// Fulfil an `ExportOutcome::Library` by zipping the appdata directory.
    pub fn do_export_library(&self, appdata: &Path, zip_path: &Path) -> Result<()> {
        zip_library(appdata, zip_path)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IOManager
// ---------------------------------------------------------------------------

pub struct IoManager {
    pub file_index: FileIndex,
    pub importer: Importer,
    pub exporter: Exporter,
}

impl IoManager {
    pub fn new(
        appdata: &Path,
        last_opened_dir: PathBuf,
        tabbed_spectrum_view: bool,
        spectrum_save: SaveFn,
        roi_save: SaveFn,
        unique_instrument_save: SaveFn,
        generic_instrument_save: SaveFn,
    ) -> Self {
        let file_index = FileIndex::new(
            appdata,
            spectrum_save,
            roi_save,
            unique_instrument_save,
            generic_instrument_save,
        );
        Self {
            file_index,
            importer: Importer::new(last_opened_dir.clone(), tabbed_spectrum_view),
            exporter: Exporter::new(last_opened_dir),
        }
    }
}
