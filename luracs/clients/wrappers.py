import time

from luracs.clients.device_wrapper_base import (
    ConnectionType,
    DeviceWrapper,
    WrappedRealTimePackage,
    WrappedSpectrumPackage,
    WrappedStatusPackage,
)

from .RadiacodeClient.src import RadiacodeClientAsync
from .RaysidClient.RaysidClient import RaysidClientAsync
from .digibase_client import digiBase

import numpy as np
import asyncio
from ..core.settings import Settings

# ==========================================
# Radiacode
# ==========================================

class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.USB, ConnectionType.BLE}
        
    def __init__(self, address, connection: ConnectionType):
        super().__init__(address, connection)
        self.name = self.name.split("#")[-1]
        self.client = RadiacodeClientAsync(address, connection == ConnectionType.USB)
        self.channels = 1024
        self.calibration_coefficients = []

    async def get_RealTimeData(self):
        latest_rtd = await self.client.get_realtime()

        if latest_rtd is None:
            return None

        return WrappedRealTimePackage(
            CPS=latest_rtd.CPS,
            DR=latest_rtd.DR,
            timestamp=getattr(
                latest_rtd,
                "timestamp",
                time.time(),
            ),
        )


    async def get_Status(self):
        latest_status = await self.client.get_status()

        if latest_status is None:
            return None

        return WrappedStatusPackage(
            battery=latest_status.battery,
            temperature=latest_status.temperature,
            charging=latest_status.charging,
            total_dose=latest_status.acc_dose,
            total_uptime=latest_status.dose_acc_time,
            timestamp=getattr(
                latest_status,
                "timestamp",
                time.time(),
            ),
        )


    async def get_Spectrum(self):
        latest_spectrum = await self.client.get_spectrum()

        if latest_spectrum is None:
            return None

        self.calibration_coefficients = latest_spectrum.calib_coeff

        return WrappedSpectrumPackage(
            y_axis=latest_spectrum.spectrum,
            live_time=latest_spectrum.uptime,
            calib_coeff=latest_spectrum.calib_coeff,
            timestamp=getattr(
                latest_spectrum,
                "timestamp",
                time.time(),
            ),
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
        
        
        
        

# ==========================================
# Raysid
# ==========================================

class RaysidWrapper(DeviceWrapper):
    type = "raysid"
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.BLE}

    def __init__(self, address, connection: ConnectionType):
        super().__init__(address, ConnectionType.BLE)

        self.client = RaysidClientAsync(address)
        self.channels = 1800

    async def get_RealTimeData(self):
        latest_rtd = await self.client.get_realtime()

        if latest_rtd is None:
            return None

        return WrappedRealTimePackage(
            CPS=latest_rtd.CPS,
            DR=latest_rtd.DR,
            timestamp=getattr(
                latest_rtd,
                "timestamp",
                time.time(),
            ),
        )


    async def get_Status(self):
        latest_status = await self.client.get_status()

        if latest_status is None:
            return None

        return WrappedStatusPackage(
            battery=latest_status.battery,
            temperature=latest_status.temperature,
            charging=latest_status.charging,
            timestamp=getattr(
                latest_status,
                "timestamp",
                time.time(),
            ),
        )


    async def get_Spectrum(self):
        latest_spectrum = await self.client.get_spectrum()

        if latest_spectrum is None:
            return None

        self.calibration_coefficients = getattr(
            latest_spectrum,
            "calib_coeff",
            None,
        )

        return WrappedSpectrumPackage(
            y_axis=latest_spectrum.spectrum,
            live_time=latest_spectrum.uptime,
            timestamp=getattr(
                latest_spectrum,
                "timestamp",
                time.time(),
            ),
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
        
class DigiBaseWrapper(DeviceWrapper):
    type = "digibase"
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.USB}
    
    def __init__(self, address, connection: ConnectionType):
        address = address.rstrip('\x00')
        super().__init__(address, ConnectionType.USB)
        self.name = f"digiBase-{address}"
        self.base = digiBase(Settings.Paths.third_party_drivers_library, address)
        self.channels = 1024
        
        self.live_time_buffer = None
        self.spectrum_buffer = None
        
        self.stopped = False
        self.started = False
        
    async def get_RealTimeData(self):
        if self.base.hv_readback < 10:
            return
        live_time = self.base.livetime
        spectrum = np.asarray(self.base.spectrum)
        if self.live_time_buffer is None:
            self.live_time_buffer = live_time
            self.spectrum_buffer = spectrum
            return
        else:
            CPS = (np.sum(spectrum) - np.sum(self.spectrum_buffer)) / max(1e-4, (live_time - self.live_time_buffer))
            self.live_time_buffer = live_time
            self.spectrum_buffer = spectrum
            
        return WrappedRealTimePackage(
            CPS=CPS,
            DR = 0,
            timestamp=time.time()
        )
    
    async def get_Spectrum(self):
        if self.base.hv_readback < 10:
            return
        
        y_axis = np.asarray(self.base.spectrum)
        real_time = float(self.base.realtime)
        live_time = float(self.base.livetime)
        return WrappedSpectrumPackage(
            y_axis=y_axis,
            live_time=live_time,
            real_time=real_time,
            timestamp=time.time()
        )
        
    async def get_Status(self):
        return
    
    async def start(self):
        self.base.hv = 700
        self.base.hv_enabled = True
        await asyncio.sleep(5)
        self.base.clear_counters()
        self.base.clear_spectrum()
        await self.start_polling()
        self.base.start()
        self.started = True
        self.base.log.info(f"Client started: id={self.name}, set_voltage={self.base.hv}V, acutal_voltage={self.base.hv_readback}V")
        
    async def stop(self):
        self.base.stop()
        self.base.hv_enabled = False
        self.base.log.info(f"Client stopping, wait 2s for HV shutdown: id={self.name}")
        await self.stop_polling()
        await asyncio.sleep(2)
        self.stopped = True
        
    def is_running(self):
        return self.started and not self.stopped
    
    def is_stopped(self):
        return self.stopped
    
    def reset_spectrum(self):
        self.base.clear_counters()
        self.base.clear_spectrum()