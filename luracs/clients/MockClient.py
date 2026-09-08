import time
from dataclasses import dataclass

import numpy as np

from luracs.clients.device_wrapper_base import (
    DeviceWrapper,
    WrappedRealTimePackage,
    WrappedSpectrumPackage,
    WrappedStatusPackage,
)


def sample_sparse_spectrum(template128):

    # normalize probability distribution
    prob = template128 / template128.sum()

    # random number of samples
    n = np.random.randint(20, 31)

    # choose bins from 128 distribution
    bins128 = np.random.choice(128, size=n, p=prob)

    # create empty 1024 spectrum
    spectrum1024 = np.zeros(1024, dtype=int)

    for b in bins128:
        # map to one of the 8 underlying channels
        channel = b * 8 + np.random.randint(0, 8)
        spectrum1024[channel] += 1

    return spectrum1024


cs137temp = (
    np.array(
        [32,47,49,68,70,63,69,69,70,81,78,64,52,44,39,
         35,32,30,29,28,28,27,25,19,13,8,6,5,5,5,9,21,
         40,44,27,10,4,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
         0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
         0,0,0,0,0,0,0,0,0,0,
        ]
    )
    + 1
)


@dataclass(frozen=True)
class CurrentValuesPackage:
    name: str
    CPS: float
    DR: float
    timestamp: float


@dataclass(frozen=True)
class StatusPackage:
    battery: int
    temperature: float
    charging: bool
    timestamp: float


@dataclass(frozen=True)
class SpectrumResult:
    spectrum: np.ndarray
    uptime: float
    calib_coeff: list
    timestamp: float


class MockClient:
    def __init__(self, name, *args):
        self.name = name
        self._latest = None
        self._running = False
        self._stopped = False
        self.start_ts = time.time()
        self.spect_buf = np.zeros(1024)

    async def start(self):
        self._running = True

    async def stop(self):
        self._running = False
        self._stopped = True

    @property
    def LatestRealTimeData(self):
        cps = np.random.normal(25, 5)
        dr = cps / 250 + np.random.normal(0, 0.01)
        return CurrentValuesPackage(self.name, cps, dr, time.time())

    @property
    def LatestStatusData(self):
        return StatusPackage(100, 22.5, False, time.time())

    @property
    def LatestSpectrum(self):
        self.spect_buf += sample_sparse_spectrum(cs137temp)
        return SpectrumResult(
            self.spect_buf.copy(),
            time.time() - self.start_ts,
            [0.0003705, 2.3694975, 4.2583089],
            time.time(),
        )


class MockClientWrapper(DeviceWrapper):
    type = "mock"
    has_calibration_settings = True

    def __init__(self, address=None, usb=None):
        super().__init__(address, usb)

        self.name = "MockClient"
        self.channels = 1024
        self.client = MockClient(self.name)

    async def get_RealTimeData(self):
        latest = getattr(self.client, "LatestRealTimeData", None)
        if latest is None:
            return None

        return WrappedRealTimePackage(
            CPS = latest.CPS,
            DR = latest.DR,
            timestamp = getattr(latest, "timestamp", time.time()),
        )

    async def get_Status(self):
        latest = getattr(self.client, "LatestStatusData", None)
        if latest is None:
            return None

        return WrappedStatusPackage(
            battery = getattr(latest, "battery", None),
            temperature = getattr(latest, "temperature", None),
            charging = getattr(latest, "charging", None),
            timestamp = getattr(latest, "timestamp", time.time()),
        )

    async def get_Spectrum(self):
        latest = getattr(self.client, "LatestSpectrum", None)
        if latest is None:
            return None

        return WrappedSpectrumPackage(
            y_axis = latest.spectrum,
            live_time = latest.uptime,
            calib_coeff = getattr(latest, "calib_coeff", None),
            timestamp = getattr(latest, "timestamp", time.time()),
        )

    def is_running(self):
        return getattr(self.client, "_running", False)

    def is_stopped(self):
        return getattr(self.client, "_stopped", True)