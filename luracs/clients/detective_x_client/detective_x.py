import logging
from dataclasses import dataclass
from datetime import datetime

import numpy as np
import requests


@dataclass(frozen=True)
class SpectrumData:
    y_axis: np.ndarray
    live_time: float
    real_time: float
    coordinates_long: float
    coordinates_lat: float
    timestamp: datetime

@dataclass(frozen=True)
class StatusData:
    battery: float
    temperature: float

@dataclass(frozen=True)
class RealTimeData:
    count_rate: float
    dose_rate: float

class DetectiveX:
    def __init__(self, ip_address: str):
        self.ip_address = ip_address
        self.session = requests.sessions.Session()
        self.log = logging.getLogger("DetectiveXClient")

    def check_status(self) -> bool:
        try:
            self.get_status()
            return True
        except requests.HTTPError:
            return False
        
    def get(self, url: str):
        response = self.session.get(url)
        response.raise_for_status()
        return response.json()


    def put(self, url: str):
        response = self.session.put(url, timeout=5)
        response.raise_for_status()


    def get_spectrum(self):
        spectrum = self.get(
            f"http://{self.ip_address}/remote/v1/measurement/spectrum"
        )['Measurement']["Spectrum"]
        return {
            "y_axis": np.asarray(spectrum["ChannelData"]),
            "live_time": float(spectrum["LiveTime"].removesuffix("s").removeprefix("PT")),
            "real_time": float(spectrum["RealTime"].removesuffix("s").removeprefix("PT")),
        }
        


    def get_status(self):
        status = self.get(
            f"http://{self.ip_address}/remote/v1/instrumentStatus"
        )['InstrumentStatus']
        return {
            "battery": round(status['Battery'], 1),
            "temperature": status['CrystalTemperature'] - 273,
        }


    def get_count_data(self):
        count_data = self.get(
            f"http://{self.ip_address}/remote/v1/measurement/doseCountRate"
        )

        measurements = count_data["Measurement"]["CountDoseData"]

        gamma_current = next(
            item
            for item in measurements
            if item["DetectorType"] == "Gamma"
            and item["Remark"] == "Current"
        )

        # Dose rate
        dose_rate = next(
            item for item in gamma_current["DoseRate"]
            if item["unit"].replace("Â", "") == "µSv/h"
        )

        assert dose_rate["unit"].replace("Â", "") == "µSv/h", (
            f"Unexpected dose rate unit: {dose_rate['unit']}"
        )

        # Count rate
        count_rate = gamma_current["CountRate"]

        assert count_rate["unit"] == "cps", (
            f"Unexpected count rate unit: {count_rate['unit']}"
        )

        return {
            "DR": dose_rate["value"],
            "CPS": count_rate["value"],
        }


    def get_calibration(self):
        calibration = self.get(
            f"http://{self.ip_address}/remote/v1/calibration"
        )
        assert calibration["CalibrationType"]["EquationModel"]["Model"] == "Polynomial", f'Calib model is {calibration["CalibrationType"]["EquationModel"]["Model"]}'
        return calibration["CalibrationType"]["EquationModel"]["Coefficients"]


    def start_acquisition(self):
        self.put(
            f"http://{self.ip_address}/remote/v1/remote/acq/start"
        )


    def stop_acquisition(self):
        self.put(
            f"http://{self.ip_address}/remote/v1/remote/acq/stop"
        )


    def clear_spectrum(self):
        self.put(
            f"http://{self.ip_address}/remote/v1/remote/acq/clear"
        )

        
if __name__ == "__main__":
    client = DetectiveX("http://localhost:8000")
    spectrum = client.get_spectrum()
    print(spectrum)