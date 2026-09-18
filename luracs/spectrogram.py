import json
import os
import sqlite3 as sql
import time
import zlib
from collections import deque
from dataclasses import dataclass
from enum import Enum, auto

import numpy as np
from PySide6.QtCore import QObject, Signal

from luracs.clients import (
    WrappedRealTimePackage,
    WrappedSpectrumPackage,
    WrappedStatusPackage,
)
from luracs.clients.gps import GPSData
from luracs.core import IOManager, Log, RunManager, Settings, SpectrumManager
from luracs.utils.numerics.compression import compress_spectrum, decompress_spectrum


def restart_spectrogram(db_name: str):
    new_log = Spectrogram(db_name, resume=True)

    RunManager.Signals.currentUpdated.connect(new_log.receive_current)
    RunManager.Signals.statusUpdated.connect(new_log.receive_status)
    RunManager.Signals.spectrumUpdated.connect(new_log.receive_spectrum)
    RunManager.Signals.GPSUpdated.connect(new_log.receive_gps)

    RunManager.add_spectrogram(db_name, new_log)
    new_log.request_data()


def start_spectrogram(db_name, device: str, save_interval: int = 1, concat: int = 0):
    device_wrapper = RunManager.device_registry.get(device, None)
    if not device_wrapper:
        Log.warning(f"Logging could not be started as {device} does not exist")
        return

    calibration_coeff = None
    for spectrum in SpectrumManager.get_spectra_dict().values():
        if device == spectrum.connection:
            calibration_coeff = spectrum.calibration_coefficients
            break

    new_log = Spectrogram(
        db_name,
        save_interval=save_interval,
        spect_channels=device_wrapper.channels,
        device_id=spectrum.instrument.name
        if spectrum.instrument
        else device_wrapper.name,
        calibration_coeff=calibration_coeff if calibration_coeff is not None else [],
        channel_concat_factor=concat,
        header_meta = {"version": 1, "gps_version": 1}
    )

    RunManager.Signals.currentUpdated.connect(new_log.receive_current)
    RunManager.Signals.statusUpdated.connect(new_log.receive_status)
    RunManager.Signals.spectrumUpdated.connect(new_log.receive_spectrum)
    RunManager.Signals.GPSUpdated.connect(new_log.receive_gps)

    RunManager.add_spectrogram(db_name, new_log)
    new_log.request_data()

# ------------------------------------------------------------------
# Connect the IOManagers signal
IOManager.Importer.sigImportSpectrogram.connect(restart_spectrogram)
# ------------------------------------------------------------------

@dataclass(kw_only=True)
class WrappedSpectrogramData:
    db_name: str
    instrument: str
    calibration_coefficients: list
    start_date: float
    duration: float
    concat: int
    spect_channels: int
    save_interval: int
    latest_spectrum: np.array
    spectrogram: deque
    timestamp_deque: deque
    time_delta: float
    estimated_dose: float
    status: object
    gps_queue: deque[GPSData] | None
    count_rate_queue: deque[float] | None
    dose_rate_queue: deque[float] | None


@dataclass(kw_only=True)
class Buffers:
    latest_timestamp: float = 0
    latest_spectrum: np.array = None
    latest_gps: GPSData | None = None
    accumulated_spectrum: np.array = None
    accumulated_dose_estimate: float = 0
    spectrum_view_queue: deque | None = None
    timestamp_queue: deque | None = None
    timedelta_queue: deque | None = None
    temperature: float = -274
    cps: float = 0
    dose_rate: float = 0
    recieved_values: int = 0
    duration: float = 0
    gps_queue: deque[GPSData] | None = None
    count_rate_queue: deque[float] | None = None
    dose_rate_queue: deque[float] | None  = None
    


