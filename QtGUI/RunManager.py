import asyncio
from PySide6.QtCore import QObject, Signal  # for Qt signals and QObject base class
from DeviceManager import DeviceManager     # your DeviceManager class
from DeviceManager import DeviceState       # enum or constants for device states

from DeviceManager import DeviceManager, Device, DeviceState

class RunManager(QObject):
    currentUpdated = Signal(str, object)   # device name, CurrentValuesPackage
    statusUpdated = Signal(str, object)    # device name, StatusPackage
    spectrumUpdated = Signal(str, object)  # device name, SpectrumResult

    def __init__(self, device_manager: DeviceManager):
        super().__init__()
        self.device_manager = device_manager
        self._running = False
        self._task: asyncio.Task | None = None

    def start(self):
        if not self._running:
            self._running = True
            self._task = asyncio.create_task(self._update_loop())

    def stop(self):
        self._running = False
        if self._task:
            self._task.cancel()

    async def _update_loop(self):
        try:
            while self._running:
                for name, device in self.device_manager.devices.items():
                    if device.state != DeviceState.RUNNING:
                        continue
                    if device.realtime_data:
                        self.currentUpdated.emit(name, device.realtime_data)
                    if device.status:
                        self.statusUpdated.emit(name, device.status)
                    if device.spectrum_data:
                        self.spectrumUpdated.emit(name, device.spectrum_data)

                await asyncio.sleep(0.5)
        except asyncio.CancelledError:
            pass