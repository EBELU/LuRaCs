from ..theme_manager import ThemeManager
from .settings import Settings

_theme_manager = ThemeManager(Settings.Paths.themes)
_theme_manager.sigUpdateSettingsTheme.connect(Settings.update_setting)
