import time
import numpy as np

from dataclasses import dataclass


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
        [
            32,
            47,
            49,
            68,
            70,
            63,
            69,
            69,
            70,
            81,
            78,
            64,
            52,
            44,
            39,
            35,
            32,
            30,
            29,
            28,
            28,
            27,
            25,
            19,
            13,
            8,
            6,
            5,
            5,
            5,
            9,
            21,
            40,
            44,
            27,
            10,
            4,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
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
