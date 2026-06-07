import os, sys
from PySide6.QtCore import QObject, Signal
from dataclasses import dataclass, field
from typing import Optional, Any
import json
from pathlib import Path
from collections import deque
import sys


@dataclass
class _Appearance:
    theme: str = "light"
    pen: bool = True
    brush: bool = False
    font_size: int = 10
    tabbed_spectrum_view: bool = False
    verbose_calculation_logging: bool = True
    
    load_rois_on_import: bool = True
    load_instrument_on_import: bool = True

@dataclass
class _State:
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
class _Advanced:
    update_loop_delay: float = 0.5
    spectrum_update_delay: float = 1
    ui_scan_length: int = 5
    headless_scan_length: int = 2

    optimizer_max_iter: int = 250
    optimizer_tolerance: float = 1e-6
    optimizer_use_chi2_weight: bool = True

    log_buffer_length: int = 1024

def get_runtime_base() -> Path:
    if hasattr(sys, "_MEIPASS"):
        return Path(sys._MEIPASS)
    return Path(__file__).parent.parent


@dataclass
class _Paths:
    appdata: Path = Path(".appdata")
    spectrum_library: Path = field(init=False)
    spectrogram_library: Path = field(init=False)
    roi_library: Path = field(init=False)
    datalog_library: Path = field(init=False)
    unique_instrument_library: Path = field(init=False)
    BASE: Path = field(init=False)

    settings_file: Path = field(init=False)
    
    last_opened_dir: Path = field(init=False)

    def __post_init__(self):
        # runtime base (where bundled resources live)
        self.BASE = get_runtime_base()

        self.nuclide_data = self.BASE / "resources" / "nuclide_data"
        self.bibliography = self.BASE  / "resources" / "docs" / "bibliography.md"
        self.documentation_dir = self.BASE  / "resources" / "docs" / "documentation"

        # Initialize all dependent paths relative to appdata
        self.spectrum_library = self.appdata / "spectrum_library"
        self.datalog_library = self.appdata / "datalog_library"
        self.spectrogram_library = self.appdata / "spectrogram_library"
        self.roi_library = self.appdata / "roi_library"
        self.unique_instrument_library = self.appdata / "unique_instrument_library"
        self.generic_instrument_library = self.appdata / "generic_instrument_library"
        self.settings_file = self.appdata / "settings.json"
        
        # Track what directory files were last loaded from or saved to during runtime
        # For the users convenience :)
        self.last_opened_dir = Path.home()


class _Settings(QObject):
    latestConnectionUpdated = Signal(list)

    def __init__(self):
        super().__init__()

        self.headless = False
        self.Advanced = _Advanced()
        self.State = _State()
        self.Appearance = _Appearance()
        self.Paths = _Paths()

    def add_new_connection(self, name):
        if name not in list(self.State.last_connections):
            self.State.last_connections.append(name)
        self.latestConnectionUpdated.emit(list(self.State.last_connections))

    def load_settings(self):
        "Ingest the previous settings file"
        with open(self.Paths.settings_file, "r") as f:
            json_content = json.load(f)

        self.Advanced = _Advanced(**json_content["advanced"])
        self.Appearance = _Appearance(**json_content["appearance"])
        self.State = _State().from_dict(json_content["state"])
        self.latestConnectionUpdated.emit(list(self.State.last_connections))

    def save_settings(self):
        "Dump settings, typically at shutdown"
        json_content = {
            "appearance": self.Appearance.__dict__,
            "state": self.State.to_dict(), # Dequeues make this annoying
            "advanced": self.Advanced.__dict__,
        }

        with open(self.Paths.settings_file, "w") as f:
            json.dump(json_content, f, indent=4)


Settings = _Settings()
