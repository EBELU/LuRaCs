import logging
from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import QTextEdit, QWidget, QVBoxLayout, QFrame

from ..Globals import Log


class LogSignalEmitter(QObject):
    logSignal = Signal(str)


class QtHandler(logging.Handler):
    """A logging.Handler that emits logs via a Qt signal."""

    def __init__(self, emitter: LogSignalEmitter):
        super().__init__()
        self.emitter = emitter

    def emit(self, record: logging.LogRecord):
        try:
            msg = self.format(record)
            self.emitter.logSignal.emit(msg)
        except Exception:
            self.handleError(record)


class LogWidget(QWidget):
    """A widget that contains a QTextEdit with borders and shows logs in real-time."""

    def __init__(self, parent=None):
        super().__init__(parent)

        # --- QTextEdit inside a border ---
        self.text_edit = QTextEdit()
        self.text_edit.setReadOnly(True)


        # Layout
        layout = QVBoxLayout(self)
        layout.setContentsMargins(20, 20, 20, 20)
        layout.addWidget(self.text_edit)

        # --- Logger setup ---
        self._setup_logger()

    def _setup_logger(self):
        # Create a QObject emitter
        self.emitter = LogSignalEmitter()

        # Create handler
        handler = QtHandler(self.emitter)

        # Formatter
        formatter = logging.Formatter(
            "%(asctime)s -- %(levelname)s: %(message)s", "%H:%M:%S"
        )
        handler.setFormatter(formatter)

        # Connect signal to append
        self.emitter.logSignal.connect(self.append_message)

        # Add to global logger
        Log.addHandler(handler)
        Log.setLevel(logging.DEBUG)

    def append_message(self, msg: str, level: str = "INFO"):
        # Just append plain text
        self.text_edit.append(msg)
        self.text_edit.verticalScrollBar().setValue(
            self.text_edit.verticalScrollBar().maximum()
        )

