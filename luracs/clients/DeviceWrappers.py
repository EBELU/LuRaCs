import time
from enum import Enum, auto
from dataclasses import dataclass
import numpy as np
from PySide6.QtCore import Signal, QObject

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

WRAPPER_METHODS = {
    "get_calibration",
    "set_calibration",
    "set_hv",
    "get_hv",
}

class DeviceWrapper(QObject):
    _registry: dict[str, "DeviceWrapper"] = {}

    stateUpdated = Signal(str, object)

    type = None

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
        super().__init__(parent)
        self.address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = str(address)
        self.connection = "USB" if usb else "BLE"
        self.connected_timestamp = time.time()
        self.state = self.DeviceState.UNINITIALIZED

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

    def set_state(self, state):
        assert isinstance(state, self.DeviceState)
        self.state = state
        self.stateUpdated.emit(self.name, state)
        
    # --- Critical methods that must be defined ---
    # All the following methods are used by the RunManager and must be defined for each new wrapper
    def get_RealTimeData(self) -> WrappedRealTimePackage:
        """
        Retrieve the latest real-time detector measurements.

        Returns
        -------
        WrappedRealTimePackage
            A package containing count-rate and dose-rate data,
            associated uncertainties, and the measurement timestamp.
        """
        raise CriticalNotImplementedError("get_RealTimeData")


    def get_Status(self) -> WrappedStatusPackage:
        """
        Retrieve the current device status information.

        Returns
        -------
        WrappedStatusPackage
            A package containing battery level, temperature,
            charging state, accumulated dose, uptime, and timestamp.
        """
        raise CriticalNotImplementedError("get_Status")


    def get_Spectrum(self) -> WrappedSpectrumPackage:
        """
        Retrieve the latest acquired spectrum.

        Returns
        -------
        WrappedSpectrumPackage
            A package containing spectral counts, detector uptime,
            calibration coefficients, and acquisition timestamp.
        """
        raise CriticalNotImplementedError("get_Spectrum")
    

    def is_running(self)->bool:
        raise CriticalNotImplementedError("is_running")

    def is_stopped(self)->bool:
        raise CriticalNotImplementedError("is_stopped")

    async def start(self):
        raise CriticalNotImplementedError("start")

    async def stop(self):
        raise CriticalNotImplementedError("stop")

    def clear_accumulation(self):
        pass

class MockClientWrapper(DeviceWrapper):
    type = "mock"

    def __init__(self, address=None, usb=None):
        super().__init__(address, usb)
        self.name = "MockClient"
        self.channels = 1024
        self.client = MockClient(self.name)
        
    def get_RealTimeData(self):
        latestRTD = getattr(self.client, "LatestRealTimeData", None)
        if latestRTD is not None:
            return WrappedRealTimePackage(
                getattr(latestRTD, "CPS"),
                getattr(latestRTD, "DR"),
                getattr(latestRTD, "CPS_error", None),
                getattr(latestRTD, "DR_error", None),
                getattr(latestRTD, "timestamp", time.time()),
            )

    def get_Status(self):
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

    def get_Spectrum(self):
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


class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"

    def __init__(self, address, usb):
        super().__init__(address, usb)

        self.name = self.name.split("#")[-1]
        self.client = RadiacodeClientAsync(address, usb)
        self.channels = 1024
        
    def get_RealTimeData(self):
        latestRTD = getattr(self.client, "LatestRealTimeData", None)
        if latestRTD is not None:
            return WrappedRealTimePackage(
                getattr(latestRTD, "CPS"),
                getattr(latestRTD, "DR"),
                getattr(latestRTD, "CPS_error", None),
                getattr(latestRTD, "DR_error", None),
                getattr(latestRTD, "timestamp", time.time()),
            )

    def get_Status(self):
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

    def get_Spectrum(self):
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
    
    def set_calibration(self, coeff):
        self.client.set_calibration(coeff)


class RaysidWrapper(DeviceWrapper):
    type = "raysid"

    def __init__(self, address, usb):
        super().__init__(address, False)

        self.client = RaysidClientAsync(address)
        self.channels = 1800
        
    def get_RealTimeData(self):
        latestRTD = getattr(self.client, "LatestRealTimeData", None)
        if latestRTD is not None:
            return WrappedRealTimePackage(
                getattr(latestRTD, "CPS"),
                getattr(latestRTD, "DR"),
                getattr(latestRTD, "CPS_error", None),
                getattr(latestRTD, "DR_error", None),
                getattr(latestRTD, "timestamp", time.time()),
            )

    def get_Status(self):
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

    def get_Spectrum(self):
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
