import asyncio
import time

import numpy as np
import usb

from luracs.clients.detective_x_client import DetectiveX
from luracs.clients.device_wrapper_base import (
    ConnectionType,
    DeviceWrapper,
    SupportedSettings,
    WrappedRealTimePackage,
    WrappedSpectrumPackage,
    WrappedStatusPackage,
)
from luracs.clients.digibase_client import digiBase
from luracs.clients.gps import GPSData
from luracs.clients.RadiacodeClient.src import RadiacodeClientAsync
from luracs.clients.RaysidClient.RaysidClient import RaysidClientAsync
from luracs.core.settings import Settings

# ==========================================
# Radiacode
# ==========================================

class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"
    
    usb_id_product=0xF123
    usb_id_vendor=0x0483
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.USB, ConnectionType.BLE}
    
    @classmethod
    def get_supported_settings(cls):
        return {SupportedSettings.CALIBRATION}
        
    def __init__(self, address, connection: ConnectionType, usb_device: usb.core.Device = None):
        super().__init__(address, connection)
        self.name = self.name.split("#")[-1]
        self.client = RadiacodeClientAsync(address, connection == ConnectionType.USB, usb_device=usb_device)
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
        self.run_manager.submit_to_thread(
        self.client.client.set_energy_calib(reversed(coeff))
        )
    def get_calibration(self) -> list:
        return self.calibration_coefficients
        
    def reset_spectrum(self):
        self.run_manager.submit_to_thread(
            self.client.client.spectrum_reset()
        )
        
        
        
        

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
        
        
        
        

# ==========================================
# digiBase
# ==========================================  
        
class DigiBaseWrapper(DeviceWrapper):
    type = "digibase"
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.USB}
    
    @classmethod
    def get_supported_settings(cls):
        return {SupportedSettings.HV_AND_AMP}
    
    def __init__(self, address, connection: ConnectionType, usb_device: usb.core.Device = None):
        address = address.rstrip('\x00')
        super().__init__(address, ConnectionType.USB)
        self.name = f"digiBase-{address}"
        self.base = digiBase(Settings.Paths.third_party_drivers_library, address, dev=usb_device)
        self.channels = 1024
        
        self.live_time_buffer = None
        self.spectrum_buffer = None
        self.hv_ramping = False
        
        self.stopped = False
        self.started = False
        
    def _read_spectrum_data(self):
        return np.asarray(self.base.spectrum), float(self.base.livetime), float(self.base.realtime)

        
    async def get_RealTimeData(self):
        if self.base.hv_readback < 10 or self.hv_ramping:
            return
        spectrum, live_time, real_time = await asyncio.to_thread(self._read_spectrum_data)
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
            DR = np.nan,
            timestamp=time.time()
        )
    
    async def get_Spectrum(self):
        if self.base.hv_readback < 10 or self.hv_ramping:
            return
        
        spectrum, live_time, real_time = await asyncio.to_thread(self._read_spectrum_data)
        return WrappedSpectrumPackage(
            y_axis=spectrum,
            live_time=live_time,
            real_time=real_time,
            timestamp=time.time()
        )
        
    async def get_Status(self):
        return await asyncio.to_thread(self._read_status)
        
    def _read_status(self):
        return WrappedStatusPackage(
            voltage=self.base.hv_readback,
            desired_voltage=self.base.hv,
            lower_level_discriminator=self.base.lld,
            upper_level_discriminator=self.base.uld,
            fine_gain=self.base.fine_gain,
            timestamp=time.time()
            )
    
    async def start(self):
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
    
    def start_acquisition(self):
        self.base.start()
        
    def stop_acquisition(self):
        self.base.stop()
    
        
    async def _update_status(self, count: int):
        for _ in range(count):
            status = await self.get_Status()
            self.run_manager.Signals.statusUpdated(self.name, status)
            await asyncio.sleep(0.25)


    async def set_hv_enabled(self, state: bool):
        if not state:
            self.hv = 0
        self.base.hv_enabled = state

        await self._update_status(8)


    async def set_hv(self, hv: float):
        self.base.hv = hv

        await self._update_status(8)


    async def set_lld(self, lld: float):
        self.base.lld = lld
        await self._update_status(4)


    async def set_uld(self, uld: float):
        self.base.uld = uld
        await self._update_status(4)


    async def set_fine_gain(self, fine_gain: float):
        self.base.fine_gain = fine_gain
        await self._update_status(4)

        
        
        
        
        