class Spectrogram(QObject):
    sigStarted = Signal()
    sigDataUpdated = Signal(str, object)

    class State(Enum):
        ACTIVE = auto()
        PAUSED = auto()
        LOADED = auto()

    def __init__(self, db_name: str, resume: bool = False, **kwargs):
        super().__init__(parent=None)
        self.db_name = db_name
        self.db_path = str(
            (Settings.Paths.spectrogram_library / self.db_name).with_suffix(".db")
        )
        self.connection = sql.connect(self.db_path)
        self.buffers = Buffers(
            spectrum_view_queue=deque([], Settings.Advanced.spectrogram_deque_length),
            timestamp_queue=deque([], Settings.Advanced.spectrogram_deque_length),
            timedelta_queue=deque([], Settings.Advanced.spectrogram_deque_length),
            gps_queue=deque([], Settings.Advanced.spectrogram_deque_length),
            count_rate_queue=deque([], Settings.Advanced.spectrogram_deque_length),
            dose_rate_queue=deque([], Settings.Advanced.spectrogram_deque_length),
        )

        self.paused = resume
        self.state = self.State.LOADED

        if resume:
            self.load_from_db()
            self.state = self.State.LOADED
        else:
            # --- Initialize new logger ---
            self.save_interval = kwargs["save_interval"]  # Interval between steps in s
            self.concat_factor = kwargs[
                "channel_concat_factor"
            ]  # How much the spectrum should be concatinated
            assert self.concat_factor in (1, 2, 4, 8)

            self.spect_channels = round(
                kwargs["spect_channels"] / self.concat_factor
            )  # Number of channels after concat
            self.device_id = kwargs["device_id"]  # What device is the spectrogram from
            self.calibration_coeff = kwargs["calibration_coeff"]
            self.header_meta = kwargs.get("header_meta", {})
            self.start_date = time.time()

            if not resume and os.path.exists(self.db_path):
                self.clean_db()

            else:
                self.init_database()

            # Initialize the wrapper
            self.data_wrapper = WrappedSpectrogramData(
                db_name=db_name,
                instrument=self.device_id,
                calibration_coefficients=self.calibration_coeff,
                start_date=self.start_date,
                duration=0,
                concat=self.concat_factor,
                spect_channels=self.spect_channels,
                save_interval=self.save_interval,
                latest_spectrum=None,
                spectrogram=self.buffers.spectrum_view_queue,
                timestamp_deque=self.buffers.timestamp_queue,
                time_delta=0,
                estimated_dose=0,
                status=self.State.ACTIVE,
                gps_queue=self.buffers.gps_queue,
                count_rate_queue=self.buffers.count_rate_queue,
                dose_rate_queue=self.buffers.dose_rate_queue
            )

            self.state = self.State.ACTIVE

    def init_database(self):
        cursor = self.connection.cursor()

        cursor.execute("""
            CREATE TABLE IF NOT EXISTS header (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                created INTEGER NOT NULL,          -- Unix timestamp
                device_id TEXT NOT NULL,
                channels INTEGER NOT NULL CHECK(channels > 0),
                calibration TEXT,
                concat INTEGER NOT NULL,
                save_interval REAL NOT NULL,
                meta TEXT
            )
        """)

        cursor.execute("""
            CREATE TABLE IF NOT EXISTS summary (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                accumulated_spectrum BLOB,
                total_duration REAL NOT NULL,
                total_dose REAL,
                last_update INTEGER
            )
        """)

        cursor.execute(
            """
            INSERT OR IGNORE INTO summary
            (id, accumulated_spectrum, total_duration, total_dose, last_update)
            VALUES (1, NULL, 0, 0, ?)
        """,
            (time.time(),),
        )

        cursor.execute("""
            CREATE TABLE IF NOT EXISTS spectrogram (
                id INTEGER PRIMARY KEY,
                timestamp REAL NOT NULL,
                avg_cps INTEGER NOT NULL,
                avg_dr INTEGER,
                temperature INTEGER,
                latitude REAL,
                longitude REAL,
                spectrum BLOB NOT NULL,
                meta BLOB
            )
        """)

        cursor.execute("""
            CREATE INDEX IF NOT EXISTS idx_spectrogram_timestamp
            ON spectrogram(timestamp)
        """)

        cursor.execute(
            """
            INSERT OR IGNORE INTO header
            (id, created, device_id, channels, calibration, concat, save_interval, meta)
            VALUES (1, ?, ?, ?, ?, ?, ?, ?)
        """,
            (
                time.time(),
                self.device_id,
                self.spect_channels,
                json.dumps(self.calibration_coeff),
                self.concat_factor,
                self.save_interval,
                json.dumps(self.header_meta)
            ),
        )

        self.connection.commit()

    def close(self):
        self.connection.close()

    def pause_unpause(self):
        if self.paused:
            self.paused = False
            self.state = self.State.ACTIVE
            self.data_wrapper.status = self.state
            self.buffers.latest_spectrum = None
        else:
            self.paused = True
            self.state = self.State.PAUSED
            self.data_wrapper.status = self.state

    def request_data(self):
        self.sigDataUpdated.emit(self.db_name, self.data_wrapper)

    def load_from_db(self):
        cursor = self.connection.cursor()

        # --- Load header ---
        cursor.execute("""
            SELECT created, device_id, channels, calibration, concat, save_interval, meta
            FROM header
            WHERE id = 1
        """)
        header = cursor.fetchone()

        if header:
            created, device_id, channels, calibration, concat, save_interval, header_meta = header

            self.device_id = device_id
            self.spect_channels = channels
            self.concat_factor = concat
            self.save_interval = save_interval
            self.calibration_coeff = json.loads(calibration) if calibration else []
            self.header_meta = header_meta

            # restore acquisition start time
            self.start_date = created

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
                self.buffers.accumulated_spectrum = (
                    decompress_spectrum(acc_blob, channels).copy().astype(np.float64)
                )

            self.buffers.accumulated_dose_estimate = total_dose
            self.buffers.duration = total_duration
            self.buffers.latest_timestamp = last_update

        # --- Load recent spectrogram entries for view buffer ---
        cursor.execute(
            """
            SELECT timestamp, avg_cps, avg_dr, latitude, longitude, spectrum, meta
            FROM spectrogram
            ORDER BY timestamp DESC
            LIMIT ?
        """,
            (self.buffers.spectrum_view_queue.maxlen,),
        )

        rows = cursor.fetchall()

        for ts, avg_cps, avg_dr, latitude, longitude, spec_blob, meta in reversed(rows):
            spectrum = decompress_spectrum(spec_blob, channels)
            self.buffers.spectrum_view_queue.append(spectrum)
            self.buffers.timestamp_queue.append(ts)
            self.buffers.count_rate_queue.append(avg_cps / 1000.)
            _dr = avg_dr / 1000. if avg_dr is not None else np.nan
            self.buffers.dose_rate_queue.append(_dr)
            _gps = GPSData(
                    latitude=latitude, 
                    longitude=longitude
                ) if latitude is not None and longitude is not None else None
            self.buffers.gps_queue.append(_gps)

        self.data_wrapper = WrappedSpectrogramData(
            db_name=self.db_name,
            instrument=self.device_id,
            calibration_coefficients=self.calibration_coeff,
            start_date=self.start_date,
            duration=self.buffers.duration,
            concat=self.concat_factor,
            spect_channels=self.spect_channels,
            save_interval=self.save_interval,
            latest_spectrum=self.buffers.accumulated_spectrum,
            spectrogram=self.buffers.spectrum_view_queue,
            timestamp_deque=self.buffers.timestamp_queue,
            time_delta=1,
            estimated_dose=self.buffers.accumulated_dose_estimate,
            status=self.State.LOADED,
            gps_queue=self.buffers.gps_queue,
            count_rate_queue=self.buffers.count_rate_queue,
            dose_rate_queue=self.buffers.dose_rate_queue
        )

    def receive_current(self, name: str, current: WrappedRealTimePackage):
        if name != self.device_id or self.paused:
            return

        self.buffers.cps += current.CPS
        self.buffers.dose_rate += current.DR
        self.buffers.recieved_values += 1

    def receive_status(self, name: str, status: WrappedStatusPackage):
        if name != self.device_id or self.paused:
            return

        self.buffers.temperature = status.temperature
        
    def receive_gps(self, data: GPSData):
        if data.valid and data.longitude is not None and data.longitude is not None:
            self.buffers.latest_gps = data

    def receive_spectrum(self, name: str, spectrum_package: WrappedSpectrumPackage):
        if name != self.device_id or self.paused:
            return

        # Extract y_axis
        spectrum = spectrum_package.y_axis

        if self.buffers.latest_spectrum is None:
            # Fill the buffer with the first data recieved
            self.buffers.latest_spectrum = spectrum
            self.buffers.latest_timestamp = spectrum_package.timestamp
            return

        new_ts = spectrum_package.timestamp

        if not round(new_ts) >= round(
            self.buffers.latest_timestamp + self.save_interval
        ):
            return

        # Make the timestamp first and then do calculations
        dt = new_ts - self.buffers.latest_timestamp
        self.buffers.latest_timestamp = new_ts
        self.buffers.timestamp_queue.append(new_ts)
        processed_spectrum = self.process_spectrum(spectrum)

        if self.buffers.accumulated_spectrum is None:
            # Fill the accumulation buffer
            self.buffers.accumulated_spectrum = processed_spectrum.copy()
        else:
            self.buffers.accumulated_spectrum += processed_spectrum

        # Put in latest spectrum
        self.buffers.spectrum_view_queue.append(processed_spectrum)

        # Update buffers
        self.buffers.timedelta_queue.append(dt)
        self.buffers.gps_queue.append(self.buffers.latest_gps)
        self.buffers.count_rate_queue.append(self.buffers.cps / max(self.buffers.recieved_values, 1))
        self.buffers.dose_rate_queue.append(self.buffers.dose_rate / max(self.buffers.recieved_values, 1))
        # print(self.buffers.timedelta_queue, np.mean(np.array(self.buffers.timedelta_queue)))
        self.buffers.duration += dt
        self.buffers.accumulated_dose_estimate += (
            (self.buffers.dose_rate / max(self.buffers.recieved_values, 1)) / 3600 * dt
        ) if self.buffers.dose_rate else 0

        # Update wrapped data package
        self.data_wrapper.latest_spectrum = processed_spectrum
        self.data_wrapper.duration = self.buffers.duration
        self.data_wrapper.estimated_dose = self.buffers.accumulated_dose_estimate
        self.data_wrapper.time_delta = dt
        self.data_wrapper.status = self.state
        self.data_wrapper.gps_queue = np.asarray(self.buffers.gps_queue)
        self.data_wrapper.count_rate_queue = np.asarray(self.buffers.count_rate_queue)
        self.data_wrapper.dose_rate_queue = np.asarray(self.buffers.dose_rate_queue)

        # Emit wrapper
        self.sigDataUpdated.emit(self.db_name, self.data_wrapper)

        # Compress spectrum and insert it into the database
        meta_data = {}
        if self.buffers.latest_gps is not None and Settings.Advanced.map_save_extra_gps_data:
            meta_data["extra_gps"] = {
                "alt": self.buffers.latest_gps.altitude,
                "course": self.buffers.latest_gps.course,
                "speed": self.buffers.latest_gps.speed,
                "hdop": self.buffers.latest_gps.hdop,
                "vdop": self.buffers.latest_gps.vdop,
                "ts": self.buffers.latest_gps.timestamp
            }
            
        spectrum_bytes = compress_spectrum(processed_spectrum)
        meta_bytes = zlib.compress(json.dumps(meta_data, separators=(",", ":")).encode("utf-8")) if meta_data else None
        self.insert_spectrogram(
            new_ts,
            self.buffers.cps / max(self.buffers.recieved_values, 1),
            self.buffers.dose_rate / max(self.buffers.recieved_values, 1),
            self.buffers.temperature,
            spectrum_bytes,
            meta_bytes
        )
        self.update_summary(new_ts)

        # Reset buffers
        self.buffers.recieved_values = 0
        self.buffers.cps = 0
        self.buffers.dose_rate = 0

    def insert_spectrogram(
        self, timestamp, avg_cps, avg_dr, temperature, spectrum_bytes, meta_bytes,
    ):
        cursor = self.connection.cursor()
        cursor.execute(
            """
            INSERT INTO spectrogram
            (timestamp, avg_cps, avg_dr, temperature, latitude, longitude, spectrum, meta)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
            (
                timestamp,
                round(avg_cps * 1000),
                round(avg_dr * 1000) if not np.isnan(avg_dr) and avg_dr is not None else None,
                round(temperature * 1000) if not np.isnan(temperature) and temperature is not None else None,
                self.buffers.latest_gps.latitude if self.buffers.latest_gps is not None else None,
                self.buffers.latest_gps.longitude if self.buffers.latest_gps is not None else None,
                spectrum_bytes,
                meta_bytes,
            ),
        )

        self.connection.commit()

    def update_summary(self, ts):
        cursor = self.connection.cursor()
        cursor.execute(
            """
            UPDATE summary
            SET total_duration = ?,
                total_dose = ?,
                accumulated_spectrum = ?,
                last_update = ?
                WHERE id = 1
        """,
            (
                self.buffers.duration,
                self.buffers.accumulated_dose_estimate if self.buffers.accumulated_dose_estimate else 0,
                compress_spectrum(self.buffers.accumulated_spectrum),
                ts,
            ),
        )

    def clean_db(self):
        cursor = self.connection.cursor()

        # Get a list of all tables
        cursor.execute("SELECT name FROM sqlite_master WHERE type='table';")
        tables = cursor.fetchall()

        # Drop each table
        for table_name in tables:
            cursor.execute(f"DROP TABLE IF EXISTS {table_name[0]}")
        self.connection.commit()

        self.init_database()

    def process_spectrum(self, spectrum):
        """Process a recieved spectrum. Subtract last recieved spectrum and concat"""

        spectrum_diff = spectrum - self.buffers.latest_spectrum
        self.buffers.latest_spectrum = spectrum

        assert not np.any(spectrum_diff < 0)

        # Perform concat
        pad_size = (-len(spectrum_diff)) % self.concat_factor
        if pad_size:
            arr = np.pad(spectrum_diff, (0, pad_size))
        else:
            arr = spectrum_diff

        new = arr.reshape(-1, self.concat_factor).sum(axis=1)

        return new

    def resize_deque(self, new_len: int):
        try:
            self.blockSignals(True)
            new_spectrum_view_queue = deque(self.buffers.spectrum_view_queue, new_len)
            new_timestamp_queue = deque(self.buffers.timestamp_queue, new_len)
            new_timedelta_queue = deque(self.buffers.timestamp_queue, new_len)

            self.buffers.spectrum_view_queue = new_spectrum_view_queue
            self.buffers.timestamp_queue = new_timestamp_queue
            self.buffers.timedelta_queue = new_timedelta_queue

            self.data_wrapper.spectrogram = self.buffers.spectrum_view_queue
            self.data_wrapper.timestamp_deque = self.buffers.timestamp_queue

            Log.debug(f"Spectrogram deque updated {self.db_name} -> {new_len}")
        finally:
            self.blockSignals(False)
