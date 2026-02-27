import os
import numpy as np
import sqlite3 as sql
import zlib
import time
from collections import deque

from ..Globals import SpectrumManager

def compress_spectrum(array: np.ndarray) -> bytes:
    raw = array.astype(np.uint32).tobytes()
    return zlib.compress(raw, level=6)

def decompress_spectrum(blob: bytes, channel_count: int) -> np.ndarray:
    raw = zlib.decompress(blob)
    return np.frombuffer(raw, dtype=np.uint32, count=channel_count)

class SpectrumLogger:
    def __init__(self, db_name: str, save_interval: float, channels: int, device: str, calibration_coeff: list,
                 channel_concat_factor: int = 1, db_commit_interval: int = 1, resume: bool = False):
        
        self.db_name = db_name
        self.save_interval = save_interval
        self.spect_channels = channels
        self.device_id = device
        self.calibration_coeff = calibration_coeff
        self.concat_factor = channel_concat_factor
        self.resume = resume
        
        self.connection = sql.connect(db_name)
        
        if not resume and os.path.exists(db_name):
            self.clean_db() 
            
        self.timestamp_buffer = None
        self.inserts = 0
        self.db_commit_interval = db_commit_interval
            
        self.spectrum_buffer: np.array = None
        self.view_buffer: deque = deque([], 128)
        
        self.temperature_buffer = -273
        self.dose_buffer = 0
        self.cps_buffer = 0
        self.current_values_recieved = 0
        
        cursor = self.connection.cursor()
        
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS header (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                created INTEGER NOT NULL,          -- Unix timestamp
                device_id TEXT NOT NULL,
                channels INTEGER NOT NULL CHECK(channels > 0),
                calibration TEXT,
                concat INTEGER NOT NULL
            )
        """)
        
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS spectrogram (
                id INTEGER PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                avg_cps REAL NOT NULL,
                avg_dr REAL NOT NULL,
                temperature REAL,
                spectrum BLOB NOT NULL
            )
        """)
        
        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_spectrogram_timestamp
            ON spectrogram(timestamp)
        """)
        
        cursor.execute("""
            INSERT OR IGNORE INTO header
            (id, created, device_id, channels, calibration, concat)
            VALUES (1, ?, ?, ?, ?, ?)
        """, (time.time(), self.device_id, self.spect_channels, str(self.calibration_coeff), self.concat_factor))

        self.connection.commit()

    def close(self):
        self.connection.close()
        
    def receive_current(self, name, current):
        if name != self.device_id:
            return
        self.cps_buffer += current.CPS
        self.dose_buffer += current.DR
        self.current_values_recieved += 1
        
    def receive_status(self, name ,status):
        if name != self.device_id:
            return
        self.temperature_buffer = status.temperature
        
    def receive_spectrum(self, name, spectrum):
        spectrum = spectrum.spectrum
        if name != self.device_id:
            return
        
        if self.spectrum_buffer is None:
            self.spectrum_buffer = spectrum
            self.timestamp_buffer = time.time()
            return
        new_ts = int(time.time())
        
        if not new_ts > self.timestamp_buffer + self.save_interval:
            return
        
        self.timestamp_buffer = new_ts

        
        spectrum_bytes = compress_spectrum(self.process_spectrum(spectrum))
        
        temperature = 0
        
        self.insert_spectrogram(new_ts, 
                                self.cps_buffer/max(self.current_values_recieved, 1), 
                                self.dose_buffer/max(self.current_values_recieved, 1), 
                                temperature, 
                                spectrum_bytes)
        
        self.current_values_recieved = 0
        self.cps_buffer = 0
        self.dose_buffer = 0
        
        
        
    def insert_spectrogram(self, timestamp, avg_cps, avg_dr, temperature, spectrum_bytes):
        cursor = self.connection.cursor()
        cursor.execute("""
            INSERT INTO spectrogram
            (timestamp, avg_cps, avg_dr, temperature, spectrum)
            VALUES (?, ?, ?, ?, ?)
        """, (timestamp, avg_cps, avg_dr, temperature, spectrum_bytes))
        self.inserts += 1        
            
        if self.inserts == self.db_commit_interval:
            self.connection.commit()
            self.inserts = 0
        
        
    def clean_db(self):
        cursor = self.connection.cursor()

        # Get a list of all tables
        cursor.execute("SELECT name FROM sqlite_master WHERE type='table';")
        tables = cursor.fetchall()
        
        # Drop each table
        for table_name in tables:
            cursor.execute(f"DROP TABLE IF EXISTS {table_name[0]}")
        self.connection.commit()
        
    def process_spectrum(self, spectrum):

        spectrum_diff = spectrum - self.spectrum_buffer
        self.spectrum_buffer = spectrum
    
        assert not np.any(spectrum_diff < 0)
    
        pad_size = (-len(spectrum_diff)) % self.concat_factor
        if pad_size:
            arr = np.pad(spectrum_diff, (0, pad_size))
        else:
            arr = spectrum_diff
    
        new = arr.reshape(-1, self.concat_factor).sum(axis=1)
        
        return new
    
class UnpackedLog:
    def __init__(self, db_name: str, start_date = None, end_data = None):
        self.connection = sql.connect(db_name)
        cursor = self.connection.cursor()
        cursor.execute("SELECT * FROM header")
        meta_data = cursor.fetchall()[0]
        
        self.created = meta_data[1]
        self.device_id = meta_data[2]
        self.spect_channels = meta_data[3]
        self.calibration_coeff = list(meta_data[4])
        self.concat_factor = meta_data[5]
        
        
    def close(self):
        self.connection.close()
        
        
    def retrieve_by_date(self, start_ts, end_ts):
        cursor = self.connection.cursor()
        cursor.execute("""
            SELECT *
            FROM spectrogram
            WHERE timestamp BETWEEN ? AND ?
            ORDER BY timestamp
        """, (start_ts, end_ts))
        
        rows = cursor.fetchall()
        results = []
        for _,ts, cps, dr, temp, blob in rows:
            spectrum = decompress_spectrum(
                blob,
                channel_count=self.spect_channels // self.concat_factor
            )
            results.append((ts, cps, dr, temp, spectrum))
    
        return results
    
    def retrieve_all(self):
        cursor = self.connection.cursor()
        cursor.execute("""
            SELECT *
            FROM spectrogram
            ORDER BY timestamp
        """)
        
        rows = cursor.fetchall()
        results = []
        for _ ,ts, cps, dr, temp, blob in rows:
            spectrum = decompress_spectrum(
                blob,
                channel_count=self.spect_channels // self.concat_factor
            )
            results.append((ts, cps, dr, temp, spectrum))
        
        return results