from collections import deque

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QFont
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLineEdit,
    QPushButton,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)


class CommandHistory:
    def __init__(self, maxlen=200):
        self.history = deque(maxlen=maxlen)
        self.index = 0

    def add(self, cmd: str):
        if cmd.strip():
            self.history.append(cmd)
            self.index = len(self.history)

    def previous(self):
        if not self.history:
            return ""

        self.index = max(0, self.index - 1)
        return self.history[self.index]

    def next(self):
        if not self.history:
            return ""

        self.index = min(len(self.history), self.index + 1)

        if self.index == len(self.history):
            return ""

        return self.history[self.index]


class ConsoleTab(QWidget):
    sigCommandEntered = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)

        self.history = CommandHistory()

        layout = QVBoxLayout()

        # Console output area
        self.console_output = QTextEdit()
        self.console_output.setReadOnly(True)
        font = QFont()
        font.setFamily("Consolas")  # Windows-friendly monospace
        font.setStyleHint(QFont.Monospace)
        self.console_output.setFont(font)
        layout.addWidget(self.console_output)

        # Command input area
        command_layout = QHBoxLayout()
        self.command_input = QLineEdit()
        self.command_input.setPlaceholderText("Enter command...")
        self.command_input.returnPressed.connect(self.send_command)
        command_layout.addWidget(self.command_input)

        # Send button
        send_button = QPushButton("Enter")
        send_button.clicked.connect(self.send_command)
        command_layout.addWidget(send_button)

        layout.addLayout(command_layout)
        self.setLayout(layout)

    def send_command(self):
        command = self.command_input.text().strip()
        if command:
            self.history.add(command)
            self.sigCommandEntered.emit(command)
            self.command_input.clear()

    def append_output(self, text):
        self.console_output.append(text)

    def set_output(self, text):
        self.console_output.setText(text)

    def keyPressEvent(self, event):
        if event.key() == Qt.Key_Up:
            self.command_input.setText(self.history.previous())

        elif event.key() == Qt.Key_Down:
            self.command_input.setText(self.history.next())

        else:
            super().keyPressEvent(event)
