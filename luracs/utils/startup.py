from luracs.core import Settings, SpectrumManager
from luracs.utils.color_rotator import ColorRotator
from pathlib import Path
import os


def startup_script():
    for path in Settings.Paths.__dict__.values():
        if isinstance(path, Path) and not path.is_dir() and "_library" in path.name:
            os.makedirs(path)

    if os.path.isfile(Settings.Paths.settings_file):
        Settings.load_settings()
        SpectrumManager.color_rotation = ColorRotator(
            ColorRotator.ColorSchemes(Settings.Appearance.color_rotator_scheme)
        )
