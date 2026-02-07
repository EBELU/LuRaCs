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


class RunManagerBase(QObject):
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

    def __init__(self):
        super().__init__()
        self.event_loop = None

        self.devices: dict[str, DeviceWrapper] = {}

        self._scan_task: asyncio.Task | None = None
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

        if name in self.devices:
            return

        if device_type not in self.available_clients:
            raise ValueError(
                f"{device_type} not available in {list(self.available_clients.keys())}"
            )

        self.deviceConnecting.emit(name)
        
        new_device = self.available_clients[device_type](device_address)
        
        try:
            await new_device.start()
        except asyncio.CancelledError:
            self.deviceCancelled.emit(name)
        
        self.deviceConnected.emit(name)
        self.devices[name] = new_device
        
        if not self._poll_task:
            self._polling = True
            self._poll_task = asyncio.create_task(self._poll_loop())



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


        # ---- stop all running devices ----        

        for device in self.devices.values():
            try:
                await device.stop()
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



RunManager = RunManagerBase()