import logging

from PySide6.QtCore import QObject, Signal
from PySide6.QtGui import QFont
from PySide6.QtWidgets import QTextEdit, QVBoxLayout, QWidget

from luracs.core import Settings


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

    sigMessageLogged = Signal(str)
    sigBufferSent = Signal(list)

    def __init__(self, parent=None):
        super().__init__(parent)

        # --- Setup QTextEdit ---
        self.text_edit = QTextEdit()
        self.text_edit.setReadOnly(True)
        self.text_edit.setLineWrapMode(QTextEdit.NoWrap)
        font = QFont()
        font.setFamily("Consolas")  # Windows-friendly monospace
        font.setStyleHint(QFont.Monospace)
        font.setPointSize(Settings.Appearance.font_size)
        self.text_edit.setFont(font)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.addWidget(self.text_edit)

        # --- Setup logging ---
        self._setup_logger()

        self.setStyleSheet("""
            QWidget#LogWidget {
                border: 3px solid #444444;
                border-radius: 6px;
            }

            QTextEdit#LogConsole {
                border: none;
                padding: 6px;
            }
        """)

        # Ensure palette-based background
        self.text_edit.setAutoFillBackground(True)

    def _setup_logger(self):
        self.emitter = LogSignalEmitter()
        self.emitter.logSignal.connect(self.append_message)

        handler = QtHandler(self.emitter)

        formatter = logging.Formatter(
            fmt="%(asctime)s | %(name)s | %(levelname)s | %(message)s",
            datefmt="%H:%M:%S",
        )
        handler.setFormatter(formatter)

        loggers = ["Application", "RaysidClient", "RadiacodeClient"]

        for name in loggers:
            logger = logging.getLogger(name)
            logger.setLevel(level=logging.INFO)
            logger.addHandler(handler)
            logger.propagate = False

    def append_message(self, msg: str):
        """Append a log message to the QTextEdit."""
        self.text_edit.append(msg)
        self.text_edit.verticalScrollBar().setValue(
            self.text_edit.verticalScrollBar().maximum()
        )
