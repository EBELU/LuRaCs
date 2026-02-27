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
from .GUILogger import gui_logger
from .Settings import Settings
from ..SpectrumClasses import SpectrumData
from ..utils.DataLogging import SpectrumLogger

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

class DeviceWrapper:
    def __init__(self, name: str, client):
        self.name = name
        self.client = client
    async def start(self):
        await self.client.start()


    async def stop(self):
        await self.client.stop()


class RunManagerBase(QObject):
    # ---- data signals ----
    currentUpdated = Signal(str, object)
    statusUpdated = Signal(str, object)
    spectrumUpdated = Signal(str, object)
    
    createDeviceSpectrum = Signal(str, int, str)
    removeDeviceSpectrum = Signal(str)

    # ---- lifecycle signals ----
    deviceConnecting = Signal(str)
    deviceConnected = Signal(str)
    deviceCancelled = Signal(str)
    deviceRemoved = Signal(str)
    deviceError = Signal(str, str)


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

        self.available_clients = None
        self.running = False

        self._poll_task: asyncio.Task | None = None
        self._polling = False
        
        self.channel_table = {"raysid": 1800, "radiacode": 1024}
    
        self.active_dataloggers: dict[str, SpectrumLogger] = {}
        
        
    def set_loop(self, loop):
        self.event_loop = loop
    
    def set_clients(self, clients):
        self.available_clients = clients

    async def _poll_loop(self):
        spectrum_skip = deepcopy(Settings.Advanced.update_loop_delay)
        try:
            while self._polling:
                for name, client in list(self.devices.items()):
                    if client._stopped:
                        self.devices.pop(name)
                        continue
                    
                    # --- Get spectrum ---
                    if spectrum_skip >= Settings.Advanced.spectrum_update_delay:
                        spectrum = getattr(client, "LatestSpectrum", None)
                        if spectrum is not None:
                            self.spectrumUpdated.emit(name, spectrum)
                    
                    # --- Get realtime CPS and DR ---
                    realtime = getattr(client, "LatestRealTimeData", None)
                    if realtime is not None:
                        packet = CurrentValuesPackage(name, realtime.CPS, realtime.DR, realtime.timestamp)
                        self.currentUpdated.emit(name, packet)
                    
                    # --- Get device status info ---
                    status = getattr(client, "LatestStatusData", None)
                    if status is not None:
                        self.statusUpdated.emit(name, status)
                
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
        if device_type == "raysid":
            ch = 1800
        elif device_type == "radiacode":
            ch = 1024
        else:
            raise RuntimeError
        try:
            name = device_address.name
        except AttributeError:
            name = str(device_address)

        if name in self.devices:
            gui_logger.debug(f"Device {device_address} already exists")
            return

        if device_type not in self.available_clients:
            raise ValueError(
                f"{device_type} not available in {list(self.available_clients.keys())}"
            )

        self.deviceConnecting.emit(name)
        
        if usb:
            new_device = self.available_clients[device_type](device_address, usb)
        else:
            new_device = self.available_clients[device_type](device_address)
        
        try:
            await new_device.start()
        except asyncio.CancelledError:
            gui_logger.error(f"Deive was cancelled {name}")
            self.deviceCancelled.emit(name)
            
        if new_device._running:
            
            gui_logger.info(f"[Device connected] {name}")
            self.deviceConnected.emit(name)
            self.createDeviceSpectrum.emit(name, ch, name)
            self.devices[name] = new_device
        
        else:
            gui_logger.error(f"Device failed to start properly {name}")
            
        
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
            await client.stop()
            gui_logger.info(f"[Device disconnected] {device_name}")
            self.deviceRemoved.emit(device_name)
        except Exception as e:
            self.deviceError.emit(device_name, str(e))
        finally:
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
        for logger_key in self.active_dataloggers.copy().keys():
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
        gui_logger.info(f"Attepting connection to {names}")
        async with self._scan_lock:
            devices = await BleakScanner.discover(timeout=Settings.Advanced.headless_scan_length)
        gui_logger.debug(f"Devices found {[d for d in devices if d.name]}")
        for device in devices:
            if device.name and any(n in device.name for n in names):
                gui_logger.debug(f"Accepted {device.name}")
                for device_type in self.channel_table.keys():
                    if device_type in device.name.lower():
                        gui_logger.info(f"Connecting device {device.name}, type: {device_type}")
                        await self.add_device(device, device_type)
                        await asyncio.sleep(1)

                
    async def find_bluetooth_headless(self, name: str):
        
        def match_name(device, advertisement_data):
            return device.name and name in device.name

        device = await BleakScanner.find_device_by_filter(match_name, timeout=5)
        
        await self.add_device(device)
        
        return True


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
    
    
    def start_logger(self, db_name = "test.db", device: str = "Raysid_1543", save_interval: int = 1):
        if device not in self.devices:
            gui_logger.warning(f"Logging could not be started as {device} does not exist")
            return
        
        self.channel_table["raysid"]
        
        
        new_log = SpectrumLogger(db_name, save_interval, self.channel_table["raysid"], device, [])
        self.currentUpdated.connect(new_log.receive_current)
        self.statusUpdated.connect(new_log.receive_status)
        self.spectrumUpdated.connect(new_log.receive_spectrum)
        self.active_dataloggers[device] = new_log
        gui_logger.info(f"[Spectrum Logger started] DB_name: {db_name}, device: {device}")
    
    def close_logger(self, name: str):
        logger = self.active_dataloggers.pop(name, None)
        if logger:
            logger.close()
            gui_logger.info(f"[Spectrum Logger Closed] {name}")
        


RunManager = RunManagerBase()