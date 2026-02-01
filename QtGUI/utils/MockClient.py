import asyncio
import time
import numpy as np

from dataclasses import dataclass

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
    timestamp: float

class MockClient:
    def __init__(self, name, *args):
        self.name = name
        self._latest = None
        self._running = False

    async def start(self):
        self._running = True
        asyncio.create_task(self._loop())

    async def stop(self):
        self._running = False

    async def _loop(self):
        while self._running:
            cps = np.random.normal(500, 50)
            dr = cps / 1000 + np.random.normal(0, 0.01)
            self._latest = CurrentValuesPackage(
                self.name, cps, dr, time.time()
            )
            await asyncio.sleep(0.5)

    @property
    def LatestRealTimeData(self):
        return self._latest

    @property
    def LatestStatusData(self):
        return None

    @property
    def LatestSpectrum(self):
        return None
