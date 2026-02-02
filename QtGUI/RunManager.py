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
    async def start(self):
        await self.client.start()


    async def stop(self):
        await self.client.stop()



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

        self._poll_task: asyncio.Task | None = None
        self._polling = False

    async def _poll_loop(self):
        try:
            while self._polling:
                for name, wrapper in list(self.devices.items()):
                    client = wrapper.client

                    if client.LatestRealTimeData is not None:
                        self.currentUpdated.emit(
                            name,
                            client.LatestRealTimeData
                        )

                    if client.LatestStatusData is not None:
                        self.statusUpdated.emit(
                            name,
                            client.LatestStatusData
                        )

                    spectrum = getattr(client, "LatestSpectrum", None)
                    if spectrum is not None:
                        self.spectrumUpdated.emit(
                            name,
                            spectrum
                        )

                await asyncio.sleep(0.5)

        except asyncio.CancelledError:
            pass


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

            await wrapper.start()   # <-- cancellable point

            self.deviceConnected.emit(name)

            if not self._poll_task:
                self._polling = True
                self._poll_task = asyncio.create_task(self._poll_loop())

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

        wrapper = self.devices.pop(device_name, None)
        if not wrapper:
            return

        try:
            await wrapper.stop()
            self.deviceRemoved.emit(device_name)
        except Exception as e:
            self.deviceError.emit(device_name, str(e))
        finally:
            # ---- stop polling if no devices remain ----
            if not self.devices and self._poll_task:
                self._polling = False
                self._poll_task.cancel()
                await asyncio.gather(self._poll_task, return_exceptions=True)
                self._poll_task = None

    async def shutdown(self):
        self.shutdownStarted.emit()

        if self._poll_task:
            self._polling = False
            self._poll_task.cancel()
            await asyncio.gather(self._poll_task, return_exceptions=True)
            self._poll_task = None
        # ---- cancel all pending connects ----
        connect_tasks = list(self._connect_tasks.values())
        self._connect_tasks.clear()

        for task in connect_tasks:
            task.cancel()

        if connect_tasks:
            await asyncio.gather(*connect_tasks, return_exceptions=True)

        # ---- stop all running devices ----        

        for wrapper in self.devices.values():
            try:
                await wrapper.stop()
            except Exception:
                pass  # never let shutdown fail
        
        self.devices.clear()
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



