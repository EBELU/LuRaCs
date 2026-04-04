import sqlite3 as sql
import numpy as np
from datetime import datetime, timezone
import zlib

from utils.numerics.compression import decompress_spectrum

class db_parser:
    def __init__(self, file_name = None, connection: sql.Connection  = None):
        if file_name is None and connection is None:
            raise ValueError("A file name or a db connection must be given")
        
        elif file_name is None:
            self.connection = connection
        
        elif connection is None:
            self.connection = sql.connect(file_name)
        
        self._channels = None
        self.get_header()

    def get_header(self) -> dict:
        """Get header information about the logger.
        
        Dict keys: ['created', 'device_id', 'channels', 'concat', 'save_interval', 'calibration']
        """
        
        cursor = self.connection.cursor()
        # --- Load header ---
        cursor.execute("""
            SELECT created, device_id, channels, calibration, concat, save_interval
            FROM header
            WHERE id = 1
        """)
        header = cursor.fetchone()
        
        # --- Parse ---

        data = {}
        if header:
            data["created"], data["device_id"], data["channels"], calibration, data["concat"], data["save_interval"] = header
            data["calibration"] = eval(calibration) if calibration else []
        
        if self._channels is None:
            self._channels = data["channels"] # Must be saved for summary

        return data

    def get_summary(self) -> dict:
        """Retrieve the logger summary
        
        Dict keys: ['total_duration', 'total_dose', 'last_update', 'total_spectrum']
        """
        cursor = self.connection.cursor()
        # --- Load summary ---
        cursor.execute("""
            SELECT accumulated_spectrum, total_duration, total_dose, last_update
            FROM summary
            WHERE id = 1
        """)

        summary = cursor.fetchone()

        data = {}
        if summary:
            acc_blob, data["total_duration"], data["total_dose"], data["last_update"] = summary

            if acc_blob is not None:
                data["total_spectrum"] = decompress_spectrum(acc_blob, self._channels).copy().astype(np.float64)
            else:
                data["total_spectrum"] = None

        return data

    def get_spectrogram_by_date(self, start_date: datetime = None, end_date: datetime = None) -> dict:
        """Retrieve logged data between specific dates. Data is as a dict with arrays separated by data.
        
        Dict keys: ['timestamp', 'datetime', 'avg_cps', 'avg_dr', 'temperature', 'spectrum']
        """
        cursor = self.connection.cursor()

        # --- Convert datetime ---
        if start_date is not None:
            start_date = int(start_date.timestamp())

        if end_date is not None:
            end_date = int(end_date.timestamp())
            
        # --- Get data ---

        query = """
            SELECT timestamp, avg_cps, avg_dr, temperature, spectrum
            FROM spectrogram
        """
        params = []
        conditions = []

        if start_date is not None:
            conditions.append("timestamp >= ?")
            params.append(start_date)

        if end_date is not None:
            conditions.append("timestamp <= ?")
            params.append(end_date)

        if conditions:
            query += " WHERE " + " AND ".join(conditions)

        query += " ORDER BY timestamp ASC"

        cursor.execute(query, tuple(params))
        rows = cursor.fetchall()

        # --- Parse ---
        data = {
            "timestamp": [],
            "datetime": [],
            "avg_cps": [],
            "avg_dr": [],
            "temperature": [],
            "spectrum": []
        }

        for ts, avg_cps, avg_dr, temp, spec_blob in rows:
            data["timestamp"].append(ts)
            data["datetime"].append(datetime.fromtimestamp(ts))  # convert back
            data["avg_cps"].append(avg_cps)
            data["avg_dr"].append(avg_dr)
            data["temperature"].append(temp)
            data["spectrum"].append(
                decompress_spectrum(spec_blob, self._channels)
            )

        return data
    
    def get_spectrogram_rows(self, nr_of_rows: int) -> dict:
        cursor = self.connection.cursor()

        cursor.execute("""
            SELECT timestamp, avg_cps, avg_dr, temperature, spectrum
            FROM spectrogram
            ORDER BY timestamp DESC
            LIMIT ?
        """, (nr_of_rows,))

        rows = cursor.fetchall()

        # --- Parse ---
        data = {
            "timestamp": [],
            "datetime": [],
            "avg_cps": [],
            "avg_dr": [],
            "temperature": [],
            "spectrum": []
        }

        # reverse so output is chronological
        for ts, avg_cps, avg_dr, temp, spec_blob in reversed(rows):
            data["timestamp"].append(ts)
            data["datetime"].append(datetime.fromtimestamp(ts, tz=timezone.utc))
            data["avg_cps"].append(avg_cps / 1000)
            data["avg_dr"].append(avg_dr / 1000)
            data["temperature"].append(temp)
            data["spectrum"].append(
                decompress_spectrum(spec_blob, self._channels)
            )

        return data
    
    
if __name__ == "__main__":
    p = db_parser(".appdata/spectrogram_library/SpectrumLog-20260325_204646.db")
    import matplotlib.pyplot as plt
    # s = p.get_summary()["total_spectrum"]
    # plt.plot(s)
    # plt.show()
    print(p.get_header().keys())