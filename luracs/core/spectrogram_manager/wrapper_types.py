from collections import deque
from dataclasses import dataclass

import numpy as np

from luracs.clients.gps import GPSData


@dataclass(kw_only=True, frozen=True)
class WrappedHeader:
    # Contain values that are const while the spectrogram is active
    db_name: str
    instrument: str
    calibration_coefficients: list
    x_axis: np.ndarray
    start_date: float
    concat: int
    spect_channels: int
    save_interval: int


@dataclass(kw_only=True, frozen=True)
class WrappedQueues:
    # Contains deques
    spectrogram: np.ndarray
    timestamp_queue: np.ndarray
    gps_queue: np.ndarray[GPSData] | None
    count_rate_queue: np.ndarray[float] | None
    dose_rate_queue: np.ndarray[float] | None


@dataclass(kw_only=True, frozen=True)
class WrappedAccumulatedValues:
    estimated_dose: float
    duration: float


@dataclass(kw_only=True, frozen=True)
class WrappedTempValues:
    # Values that are replaced each update
    latest_row: np.array
    time_delta: float
    status: object


@dataclass(kw_only=True, frozen=True)
class WrappedSpectrogramData:
    # Containing Wrapper
    # Updated with the 'replace' function
    wrapped_queues: WrappedQueues
    wrapped_accumulated_values: WrappedAccumulatedValues
    wrapped_header: WrappedHeader
    wrapped_temp_values: WrappedTempValues
