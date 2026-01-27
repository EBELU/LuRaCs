from PySide6.QtWidgets import QMenuBar, QMessageBox
from PySide6.QtCore import QObject
import time
from dataclasses import dataclass

@dataclass
class MenuActions:
    """Optional container for menu-related callbacks."""
    reset_callback: callable = None
    about_callback: callable = None
    exit_callback: callable = None


class MainMenuBar(QMenuBar):
    def __init__(self, parent=None, actions: MenuActions = None):
        super().__init__(parent)
        self.parent = parent
        self.actions = actions or MenuActions()

        # ---------- File Menu ----------
        file_menu = self.addMenu("&File")
        exit_action = file_menu.addAction("Exit")
        exit_action.triggered.connect(self.on_exit)

        # Device Menu

        device_menu = self.addMenu("&Device")

        spectrum = self.addMenu("&Spectrum")

        # ---------- Options Menu ----------
        options_menu = self.addMenu("&Options")
        reset_action = options_menu.addAction("Reset Data")
        reset_action.triggered.connect(self.on_reset)

        # ---------- Help Menu ----------
        help_menu = self.addMenu("&Help")
        about_action = help_menu.addAction("About")
        about_action.triggered.connect(self.on_about)

    # ---------- Action Handlers ----------
    def on_reset(self):
        if self.actions.reset_callback:
            self.actions.reset_callback()

    def on_about(self):
        if self.actions.about_callback:
            self.actions.about_callback()
        else:
            QMessageBox.information(
                self.parent,
                "About",
                "Gamma Spectroscopy GUI\nMock Data Version\nAuthor: Your Name"
            )

    def on_exit(self):
        if self.actions.exit_callback:
            self.actions.exit_callback()
        else:
            if self.parent:
                self.parent.close()
