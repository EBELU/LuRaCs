import sys
import asyncio
import time
import numpy as np
from collections import deque
from dataclasses import dataclass

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QGroupBox, QPushButton, QLabel, QTextEdit,    QTabWidget,
    QLabel, QSizePolicy
)
from PySide6.QtCore import Qt
from qasync import QEventLoop

from PySide6.QtCore import QTimer
from functools import partial

from QtGUI.GUI_components.SpectrumPlot import SpectrumPlot
from QtGUI.GUI_components.ROIInfoTab import ROIInfoPane
from QtGUI.GUI_components.MainMenuBar import MainMenuBar, MenuActions
from QtGUI.SpectrumClasses import Spectrum
from QtGUI.GUI_components.CurrentValuesTab import CurrentValuesPlot
from QtGUI.ThemeManager import ThemeManager

from QtGUI.RunManager import RunManager
from QtGUI.utils.MockClient import MockClient

from QtGUI.GUI_components.ListPopup import BluetoothListPopup

import pandas as pd
imported_data = pd.read_csv("Cyklotron_Cs.csv").to_numpy().T[1]
imported_cobolt_data = pd.read_csv("Cyklotron_Co.csv").to_numpy().T[1]
imported_bkg = pd.read_csv("Cyklotron_Bkg_69714s.csv").to_numpy().T[1]

imported_spectrum = Spectrum(len(imported_data), "RC103")
imported_spectrum.set_y_data(imported_data, 4352)
imported_spectrum.set_y_bkg(imported_bkg, 69714)
imported_spectrum.apply_calibration([0.0003705, 2.3694975, 4.2583089])

co_spect = Spectrum(len(imported_data), "RC103Co")
co_spect.set_y_data(imported_cobolt_data, 67286)
co_spect.apply_calibration([0.0003705, 2.3694975, 4.2583089])


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
    uptime: float
    timestamp: float




# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    new_current_signal = Signal(object)
    new_spectrum_signal = Signal(object)
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gamma Spectroscopy")

        self.run_manager = None
        self.mock_running = True

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)

        self.menu_bar = MainMenuBar()
        self.setMenuBar(self.menu_bar)


        # ---------- SPECTRUM PLOT ----------
        self.spectrum_plot = SpectrumPlot()
        layout.addWidget(self.spectrum_plot, 4)


        self.bottom_tabs = QTabWidget(self)
        self.bottom_tabs.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding, )
 
        
        self.current_value_tab = CurrentValuesPlot()
        self.bottom_tabs.addTab(self.current_value_tab, "Current Values")

        self.roi_info_pane = ROIInfoPane()
        self.bottom_tabs.addTab(self.roi_info_pane, "ROI Info")


        self.bottom_tabs.addTab(QWidget(), "Devices")

        self.bottom_tabs.addTab(QWidget(), "Spectra")

        self.bottom_tabs.addTab(QWidget(), "System Log")




        layout.addWidget(self.bottom_tabs, 1)

        self.spectrum_plot.EmittedSignlas.roi_created.connect(self.roi_info_pane.recieve_roi)
        self.spectrum_plot.recieve_spectrum(imported_spectrum, True)
        self.spectrum_plot.recieve_spectrum(co_spect)

        self.new_current_signal.connect(self.update_current)
        # self.new_spectrum_signal.connect(self.update_spectrum)


        self.theme = ThemeManager(ThemeManager.DARK)

        self.theme.apply(plot_widgets=[
            self.spectrum_plot.plot_widget, 
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget],
            legends=self.current_value_tab.legends)

        
    def update_current(self, package):
        self.current_value_tab.receive_data_packet(package)
    
    def update_spectrum(self, package):
        pass

    def recieve_BT_list(self, device_list):
        # Lazy-create popup once
        if not hasattr(self, "_bt_popup"):
            self._bt_popup = BluetoothListPopup(parent=self)

            # User selected a device
            self._bt_popup.deviceSelected.connect(self._on_bt_device_selected)

            # User pressed rescan
            self._bt_popup.rescanRequested.connect(self._request_bt_scan)

        # Update popup contents
        self._bt_popup.set_devices(device_list)

        if not self._bt_popup.isVisible():
            self._bt_popup.show()


    def _on_bt_device_selected(self, device):
        print("Selected device:", device)

        if "radiacode" in device.name.lower():
            device_type = "radiacode"
        elif "raysid" in device.name.lower():
            device_type = "raysid"
        else:
            print(f"Invalid device type {device.name}")
            return
        asyncio.create_task(
            self.run_manager.add_device(device, device_type)
        )
    def _request_bt_scan(self):
        self._bt_popup.start_scan_ui()
        asyncio.create_task(self.run_manager.find_bluetooth())



# ===================== MOCK DATA TASK =====================
async def mock_data_task(win: MainWindow, name):
    while True:
        if win.mock_running:
            cps = np.random.normal(500,50)
            dr  = cps/1000 + np.random.normal(0,0.01)
            timestamp = time.time()
            # Emit signal
            win.new_current_signal.emit(CurrentValuesPackage(name, cps, dr, timestamp))
        await asyncio.sleep(0.5)


# ===================== ENTRY =====================
def main():
    app = QApplication(sys.argv)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    win = MainWindow()
    win.show()

    run_manager = RunManager(loop, {"mock": MockClient})
    win.run_manager = run_manager  # store reference

    run_manager.bluetoothFound.connect(win.recieve_BT_list)
    run_manager.bluetoothError.connect(print)

    # Schedule adding mock device safely after loop starts
    QTimer.singleShot(0, lambda: asyncio.create_task(run_manager.add_device("Mock", "mock")))

    # Schedule first Bluetooth scan safely
    QTimer.singleShot(0, lambda: asyncio.create_task(run_manager.find_bluetooth()))

    # Start mock tasks safely after loop starts
    QTimer.singleShot(0, lambda: asyncio.create_task(mock_data_task(win, "Raysid1")))
    QTimer.singleShot(0, lambda: asyncio.create_task(mock_data_task(win, "Raysid2")))

    # Cancel mock tasks on exit
    def shutdown():
        for t in asyncio.all_tasks(loop):
            t.cancel()

    app.aboutToQuit.connect(shutdown)

    # Start the event loop
    with loop:
        loop.run_forever()





if __name__ == "__main__":
    main()
