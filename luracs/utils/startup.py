from core import Settings
from pathlib import Path
import os


def startup_script():
    for path in Settings.Paths.__dict__.values():
        if isinstance(path, Path) and not path.is_dir() and "_library" in path.name:
            os.makedirs(path)

    if os.path.isfile(Settings.Paths.settings_file):
        Settings.load_settings()
