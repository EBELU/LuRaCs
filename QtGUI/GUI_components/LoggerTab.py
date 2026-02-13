import logging
from PySide6.QtCore import QObject, Signal, Qt
from PySide6.QtWidgets import QTextEdit, QWidget, QVBoxLayout

class LogSignalEmitter(QObject):
    """QObject that emits log messages to the GUI."""
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
    """QTextEdit widget showing all logs in real-time."""

    def __init__(self, parent=None):
        super().__init__(parent)

        # --- Setup QTextEdit ---
        self.text_edit = QTextEdit()
        self.text_edit.setReadOnly(True)
        self.text_edit.setLineWrapMode(QTextEdit.NoWrap)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.addWidget(self.text_edit)

        # --- Setup logging ---
        self._setup_logger()

    def _setup_logger(self):
        # Create a signal emitter
        self.emitter = LogSignalEmitter()
        self.emitter.logSignal.connect(self.append_message)

        # Create the QtHandler
        handler = QtHandler(self.emitter)

        # Formatter (timestamp HH:MM:SS)
        formatter = logging.Formatter(
            fmt="%(asctime)s | %(name)s -- %(levelname)s: %(message)s",
            datefmt="%H:%M:%S"
        )
        handler.setFormatter(formatter)

        # Attach to root logger
        root_logger = logging.getLogger()
        root_logger.setLevel(logging.INFO)
        root_logger.addHandler(handler)

    def append_message(self, msg: str):
        """Append a log message to the QTextEdit."""
        self.text_edit.append(msg)
        self.text_edit.verticalScrollBar().setValue(
            self.text_edit.verticalScrollBar().maximum()
        )
