from PySide6.QtCore import QObject, Signal

class SettingsBase(QObject):
    def __init__(self):
        super().__init__()

Settings = SettingsBase()