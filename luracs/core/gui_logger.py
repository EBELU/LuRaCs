import logging
import sys
from collections import deque
from .settings import Settings

gui_logger = logging.getLogger("Application")
gui_logger.setLevel(logging.DEBUG)

formatter = logging.Formatter(
    fmt="%(asctime)s | Application | %(levelname)s | %(message)s",
    datefmt="%H:%M:%S"
)

# ------------------------------------------------------------------
# Handlers
# ------------------------------------------------------------------

class BufferHandler(logging.Handler):
    def __init__(self, max_entries=1000):
        super().__init__()
        self.buffer = deque(maxlen=max_entries)

    def emit(self, record):
        self.buffer.append(self.format(record))

    def get_messages(self):
        return list(self.buffer)

console_handler = None
file_handler = None

# --- Buffer Handler ---
log_buffer = BufferHandler(max_entries=Settings.Advanced.log_buffer_length)
log_buffer.setFormatter(formatter)

gui_logger.addHandler(log_buffer)

# --- Console Handler ---
# Forwards log messages to the console
def _attach_console_handler():
    global console_handler

    if console_handler is not None:
        return

    console_handler = logging.StreamHandler()
    console_handler.setLevel(logging.DEBUG)
    console_handler.setFormatter(formatter)
    gui_logger.addHandler(console_handler)


def _detach_console_handler():
    global console_handler

    if console_handler is None:
        return

    gui_logger.removeHandler(console_handler)
    console_handler.close()
    console_handler = None

# --- File Handler ---
# Writes log error messages to a crash report file
def _attach_file_handler():
    global file_handler

    if file_handler is not None:
        return

    file_handler = logging.FileHandler("application_crash.log")
    file_handler.setLevel(logging.ERROR)
    file_handler.setFormatter(formatter)
    gui_logger.addHandler(file_handler)

def _detach_file_handler():
    global file_handler

    if file_handler is None:
        return

    gui_logger.removeHandler(file_handler)
    file_handler.close()
    file_handler = None

 
# --- Exception Handler ---
# Pipes unhandled exceptions to the console
def handle_exception(exc_type, exc_value, exc_traceback):
    if issubclass(exc_type, KeyboardInterrupt):
        sys.__excepthook__(exc_type, exc_value, exc_traceback)
        return

    gui_logger.critical(
        "Unhandled exception",
        exc_info=(exc_type, exc_value, exc_traceback)
    )

def _attach_exception_handler():
    sys.excepthook = handle_exception

def _detach_exception_handler():
    sys.excepthook = sys.__excepthook__