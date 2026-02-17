from PySide6.QtCore import QObject, Signal
from dataclasses import dataclass, field
from typing import Optional, Any
import json
from os.path import join
from collections import deque

@dataclass
class Appearance:
    theme: str = "dark"
    pen: bool = True
    brush: bool = False


@dataclass
class State:
    last_connections: deque = field(default_factory=lambda: deque(maxlen=10))
    loaded_spectra: Optional[Any] = None
    roi_regions: Optional[Any] = None

    def to_dict(self):
        return {
            "last_connections": list(self.last_connections),
            "loaded_spectra": self.loaded_spectra,
            "roi_regions": self.roi_regions,
        }

    @classmethod
    def from_dict(cls, data):
        last_connections = deque(data.get("last_connections", []), maxlen=10)
        return cls(
            last_connections=last_connections,
            loaded_spectra=data.get("loaded_spectra"),
            roi_regions=data.get("roi_regions"),
        )

@dataclass
class Advanced:
    update_loop_delay: float = 0.5
    ui_scan_length: int = 5
    headless_scan_length: int = 2


@dataclass
class Paths:
    appdata: str = ".appdata"
    spect_lib: str = field(init=False)
    spect_logs: str = field(init=False)
    roi_lib: str = field(init=False)
    logs: str = field(init=False)
    settings_file: str = field(init=False)

    def __post_init__(self):
        self.spect_lib = join(self.appdata, "spect_lib")
        self.spect_logs = join(self.appdata, "spect_logs")
        self.roi_lib = join(self.appdata, "rois")
        self.logs = join(self.appdata, "logs")
        self.settings_file = join(self.appdata, "settings.json")


class SettingsBase(QObject):
    latestConnectionUpdated = Signal(list)
    def __init__(self):
        super().__init__()
        
        self.Advanced = Advanced()
        self.State = State()
        self.Appearance = Appearance()
        self.Paths = Paths()
        
    def add_new_connection(self, name):
        if name not in list(self.State.last_connections):
            self.State.last_connections.append(name)
        self.latestConnectionUpdated.emit(list(self.State.last_connections))

    
    def load_settings(self):
        with open(self.Paths.settings_file, "r") as f:
            json_content = json.load(f)

        self.Advanced = Advanced(**json_content["advanced"])
        self.Appearance = Appearance(**json_content["appearance"])
        self.State = State().from_dict(json_content["state"])
        self.latestConnectionUpdated.emit(list(self.State.last_connections))
        
    def save_settings(self):
        json_content = {
            "appearance": self.Appearance.__dict__,
            "state": self.State.to_dict(),
            "advanced": self.Advanced.__dict__
        }
        
        with open(self.Paths.settings_file, "w") as f:
            json.dump(json_content, f, indent=4)

Settings = SettingsBase()