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

# ==========================================
# Radiacode
# ==========================================

class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"

    def __init__(self, address, connection: ConnectionType):
        super().__init__(address, connection == ConnectionType.USB)
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