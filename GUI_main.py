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
from QtGUI.GUI_components.ROIInfoTab import ROIInfoPane
from QtGUI.GUI_components.SpectrumInfoTab import SpectrumInfoPane
from QtGUI.GUI_components.LoggerTab import LogWidget
from QtGUI.GUI_components.MainMenuBar import MainMenuBar, MenuActions
from QtGUI.SpectrumClasses import Spectrum
from QtGUI.GUI_components.CurrentValuesTab import CurrentValuesPlot
from QtGUI.ThemeManager import ThemeManager
from QtGUI.utils.ArgParser import parse_cli_args

from QtGUI.Globals import SpectrumManager

from QtGUI.utils.MockClient import MockClient
from QtGUI.utils.startup import startup_script
from QtGUI.Globals import RunManager, Log, Settings

from QtGUI.GUI_components.popup_windows.BluetoothListPopup import BluetoothListPopup

# from QtGUI.GUI_components.ListPopup import BluetoothListPopup

from PySide6.QtWidgets import QApplication, QPushButton, QColorDialog

from Clients.RaysidClient.RaysidClient import RaysidClientAsync

import pandas as pd
imported_data = pd.read_csv("Cyklotron_Cs.csv").to_numpy().T[1]
imported_cobolt_data = pd.read_csv("Cyklotron_Co.csv").to_numpy().T[1]
imported_bkg = pd.read_csv("Cyklotron_Bkg_69714s.csv").to_numpy().T[1]

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
    y_axis: np.ndarray
    live_time: float
    timestamp: float




# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    new_current_signal = Signal(str, object)
    new_spectrum_signal = Signal(object)
    def __init__(self):
        super().__init__()
        
        self.setWindowTitle("Gamma Spectroscopy")

        self.mock_running = True
        self._closing = False
        
        self.theme = ThemeManager(Settings.Appearance.theme)
        self.theme.apply() 
        
        self.bt_window = BluetoothListPopup()

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.menu_bar = MainMenuBar(self)
        self.setMenuBar(self.menu_bar)


        # ---------- SPECTRUM PLOT ----------
        self.spectrum_plot = SpectrumPlot()
        layout.addWidget(self.spectrum_plot, 4)


        self.bottom_tabs = QTabWidget(self)
        self.bottom_tabs.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding, )
 
        self.spectrum_info_tab = SpectrumInfoPane()
        self.bottom_tabs.addTab(self.spectrum_info_tab, "Spectra")

        self.roi_info_pane = ROIInfoPane()
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")


        self.current_value_tab = CurrentValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Current Values")




        self.bottom_tabs.addTab(QWidget(), "Devices")


        


        

        
        self.log_tab = LogWidget()
        self.bottom_tabs.addTab(self.log_tab, "System Log")




        layout.addWidget(self.bottom_tabs, 1)
        
        # SpectrumManager.create_spectrum( "RC103", len(imported_data),)
        # CsSpectrum = SpectrumResult(imported_data, 4352, 0)
        # bkgSpectrum = SpectrumResult(imported_bkg, 69714, 0)
        # SpectrumManager.set_foreground_spectrum("RC103", CsSpectrum)
        # SpectrumManager.set_background_spectrum("RC103", bkgSpectrum)
        # SpectrumManager.calibrate_spectrum("RC103", [0.0003705, 2.3694975, 4.2583089])

        CoSpectrum = SpectrumResult(imported_cobolt_data, 67286, 0)
        # SpectrumManager.create_spectrum("RC103Co", len(imported_cobolt_data))
        # SpectrumManager.set_foreground_spectrum("RC103Co", CoSpectrum)
        # SpectrumManager.set_background_spectrum("RC103Co", bkgSpectrum)
        # SpectrumManager.calibrate_spectrum("RC103Co", [0.0003705, 2.3694975, 4.2583089])


        self.new_current_signal.connect(self.update_current)




        self.theme.apply(plot_widgets=[
            self.spectrum_plot.plot_widget, 
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget,
            ],
            legends=self.current_value_tab.legends)
        
        if len(sys.argv) > 1:
            parse_cli_args()
            
            
    def closeEvent(self, event: QCloseEvent):
        if self._closing:
            event.accept()
            return

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
        
    def update_current(self, package):
        self.current_value_tab.receive_data_packet(package)
    
    def update_spectrum(self, package):
        pass












# ===================== MOCK DATA TASK =====================
async def mock_data_task(win: MainWindow, name):
    while True:
        if win.mock_running:
            cps = np.random.normal(500,50)
            dr  = cps/1000 + np.random.normal(0,0.01)
            timestamp = time.time()
            # Emit signal
            win.new_current_signal.emit(name, CurrentValuesPackage(name, cps, dr, timestamp))
        await asyncio.sleep(0.5)


# ===================== ENTRY =====================
def main():
    app = QApplication(sys.argv)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)
    
    startup_script()

    win = MainWindow()
    win.show()

    RunManager.set_loop(loop)
    RunManager.set_clients({"mock": MockClient, "raysid":RaysidClientAsync})

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
