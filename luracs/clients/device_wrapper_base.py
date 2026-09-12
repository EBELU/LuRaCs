from __future__ import annotations

import math
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from ..core.run_manager import _RunManager
import asyncio
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum, auto

import numpy as np
import usb

from luracs.core.gui_logger import gui_logger
from luracs.core.settings import Settings


class CriticalNotImplementedError(NotImplementedError):
    "Helper exception to enforce good wrappers"

@dataclass(frozen=True, kw_only=True)
class WrappedRealTimePackage:
    CPS: float
    DR: float
    dead_time: float = np.nan
    CPS_error: float = np.nan
    DR_error: float = np.nan
    timestamp: float


@dataclass(frozen=True, kw_only=True)
class WrappedStatusPackage:
    battery: int = np.nan
    temperature: float = np.nan
    charging: bool = False
    total_dose: float = np.nan
    total_uptime: float = np.nan
    voltage: float = np.nan
    desired_voltage: float = np.nan
    lower_level_discriminator: float = np.nan
    upper_level_discriminator: float = np.nan
    fine_gain: float = np.nan
    timestamp: float


@dataclass(frozen=True, kw_only=True)
class WrappedSpectrumPackage:
    y_axis: np.ndarray
    live_time: float = 1
    real_time: float = None
    calib_coeff: list | None = None
    timestamp: float

class ConnectionType(Enum):
    USB = "USB"
    BLE = "BLE"
    NETWORK = "NETWORK"
    
class SupportedSettings(Enum):
    CALIBRATION = auto()
    HV_AND_AMP = auto()


class DeviceWrapper(ABC):
    _registry: dict[str, DeviceWrapper] = {}
    run_manager: _RunManager | None = None

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
        
    @classmethod
    @abstractmethod
    def get_connection_types(cls):
        pass
    
    @classmethod
    def get_supported_settings(cls):
        return set()

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if hasattr(cls, "type") and cls.type:
            cls._registry[cls.type] = cls

    def __init__(self, address, connection: ConnectionType, parent=None):
        self.address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = str(address)
        self.connection = connection
        assert isinstance(self.connection, ConnectionType), f"Error connection is of type {type(self.connection)}"
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
            start_time = self.run_manager.poll_start_time
            loops_since_spectrum_sample = 0
            
            update_delay = Settings.Advanced.update_loop_delay
            spectrum_delay = Settings.Advanced.spectrum_update_delay
            spectrum_sample_interval = max(
                1,
                math.ceil(spectrum_delay / update_delay),
            )
            
            try:
                while self.is_running():
                    try:
                        if update_delay != Settings.Advanced.update_loop_delay or spectrum_delay != Settings.Advanced.spectrum_update_delay:
                            update_delay = Settings.Advanced.update_loop_delay
                            spectrum_delay = Settings.Advanced.spectrum_update_delay
                            spectrum_sample_interval = max(
                                1,
                                math.ceil(spectrum_delay / update_delay),
                            )
                            loops_since_spectrum_sample = 0
                        
                        realtime = await self.get_RealTimeData()
                        if realtime is not None:
                            self.run_manager.Signals.currentUpdated.emit(self.name, realtime)
                        
                        status = await self.get_Status()
                        if status is not None:
                            self.run_manager.Signals.statusUpdated.emit(self.name, status)
                            
                        loops_since_spectrum_sample += 1 
                        if loops_since_spectrum_sample >= spectrum_sample_interval:
                            spectrum = await self.get_Spectrum()
                            if spectrum is not None:
                                self.run_manager.Signals.spectrumUpdated.emit(self.name, spectrum)
                            loops_since_spectrum_sample = 0
                                                       
                        now = time.monotonic()

                        cycle = math.floor(
                            (now - start_time) / update_delay
                        ) + 1

                        next_poll = start_time + cycle * update_delay

                        sleep_time = next_poll - time.monotonic()

                        if sleep_time > 0:
                            await asyncio.sleep(sleep_time)
                        elif sleep_time < 0:
                            gui_logger.warning(
                                f"Poll loop for {self.name} is running behind by {-sleep_time:.2f}s"
                            )
                            
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
    
    def set_calibration(self):
        raise NotImplementedError()
    
    def start_acquisition(self):
        pass
    
    def stop_acquisition(self):
        pass
        
        