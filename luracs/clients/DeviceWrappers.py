from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from ..core.run_manager import _RunManager
import time
from enum import Enum, auto
from abc import ABC, abstractmethod
from dataclasses import dataclass
import numpy as np
import asyncio
import usb
from ..core.settings import Settings
from ..core.gui_logger import gui_logger


from .MockClient import MockClient

from .RadiacodeClient.src import RadiacodeClientAsync
from .RaysidClient.RaysidClient import RaysidClientAsync


class CriticalNotImplementedError(NotImplementedError):
    "Helper exception to enforce good wrappers"
    pass


@dataclass(frozen=True)
class WrappedRealTimePackage:
    CPS: float
    DR: float
    CPS_error: float
    DR_error: float
    timestamp: float


@dataclass(frozen=True)
class WrappedStatusPackage:
    battery: int
    temperature: float
    charging: bool
    total_dose: float
    total_uptime: float
    timestamp: float


@dataclass(frozen=True)
class WrappedSpectrumPackage:
    y_axis: np.ndarray
    live_time: float
    real_time: float
    calib_coeff: list
    timestamp: float


OPTIONAL_WRAPPER_METHODS = {
    "get_calibration",
    "set_calibration",
    "set_hv",
    "get_hv",
    "set_gain",
    "get_gain",
    "set_energy_range",
    "get_energy_range",
    "clear_accumulation",
}


