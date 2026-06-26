import asyncio
import time
from dataclasses import dataclass

import numpy as np

from .types import RealTimeData, RawData, RareData
from .radiacode import RadiaCode
from .logger import logger


@dataclass(frozen=True)
class CurrentValuesPackage:
    CPS: float
    DR: float
    timestamp: float


@dataclass(frozen=True)
class StatusPackage:
    battery: int
    temperature: float
    charging: bool
    acc_dose: float
    dose_acc_time: float
    timestamp: float


@dataclass(frozen=True)
class SpectrumResult:
    spectrum: np.ndarray
    counts: int
    uptime: float
    calib_coeff: list
    timestamp: float


class RadiacodeAsync:
    def __init__(self, address, usb=False):
        self.address = address
        self._usb = usb

        self.name = getattr(address, 'name', str(address))

        self.client: RadiaCode | None = None

        self._latest_cps = None
        self._latest_spectrum = None
        self._latest_status = None

        self._task: asyncio.Task | None = None
        self._stopped = False

    # ---------------- PUBLIC API ----------------

    @property
    def latest_realtime(self):
        return self._latest_cps

    @property
    def latest_spectrum(self):
        return self._latest_spectrum

    @property
    def latest_status(self):
        return self._latest_status

    async def start(self):
        if self._usb:
            self.client = await RadiaCode.connect(serial_number=self.address)
        else:
            self.client = await RadiaCode.connect(bluetooth_mac=self.address)

        self._stopped = False
        self._task = asyncio.create_task(self._poll_loop())

        logger.info(f'Radiacode {self.name} connected')

    async def stop(self):
        self._stopped = True

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass

        if self.client:
            await self.client.stop()

    def reset(self):
        if self.client:
            asyncio.create_task(self.client.spectrum_reset())

    def set_calibration(self, calib_coeff):
        if self.client:
            asyncio.create_task(self.client.set_energy_calib(calib_coeff))

    # ---------------- INTERNAL LOOP ----------------

    async def _poll_loop(self):
        """
        Single serialized polling loop.
        This replaces ALL executor usage and task spawning.
        """
        try:
            while not self._stopped:
                try:
                    print(f'Polling {self.name}...')
                    data = await self.client.data_buf()
                    self._decode_cps_packet(data)

                    spectrum = await self.client.spectrum()
                    self._latest_spectrum = SpectrumResult(
                        spectrum=np.array(spectrum.counts),
                        counts=sum(spectrum.counts),
                        uptime=spectrum.duration.total_seconds(),
                        calib_coeff=[spectrum.a0, spectrum.a1, spectrum.a2][::-1],
                        timestamp=time.time(),
                    )

                except Exception as e:
                    logger.error(f'Polling error: {e}')

                await asyncio.sleep(0.2)

        except asyncio.CancelledError:
            return

    # ---------------- DECODING ----------------

    def _decode_cps_packet(self, data: list):
        for packet in data:
            if isinstance(packet, RealTimeData):
                self._latest_cps = CurrentValuesPackage(
                    CPS=packet.count_rate,
                    DR=packet.dose_rate * 1e4,
                    timestamp=packet.dt.timestamp(),
                )

            elif isinstance(packet, RareData):
                self._latest_status = StatusPackage(
                    battery=packet.charge_level,
                    temperature=packet.temperature,
                    charging=False,
                    acc_dose=packet.dose,
                    dose_acc_time=packet.duration,
                    timestamp=packet.dt.timestamp(),
                )
