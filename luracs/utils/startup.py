from core import Settings
from pathlib import Path
import os

def startup_script():
    for path in Settings.Paths.__dict__.values():
        if not str(path).endswith(".json"):
            if isinstance(path, Path) and not os.path.isdir(path):
                os.makedirs(path)
    
    if os.path.isfile(Settings.Paths.settings_file):
        Settings.load_settings()