# ==========================================
# Detective X
# ==========================================
        
class DetectiveXClient(DeviceWrapper):
    type = "detective_x"
    
    @classmethod
    def get_connection_types(cls):
        return {ConnectionType.NETWORK}
    
    def __init__(self, address, connection: ConnectionType, use_gps: bool = False):
        super().__init__(address, ConnectionType.NETWORK)
        self.name = f"DetectiveX_{address}"
        self.client = DetectiveX(address)
        self.calibration_buffer: list | None = None
        self.use_gps = use_gps
        
        self.channels = 2**14
        
        self.stopped = False
        self.started = False
        
    async def start(self):
        # Test connection
        total_time = 0
        for _ in range(5):
            start = time.time()
            status = await asyncio.to_thread(
                self.client.check_status
                )
            end = time.time()
            
            if not status:
                self.client.log.error("Connection check: status=FAILED, response_time=nan")
                raise RuntimeError("Connection check failed!")
            
            total_time += end - start
        
        # Test Passed
        self.client.log.info(f"Connection check: status=OK!, response_time={round(total_time/5*1e3, 2)}ms")
        
        self.calibration_buffer = (await asyncio.to_thread(self.client.get_calibration))[::-1]
        
        self.client.start_acquisition()
        await self.start_polling()
        if self.use_gps:
            self.run_manager.Signals.GPSConnection.emit(True)
        
        self.started = True
        
        
    def is_running(self) -> bool:
        return self.started and not self.stopped

    def is_stopped(self) -> bool:
        return self.stopped
        
    async def stop(self):
        self.client.stop_acquisition()
        self.client.session.close()
        self.stopped = True
        
    async def get_Spectrum(self):
        ts = time.time()
        spectrum_dict = await asyncio.to_thread(self.client.get_spectrum)
        return WrappedSpectrumPackage(**spectrum_dict, calib_coeff=self.calibration_buffer, timestamp=ts)
    
    async def get_RealTimeData(self):
        real_time_dict = await asyncio.to_thread(self.client.get_count_data)                
        return WrappedRealTimePackage(**real_time_dict, timestamp=time.time())
    
    async def get_Status(self):
        status_dict = await asyncio.to_thread(self.client.get_status)
        coordinates = status_dict.pop("coordinates")
        if self.use_gps:
            if all(coordinates):
                self.run_manager.Signals.GPSUpdated.emit(GPSData(source = "DetectiveX", latitude=coordinates[0], longitude=coordinates[1], valid=True))
            else:
                self.run_manager.Signals.GPSUpdated.emit(GPSData(source = "DetectiveX", latitude=0, longitude=0, valid=False))
        return WrappedStatusPackage(**status_dict, timestamp=time.time())
    
    def start_acquisition(self):
        self.run_manager.submit_to_thread(self._start_acquisition)
        
    async def _start_acquisition(self):
        await asyncio.to_thread(self.client.start_acquisition)
        
    def stop_acquisition(self):
        self.run_manager.submit_to_thread(self._stop_acquisition)

    async def _stop_acquisition(self):
        await asyncio.to_thread(self.client.stop_acquisition)
        
    def reset_spectrum(self):
        self.run_manager.submit_to_thread(self._reset_spectrum)
        
    async def _reset_spectrum(self):
        await asyncio.to_thread(self.client.clear_spectrum)
        