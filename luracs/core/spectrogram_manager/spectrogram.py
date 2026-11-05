import json
import os
import sqlite3 as sql
import time
import zlib
from collections import deque
from dataclasses import replace
from enum import Enum, auto

import numpy as np
from PySide6.QtCore import QObject, Signal

from luracs.clients import (
    WrappedRealTimePackage,
    WrappedSpectrumPackage,
    WrappedStatusPackage,
)
from luracs.clients.gps import GPSData
from luracs.core import Log, Settings
from luracs.utils.numerics.compression import compress_spectrum, decompress_spectrum

from . import buffer_types, wrapper_types


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

        queue_buffers = buffer_types.BuffQueues()
        queue_buffers.init_queues(Settings.Advanced.spectrogram_deque_length)
        self.buffers = buffer_types.Buffers(
            queues=queue_buffers,
            latest_values=buffer_types.BuffLatestValues(),
            temp_values=buffer_types.BuffTempValues(),
            accumulators=buffer_types.BuffAccumulators(),
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
            # The x-axis is calculated for the header to save avoid having to do so later on. If no calibration is provided, the x-axis will be the channel numbers.
            x_axis = (
                np.arange(self.spect_channels)
                if self.calibration_coeff is None
                else np.polyval(self.calibration_coeff, np.arange(self.spect_channels))
            )
            self.data_wrapper = wrapper_types.WrappedSpectrogramData(
                wrapped_queues=wrapper_types.WrappedQueues(
                    # Spectrogram is a 2D array where each row is a spectrum at a given timestamp
                    spectrogram=np.stack(self.buffers.queues.spectrum_view_queue)
                    if len(self.buffers.queues.spectrum_view_queue)
                    else np.array([]),
                    # Timestamps for each spectrum
                    timestamp_queue=np.asarray(self.buffers.queues.timestamp_queue),
                    # GPS data for each spectrum, if no GPS data is available, the values will be None
                    gps_queue=np.asarray(self.buffers.queues.gps_queue),
                    # Count rate and dose rate for time intervals between spectra, if no data is available, the values will be NaN
                    count_rate_queue=np.asarray(self.buffers.queues.count_rate_queue),
                    dose_rate_queue=np.asarray(self.buffers.queues.dose_rate_queue),
                ),
                wrapped_accumulated_values=wrapper_types.WrappedAccumulatedValues(
                    estimated_dose=self.buffers.accumulators.accumulated_dose_estimate,
                    duration=self.buffers.accumulators.duration,
                ),
                wrapped_header=wrapper_types.WrappedHeader(
                    db_name=self.db_name,
                    instrument=self.device_id,
                    calibration_coefficients=self.calibration_coeff,
                    x_axis=x_axis,
                    start_date=self.start_date,
                    concat=self.concat_factor,
                    spect_channels=self.spect_channels,
                    save_interval=self.save_interval,
                ),
                wrapped_temp_values=wrapper_types.WrappedTempValues(
                    latest_row=self.buffers.queues.spectrum_view_queue[-1]
                    if len(self.buffers.queues.spectrum_view_queue)
                    else np.array([]),
                    time_delta=0,
                    status=self.state,
                ),
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
                json.dumps(list(self.calibration_coeff)),
                self.concat_factor,
                self.save_interval,
                json.dumps(self.header_meta),
            ),
        )

        self.connection.commit()

    def close(self):
        self.connection.close()

    def set_paused(self, state: bool):
        self.paused = state
        if state:
            self.state = self.State.PAUSED
        else:
            self.state = self.State.ACTIVE
            self.buffers.latest_values.latest_spectrum = None

        replace(self.data_wrapper.wrapped_temp_values, status=self.state)

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
            (
                created,
                device_id,
                channels,
                calibration,
                concat,
                save_interval,
                header_meta,
            ) = header

            self.device_id = device_id
            self.spect_channels = channels
            self.concat_factor = concat
            self.save_interval = save_interval
            self.calibration_coeff = json.loads(calibration) if calibration else []
            self.header_meta = header_meta

            # Restore acquisition start time
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
                self.buffers.accumulators.accumulated_spectrum = (
                    decompress_spectrum(acc_blob, self.spect_channels)
                    .copy()
                    .astype(np.float64)
                )

            self.buffers.accumulators.accumulated_dose_estimate = total_dose or 0
            self.buffers.accumulators.duration = total_duration or 0

            self.buffers.latest_values.latest_timestamp = last_update or 0

        # --- Load recent spectrogram entries for view buffer ---
        queues = self.buffers.queues

        cursor.execute(
            """
            SELECT timestamp, avg_cps, avg_dr, latitude, longitude, spectrum, meta
            FROM spectrogram
            ORDER BY timestamp DESC
            LIMIT ?
            """,
            (queues.spectrum_view_queue.maxlen,),
        )

        rows = cursor.fetchall()

        for ts, avg_cps, avg_dr, latitude, longitude, spec_blob, meta in reversed(rows):
            spectrum = decompress_spectrum(spec_blob, self.spect_channels)

            queues.spectrum_view_queue.append(spectrum)
            queues.timestamp_queue.append(ts)

            queues.count_rate_queue.append(
                avg_cps / 1000.0 if avg_cps is not None else np.nan
            )

            dose_rate = avg_dr / 1000.0 if avg_dr is not None else np.nan
            queues.dose_rate_queue.append(dose_rate)

            gps = (
                GPSData(latitude=latitude, longitude=longitude)
                if latitude is not None and longitude is not None
                else None
            )
            queues.gps_queue.append(gps)

        # --- Reconstruct x-axis ---
        x_axis = (
            np.arange(self.spect_channels)
            if self.calibration_coeff is None
            else np.polyval(self.calibration_coeff, np.arange(self.spect_channels))
        )

        # --- Initialize wrapper ---
        self.data_wrapper = wrapper_types.WrappedSpectrogramData(
            wrapped_queues=wrapper_types.WrappedQueues(
                spectrogram=np.stack(queues.spectrum_view_queue)
                if queues.spectrum_view_queue
                else np.empty((0, self.spect_channels)),
                timestamp_queue=np.asarray(queues.timestamp_queue),
                gps_queue=np.asarray(queues.gps_queue, dtype=object),
                count_rate_queue=np.asarray(queues.count_rate_queue),
                dose_rate_queue=np.asarray(queues.dose_rate_queue),
            ),
            wrapped_accumulated_values=wrapper_types.WrappedAccumulatedValues(
                estimated_dose=self.buffers.accumulators.accumulated_dose_estimate,
                duration=self.buffers.accumulators.duration,
            ),
            wrapped_header=wrapper_types.WrappedHeader(
                db_name=self.db_name,
                instrument=self.device_id,
                calibration_coefficients=self.calibration_coeff,
                x_axis=x_axis,
                start_date=self.start_date,
                concat=self.concat_factor,
                spect_channels=self.spect_channels,
                save_interval=self.save_interval,
            ),
            wrapped_temp_values=wrapper_types.WrappedTempValues(
                latest_row=self.buffers.accumulators.accumulated_spectrum,
                time_delta=1,
                status=self.State.LOADED,
            ),
        )

    def receive_current(self, name: str, current: WrappedRealTimePackage):
        if name != self.device_id or self.paused:
            return

        self.buffers.temp_values.cps += current.CPS
        self.buffers.temp_values.dose_rate += current.DR
        self.buffers.temp_values.received_values += 1

    def receive_status(self, name: str, status: WrappedStatusPackage):
        if name != self.device_id or self.paused:
            return

        self.buffers.temp_values.temperature = status.temperature

    def receive_gps(self, data: GPSData):
        if data.valid and data.longitude is not None and data.latitude is not None:
            self.buffers.latest_values.latest_gps = data

    def receive_spectrum(self, name: str, spectrum_package: WrappedSpectrumPackage):
        if name != self.device_id or self.paused:
            return

        # Extract spectrum
        spectrum = spectrum_package.y_axis

        latest = self.buffers.latest_values
        temp = self.buffers.temp_values
        accum = self.buffers.accumulators
        queues = self.buffers.queues

        if latest.latest_spectrum is None:
            # Fill the buffer with the first data received
            latest.latest_spectrum = spectrum
            latest.latest_timestamp = spectrum_package.timestamp
            return

        new_ts = spectrum_package.timestamp

        if not round(new_ts) >= round(latest.latest_timestamp + self.save_interval):
            return

        # Calculate time difference
        dt = new_ts - latest.latest_timestamp
        latest.latest_timestamp = new_ts

        processed_spectrum = self.process_spectrum(spectrum)
        print(
            f"Processed spectrum: ({np.sum(processed_spectrum)}) {processed_spectrum}"
        )
        # Update accumulated spectrum
        if accum.accumulated_spectrum is None:
            accum.accumulated_spectrum = processed_spectrum.copy()
        else:
            accum.accumulated_spectrum += processed_spectrum

        # Calculate averaged values
        received = max(temp.received_values, 1)
        avg_cps = temp.cps / received
        avg_dose_rate = temp.dose_rate / received

        # Update queues
        queues.timestamp_queue.append(new_ts)
        queues.spectrum_view_queue.append(processed_spectrum)
        queues.timedelta_queue.append(dt)
        queues.gps_queue.append(latest.latest_gps)
        queues.count_rate_queue.append(avg_cps)
        queues.dose_rate_queue.append(avg_dose_rate)

        # Update accumulated values
        accum.duration += dt

        if temp.dose_rate:
            accum.accumulated_dose_estimate += avg_dose_rate / 3600 * dt

        # --- Update wrapped data package ---
        self.data_wrapper = replace(
            self.data_wrapper,
            wrapped_queues=replace(
                self.data_wrapper.wrapped_queues,
                spectrogram=np.vstack(queues.spectrum_view_queue),
                timestamp_queue=np.asarray(queues.timestamp_queue),
                gps_queue=np.asarray(queues.gps_queue, dtype=object),
                count_rate_queue=np.asarray(queues.count_rate_queue),
                dose_rate_queue=np.asarray(queues.dose_rate_queue),
            ),
            wrapped_accumulated_values=replace(
                self.data_wrapper.wrapped_accumulated_values,
                duration=accum.duration,
                estimated_dose=accum.accumulated_dose_estimate,
            ),
            wrapped_temp_values=replace(
                self.data_wrapper.wrapped_temp_values,
                latest_row=processed_spectrum,
                time_delta=dt,
                status=self.state,
            ),
        )
        # print(self.data_wrapper.wrapped_queues.spectrogram)
        # Emit wrapper
        self.sigDataUpdated.emit(self.db_name, self.data_wrapper)

        # --- Prepare metadata ---
        meta_data = {}

        if latest.latest_gps is not None and Settings.Advanced.map_save_extra_gps_data:
            meta_data["extra_gps"] = {
                "alt": latest.latest_gps.altitude,
                "course": latest.latest_gps.course,
                "speed": latest.latest_gps.speed,
                "hdop": latest.latest_gps.hdop,
                "vdop": latest.latest_gps.vdop,
                "ts": latest.latest_gps.timestamp,
            }

        if Settings.Advanced.map_save_extra_measurement_time_data:
            meta_data["extra_measurement_time"] = {
                "live_time": spectrum_package.live_time,
                "real_time": spectrum_package.real_time,
            }

        # --- Save to database ---
        spectrum_bytes = compress_spectrum(processed_spectrum)

        meta_bytes = (
            zlib.compress(json.dumps(meta_data, separators=(",", ":")).encode("utf-8"))
            if meta_data
            else None
        )

        self.insert_spectrogram(
            new_ts,
            avg_cps,
            avg_dose_rate,
            temp.temperature,
            spectrum_bytes,
            meta_bytes,
        )

        self.update_summary(new_ts)

        # --- Reset temporary measurement values ---
        temp.reset()

    def process_spectrum(self, spectrum):
        """Process a received spectrum. Subtract the last received spectrum and concat."""

        spectrum_diff = spectrum - self.buffers.latest_values.latest_spectrum
        self.buffers.latest_values.latest_spectrum = spectrum

        assert not np.any(spectrum_diff < 0)

        # Perform concat
        pad_size = (-len(spectrum_diff)) % self.concat_factor

        if pad_size:
            arr = np.pad(spectrum_diff, (0, pad_size))
        else:
            arr = spectrum_diff

        new = arr.reshape(-1, self.concat_factor).sum(axis=1)

        return new

    def insert_spectrogram(
        self,
        timestamp,
        avg_cps,
        avg_dr,
        temperature,
        spectrum_bytes,
        meta_bytes,
    ):
        cursor = self.connection.cursor()

        latest_gps = self.buffers.latest_values.latest_gps

        cursor.execute(
            """
            INSERT INTO spectrogram
            (timestamp, avg_cps, avg_dr, temperature, latitude, longitude, spectrum, meta)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                timestamp,
                round(avg_cps * 1000),
                round(avg_dr * 1000)
                if avg_dr is not None and not np.isnan(avg_dr)
                else None,
                round(temperature * 1000)
                if temperature is not None and not np.isnan(temperature)
                else None,
                latest_gps.latitude if latest_gps is not None else None,
                latest_gps.longitude if latest_gps is not None else None,
                spectrum_bytes,
                meta_bytes,
            ),
        )

        self.connection.commit()

    def update_summary(self, ts):
        cursor = self.connection.cursor()

        accum = self.buffers.accumulators

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
                accum.duration,
                accum.accumulated_dose_estimate or 0,
                compress_spectrum(accum.accumulated_spectrum)
                if accum.accumulated_spectrum is not None
                else None,
                ts,
            ),
        )

        self.connection.commit()

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

    def resize_deque(self, new_len: int):
        try:
            self.blockSignals(True)

            queues = self.buffers.queues

            # Resize queues
            queues.spectrum_view_queue = deque(
                queues.spectrum_view_queue, maxlen=new_len
            )
            queues.timestamp_queue = deque(queues.timestamp_queue, maxlen=new_len)
            queues.timedelta_queue = deque(queues.timedelta_queue, maxlen=new_len)
            queues.gps_queue = deque(queues.gps_queue, maxlen=new_len)
            queues.count_rate_queue = deque(queues.count_rate_queue, maxlen=new_len)
            queues.dose_rate_queue = deque(queues.dose_rate_queue, maxlen=new_len)

            # Update immutable wrapper
            self.data_wrapper = replace(
                self.data_wrapper,
                wrapped_queues=replace(
                    self.data_wrapper.wrapped_queues,
                    spectrogram=np.stack(queues.spectrum_view_queue)
                    if queues.spectrum_view_queue
                    else np.empty((0, self.spect_channels)),
                    timestamp_queue=np.asarray(queues.timestamp_queue),
                    gps_queue=np.asarray(queues.gps_queue, dtype=object),
                    count_rate_queue=np.asarray(queues.count_rate_queue),
                    dose_rate_queue=np.asarray(queues.dose_rate_queue),
                ),
            )

            Log.debug(f"Spectrogram deque updated {self.db_name} -> {new_len}")

        finally:
            self.blockSignals(False)