class DeviceWrapper(ABC):
    _registry: dict[str, "DeviceWrapper"] = {}
    run_manager: _RunManager | None = None

    type = None
    
    has_calibration_settings = False
    has_alarm_settings = False
    has_hv_settings = False

    class DeviceState(Enum):
        UNINITIALIZED = auto()
        CONNECTING = auto()
        CONNECTED = auto()
        CONNECTION_FAILED = auto()
        CONNECTION_LOST = auto()
        STOPPING = auto()
        STOPPED = auto()
        ERROR = auto()

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if hasattr(cls, "type") and cls.type:
            cls._registry[cls.type] = cls

    def __init__(self, address, usb: bool, parent=None):
        self.address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = str(address)
        self.connection = "USB" if usb else "BLE"
        self.connected_timestamp = time.time()
        self.state = self.DeviceState.UNINITIALIZED
        
        self.poll_task: asyncio.Task | None = None

        # Virtual placeholders
        self.client = None
        self.channels = None

        # These should be overwritten like the following for all new wrappers
        # self.name = "raysid"
        # self.client = RaysidClientAsync(address)
        # self.channels = 1800

    # --- Helpers ---
    @classmethod
    def get_registry(cls):
        return cls._registry

    @classmethod
    def match_model_to_str(cls, name_string: str):
        for key, obj in cls._registry.items():
            if key in name_string:
                return obj

    def set_state(self, state):
        assert isinstance(state, self.DeviceState)
        self.state = state
        
    async def _poll_loop(self):
            update_delay = Settings.Advanced.update_loop_delay
            spectrum_delay = Settings.Advanced.spectrum_update_delay

            next_loop_time = time.monotonic()
            next_spectrum_time = time.monotonic()
            try:
                while self.is_running():
                    try:
                        now = time.monotonic()

                        realtime = await self.get_RealTimeData()
                        if realtime is not None:
                            self.run_manager.Signals.currentUpdated.emit(self.name, realtime)
                        
                        status = await self.get_Status()
                        if status is not None:
                            self.run_manager.Signals.statusUpdated.emit(self.name, status)
                            
                        if now >= next_spectrum_time:
                            spectrum = await self.get_Spectrum()
                            if spectrum is not None:
                                self.run_manager.Signals.spectrumUpdated.emit(self.name, spectrum)

                        # Schedule next spectrum update
                        if now >= next_spectrum_time:
                            next_spectrum_time += spectrum_delay

                        # Schedule next loop iteration
                        next_loop_time += update_delay
                        sleep_time = next_loop_time - time.monotonic()

                        if sleep_time > 0:
                            await asyncio.sleep(sleep_time)
                        else:
                            # Were behind -> resync to avoid spiral of death
                            next_loop_time = time.monotonic()
                    except asyncio.exceptions.TimeoutError:
                        gui_logger.warning("Poll request timed out")
                    
                    except asyncio.CancelledError:
                        raise
                    
                    except usb.core.USBError:
                        gui_logger.exception(f"USB error for {self.name}, likely disconnect")
                        self.set_state(self.DeviceState.ERROR)
                        self.run_manager.Signals.deviceStateUpdated.emit(self.name, self.state)
                        break
                    
            except asyncio.CancelledError:
                raise
                    
            except Exception:
                gui_logger.exception(f"Polling crashed for {self.name}")
                self.set_state(self.DeviceState.ERROR)
                self.run_manager.Signals.deviceStateUpdated.emit(self.name, self.state)
                
    async def start_polling(self):
        if self.poll_task is None or self.poll_task.done():
            self.poll_task = asyncio.create_task(self._poll_loop())

    async def stop_polling(self):
        if self.poll_task:
            self.poll_task.cancel()
            await asyncio.gather(self.poll_task, return_exceptions=True)
            self.poll_task = None
        

    # --- Critical methods that must be defined ---
    # All the following methods are used by the RunManager and must be defined for each new wrapper
    @abstractmethod
    async def get_RealTimeData(self) -> WrappedRealTimePackage:
        """
        Retrieve the latest real-time detector measurements.

        Returns
        -------
        WrappedRealTimePackage
            A package containing count-rate and dose-rate data,
            associated uncertainties, and the measurement timestamp.
        """
        raise CriticalNotImplementedError("get_RealTimeData")

    @abstractmethod
    async def get_Status(self) -> WrappedStatusPackage:
        """
        Retrieve the current device status information.

        Returns
        -------
        WrappedStatusPackage
            A package containing battery level, temperature,
            charging state, accumulated dose, uptime, and timestamp.
        """
        raise CriticalNotImplementedError("get_Status")
    
    @abstractmethod
    async def get_Spectrum(self) -> WrappedSpectrumPackage:
        """
        Retrieve the latest acquired spectrum.

        Returns
        -------
        WrappedSpectrumPackage
            A package containing spectral counts, detector uptime,
            calibration coefficients, and acquisition timestamp.
        """
        raise CriticalNotImplementedError("get_Spectrum")
    
    @abstractmethod
    def is_running(self) -> bool:
        raise CriticalNotImplementedError("is_running")

    @abstractmethod
    def is_stopped(self) -> bool:
        raise CriticalNotImplementedError("is_stopped")

    async def start(self):
        await self.client.start()
        await self.start_polling()

    async def stop(self):
        await self.stop_polling()
        await self.client.stop()
        
    def reset_spectrum(self):
        pass


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
            latest.CPS,
            latest.DR,
            getattr(latest, "CPS_error", None),
            getattr(latest, "DR_error", None),
            getattr(latest, "timestamp", time.time()),
        )

    async def get_Status(self):
        latest = getattr(self.client, "LatestStatusData", None)
        if latest is None:
            return None

        return WrappedStatusPackage(
            getattr(latest, "battery", None),
            getattr(latest, "temperature", None),
            getattr(latest, "charging", None),
            getattr(latest, "acc_dose", None),
            getattr(latest, "dose_acc_time", None),
            getattr(latest, "timestamp", time.time()),
        )

    async def get_Spectrum(self):
        latest = getattr(self.client, "LatestSpectrum", None)
        if latest is None:
            return None

        return WrappedSpectrumPackage(
            latest.spectrum,
            latest.uptime,
            None,
            getattr(latest, "calib_coeff", None),
            getattr(latest, "timestamp", time.time()),
        )

    def is_running(self):
        return getattr(self.client, "_running", False)

    def is_stopped(self):
        return getattr(self.client, "_stopped", True)


        


