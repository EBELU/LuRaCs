import os
import shutil
import sys
from pathlib import Path

from luracs.core import Settings, SpectrumManager
from luracs.utils.color_rotator import ColorRotator


def startup_script():
    if "--clear_local_cache" in sys.argv and Settings.Paths.appdata.exists():
        for item in Settings.Paths.appdata.iterdir():
            if item.is_dir():
                shutil.rmtree(item)
            else:
                item.unlink()
        sys.exit(0)
    
    for path in Settings.Paths.__dict__.values():
        if isinstance(path, Path) and not path.is_dir() and "_library" in path.name:
            os.makedirs(path)

    if os.path.isfile(Settings.Paths.settings_file):
        Settings.load_settings()
        SpectrumManager.color_rotation = ColorRotator(
            ColorRotator.ColorSchemes(Settings.Appearance.color_rotator_scheme)
        )
