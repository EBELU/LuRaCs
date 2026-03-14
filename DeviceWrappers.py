import time
import asyncio
from enum import Enum, auto
from dataclasses import dataclass
import numpy as np

from Clients.RadiacodeClient.src import RadiacodeClientAsync
from Clients.RaysidClient.RaysidClient import RaysidClientAsync


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
    uptime: float
    calib_coeff: list
    timestamp: float
    
    
class DeviceWrapper:
    _registry = {}

    class DeviceState(Enum):
        CONNECTING = auto()
        CONNECTED = auto()
        CONNECTION_FAILED = auto()
        CONNECTION_LOST = auto()
        STOPPED = auto()
        ERROR = auto()

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if hasattr(cls, "type") and cls.type:
            cls._registry[cls.type] = cls

    def __init__(self, address, usb):
        self.address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = str(address)
        self.connection = "USB" if usb else "BLE"
        self.connected_timestamp = time.time()
        self.state = 0

        # Virtual placeholders
        self.client = None
        self.type = None
        self.channels = None

    @classmethod
    def get_registry(cls):
        return cls._registry
    
    def set_state(self, state):
        pass
    
    def get_RealTimeData(self):
        latestRTD = getattr(self.client, "LatestRealTimeData", None)
        
        return WrappedRealTimePackage(
            getattr(latestRTD, "CPS"),
            getattr(latestRTD, "DR"),
            getattr(latestRTD, "CPS_error", None),
            getattr(latestRTD, "DR_error", None),
            getattr(latestRTD, "timestamp", time.time())
        )
    
    def get_Status(self):
        latestStatus = getattr(self.client, "LatestStatusData", None)
        
        return WrappedStatusPackage(
            getattr(latestStatus, "battery", None),
            getattr(latestStatus, "temperature", None),
            getattr(latestStatus, "charging", None),
            getattr(latestStatus, "acc_dose", None),
            getattr(latestStatus, "dose_acc_time", None),
            getattr(latestStatus, "timestamp", time.time())
            
        )
    
    def get_Spectrum(self):
        latestSpectrum = getattr(self.client, "LatestSpectrum", None)
        
        return WrappedSpectrumPackage(
            getattr(latestSpectrum, "spectrum"),
            getattr(latestSpectrum, "uptime"),
            getattr(latestSpectrum, "calib_coeff", None),
            getattr(latestSpectrum, "timestamp", time.time())
        )
    
    @property
    def is_running(self):
        return getattr(self.device, "_running", False)
    
    @property
    def is_stopped(self):
        return getattr(self.device, "_stopped", True)
    
    async def start(self):
        return await self.client.start()
    
    async def stop(self):
        return await self.client.stop()
    
    def clear_accumulation(self):
        self.client.clear()
        
        
        
class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"
    def __init__(self, address, usb):
        super().__init__(address, usb)
        
        self.client = RadiacodeClientAsync(address, usb)
        self.channels = 1024
        
    def set_calibration(self, coeff):
        self.client.set_calibration(coeff)
            
        
        
        
class RaysidWrapper(DeviceWrapper):
    type = "raysid"
    def __init__(self, address, usb):
        super().__init__(address, False)
        
        self.client = RaysidClientAsync(address)
        self.channels = 1800
        
    def clear_accumulation(self, energy_range = 2):
        self.client.clear(energy_range)
        
    
