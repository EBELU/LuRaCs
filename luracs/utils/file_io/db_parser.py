import sqlite3 as sql
import numpy as np
from datetime import datetime
import json
from dataclasses import dataclass
from utils.numerics.compression import decompress_spectrum

@dataclass(frozen=True, kw_only=True)
class dbHeader:
    created: datetime
    device_id: str
    channels: int
    concat: int
    save_interval: int
    calibration: list
    
@dataclass(frozen=True, kw_only=True)
class dbSummary:
    total_duration: float
    total_dose: float
    last_update: datetime
    total_spectrum: np.ndarray
    
@dataclass(frozen=True, kw_only=True)
class dbDataColumns:
    timestamps: np.ndarray
    timestamps_datetime: np.ndarray
    temperatures: np.ndarray
    avg_cps: np.ndarray
    avg_dr: np.ndarray
    longitude: np.ndarray
    latitude: np.ndarray
    spectra: np.ndarray
    
class db_parser:
    def __init__(self, file_name=None, connection: sql.Connection = None):
        if file_name is None and connection is None:
            raise ValueError("A file name or a db connection must be given")

        elif file_name is None:
            self.connection = connection

        elif connection is None:
            self.connection = sql.connect(file_name)

        self._channels = None
        self.get_header()

    def get_header(self) -> dbHeader:
        cursor = self.connection.cursor()

        cursor.execute(
            """
            SELECT created, device_id, channels, calibration, concat, save_interval
            FROM header
            WHERE id = 1
            """
        )

        row = cursor.fetchone()

        if row is None:
            raise ValueError("No header found in database")

        (
            created,
            device_id,
            channels,
            calibration,
            concat,
            save_interval,
        ) = row

        if self._channels is None:
            self._channels = channels

        return dbHeader(
            created=datetime.fromtimestamp(created),
            device_id=device_id,
            channels=channels,
            concat=concat,
            save_interval=save_interval,
            calibration=json.loads(calibration) if calibration else [],
        )

    def get_summary(self) -> dbSummary:
        cursor = self.connection.cursor()

        cursor.execute(
            """
            SELECT accumulated_spectrum, total_duration, total_dose, last_update
            FROM summary
            WHERE id = 1
            """
        )

        row = cursor.fetchone()

        if row is None:
            raise ValueError("No summary found in database")

        acc_blob, total_duration, total_dose, last_update = row

        if acc_blob is not None:
            spectrum = decompress_spectrum(
                acc_blob,
                self._channels,
            ).astype(np.float64)
        else:
            spectrum = np.zeros(self._channels, dtype=np.float64)

        return dbSummary(
            total_duration=total_duration,
            total_dose=total_dose,
            last_update=datetime.fromtimestamp(
                last_update,
            ),
            total_spectrum=spectrum,
        )

    def _rows_to_datacolumns(self, rows) -> dbDataColumns:
        timestamps = []
        timestamps_datetime = []
        temperatures = []
        avg_cps = []
        avg_dr = []
        longitudes = []
        latitudes = []
        spectra = []

        for (
            ts,
            cps,
            dr,
            temp,
            lat,
            lon,
            spec_blob,
        ) in rows:
            timestamps.append(ts)
            timestamps_datetime.append(
                datetime.fromtimestamp(ts)
            )
            temperatures.append(temp)
            avg_cps.append(cps / 1000.0)
            avg_dr.append(dr / 1000.0)
            latitudes.append(lat)
            longitudes.append(lon)

            spectra.append(
                decompress_spectrum(
                    spec_blob,
                    self._channels,
                )
            )

        return dbDataColumns(
            timestamps=np.asarray(timestamps, dtype=np.int64),
            timestamps_datetime=np.asarray(
                timestamps_datetime,
                dtype=object,
            ),
            temperatures=np.asarray(
                temperatures,
                dtype=np.float32,
            ),
            avg_cps=np.asarray(
                avg_cps,
                dtype=np.float32,
            ),
            avg_dr=np.asarray(
                avg_dr,
                dtype=np.float32,
            ),
            longitude=np.asarray(
                longitudes,
                dtype=np.float64,
            ),
            latitude=np.asarray(
                latitudes,
                dtype=np.float64,
            ),
            spectra=np.asarray(
                spectra,
                dtype=np.int32,
            ),
        )

    def get_spectrogram_by_date(
        self,
        start_date: datetime | None = None,
        end_date: datetime | None = None,
    ) -> dbDataColumns:

        cursor = self.connection.cursor()

        query = """
            SELECT
                timestamp,
                avg_cps,
                avg_dr,
                temperature,
                latitude,
                longitude,
                spectrum
            FROM spectrogram
        """

        params = []
        conditions = []

        if start_date is not None:
            conditions.append("timestamp >= ?")
            params.append(int(start_date.timestamp()))

        if end_date is not None:
            conditions.append("timestamp <= ?")
            params.append(int(end_date.timestamp()))

        if conditions:
            query += " WHERE " + " AND ".join(conditions)

        query += " ORDER BY timestamp ASC"

        cursor.execute(query, params)

        return self._rows_to_datacolumns(
            cursor.fetchall()
        )

    def get_spectrogram_rows(
        self,
        nr_of_rows: int,
    ) -> dbDataColumns:

        cursor = self.connection.cursor()

        cursor.execute(
            """
            SELECT
                timestamp,
                avg_cps,
                avg_dr,
                temperature,
                lat,
                long,
                spectrum
            FROM spectrogram
            ORDER BY timestamp DESC
            LIMIT ?
            """,
            (nr_of_rows,),
        )

        rows = list(reversed(cursor.fetchall()))

        return self._rows_to_datacolumns(rows)

if __name__ == "__main__":
    p = db_parser(".appdata/spectrogram_library/SpectrumLog-20260325_204646.db")
    import matplotlib.pyplot as plt

    # s = p.get_summary()["total_spectrum"]
    # plt.plot(s)
    # plt.show()
    print(p.get_header().keys())