class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"

    def __init__(self, address, usb):
        super().__init__(address, usb)
        self.name = self.name.split("#")[-1]
        self.client = RadiacodeClientAsync(address, usb)
        self.channels = 1024
        self.calibration_coefficients = []

    async def get_RealTimeData(self):
        latestRTD = await self.client.get_realtime()
        if latestRTD is not None:
            return WrappedRealTimePackage(
                getattr(latestRTD, "CPS"),
                getattr(latestRTD, "DR"),
                getattr(latestRTD, "CPS_error", None),
                getattr(latestRTD, "DR_error", None),
                getattr(latestRTD, "timestamp", time.time()),
            )

    async def get_Status(self):
        latestStatus = await self.client.get_status()
        if latestStatus is not None:
            return WrappedStatusPackage(
                getattr(latestStatus, "battery", None),
                getattr(latestStatus, "temperature", None),
                getattr(latestStatus, "charging", None),
                getattr(latestStatus, "acc_dose", None),
                getattr(latestStatus, "dose_acc_time", None),
                getattr(latestStatus, "timestamp", time.time()),
            )

    async def get_Spectrum(self):
        latestSpectrum = await self.client.get_spectrum()
        if latestSpectrum is not None:
            self.calibration_coefficients = getattr(latestSpectrum, "calib_coeff", None)
            return WrappedSpectrumPackage(
                getattr(latestSpectrum, "spectrum"),
                getattr(latestSpectrum, "uptime"),
                None,
                getattr(latestSpectrum, "calib_coeff", None),
                getattr(latestSpectrum, "timestamp", time.time()),
            )
            

    def is_running(self):
        stopped = getattr(self.client, "_stopped", True)
        return not stopped

    def is_stopped(self):
        return getattr(self.client, "_stopped", True)

    def set_calibration(self, coeff: list):
        self.client.set_calibration(reversed(coeff))
        
    def get_calibration(self) -> list:
        return self.calibration_coefficients
        
    def reset_spectrum(self):
        self.client.reset()


class RaysidWrapper(DeviceWrapper):
    type = "raysid"

    def __init__(self, address, usb):
        super().__init__(address, False)

        self.client = RaysidClientAsync(address)
        self.channels = 1800

    async def get_RealTimeData(self):
        latestRTD = getattr(self.client, "LatestRealTimeData", None)
        if latestRTD is not None:
            return WrappedRealTimePackage(
                getattr(latestRTD, "CPS"),
                getattr(latestRTD, "DR"),
                getattr(latestRTD, "CPS_error", None),
                getattr(latestRTD, "DR_error", None),
                getattr(latestRTD, "timestamp", time.time()),
            )

    async def get_Status(self):
        latestStatus = getattr(self.client, "LatestStatusData", None)
        if latestStatus is not None:
            return WrappedStatusPackage(
                getattr(latestStatus, "battery", None),
                getattr(latestStatus, "temperature", None),
                getattr(latestStatus, "charging", None),
                getattr(latestStatus, "acc_dose", None),
                getattr(latestStatus, "dose_acc_time", None),
                getattr(latestStatus, "timestamp", time.time()),
            )

    async def get_Spectrum(self):
        latestSpectrum = getattr(self.client, "LatestSpectrum", None)
        if latestSpectrum is not None:
            return WrappedSpectrumPackage(
                getattr(latestSpectrum, "spectrum"),
                getattr(latestSpectrum, "uptime"),
                None,
                getattr(latestSpectrum, "calib_coeff", None),
                getattr(latestSpectrum, "timestamp", time.time()),
            )

    def is_running(self):
        return getattr(self.client, "_running", False)

    def is_stopped(self):
        return getattr(self.client, "_stopped", True)

    async def start(self):
        return await self.client.start()

    async def stop(self):
        return await self.client.stop()

    def set_energy_range(self, energy_range):
        self.client.clear(energy_range)
