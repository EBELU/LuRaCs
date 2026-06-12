from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from main import MainWindow

from PySide6.QtWidgets import QMenuBar, QMessageBox
from PySide6.QtGui import QAction, QActionGroup
from PySide6.QtCore import Signal
import asyncio

from luracs.core import RunManager, Settings, IOManager

from .save_to_internal import save_roi_references

from luracs.gui.popup_windows.settings_dialog import edit_settings, edit_advanced_settings

class MainMenuBar(QMenuBar):
    sigSetSpectrumViewToTabs = Signal()
    sigSetSpectrumViewToCombined = Signal()
    
    def __init__(self, parent: MainWindow = None):
        super().__init__(parent)
        self.parent = parent

        Settings.latestConnectionUpdated.connect(self.update_last_connections)


        # ---------- File Menu ----------
        file_menu = self.addMenu("&File")
        file_menu_import = file_menu.addAction("Import")
        file_menu_import.triggered.connect(
            lambda : IOManager.Importer.import_generic()
            )
        file_load = file_menu.addAction("Data Store")
        file_load.triggered.connect(parent.data_store.show)
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

        calculate_menu = self.addMenu("&Gamma Tools")
        calculate_menu_photoCalibration = calculate_menu.addAction("Calibration")
        calculate_menu_photoCalibration.triggered.connect(parent.calc_win_calibration.show)
        calculate_menu_photoEff = calculate_menu.addAction("Efficiency")
        calculate_menu_photoEff.triggered.connect(parent.calc_win_efficiency.show)
        calculate_menu_photoResolution = calculate_menu.addAction("Resolution")
        calculate_menu_photoResolution.triggered.connect(parent.calc_win_resolution.show)

        
        #calculate_menu = self.addMenu("&MRI Tools")

        # ---------- Options Menu ----------
        options_menu = self.addMenu("&Options")
        settings_action = options_menu.addAction("Settings")
        settings_action.triggered.connect(lambda : edit_settings(parent))
        advanced_settings_action = options_menu.addAction("Advanced Settings")
        advanced_settings_action.triggered.connect(lambda : edit_advanced_settings(parent))
        
        spectrum_tabbed_group = QActionGroup(self)
        spectrum_tabbed_group.setExclusive(True)
        
        # --- Spectrum view options ---
        options_menu.addSeparator()

        # Create exclusive group
        view_group = QActionGroup(self)
        view_group.setExclusive(True)

        # Create checkable actions
        self.combined_action = QAction("Combined Spectrum View", self, checkable=True)
        self.tabbed_action = QAction("Tabbed Spectrum View", self, checkable=True)


        # Add to group
        view_group.addAction(self.combined_action)
        view_group.addAction(self.tabbed_action)


        # Set default
        if Settings.Appearance.tabbed_spectrum_view:
            self.tabbed_action.setChecked(True)
        else:
            self.combined_action.setChecked(True)

        # Add to menu
        options_menu.addAction(self.combined_action)
        options_menu.addAction(self.tabbed_action)

        view_group.triggered.connect(self.on_view_changed)
        
        # ---------- Help Menu ----------
        help_menu = self.addMenu("&Help")
        documentation_action = help_menu.addAction("Documentation")
        documentation_action.triggered.connect(parent.documentation_dialog.show)
        bibliography_action = help_menu.addAction("Bibliography")
        bibliography_action.triggered.connect(parent.bibliography_dialog.show)
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
        QMessageBox.information(
            self.parent,
            "About",
            "LuRaCs - Lund Radiation analysis Computer software\n\n A free and open source tool for measuring and analysing radiation spectra.\n\nCreated by Erik Ewald & Malte Axner \n\nSource code licenced under GPL-3.0\nDocumentation & Images licenced under CC BY-SA 4.0",
        )

    def on_exit(self):
        if self.parent:
            self.parent.close()
                
    # Handle selection
    def on_view_changed(self, action):
        if action == self.tabbed_action:
            Settings.Appearance.tabbed_spectrum_view = True
            self.sigSetSpectrumViewToTabs.emit()
            

        elif action == self.combined_action:
            Settings.Appearance.tabbed_spectrum_view = False
            self.sigSetSpectrumViewToCombined.emit()

