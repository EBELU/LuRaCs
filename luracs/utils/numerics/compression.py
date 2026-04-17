import numpy as np
import zlib
import zipfile
import os
import base64


def decompress_spectrum(blob: bytes, channel_count: int) -> np.ndarray:
    """Uncompress spectrum from bytes to array of uint32"""
    raw = zlib.decompress(blob)
    return np.frombuffer(raw, dtype=np.uint32, count=channel_count)


def compress_spectrum(array: np.ndarray) -> bytes:
    """Compress a spectrum to bytes with zlib"""
    raw = array.astype(np.uint32).tobytes()
    return zlib.compress(raw, level=6)


def encode_base64(bts: bytes) -> bytes:
    """
    Encode bytes to Base64 (returns bytes).
    """
    return base64.b64encode(bts)


def decode_base64(bts: bytes) -> bytes:
    """
    Decode Base64 bytes back to original bytes.
    """
    return base64.b64decode(bts)


def zip_files(file_paths, zip_path):
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for path in file_paths:
            zf.write(path, arcname=os.path.basename(path))
