import asyncio
from copy import deepcopy
from PySide6.QtCore import QObject, Signal
from qasync import QEventLoop
from dataclasses import dataclass
import numpy as np

from bleak import BleakScanner
import usb.core
import usb.util
from PySide6.QtCore import QObject, Signal
from .CoreGUILogger import gui_logger
from .CoreSettings import Settings
from ..SpectrumClasses import SpectrumData

from ..clients.DeviceWrappers import DeviceWrapper

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
    y_axis: np.ndarray
    live_time: float
    timestamp: float


class RunManagerBase(QObject):
    # ---- data signals ----
    currentUpdated = Signal(str, object)
    statusUpdated = Signal(str, object)
    spectrumUpdated = Signal(str, object)
    
    createDeviceSpectrum = Signal(str, int, str)
    removeDeviceSpectrum = Signal(str)

    # ---- lifecycle signals ----
    newDeviceWrapped = Signal(str, object)
    deviceConnecting = Signal(str)
    deviceConnected = Signal(str)
    deviceCancelled = Signal(str)
    deviceRemoved = Signal(str)
    deviceError = Signal(str, str)
    
    loggerStarted = Signal(str)
    loggerClosed = Signal(str)

    shutdownStarted = Signal()
    shutdownFinished = Signal()

    bluetoothScanStarted = Signal(str)
    bluetoothTimer = Signal(float)
    bluetoothFound = Signal(list)
    bluetoothError = Signal(str)

    def __init__(self):
        super().__init__()
        self.event_loop = None

        self.deviceConnecting.connect(Settings.add_new_connection)

        self.devices: dict[str, DeviceWrapper] = {}

        self._scan_task: asyncio.Task | None = None
        self._scan_lock = asyncio.Lock()
        self._scanner = None
        self._seen_devices: dict[str, object] = {}
        self._scan_task: asyncio.Task | None = None

        self.running = False

        self._poll_task: asyncio.Task | None = None
        self._polling = False
        
        self.channel_table = {"raysid": 1800, "radiacode": 1024}
    
        self.dataloggers: dict[str, "SpectrumLogger"] = {}
        
        
    def set_loop(self, loop):
        self.event_loop = loop

    async def _poll_loop(self):
        spectrum_skip = deepcopy(Settings.Advanced.update_loop_delay)
        try:
            while self._polling:
                for name, client in list(self.devices.items()):
                    ## === Check the state of connected devices === ##
                    # --- Remove stopped clients ---
                    if client.is_stopped:
                        self.devices.pop(name)
                        continue
                    
                    # --- Check if something has crashed ---
                    if not client.is_running and not client.is_stopped:
                        client.set_state(DeviceWrapper.DeviceState.ERROR)
                        continue
                    
                    ## === Get data === ##
                    # --- Get spectrum ---
                    if spectrum_skip >= Settings.Advanced.spectrum_update_delay:
                        spectrum = client.get_Spectrum()
                        if spectrum is not None:
                            self.spectrumUpdated.emit(name, spectrum)
                    
                    # --- Get realtime CPS and DR ---
                    realtime = client.get_RealTimeData()
                    if realtime is not None:
                        self.currentUpdated.emit(name, realtime)
                    
                    # --- Get device status info ---
                    status = client.get_Status()
                    if status is not None:
                        self.statusUpdated.emit(name, status)
                
                # Increment if spectrum is to be skipped this iteration
                if spectrum_skip >= Settings.Advanced.spectrum_update_delay:      
                    spectrum_skip = deepcopy(Settings.Advanced.update_loop_delay)
                else:
                    spectrum_skip += Settings.Advanced.update_loop_delay
                        

                await asyncio.sleep(Settings.Advanced.update_loop_delay)
        except asyncio.CancelledError:
            raise
        except Exception as e:
            gui_logger.critical("Polling crashed:", e)

    

    async def add_device(self, device_address, device_type, usb = False):
        client_wrapper = DeviceWrapper.get_registry().get(device_type, None)
        if client_wrapper is None:
            gui_logger.error(f"Invald device type! {device_type}")
            return

        new_device: DeviceWrapper = client_wrapper(device_address, usb)

        if new_device.name in self.devices:
            gui_logger.debug(f"Device {device_address} already exists")
            return

        self.deviceConnecting.emit(new_device.name)
        self.newDeviceWrapped.emit(new_device.name, new_device)
        new_device.set_state(DeviceWrapper.DeviceState.CONNECTING)
        
        
        try:
            await new_device.start()
        except asyncio.CancelledError:
            gui_logger.error(f"Deive was cancelled {new_device.name}")
            self.deviceCancelled.emit(new_device.name)
        
        except Exception as e:
            gui_logger.error(f"Device start threw exception {e}. Start failed!")
            new_device.set_state(DeviceWrapper.DeviceState.CONNECTION_FAILED)
            return
        if new_device.is_running:
            
            gui_logger.info(f"[Device connected] {new_device.name}")
            self.deviceConnected.emit(new_device.name)
            new_device.set_state(DeviceWrapper.DeviceState.CONNECTED)
            self.createDeviceSpectrum.emit(new_device.name, new_device.channels, new_device.name)
            self.devices[new_device.name] = new_device
            
        
        else:
            new_device.set_state(DeviceWrapper.DeviceState.CONNECTION_FAILED)
            gui_logger.error(f"Device failed to start properly {new_device.name}")
            
        
        if not self._polling:
            self._polling = True
            self._poll_task = asyncio.create_task(self._poll_loop())

    def remove_device(self, device_name: str = "Raysid_1543", remove_spectrum: bool = False):
        asyncio.create_task(self._remove_device(device_name, remove_spectrum))

    async def _remove_device(self, device_name: str, remove_spectrum: bool = False):
        client = self.devices.pop(device_name, None)
        if not client:
            return

        try:
            client.set_state(DeviceWrapper.DeviceState.STOPPING)
            await client.stop()
            gui_logger.info(f"[Device disconnected] {device_name}")
            self.deviceRemoved.emit(device_name)
        except Exception as e:
            gui_logger.warning(str(e))
            self.deviceError.emit(device_name, str(e))
        finally:
            client.set_state(DeviceWrapper.DeviceState.STOPPED)
            if remove_spectrum:
                self.removeDeviceSpectrum.emit(device_name)
            # --- stop polling if no devices remain ---
            if not self.devices and self._poll_task:
                self._polling = False
                self._poll_task.cancel()
                await asyncio.gather(self._poll_task, return_exceptions=True)
                self._poll_task = None

    async def shutdown(self):
        self.shutdownStarted.emit()
        
        # --- Close active loggers ---
        for logger_key in self.dataloggers.copy().keys():
            try:
                self.close_logger(logger_key)
            except Exception as e:
                gui_logger.warning(f"Closing logger {logger_key} raised {e}")

        # --- Stop devices ---
        async def stop_device(device):
            gui_logger.debug(f"Shutting down {device.name}")
            try:
                await asyncio.wait_for(device.stop(), timeout=5)
            except asyncio.TimeoutError:
                gui_logger.warning(f"{device.name} stop timed out")
            except Exception as e:
                gui_logger.error(f"{device.name} stop failed: {e}")

        tasks = [
            asyncio.create_task(stop_device(device))
            for device in self.devices.values()
        ]

        await asyncio.gather(*tasks, return_exceptions=True)

        # --- Close the polling ---
        if self._poll_task:
            gui_logger.debug("Cancelling poll task")
            self._polling = False
            self._poll_task.cancel()
            try:
                await asyncio.wait_for(self._poll_task, timeout=3)
            except asyncio.TimeoutError:
                gui_logger.warning("Poll task did not finish in time")
            self._poll_task = None
            
        self.devices.clear()
        self.shutdownFinished.emit()
        
    def cancel_scan_task(self):
        if self._scan_task and not self._scan_task.done():
            self._scan_task.cancel()
            gui_logger.debug("Bluetooth scan cancelled")
            
    async def connect_bluetooth_list(self, names):
        connections_made = 0
        gui_logger.info(f"Attepting connection to {names}")
        async with self._scan_lock:
            devices = await BleakScanner.discover(timeout=Settings.Advanced.headless_scan_length)
        gui_logger.debug(f"Devices found {[d for d in devices if d.name]}")
        for device in devices:
            if device.name and any(n in device.name for n in names):
                gui_logger.debug(f"Accepted {device.name}")
                for device_type in DeviceWrapper.get_registry().keys():
                    if device_type in device.name.lower():
                        gui_logger.info(f"Connecting device {device.name}, type: {device_type}")
                        await self.add_device(device, device_type)
                        connections_made += 1
                        await asyncio.sleep(0.2)

        if connections_made == 0:
            gui_logger.warning(f"No BLE devices matching {names} were found!")
    async def find_bluetooth(self, timeout=5):
        # Cancel any previous scan
        if self._scan_task and not self._scan_task.done():
            gui_logger.debug("Cancelling previous scan")
            self._scan_task.cancel()
            try:
                await self._scan_task
            except asyncio.CancelledError:
                gui_logger.debug("Previous scan cancelled")

        async def _scan():
            async with self._scan_lock:
                try:
                    gui_logger.info("Started bluetooth scan")
                    devices = await BleakScanner.discover(timeout)
                    self.bluetoothFound.emit(devices)
                except asyncio.CancelledError:
                    gui_logger.info("Bluetooth scan was cancelled")
                    self.bluetoothFound.emit([])
                except Exception as e:
                    gui_logger.error(f"Bluetooth scan error: {e}")
                    self.bluetoothError.emit(str(e))
                finally:
                    self._scan_task = None

        self._scan_task = asyncio.create_task(_scan())
        
    def scan_all_usb(self):
        devices = usb.core.find(find_all=True)
        results = []

        for dev in devices or []:
            try:
                try:
                    dev.set_configuration()
                except usb.core.USBError:
                    pass

                results.append({
                    "vendor_id": hex(dev.idVendor),
                    "product_id": hex(dev.idProduct),
                    "serial_number": usb.util.get_string(dev, dev.iSerialNumber),
                    "manufacturer": usb.util.get_string(dev, dev.iManufacturer),
                    "product": usb.util.get_string(dev, dev.iProduct),
                    "bus": getattr(dev, "bus", None),
                    "address": getattr(dev, "address", None),
                })
            except Exception:
                # Skip devices we can't access
                continue

        return results
    
    
    def add_logger(self, device_name, new_log):
        self.dataloggers[device_name] = new_log
        self.loggerStarted.emit(device_name)
        gui_logger.info(f"[Spectrum Logger Opened] DB_name: {new_log.db_name}, device: {new_log.device_id}")
    
    def close_logger(self, name: str):
        logger = self.dataloggers.pop(name, None)
        if logger:
            logger.close()
            self.loggerClosed.emit(name)
            gui_logger.info(f"[Spectrum Logger Closed] {name}")
        


RunManager = RunManagerBase()