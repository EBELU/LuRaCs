import asyncio
from PySide6.QtCore import QObject, Signal  # for Qt signals and QObject base class
from qasync import QEventLoop

from bleak import BleakScanner
import usb.core
import usb.util
from PySide6.QtCore import QObject, Signal


class DeviceWrapper:
    def __init__(self, name: str, client):
        self.name = name
        self.client = client

        self._poll_task: asyncio.Task | None = None
        self._running = False

    async def start(self, run_manager):
        await self.client.start()
        self._running = True

        self._poll_task = asyncio.create_task(
            self._poll_loop(run_manager)
        )

    async def stop(self):
        self._running = False
        if self._poll_task:
            self._poll_task.cancel()
            await asyncio.gather(self._poll_task, return_exceptions=True)

        await self.client.stop()

    async def _poll_loop(self, rm):
        """Poll latest values and emit signals"""
        while self._running:
            if self.client.LatestRealTimeData:
                rm.currentUpdated.emit(
                    self.name,
                    self.client.LatestRealTimeData
                )

            if self.client.LatestStatusData:
                rm.statusUpdated.emit(
                    self.name,
                    self.client.LatestStatusData
                )

            spectrum = self.client.LatestSpectrum
            if spectrum is not None:
                rm.spectrumUpdated.emit(
                    self.name,
                    spectrum
                )

            await asyncio.sleep(0.5)


from PySide6.QtCore import QObject, Signal
import asyncio
from bleak import BleakScanner


class RunManager(QObject):
    # ---- data signals ----
    currentUpdated = Signal(str, object)
    statusUpdated = Signal(str, object)
    spectrumUpdated = Signal(str, object)

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

    def __init__(self, main_event_loop, available_clients):
        super().__init__()
        self.event_loop = main_event_loop

        self.devices: dict[str, DeviceWrapper] = {}
        self._connect_tasks: dict[str, asyncio.Task] = {}
        self._scan_task: asyncio.Task | None = None
        self._scanner = None
        self._seen_devices: dict[str, object] = {}
        self._scan_task: asyncio.Task | None = None

        self.available_clients = available_clients
        self.running = False


    async def add_device(self, device_address, device_type):
        name = str(device_address)

        if name in self.devices or name in self._connect_tasks:
            return

        if device_type not in self.available_clients:
            raise ValueError(
                f"{device_type} not available in {list(self.available_clients.keys())}"
            )

        self.deviceConnecting.emit(name)

        task = asyncio.create_task(
            self._connect_device_task(device_address, device_type)
        )
        self._connect_tasks[name] = task

    def cancel_add_device(self, device_address: str):
        name = str(device_address)
        task = self._connect_tasks.get(name)
        if task:
            task.cancel()


    async def _connect_device_task(self, device_address, device_type):
        name = str(device_address)
        client = None

        try:
            client_cls = self.available_clients[device_type]
            client = client_cls(device_address)

            wrapper = DeviceWrapper(name, client)
            self.devices[name] = wrapper

            await wrapper.start(self)   # <-- cancellable point

            self.deviceConnected.emit(name)

        except asyncio.CancelledError:
            # ---- cancelled while connecting ----
            if client:
                await client.stop()

            self.devices.pop(name, None)
            self.deviceCancelled.emit(name)
            raise

        except Exception as e:
            self.devices.pop(name, None)
            self.deviceError.emit(name, str(e))

        finally:
            self._connect_tasks.pop(name, None)

    async def remove_device(self, device_name: str):
        # cancel if still connecting
        task = self._connect_tasks.pop(device_name, None)
        if task:
            task.cancel()
            return

        wrapper = self.devices.pop(device_name, None)
        if not wrapper:
            return

        try:
            await wrapper.stop()
            self.deviceRemoved.emit(device_name)
        except Exception as e:
            self.deviceError.emit(device_name, str(e))

    async def shutdown(self):
        self.shutdownStarted.emit()

        # ---- cancel all pending connects ----
        connect_tasks = list(self._connect_tasks.values())
        self._connect_tasks.clear()

        for task in connect_tasks:
            task.cancel()

        if connect_tasks:
            await asyncio.gather(*connect_tasks, return_exceptions=True)

        # ---- stop all running devices ----
        wrappers = list(self.devices.values())
        self.devices.clear()

        for wrapper in wrappers:
            try:
                await wrapper.stop()
            except Exception:
                pass  # never let shutdown fail

        self.shutdownFinished.emit()


    async def find_bluetooth(self, timeout = 5):
        if self._scan_task and not self._scan_task.done():
            return

        async def _scan():
            try:
                devices = await BleakScanner.discover(timeout)
                self.bluetoothFound.emit(devices)
            except Exception as e:
                self.bluetoothError.emit(str(e))
            finally:
                self._scan_task = None

        self._scan_task = asyncio.create_task(_scan())



