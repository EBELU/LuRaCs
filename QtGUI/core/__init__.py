from .CoreRunManager import RunManager
from .CoreSettings import Settings
from .CoreSpectrumManager import SpectrumManager
from .CoreGUILogger import gui_logger as Log

from .spec_run_signaling import _

"""
Global state classes shared by the entire program. 

Global objects outside of other globals should only be included through this __init__ file.
"""

__all__ = ["RunManager", "Settings", "SpectrumManager", "Log"]