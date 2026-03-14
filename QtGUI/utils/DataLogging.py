import os
import numpy as np
import sqlite3 as sql
import zlib
import time
from dataclasses import dataclass
from collections import deque
from datetime import datetime, timedelta
from os.path import join

from PySide6.QtCore import Signal, QObject

from ..clients.DeviceWrappers import WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage

from ..core import Settings, RunManager, Log

def compress_spectrum(array: np.ndarray) -> bytes:
    raw = array.astype(np.uint32).tobytes()
    return zlib.compress(raw, level=6)

def decompress_spectrum(blob: bytes, channel_count: int) -> np.ndarray:
    raw = zlib.decompress(blob)
    return np.frombuffer(raw, dtype=np.uint32, count=channel_count)

def start_logger(db_name, device: str = "Raysid_1543", save_interval: int = 1, truncation: int = 0, resume: bool = False):
    device_wrapper = RunManager.devices.get(device, None)
    if not device_wrapper:
        Log.warning(f"Logging could not be started as {device} does not exist")
        return
    
    if device_wrapper.name in RunManager.dataloggers:
        return
    
    print(db_name)
    
    new_log = SpectrumLogger(db_name, 
                                save_interval, 
                                device_wrapper.channels, 
                                device_wrapper.name,
                                device_wrapper.calibration,
                                truncation,
                                resume)
    RunManager.currentUpdated.connect(new_log.receive_current)
    RunManager.statusUpdated.connect(new_log.receive_status)
    RunManager.spectrumUpdated.connect(new_log.receive_spectrum)
    
    RunManager.add_logger(device, new_log)

@dataclass
class WrappedSpectrogramData:
    db_name: str
    instrument: str
    start_date: float
    duration: float
    concat: int
    spect_channels: int
    save_interval: int
    latest_spectrum: np.array
    spectrogram: deque
    estimated_dose: float

