from PySide6.QtWidgets import QMenuBar, QMessageBox
from PySide6.QtCore import QObject
import time
from dataclasses import dataclass

from .popup_windows.BluetoothListPopup import BluetoothListPopup

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
        file_menu_import = file_menu.addAction("Import")
        spectrum_export_menu = file_menu.addMenu("&Export Spectra")
        spectrum_export__menu_csv = spectrum_export_menu.addAction("csv")
        spectrum_export__menu_xml = spectrum_export_menu.addAction("xml")
        file_menu_exportRoi = file_menu.addAction("Export ROIs")
        file_menu_importRoi = file_menu.addAction("Import ROIs")
        file_menu_reset = file_menu.addAction("Reset Accumulation")
        exit_action = file_menu.addAction("Exit")
        exit_action.triggered.connect(self.on_exit)

        # Device Menu

        device_menu = self.addMenu("&Device")
        device_menu_connectBT = device_menu.addAction("Connect Bluetooth")
        device_menu_connectBT.triggered.connect(parent.bt_window.start_popup)
        
        device_menu_retryLast = device_menu.addAction("Retry Last Connection")
        device_menu_connectUSB = device_menu.addAction("Connect USB")
        device_menu_connectUSB = device_menu.addAction("Disconnect")
        device_menu_info = device_menu.addAction("Device Info")





        calculate_menu = self.addMenu("&Gamma Tools")
        calculate_menu_photoEff = calculate_menu.addAction("Efficiency")
        calculate_menu_photoActi = calculate_menu.addAction("Activity")
        calculate_menu_photoFrac = calculate_menu.addAction("Photofraction")

        calculate_menu = self.addMenu("&MRI Tools")


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
