from PySide6.QtWidgets import QMenuBar, QMessageBox
from dataclasses import dataclass
import asyncio

from core import RunManager, Settings
from gui.popup_windows.efficiency_window import EfficiencyWindow

from .import_export import save_roi_references


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

        Settings.latestConnectionUpdated.connect(self.update_last_connections)

        # ---------- File Menu ----------
        file_menu = self.addMenu("&File")
        file_menu_import = file_menu.addAction("Import Spectrum")
        file_menu_import.triggered.connect(
            lambda: self.parent.file_import_export.import_generic()
        )
        file_load = file_menu.addAction("Data Store")
        file_load.triggered.connect(lambda: parent.data_store.show())
        file_menu_saveRoi = file_menu.addAction("Save reference ROIs")
        file_menu_saveRoi.triggered.connect(save_roi_references)
        exit_action = file_menu.addAction("Exit")
        exit_action.triggered.connect(self.on_exit)

        # Device Menu

        device_menu = self.addMenu("&Device")
        device_menu_connectBT = device_menu.addAction("Connect Bluetooth")
        device_menu_connectBT.triggered.connect(parent.bt_window.start_popup)

        self.device_menu_retryLast = device_menu.addMenu("&Retry Last Connection")
        device_menu_connectUSB = device_menu.addAction("Connect USB")
        device_menu_connectUSB.triggered.connect(parent.usb_window.start_popup)
        device_menu_disconnect = device_menu.addAction("Disconnect All")
        device_menu_disconnect.triggered.connect(
            lambda x: RunManager.remove_all_devices()
        )
        # device_menu_info.triggered.connect(lambda x: RunManager.start_logger())

        calculate_menu = self.addMenu("&Gamma Tools")
        calculate_menu_photoEff = calculate_menu.addAction("Efficiency")
        calculate_menu_photoEff.triggered.connect(lambda: EfficiencyWindow(self).show())
        calculate_menu_photoActi = calculate_menu.addAction("Activity")
        calculate_menu_photoFrac = calculate_menu.addAction("Photofraction")

        calculate_menu = self.addMenu("&MRI Tools")

        # ---------- Options Menu ----------
        options_menu = self.addMenu("&Options")
        reset_action = options_menu.addAction("Reset Data")

        # ---------- Help Menu ----------
        help_menu = self.addMenu("&Help")
        about_action = help_menu.addAction("About")
        about_action.triggered.connect(self.on_about)

        self.update_last_connections(list(Settings.State.last_connections))

    # ---------- Action Handlers ----------
    def update_last_connections(self, names: list):
        self.device_menu_retryLast.clear()

        for name in names:
            # Create the action
            def _connect(x, n=name):  # bind current name to n
                loop = asyncio.get_event_loop()
                loop.create_task(RunManager.connect_bluetooth_list([n]))

            retryDevice = self.device_menu_retryLast.addAction(name)
            retryDevice.triggered.connect(_connect)

    def on_about(self):
        if self.actions.about_callback:
            self.actions.about_callback()
        else:
            QMessageBox.information(
                self.parent,
                "About",
                "Gamma Spectroscopy GUI\nMock Data Version\nAuthor: Your Name",
            )

    def on_exit(self):
        if self.actions.exit_callback:
            self.actions.exit_callback()
        else:
            if self.parent:
                self.parent.close()