class SpectrumLogger(QObject):
    loggerStarted = Signal()
    dataUpdated = Signal(str, object)
    def __init__(self, db_name: str, save_interval: float, channels: int, device: str, calibration_coeff: list,
                 channel_concat_factor: int = 0, db_commit_interval: int = 1, resume: bool = False):
        super().__init__(parent = None)
        self.db_name = db_name
        self.db_path = str(Settings.Paths.spect_logs / self.db_name)
        self.save_interval = save_interval
        self.concat_factor = channel_concat_factor + 1
        self.spect_channels = channels // self.concat_factor
        self.device_id = device
        self.calibration_coeff = calibration_coeff

        self.resume = resume
        self.start_date = None
                
        self.connection = sql.connect(self.db_path)
        
        if not resume and os.path.exists(self.db_path):
            self.clean_db() 
            
        self.timestamp_buffer = None
        self.inserts = 0
        self.db_commit_interval = db_commit_interval
            
        self.spectrum_buffer: np.array = None
        self.accumulator = None
        self.estimated_dose = 0
        self.view_buffer: deque = deque([], 256)
        self.view_ts_buffer: deque = deque([], 256)
        
        self.temperature_buffer = -273
        self.dose_buffer = 0
        self.cps_buffer = 0
        self.current_values_recieved = 0
        
        cursor = self.connection.cursor()
        
        self.paused = False
        
        self.data_wrapper = WrappedSpectrogramData(db_name,
                                            device,
                                            self.start_date,
                                            0,
                                            self.concat_factor,
                                            self.spect_channels,
                                            self.save_interval,
                                            None,
                                            self.view_buffer,
                                            0)
        
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
            CREATE TABLE IF NOT EXISTS summary (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                accumulated_spectrum BLOB,
                total_duration REAL NOT NULL,
                total_dose REAL NOT NULL,
                last_update INTEGER
            )
        """)
        
        cursor.execute("""
            INSERT OR IGNORE INTO summary
            (id, accumulated_spectrum, total_duration, total_dose, last_update)
            VALUES (1, NULL, 0, 0, ?)
        """, (time.time(),))
        
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS spectrogram (
                id INTEGER PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                avg_cps INTEGER NOT NULL,
                avg_dr INTEGER NOT NULL,
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
        
    def load_from_db(self):
        cursor = self.connection.cursor()

        # --- Load header ---
        cursor.execute("""
            SELECT created, device_id, channels, calibration, concat
            FROM header
            WHERE id = 1
        """)
        header = cursor.fetchone()

        if header:
            created, device_id, channels, calibration, concat = header

            # overwrite runtime config from DB
            self.device_id = device_id
            self.spect_channels = channels
            self.concat_factor = concat
            self.calibration_coeff = eval(calibration) if calibration else []

            # restore acquisition start time
            self.start_date = datetime.fromtimestamp(created)

            # keep wrapper synchronized
            self.data_wrapper.instrument = device_id
            self.data_wrapper.start_date = self.start_date
            self.data_wrapper.concat = concat
            self.data_wrapper.spect_channels = channels

        # --- Load summary ---
        cursor.execute("""
            SELECT accumulated_spectrum, total_duration, total_dose, last_update
            FROM summary
            WHERE id = 1
        """)
        summary = cursor.fetchone()

        if summary:
            acc_blob, total_duration, total_dose, last_update = summary

            if acc_blob is not None:
                self.accumulator = decompress_spectrum(acc_blob)

            self.estimated_dose = total_dose
            self.data_wrapper.estimated_dose = total_dose

            if self.start_date:
                self.data_wrapper.duration = timedelta(seconds=total_duration)

            if last_update:
                self.timestamp_buffer = last_update

        # --- Load recent spectrogram entries for view buffer ---
        cursor.execute("""
            SELECT timestamp, spectrum
            FROM spectrogram
            ORDER BY timestamp DESC
            LIMIT ?
        """, (self.view_buffer.maxlen,))

        rows = cursor.fetchall()

        self.view_buffer.clear()
        self.view_ts_buffer.clear()

        for ts, spec_blob in rows:
            spectrum = decompress_spectrum(spec_blob)
            self.view_buffer.append(spectrum)
            self.view_ts_buffer.append(ts)

        # Update latest spectrum in wrapper
        if self.view_buffer:
            self.data_wrapper.latest_spectrum = self.view_buffer[-1]

        # Recalculate duration if possible
        if self.start_date and self.timestamp_buffer:
            now = datetime.fromtimestamp(self.timestamp_buffer)
            self.data_wrapper.duration = now - self.start_date
            
        self.dataUpdated.emit(self.device_id, self.data_wrapper)
        
    def receive_current(self, name, current):
        if name != self.device_id or self.paused:
            return
        self.cps_buffer += current.CPS
        self.dose_buffer += current.DR
        self.current_values_recieved += 1
        
    def receive_status(self, name ,status):
        if name != self.device_id or self.paused:
            return
        self.temperature_buffer = status.temperature
        
    def receive_spectrum(self, name, spectrum: WrappedSpectrumPackage):
        if name != self.device_id or self.paused:
            return
        
        spectrum = spectrum.y_axis
        

        if self.spectrum_buffer is None:
            self.spectrum_buffer = spectrum
            self.timestamp_buffer = time.time()
            return
        new_ts = int(time.time())
        
        if not round(new_ts) >= round(self.timestamp_buffer + self.save_interval):
            return
        
        self.timestamp_buffer = new_ts

        processed_spectrum = self.process_spectrum(spectrum)
        
        if self.accumulator is None:
            self.accumulator = processed_spectrum.copy()
        else:
            self.accumulator += processed_spectrum
            
        self.view_buffer.append(processed_spectrum)
        if self.start_date is None:
            self.start_date = (datetime.now() + timedelta(microseconds=500_000)).replace(microsecond=0)
            self.data_wrapper.start_date = self.start_date
            
        self.data_wrapper.latest_spectrum = processed_spectrum
        self.data_wrapper.duration = (datetime.now() + timedelta(microseconds=500_000)).replace(microsecond=0) - self.start_date
        
        self.estimated_dose += (self.dose_buffer/max(self.current_values_recieved, 1)) / 3600 * self.save_interval
        self.data_wrapper.estimated_dose = self.estimated_dose

        self.dataUpdated.emit(self.device_id, self.data_wrapper)
        
        spectrum_bytes = compress_spectrum(processed_spectrum)
        
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
        """, (timestamp, round(avg_cps * 1000), round(avg_dr * 1000), temperature, spectrum_bytes))
        self.inserts += 1        
        
        self.update_summary(avg_dr, timestamp)
            
        if self.inserts >= self.db_commit_interval:
            self.connection.commit()
            self.inserts = 0
        
    def update_summary(self, avg_dr, ts):
        cursor = self.connection.cursor()
        cursor.execute("""
            UPDATE summary
            SET total_duration = total_duration + ?,
                total_dose = total_dose = ?,
                accumulated_spectrum = ?,
                last_update = ?
            WHERE id = 1
        """, (self.save_interval, self.estimated_dose, compress_spectrum(self.accumulator), ts))
        
        
        
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
    
class UnpackLog:
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