from PySide6.QtCore import QObject, Signal
from dataclasses import dataclass

@dataclass
class Appearence:
    theme = "dark"
    pen = True
    brush = True

@dataclass
class State:
    last_connection = None
    loaded_spectra = None
    roi_regions = None


class SettingsBase(QObject):
    def __init__(self):
        super().__init__()
        
        self.Apperance = Appearence()
    

    
    def load_settings(self, file):
        pass

    def save_settings(self, file):
        pass

Settings = SettingsBase()