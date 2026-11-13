//! Zip or unzip an entire nuclide/instrument library while preserving its
//! directory layout.
//!
//! Extracted entries are safety-checked via `ZipArchive::enclosed_name`,
//! which rejects absolute paths and `..` components — this is the Rust
//! equivalent of Python 3.6+'s Zip Slip protection in `ZipFile.extractall`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use thiserror::Error;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum LibraryZipError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("directory walk error: {0}")]
    Walk(#[from] walkdir::Error),

    #[error("refusing to extract entry {entry:?}: {reason}")]
    UnsafeEntry { entry: String, reason: &'static str },

    #[error("path is not under library root: {0}")]
    NotUnderRoot(PathBuf),
}

pub type LibraryZipResult<T> = std::result::Result<T, LibraryZipError>;

// ---------------------------------------------------------------------------
// Zipping
// ---------------------------------------------------------------------------

/// Archive every file under `library_path`, preserving relative paths.
///
/// Skipped:
/// - `settings.json` anywhere in the tree (per the Python spec)
/// - directories (they are implied by their file entries)
/// - symlinks (WalkDir is configured with `follow_links(false)`; a symlink
///   that pointed *outside* the library would let the archive leak files
///   from the host machine — worth skipping).
///
/// The output file must not live inside `library_path`; the archive would
/// otherwise be read while being written.
pub fn zip_library(
    library_path: impl AsRef<Path>,
    output_file: impl AsRef<Path>,
) -> LibraryZipResult<()> {
    let library_path = library_path.as_ref().canonicalize()?;

    let out = File::create(output_file.as_ref())?;
    let mut zip = ZipWriter::new(out);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for entry in WalkDir::new(&library_path).follow_links(false) {
        let entry = entry?;

        // Skip directories and symlinks; only regular files get archived.
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();

        if path.file_name().and_then(|n| n.to_str()) == Some("settings.json") {
            continue;
        }

        let rel = path
            .strip_prefix(&library_path)
            .map_err(|_| LibraryZipError::NotUnderRoot(path.to_path_buf()))?;

        // Zip spec requires forward slashes regardless of platform; reject
        // `ParentDir`/`RootDir` components in the arcname for good measure.
        let arcname = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");

        if arcname.is_empty() {
            continue;
        }

        zip.start_file(&arcname, options)?;
        let mut f = File::open(path)?;
        io::copy(&mut f, &mut zip)?;
    }

    zip.finish()?.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Unzipping
// ---------------------------------------------------------------------------

/// Restore an archive created by `zip_library` (or any well-formed zip) into
/// `library_path`. Creates the directory if it does not exist. Existing
/// files with the same names are overwritten.
pub fn unzip_library(
    zip_file: impl AsRef<Path>,
    library_path: impl AsRef<Path>,
) -> LibraryZipResult<()> {
    let library_path = library_path.as_ref();
    fs::create_dir_all(library_path)?;

    let file = File::open(zip_file.as_ref())?;
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;

        // `enclosed_name()` returns None for absolute paths or paths
        // containing `..`. This is the Zip Slip guard.
        let entry_name = entry.name()?.into_owned();
        let safe_name = entry
            .enclosed_name()
            .ok_or_else(|| LibraryZipError::UnsafeEntry {
                entry: entry_name,
                reason: "path escapes the archive root",
            })?
            .to_owned();

        let out_path = library_path.join(&safe_name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut out = File::create(&out_path)?;
        io::copy(&mut entry, &mut out)?;

        // Preserve the unix permission bits stored in the archive, matching
        // what `extractall` does.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))?;
            }
        }
    }

    Ok(())
}
