from enum import Enum, auto
import asyncio
from SpectrumClasses import Spectrum

class DeviceState(Enum):
    CREATED = auto()
    CONNECTING = auto()
    RUNNING = auto()
    DISCONNECTED = auto()
    ERROR = auto()
    STOPPED = auto()


class Device:
    def __init__(self, model, name, address, spectrum_channels):
        self.model = model
        self.name = name
        self.address = address
        self.spectrum = Spectrum(spectrum_channels, name)

        self.client: RaysidClientAsync | None = None
        self.state = DeviceState.CREATED
        self.last_error: Exception | None = None

        self._poll_task: asyncio.Task | None = None

    async def start(self):
        if self.state not in (DeviceState.CREATED, DeviceState.DISCONNECTED):
            return self.state == DeviceState.RUNNING

        self.state = DeviceState.CONNECTING

        try:
            if self.model.lower() == "raysid":
                self.client = RaysidClientAsync(self.address)
                self.client._parent_on_disconnect = self._handle_disconnect
                await self.client.start()

            self.state = DeviceState.RUNNING

            # start polling task
            self._poll_task = asyncio.create_task(self._poll_loop())

            return True

        except Exception as e:
            self.state = DeviceState.ERROR
            self.last_error = e
            return False

    async def stop(self):
        self.state = DeviceState.STOPPED

        if self._poll_task:
            self._poll_task.cancel()
            await asyncio.gather(self._poll_task, return_exceptions=True)
            self._poll_task = None

        if self.client:
            await self.client.stop()
            self.client = None

    async def _poll_loop(self):
        while self.state == DeviceState.RUNNING:
            await asyncio.sleep(0.5)
            # update spectrum if available
            if self.client:
                spectrum = self.client.LatestSpectrum
                if spectrum is not None:
                    self.spectrum.set_y_data(spectrum)

    def _handle_disconnect(self):
        self.state = DeviceState.DISCONNECTED

    @property
    def realtime_data(self):
        return self.client.LatestRealTimeData if self.client else None

    @property
    def status(self):
        return self.client.LatestStatusData if self.client else None

    @property
    def spectrum_data(self):
        return self.client.LatestSpectrum if self.client else None


class DeviceManager:
    def __init__(self):
        self.devices: dict[str, Device] = {}

    async def connect_device(self, model, name, address):
        if model.lower() == "raysid":
            channels = 1800
        elif model.lower() == "radiacode":
            channels = 1023
        else:
            raise TypeError("Invalid device model")

        device = Device(model, name, address, channels)
        success = await device.start()
        if success:
            self.devices[name] = device
        else:
            raise RuntimeError(f"Device {name} failed to start")

    async def disconnect_device(self, name: str, remove_from_manager=True):
        device = self.devices.get(name)
        if not device:
            raise ValueError(f"No device with name {name} found")

        await device.stop()

        if remove_from_manager:
            del self.devices[name]

        return device
