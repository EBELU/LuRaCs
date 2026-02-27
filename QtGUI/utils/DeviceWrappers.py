import time
from ...Clients.RadiacodeClient.src import RadiacodeClientAsync
from ...Clients.RaysidClient.RaysidClient import RaysidClientAsync

class DeviceWrapper:
    _registry = {}

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        if hasattr(cls, "type") and cls.type:
            cls._registry[cls.type] = cls

    def __init__(self, address, usb):
        self.address = address
        try:
            self.name = address.name
        except AttributeError:
            self.name = str(address)
        self.connection = "USB" if usb else "BLE"
        self.connected_timestamp = time.time()

        # Virtual placeholders
        self.client = None
        self.type = None
        self.channels = None

    @classmethod
    def get_registry(cls):
        return cls._registry
        
        
        
class RadiacodeWrapper(DeviceWrapper):
    type = "radiacode"
    def __init__(self, address, usb):
        super().__init__(address, usb)
        
        self.client = RadiacodeClientAsync(address, usb)
        self.channels = 1024
        
        
        
class RaysidWrapper(DeviceWrapper):
    type = "raysid"
    def __init__(self, address, usb):
        super().__init__(address, False)
        
        self.client = RaysidClientAsync(address)
        self.channels = 1800