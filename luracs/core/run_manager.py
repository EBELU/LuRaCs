from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.spectrogram import Spectrogram

import asyncio
from PySide6.QtCore import QObject, Signal
from dataclasses import dataclass
import numpy as np
from .settings import Settings

from bleak import BleakScanner
import sys
import time


# -----------------------------
# Deal with windows usb BS!!! >:(
# -----------------------------
if sys.platform.startswith("win"):
    import libusb_package  # <- thank you!
    import usb.core
    import usb.util
    import usb.backend.libusb1

    backend = usb.backend.libusb1.get_backend(find_library=libusb_package.find_library)
    if backend is None:
        raise RuntimeError(
            "PyUSB could not load the backend. Check DLL and Visual C++ runtime."
        )

else:
    import usb.core
    import usb.util


from .gui_logger import gui_logger

from luracs.clients.DeviceWrappers import DeviceWrapper, CriticalNotImplementedError


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


class _RunManager(QObject):
    """
    The run manager is luracs.core singleton that manages connected devices and spectrogram, since they are tightly connected to the devices. All communications with Bluetooth or USB goes through this class.
    """

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

    spectrogramStarted = Signal(str)
    spectrogramClosed = Signal(str)
    spectrogramDequeResized = Signal()

    shutdownStarted = Signal()
    shutdownFinished = Signal()

    bluetoothScanStarted = Signal(str)
    bluetoothTimer = Signal(float)
    bluetoothFound = Signal(list)
    bluetoothError = Signal(str)

    def __init__(self):
        super().__init__()

        self.deviceConnecting.connect(Settings.add_new_connection)

        self.device_registry: dict[str, DeviceWrapper] = {}

        self._scan_task: asyncio.Task | None = None
        self._scan_lock = asyncio.Lock()
        self._scanner = None
        self._seen_devices: dict[str, object] = {}
        self._scan_task: asyncio.Task | None = None

        self.running = False

        self._poll_task: asyncio.Task | None = None
        self._polling = False

        self.loaded_spectrogram: dict[str, Spectrogram] = {}

    async def _poll_loop(self):
        update_delay = Settings.Advanced.update_loop_delay
        spectrum_delay = Settings.Advanced.spectrum_update_delay

        next_loop_time = time.monotonic()
        next_spectrum_time = time.monotonic()

        try:
            while self._polling:
                now = time.monotonic()

                for name, client in list(self.device_registry.items()):
                    # --- Remove stopped clients ---
                    if client.is_stopped():
                        self.device_registry.pop(name)
                        continue

                    # --- Check crash ---
                    if not client.is_running() and not client.is_stopped():
                        client.set_state(DeviceWrapper.DeviceState.ERROR)
                        continue

                    # --- Spectrum (time-based, not loop-based) ---
                    if now >= next_spectrum_time:
                        spectrum = client.get_Spectrum()
                        if spectrum is not None:
                            self.spectrumUpdated.emit(name, spectrum)

                    # --- Realtime ---
                    realtime = client.get_RealTimeData()
                    if realtime is not None:
                        self.currentUpdated.emit(name, realtime)

                    # --- Status ---
                    status = client.get_Status()
                    if status is not None:
                        self.statusUpdated.emit(name, status)

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

        except asyncio.CancelledError:
            raise

        except CriticalNotImplementedError as e:
            gui_logger.error(
                f"Wrapper for device {name} has critical method '{e}' not implemented! Removing device."
            )
            self.remove_device(name)

        except Exception as e:
            gui_logger.critical("Polling crashed:", e)

    async def add_device(
        self, device_address: str, device_type: str, usb: bool = False
    ):
        client_wrapper = DeviceWrapper.match_model_to_str(device_type)
        if client_wrapper is None:
            gui_logger.error(f"Invald device type! {device_type}")
            return

        new_device: DeviceWrapper = client_wrapper(device_address, usb)

        if new_device.name in self.device_registry:
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

        except CriticalNotImplementedError as e:
            gui_logger.error(
                f"Wrapper for device {new_device.name} has critical method '{e}' not implemented! Aborting!"
            )

        except Exception as e:
            gui_logger.error(f"Device start threw exception {e}. Start failed!")
            new_device.set_state(DeviceWrapper.DeviceState.CONNECTION_FAILED)
            return

        try:
            if new_device.is_running():
                gui_logger.info(
                    f"Device connected: name={new_device.name}, type={device_type}, connection_type={'USB' if usb else 'BLE'}"
                )
                self.deviceConnected.emit(new_device.name)
                new_device.set_state(DeviceWrapper.DeviceState.CONNECTED)
                self.createDeviceSpectrum.emit(
                    new_device.name, new_device.channels, new_device.name
                )
                self.device_registry[new_device.name] = new_device

            else:
                new_device.set_state(DeviceWrapper.DeviceState.CONNECTION_FAILED)
                gui_logger.error(f"Device failed to start properly {new_device.name}")
        except CriticalNotImplementedError as e:
            gui_logger.error(
                f"Wrapper for device {new_device.name} has critical method '{e}' not implemented! Aborting!"
            )

        if not self._polling:
            self._polling = True
            self._poll_task = asyncio.create_task(self._poll_loop())

    def remove_all_devices(self):
        for device_name in list(self.device_registry.keys()):
            self.remove_device(device_name)

    def remove_device(self, device_name: str, remove_spectrum: bool = False):
        asyncio.create_task(self._remove_device(device_name, remove_spectrum))

    async def _remove_device(self, device_name: str, remove_spectrum: bool = False):
        client = self.device_registry.pop(device_name, None)
        if not client:
            return

        try:
            client.set_state(DeviceWrapper.DeviceState.STOPPING)
            print(f"Stopping device {device_name}")
            await client.stop()
            print(f"Device {device_name} stopped")
            gui_logger.info(f"Device disconnected: {device_name}")
            self.deviceRemoved.emit(device_name)
        except Exception as e:
            gui_logger.warning(str(e))
            self.deviceError.emit(device_name, str(e))
        finally:
            client.set_state(DeviceWrapper.DeviceState.STOPPED)
            if remove_spectrum:
                self.removeDeviceSpectrum.emit(device_name)
            # --- stop polling if no devices remain ---
            if not self.device_registry and self._poll_task:
                self._polling = False
                self._poll_task.cancel()
                await asyncio.gather(self._poll_task, return_exceptions=True)
                self._poll_task = None

    async def shutdown(self):
        self.shutdownStarted.emit()
        # --- Close active loggers ---
        for logger_key in self.loaded_spectrogram.copy().keys():
            try:
                self.close_spectrogram(logger_key)
            except Exception as e:
                gui_logger.warning(f"Closing spectrogram {logger_key} raised {e}")

        # --- Stop devices ---
        async def stop_device(device: DeviceWrapper):
            gui_logger.debug(f"Shutting down {device.name}")
            try:
                await asyncio.wait_for(device.stop(), timeout=5)
            except asyncio.TimeoutError:
                gui_logger.warning(f"{device.name} stop timed out")
            except Exception as e:
                gui_logger.error(f"{device.name} stop failed: {e}")

        tasks = [
            asyncio.create_task(stop_device(device))
            for device in self.device_registry.values()
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

        self.device_registry.clear()
        self.shutdownFinished.emit()

    def cancel_scan_task(self):
        if self._scan_task and not self._scan_task.done():
            self._scan_task.cancel()
            gui_logger.debug("Bluetooth scan cancelled")

    async def connect_bluetooth_list(self, names):
        connections_made = 0
        gui_logger.info(f"Attepting connection to {names}")
        async with self._scan_lock:
            devices = await BleakScanner.discover(
                timeout=Settings.Advanced.headless_scan_length
            )
        gui_logger.debug(f"Devices found {[d for d in devices if d.name]}")
        for device in devices:
            if device.name and any(n in device.name for n in names):
                gui_logger.debug(f"Accepted {device.name}")
                for device_type in DeviceWrapper.get_registry().keys():
                    if device_type in device.name.lower():
                        gui_logger.info(
                            f"Connecting device: name={device.name}, type={device_type}"
                        )
                        await self.add_device(device, device_type)
                        connections_made += 1
                        await asyncio.sleep(0.2)

        if connections_made == 0:
            gui_logger.warning(f"No BLE devices matching {names} were found!")

    async def _scan_bluetooth(self, timeout):
        async with self._scan_lock:
            try:
                gui_logger.info(f"Started bluetooth scan: scan_time={timeout}")
                devices = await BleakScanner.discover(timeout)
                self.bluetoothFound.emit(devices)
                return devices
            except asyncio.CancelledError:
                gui_logger.info("Bluetooth scan cancelled")
                self.bluetoothFound.emit([])
            # Include handling for missing connector
            except Exception as e:
                gui_logger.error(f"Bluetooth scan error: {e}")
                self.bluetoothError.emit(str(e))
            finally:
                self._scan_task = None

    async def find_bluetooth(self, timeout=5):
        # Cancel any previous scan
        if self._scan_task and not self._scan_task.done():
            gui_logger.debug("Cancelling previous scan")
            self._scan_task.cancel()
            try:
                await self._scan_task
            except asyncio.CancelledError:
                gui_logger.debug("Previous scan cancelled")

        self._scan_task = asyncio.create_task(self._scan_bluetooth(timeout))

    def scan_all_usb(self):
        devices = usb.core.find(find_all=True)
        results = []

        for dev in devices or []:
            try:
                try:
                    dev.set_configuration()
                except usb.core.USBError:
                    pass

                results.append(
                    {
                        "vendor_id": hex(dev.idVendor),
                        "product_id": hex(dev.idProduct),
                        "serial_number": usb.util.get_string(dev, dev.iSerialNumber),
                        "manufacturer": usb.util.get_string(dev, dev.iManufacturer),
                        "product": usb.util.get_string(dev, dev.iProduct),
                        "bus": getattr(dev, "bus", None),
                        "address": getattr(dev, "address", None),
                    }
                )
            except Exception:
                # Skip devices we can't access
                continue

        return results

    def add_spectrogram(self, device_name: str, new_log: bool):
        self.loaded_spectrogram[device_name] = new_log
        self.spectrogramStarted.emit(device_name)
        gui_logger.info(
            f"Spectrogram Opened: db_name = {new_log.db_name}, device = {new_log.device_id}"
        )

    def close_spectrogram(self, name: str):
        spectrogram = self.loaded_spectrogram.pop(name, None)
        if spectrogram:
            spectrogram.close()
            self.spectrogramClosed.emit(name)
            gui_logger.info(f"Spectrogram Closed: name = {name}")

    def resize_spectrogram_deque(self, new_len: str):
        for spectrogram in self.loaded_spectrogram.values():
            spectrogram.resize_deque(new_len)

        self.spectrogramDequeResized.emit()


RunManager = _RunManager()
