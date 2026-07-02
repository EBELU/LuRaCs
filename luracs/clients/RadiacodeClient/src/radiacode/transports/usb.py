import struct
import usb.core

from ..bytes_buffer import BytesBuffer
from ..logger import logger


class DeviceNotFound(Exception):
    pass


class MultipleUSBReadFailure(Exception):
    """Raised when max. number of USB read failues reached"""

    def __init__(self, message=None):
        self.message = 'Multiple USB Read Failures' if message is None else message
        super().__init__(self.message)


class Usb:
    def __init__(self, serial_number=None, timeout_ms=2000):
        _vid = 0x0483
        _pid = 0xF123

        if serial_number:
            self._device = usb.core.find(idVendor=_vid, idProduct=_pid, serial_number=serial_number)
        else:
            self._device = usb.core.find(idVendor=_vid, idProduct=_pid)

        if self._device is None:
            raise DeviceNotFound

        self._timeout_ms = timeout_ms
        # Detach kernel driver if active
        try:
            if self._device.is_kernel_driver_active(0):
                self._device.detach_kernel_driver(0)
        except usb.core.USBError:
            pass  # Some backends/OSes may throw here, ignore

        # Set configuration safely
        try:
            self._device.set_configuration()
        except usb.core.USBError as e:
            # If it's busy, try resetting device
            if e.errno == 16:
                self._device.reset()
                self._device.set_configuration()
            else:
                raise

        cfg = self._device.get_active_configuration()
        intf = cfg[(0, 0)]
        self._interface_number = intf.bInterfaceNumber

        self._kernel_was_attached = False
        if self._device.is_kernel_driver_active(self._interface_number):
            self._device.detach_kernel_driver(self._interface_number)
            self._kernel_was_attached = True

        try:
            usb.util.claim_interface(self._device, self._interface_number)
        except usb.core.USBError:
            logger.error('Claim failed')
            return

    def execute(self, request: bytes) -> BytesBuffer:
        self._device.write(0x1, request)

        trials = 0
        max_trials = 1
        while trials < max_trials:  # repeat until non-zero lenght data received
            try:
                data = self._device.read(0x81, 256, timeout=self._timeout_ms).tobytes()
            except usb.core.USBTimeoutError:
                trials += 1
                continue
            if len(data) != 0:
                break
            else:
                trials += 1
        if trials >= max_trials:
            logger.critical(str(trials) + ' USB Read Failures in sequence')
            raise MultipleUSBReadFailure(str(trials) + ' USB Read Failures in sequence')

        response_length = struct.unpack_from('<I', data)[0]
        data = data[4:]

        while len(data) < response_length:
            r = self._device.read(0x81, response_length - len(data)).tobytes()
            data += r

        return BytesBuffer(data)

    def stop(self):
        if not self._device:
            return

        try:
            cfg = self._device.get_active_configuration()
            intf = cfg[(0, 0)]
            interface_number = intf.bInterfaceNumber

            try:
                usb.util.release_interface(self._device, interface_number)
            except usb.core.USBError:
                pass
            except Exception as e:
                logger.error('Release failed:', e)

            if getattr(self, '_kernel_was_attached', False):
                try:
                    self._device.attach_kernel_driver(interface_number)
                except Exception as e:
                    logger.error('Reattach failed:', e)

        finally:
            usb.util.dispose_resources(self._device)
            self._device = None
            logger.info(f'USB conncection closed')
