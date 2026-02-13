import asyncio
from PySide6.QtCore import QObject, Signal  # for Qt signals and QObject base class
from qasync import QEventLoop

from dataclasses import dataclass
import numpy as np

from bleak import BleakScanner
import usb.core
import usb.util
from PySide6.QtCore import QObject, Signal
from .GUILogger import gui_logger
from ..SpectrumClasses import SpectrumData

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
    
    createDeviceSpectrum = Signal(str, int)

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

    usbFindPorts = Signal(list)

    def __init__(self):
        super().__init__()
        self.event_loop = None

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
        
        
        
    def set_loop(self, loop):
        self.event_loop = loop
    
    def set_clients(self, clients):
        self.available_clients = clients

    async def _poll_loop(self):
        try:
            while self._polling:
                for name, client in list(self.devices.items()):
                    realtime = getattr(client, "LatestRealTimeData", None)
                    if realtime is not None:
                        packet = CurrentValuesPackage(name, realtime.CPS, realtime.DR, realtime.timestamp)
                        self.currentUpdated.emit(name, packet)
                        
                    status = getattr(client, "LatestStatusData", None)
                    if status is not None:
                        self.statusUpdated.emit(name, status)


                    spectrum = getattr(client, "LatestSpectrum", None)
                    if spectrum is not None:
                        packet = SpectrumData(spectrum.spectrum,
                                              len(spectrum.spectrum),
                                              sum(spectrum.spectrum),
                                              spectrum.uptime)
                        self.spectrumUpdated.emit(name, packet)

                await asyncio.sleep(0.5)

        except asyncio.CancelledError:
            raise
        except Exception as e:
            gui_logger.critical("Polling crashed:", e)


    async def add_device(self, device_address, device_type):
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
            return

        if device_type not in self.available_clients:
            raise ValueError(
                f"{device_type} not available in {list(self.available_clients.keys())}"
            )

        self.deviceConnecting.emit(name)
        
        new_device = self.available_clients[device_type](device_address)
        new_device.name
        
        try:
            await new_device.start()
        except asyncio.CancelledError:
            self.deviceCancelled.emit(name)
            
        if new_device._running:
            
            gui_logger.info(f"[Device connected] {name}")
            self.deviceConnected.emit(name)
            self.createDeviceSpectrum.emit(name, ch)
            self.devices[name] = new_device
            
        
        if not self._poll_task:
            self._polling = True
            self._poll_task = asyncio.create_task(self._poll_loop())

    def remove_device(self, _, device_name: str = "Raysid_1543"):
        asyncio.create_task(self._remove_device(device_name))

    async def _remove_device(self, device_name: str):
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
            # --- stop polling if no devices remain ---
            if not self.devices and self._poll_task:
                self._polling = False
                self._poll_task.cancel()
                await asyncio.gather(self._poll_task, return_exceptions=True)
                self._poll_task = None

    async def shutdown(self):
        self.shutdownStarted.emit()

        for device in list(self.devices.values()):
            try:
                await asyncio.wait_for(device.stop(), timeout=5)
            except asyncio.TimeoutError:
                gui_logger.warning(f"{device.name} stop timed out")
            except Exception as e:
                gui_logger.error(f"{device.name} stop failed: {e}")


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










RunManager = RunManagerBase()