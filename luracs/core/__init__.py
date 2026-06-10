from .run_manager import RunManager
from .gui_logger import gui_logger as Log # This must be here, if bellow IOManager it breaks
from .settings import Settings
from .spectrum_manager import SpectrumManager
from .calculator import Calculator
from .io_manager import IOManager

from .gui_logger import (
    _attach_file_handler,
    _detach_file_handler,
    _attach_exception_handler,
    _detach_exception_handler,
    _attach_console_handler,
    _detach_console_handler,
)

class log_utils:
    attach_file_handler = _attach_file_handler
    detach_file_handler = _detach_file_handler

    attach_exception_handler = _attach_exception_handler
    detach_exception_handler = _detach_exception_handler

    attach_console_handler = _attach_console_handler
    detach_console_handler = _detach_console_handler

"""
Global state classes shared by the entire program.

Global objects outside of other globals should only be included through this __init__ file.
"""

__all__ = [
    "RunManager",
    "Settings",
    "SpectrumManager",
    "Calculator",
    "IOManager",
    "Log",
    "log_utils"
]
