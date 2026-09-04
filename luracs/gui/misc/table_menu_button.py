from PySide6.QtWidgets import (
    QWidget,
    QToolButton,
    QMenu,
    QVBoxLayout,
)

from PySide6.QtGui import QAction


class MenuButton(QWidget):
    def __init__(self, title="Menu", parent=None):
        super().__init__(parent)
        self.parent = parent

        self.button = QToolButton(self)
        self.button.setText(title)
        self.button.setPopupMode(QToolButton.InstantPopup)
        self.button.setToolButtonStyle(self.button.toolButtonStyle())

        self.menu = QMenu(self)
        self.button.setMenu(self.menu)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.button)

    def add_action(self, text: str) -> QAction:
        action = QAction(text, self)
        self.menu.addAction(action)
        return action
    
    def add_separator(self):
        self.menu.addSeparator()
