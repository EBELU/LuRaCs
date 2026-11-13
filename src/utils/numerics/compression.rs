//! Byte-level codecs for spectrum data.
//!
//! - `compress_spectrum` / `decompress_spectrum`: zlib (RFC 1950), level 6
//! - `encode_base64` / `decode_base64`: standard Base64 (RFC 4648)
//! - `zip_files`: bundle a set of files into a zip archive (deflate)
//!
//! Byte-order note: this port writes u32 counts in **little-endian**.
//! The Python original uses `np.frombuffer(..., dtype=np.uint32)` and
//! `astype(np.uint32).tobytes()`, both of which use *native* byte order.
//! On x86_64 / ARM64 (which is where almost all runs happen) native is LE,
//! so the two are byte-compatible; the Rust version is additionally safe
//! to round-trip across architectures.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error(
        "decompressed buffer is {actual} bytes but {channels} u32 channels \
         need {needed} bytes"
    )]
    DecompressedTooShort {
        actual: usize,
        needed: usize,
        channels: usize,
    },

    #[error("path has no filename component: {0}")]
    NoFileName(String),
}

pub type CompressionResult<T> = std::result::Result<T, CompressionError>;

// ---------------------------------------------------------------------------
// Spectrum compression (zlib)
// ---------------------------------------------------------------------------

/// Compress a `u32` count array with zlib at level 6.
pub fn compress_spectrum(counts: &[u32]) -> CompressionResult<Vec<u8>> {
    let mut raw = Vec::with_capacity(counts.len() * 4);
    for c in counts {
        raw.extend_from_slice(&c.to_le_bytes());
    }

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
    encoder.write_all(&raw)?;
    Ok(encoder.finish()?)
}

/// Convenience wrapper for spectra whose counts are stored as `f64` (which
/// matches `Spectrum::y_axis` in this codebase).
///
/// Uses Rust's saturating `as` cast: `NaN` and negatives become `0`, values
/// above `u32::MAX` saturate. This differs from NumPy's `astype(np.uint32)`,
/// which *wraps* negative values into the high range — the Rust behaviour
/// is the safer one for count data, and round-trips identically for the
/// integral, in-range values that a real spectrum contains.
pub fn compress_spectrum_f64(counts: &[f64]) -> CompressionResult<Vec<u8>> {
    let converted: Vec<u32> = counts.iter().map(|&v| v as u32).collect();
    compress_spectrum(&converted)
}

/// Decompress a zlib blob produced by `compress_spectrum` (or by the Python
/// `compress_spectrum`) into `channel_count` u32 counts.
///
/// Matches NumPy's `np.frombuffer(raw, dtype=np.uint32, count=n)`: raises
/// if the decompressed buffer is shorter than `n * 4` bytes, otherwise
/// reads exactly `n` values and ignores any trailing bytes.
pub fn decompress_spectrum(blob: &[u8], channel_count: usize) -> CompressionResult<Vec<u32>> {
    let mut decoder = ZlibDecoder::new(blob);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw)?;

    let needed = channel_count * 4;
    if raw.len() < needed {
        return Err(CompressionError::DecompressedTooShort {
            actual: raw.len(),
            needed,
            channels: channel_count,
        });
    }

    let mut out = Vec::with_capacity(channel_count);
    for chunk in raw[..needed].chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Base64
// ---------------------------------------------------------------------------

/// Standard Base64 encoding.
///
/// Returns `String` rather than `Vec<u8>` because Base64 output is always
/// ASCII and a `String` is the natural Rust type; `.into_bytes()` gives the
/// `Vec<u8>` form if you need it to match the Python `bytes` return exactly.
pub fn encode_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

pub fn decode_base64(text: &str) -> CompressionResult<Vec<u8>> {
    Ok(BASE64.decode(text)?)
}

// ---------------------------------------------------------------------------
// Zip bundling
// ---------------------------------------------------------------------------

/// Bundle each file in `file_paths` into a single deflate-compressed zip at
/// `zip_path`. Files are stored under their basename (matching Python's
/// `arcname=os.path.basename(path)`), so passing absolute paths does not
/// produce nested directories inside the archive.
pub fn zip_files<P, Q>(
    file_paths: impl IntoIterator<Item = P>,
    zip_path: Q,
) -> CompressionResult<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let file = File::create(zip_path.as_ref())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for path in file_paths {
        let path = path.as_ref();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| CompressionError::NoFileName(path.display().to_string()))?;

        zip.start_file(name, options)?;
        let mut f = BufReader::new(File::open(path)?);
        std::io::copy(&mut f, &mut zip)?;
    }

    zip.finish()?;
    Ok(())
}
