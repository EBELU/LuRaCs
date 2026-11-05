from collections import deque
from dataclasses import dataclass

import numpy as np

from luracs.clients.gps import GPSData


@dataclass(kw_only=True)
class BuffQueues:
    gps_queue: deque[GPSData] | None = None
    count_rate_queue: deque[float] | None = None
    dose_rate_queue: deque[float] | None = None
    timestamp_queue: deque | None = None
    timedelta_queue: deque | None = None
    spectrum_view_queue: deque | None = None

    def init_queues(self, initial_length: int):
        self.gps_queue = deque(maxlen=initial_length)
        self.count_rate_queue = deque(maxlen=initial_length)
        self.dose_rate_queue = deque(maxlen=initial_length)
        self.timestamp_queue = deque(maxlen=initial_length)
        self.timedelta_queue = deque(maxlen=initial_length)
        self.spectrum_view_queue = deque(maxlen=initial_length)

    def add_to_queues(
        self,
        gps: GPSData,
        count_rate: float,
        dose_rate: float,
        timestamp,
        timedelta,
        spectrum_view,
    ):
        self.gps_queue.append(gps)
        self.count_rate_queue.append(count_rate)
        self.dose_rate_queue.append(dose_rate)
        self.timestamp_queue.append(timestamp)
        self.timedelta_queue.append(timedelta)
        self.spectrum_view_queue.append(spectrum_view)


@dataclass(kw_only=True)
class BuffLatestValues:
    latest_timestamp: float = 0
    latest_spectrum: np.array = None
    latest_gps: GPSData | None = None


@dataclass(kw_only=True)
class BuffTempValues:
    cps: float = 0
    dose_rate: float = 0
    temperature: float = -274
    received_values: int = 0

    def reset(self):
        self.cps = 0
        self.dose_rate = 0
        self.temperature = -274
        self.received_values = 0


@dataclass(kw_only=True)
class BuffAccumulators:
    accumulated_spectrum: np.array = None
    accumulated_dose_estimate: float = 0
    duration: float = 0


@dataclass(kw_only=True)
class Buffers:
    queues: BuffQueues
    latest_values: BuffLatestValues
    temp_values: BuffTempValues
    accumulators: BuffAccumulators
