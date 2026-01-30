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

from QtGUI import SpectrumPlot, ROIInfoPane
from QtGUI import MainMenuBar, MenuActions, ThemeManager
from QtGUI.SpectrumClasses import Spectrum
from QtGUI import CurrentValuesPlot

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
    timestamp: float


mock_data = [
    CurrentValuesPackage(CPS=10.2, DR=0.80, timestamp=1700000000.0),
    CurrentValuesPackage(CPS=11.0, DR=0.83, timestamp=1700000060.0),
    CurrentValuesPackage(CPS=12.4, DR=0.86, timestamp=1700000120.0),
    CurrentValuesPackage(CPS=13.1, DR=0.88, timestamp=1700000180.0),
    CurrentValuesPackage(CPS=14.0, DR=0.90, timestamp=1700000240.0),
]


# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    new_current_signal = Signal(object)
    new_spectrum_signal = Signal(object)
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gamma Spectroscopy")

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



        layout.addWidget(self.bottom_tabs, 1)

        self.spectrum_plot.roi_signal_sender.connect(self.roi_info_pane.recieve_roi)
        self.spectrum_plot.handle_spectrum(imported_spectrum, True)
        self.spectrum_plot.handle_spectrum(co_spect)

        self.new_current_signal.connect(self.update_current)
        # self.new_spectrum_signal.connect(self.update_spectrum)

        for mock_packet in mock_data:
            self.current_value_tab.receive_data_packet(mock_packet, "Raysid")

        self.theme = ThemeManager(ThemeManager.DARK)

        self.theme.apply(plot_widgets=[
            self.spectrum_plot.plot_widget, 
            self.current_value_tab.cps_plot_widget,
            self.current_value_tab.dose_plot_widget])
        
    def update_current(self, package):
        self.current_value_tab.receive_data_packet(package, "Raysid")

    


# ===================== MOCK DATA TASK =====================
async def mock_data_task(win: MainWindow):
    while True:
        if win.mock_running:
            cps = np.random.normal(500,50)
            dr  = cps/1000 + np.random.normal(0,0.01)
            timestamp = time.time()
            # Emit signal
            win.new_current_signal.emit(CurrentValuesPackage(cps, dr, timestamp))
        await asyncio.sleep(0.5)


# ===================== ENTRY =====================
def main():
    app = QApplication(sys.argv)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    win = MainWindow()
    win.show()

    task = loop.create_task(mock_data_task(win))

    def shutdown():
        task.cancel()

    app.aboutToQuit.connect(shutdown)

    with loop:
        loop.run_forever()



if __name__ == "__main__":
    main()
