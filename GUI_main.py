import sys
import asyncio
import time
import numpy as np
from collections import deque
from dataclasses import dataclass
from textwrap import dedent
import logging

logging.basicConfig(
    level=logging.INFO
)

from PySide6.QtGui import QCloseEvent
from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QGroupBox, QPushButton, QLabel, QTextEdit,    QTabWidget,
    QLabel, QSizePolicy
)
from PySide6.QtCore import Qt
from qasync import QEventLoop



from PySide6.QtCore import QTimer


from QtGUI.GUI_components.SpectrumPlot import SpectrumPlot
from QtGUI.GUI_components.Spectrogram import SpectrogramWidget
from QtGUI.GUI_components.ROIInfoTab import ROIInfoPane
from QtGUI.GUI_components.SpectrumInfoTab import SpectrumInfoPane
from QtGUI.GUI_components.LoggerTab import LogWidget
from QtGUI.GUI_components.DevicesTab import DevicesInfoTab
from QtGUI.GUI_components.MainMenuBar import MainMenuBar
from QtGUI.GUI_components.import_export import FileDialogs
from QtGUI.SpectrumClasses import Spectrum
from QtGUI.GUI_components.CurrentValuesTab import CurrentValuesPlot
from QtGUI.ThemeManager import ThemeManager
from QtGUI.utils.ArgParser import parse_cli_args

from QtGUI.core import SpectrumManager

from QtGUI.utils.file_io import xml_io

from QtGUI.utils.startup import startup_script
from QtGUI.core import RunManager, Log, Settings

from QtGUI.GUI_components.popup_windows.BluetoothListPopup import BluetoothListPopup
from QtGUI.GUI_components.popup_windows.USBListPopup import USBListPopup

# from QtGUI.GUI_components.ListPopup import BluetoothListPopup

from PySide6.QtWidgets import QApplication, QPushButton, QColorDialog

from QtGUI.clients.RaysidClient.RaysidClient import RaysidClientAsync
from QtGUI.clients.RadiacodeClient.src import RadiacodeClientAsync

# import pandas as pd
# imported_data = pd.read_csv("Cyklotron_Cs.csv").to_numpy().T[1]
# imported_cobolt_data = pd.read_csv("Cyklotron_Co.csv").to_numpy().T[1]
# imported_bkg = pd.read_csv("Cyklotron_Bkg_69714s.csv").to_numpy().T[1]

# imported_spectrum = Spectrum(len(imported_data), "RC103")
# imported_spectrum.set_y_data(imported_data, 4352)
# imported_spectrum.set_y_bkg(imported_bkg, 69714)
# imported_spectrum.apply_calibration([0.0003705, 2.3694975, 4.2583089])

# co_spect = Spectrum(len(imported_data), "RC103Co")
# co_spect.set_y_data(imported_cobolt_data, 67286)
# co_spect.apply_calibration([0.0003705, 2.3694975, 4.2583089])




# -------------------- MOCK DATA PACKAGES --------------------
@dataclass(frozen=True)
class CurrentValuesPackage:
    name: str
    CPS: float
    DR: float
    timestamp: float

@dataclass(frozen=True)
class StatusPackage:
    battery: int
    temperature: float
    charging: bool
    timestamp: float

@dataclass(frozen=True)
class SpectrumResult:
    spectrum: np.ndarray
    live_time: float
    timestamp: float




# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        
        self.setWindowTitle("Gamma Spectroscopy")

        self.mock_running = True
        self._closing = False
        
        self.theme = ThemeManager(Settings.Appearance.theme)
        self.theme.apply() 
        
        self.bt_window = BluetoothListPopup()
        self.usb_window = USBListPopup()
        self.file_import_export = FileDialogs(self)

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.menu_bar = MainMenuBar(self)
        self.setMenuBar(self.menu_bar)


        # ---------- SPECTRUM PLOT ----------
        self.spect_tab = QTabWidget()
        self.spect_tab.setTabPosition(QTabWidget.South)
        self.spectrum_plot = SpectrumPlot()
        self.spect_tab.addTab(self.spectrum_plot, "Spectrum")
        
        self.spectrogram = SpectrogramWidget()
        self.spect_tab.addTab(self.spectrogram, "Spectrogram")
        
        layout.addWidget(self.spect_tab, 5)

        # ---------- Bottom Tabs ---------- Should be moved

        self.bottom_tabs = QTabWidget(self)
 
        # Spectrum Infp
        self.spectrum_info_tab = SpectrumInfoPane(parent=self)
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectra")

        # ROI info
        self.roi_info_pane = ROIInfoPane(parent=self)
        self.roi_info_pane.clearROIs.connect(self.spectrum_plot._clear_rois)
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")

        # Current values
        self.current_value_tab = CurrentValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Current Values")

        # Devices
        self.devices_tab = DevicesInfoTab()
        self.bottom_tabs.addTab(self.devices_tab, "Devices")

        # System log
        self.log_tab = LogWidget()
        self.bottom_tabs.addTab(self.log_tab, "System Log")




        bottom_layout = QVBoxLayout()
        bottom_layout.addWidget(self.bottom_tabs)
        layout.addLayout(bottom_layout, stretch=3)
        
        # SpectrumManager.create_spectrum( "RC103", len(imported_data),)
        # CsSpectrum = SpectrumResult(imported_data, 4352, 0)
        # bkgSpectrum = SpectrumResult(imported_bkg, 69714, 0)
        # SpectrumManager.set_foreground_spectrum("RC103", CsSpectrum)
        # SpectrumManager.set_background_spectrum("RC103", bkgSpectrum)
        # SpectrumManager.calibrate_spectrum("RC103", [0.0003705, 2.3694975, 4.2583089])

        # CoSpectrum = SpectrumResult(imported_cobolt_data, 67286, 0)
        # SpectrumManager.create_spectrum("RC103Co", len(imported_cobolt_data))
        # SpectrumManager.set_foreground_spectrum("RC103Co", CoSpectrum)
        # SpectrumManager.set_background_spectrum("RC103Co", bkgSpectrum)
        # SpectrumManager.calibrate_spectrum("RC103Co", [0.0003705, 2.3694975, 4.2583089])
        
        # xml_io.load("/home/eewa/Documents/git/MySpect/debug/xml/Cyklotron_Ba.n42")
        # xml_io.load("/home/eewa/Documents/git/MySpect/debug/xml/Raysid-GRF-Ba133.xml")
        # xml_io.load("/home/eewa/Documents/git/MySpect/debug/xml/103-GRF-Ba133.xml")

        self.theme.apply(plot_widgets=[
            self.spectrum_plot.plot_widget, 
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget,
            ],
            legends=self.current_value_tab.legends)
        
        if len(sys.argv) > 1:
            QTimer.singleShot(0, parse_cli_args)
            
            
    def closeEvent(self, event: QCloseEvent):
        if self._closing:
            event.accept()
            return
        Log.info("Disconnecting devices and shutting down application...")
        event.ignore()
        self.hide()
        asyncio.create_task(self._async_close())
        
    async def _async_close(self):
        if self._closing:
            return
        self._closing = True

        try:
            await RunManager.shutdown()
            Settings.save_settings()
        finally:
            QApplication.quit()
        


# ===================== ENTRY =====================
def main():
    startup_script()
    app = QApplication(sys.argv)
    font = app.font()
    font.setPointSize(Settings.Appearance.font_size)  # Change the font size
    app.setFont(font)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)
    


    win = MainWindow()
    win.show()

    RunManager.set_loop(loop)

    header = "[ APPLICATION STARTED ]"
    version = "Version: Alpha"
    platform = f"Platform: {sys.platform}"

    frame_width = max(len(header), len(version), len(platform)) + 4  # extra padding

    line = "=" * frame_width

    Log.info(f"\n{line}\n"
            f"| {header.center(frame_width-4)} |\n"
            f"| {version.center(frame_width-4)} |\n"
            f"| {platform.center(frame_width-4)} |\n"
            f"{line}")

    # run_manager.bluetoothError.connect(print)

    # # Schedule adding mock device safely after loop starts
    # QTimer.singleShot(0, lambda: asyncio.create_task(RunManager.add_device("Mock", "mock")))

    # # Schedule first Bluetooth scan safely
    # QTimer.singleShot(0, lambda: asyncio.create_task(RunManager.find_bluetooth()))

    # Start mock tasks safely after loop starts
    # QTimer.singleShot(0, lambda: asyncio.create_task(mock_data_task(win, "Raysid1")))
    # QTimer.singleShot(0, lambda: asyncio.create_task(mock_data_task(win, "Raysid2")))



    # Start the event loop
    with loop:
        loop.run_forever()





if __name__ == "__main__":
    main()



# pyinstaller \
#     --name MySpect \
#     --onedir \
#     --exclude-module PySide6.QtNetwork \
#     --exclude-module PySide6.Qt3DCore \
#     --exclude-module PySide6.Qt3DRender \
#     --exclude-module PySide6.Qt3DExtras \
#     --exclude-module PySide6.QtMultimedia \
#     --exclude-module PySide6.QtWebEngineWidgets \
#     --exclude-module scipy.spatial \
#     --exclude-module scipy.linalg \
#     --exclude-module scipy.ndimage \
#     --exclude-module matplotlib \
#     --hidden-import scipy.optimize \
#     --hidden-import bleak \
#     GUI_main.py