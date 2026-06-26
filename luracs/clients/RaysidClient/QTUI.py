import sys
import asyncio
from collections import deque
from dataclasses import dataclass
import time
import numpy as np

from RaysidClient import RaysidClientAsync, find_device_by_name
from RaysidClient import logger as raysid_logger

from gaussian_fitting import Gaussian, fit_gaussian

from PySide6.QtWidgets import (
    QApplication,
    QMainWindow,
    QWidget,
    QGridLayout,
    QVBoxLayout,
    QHBoxLayout,
    QGroupBox,
    QPushButton,
    QLabel,
    QTextEdit,
    QComboBox,
)
from PySide6.QtCore import Qt, Signal
from qasync import QEventLoop

import matplotlib

matplotlib.use("QtAgg")
from matplotlib.backends.backend_qtagg import FigureCanvasQTAgg, NavigationToolbar2QT
from matplotlib.figure import Figure
from matplotlib.widgets import RectangleSelector
import matplotlib.pyplot as plt
import logging

raysid_logger.setLevel(logging.INFO)

logging.basicConfig(format="%(name)s - %(levelname)s: %(message)s")


# -------------------- UI LOGGER --------------------
UI_logger = logging.getLogger("RaysidClient")
UI_logger.setLevel(logging.DEBUG)


class QTextEditLogger(logging.Handler):
    def __init__(self, text_edit):
        super().__init__()
        self.text_edit = text_edit

    def emit(self, record):
        msg = self.format(record)
        self.text_edit.append(msg)


logging.basicConfig(format="%(name)s - %(levelname)s: %(message)s")

# ===================== DATA API =====================
from RaysidClient.src.data_classes import (
    SpectrumResult,
    CurrentValuesPackage,
    StatusPackage,
)


class DetectorInfo:
    def __init__(self, name, type, connection_type, address=None):
        self.name = name
        self.type = type
        self.BLE_address = address
        self.connection_type = connection_type

        self.calibration_coefficients = None
        if type == "raysid":
            self.nr_channels = 1800
        else:
            self.nr_channels = 1024


class ROI:
    def __init__(self, low_x: float, high_x: float):
        self.low_x: float = low_x
        self.high_x: float = high_x
        self.mid_E: float = (low_x + high_x) / 2
        self.gaussian = None
        self.patch = None

    def fit_gaussian(self, x_axis, spectrum, gaussian_settings: dict = None):
        if gaussian_settings is None:
            gaussian_settings = {}
        # try:
        self.gaussian = fit_gaussian(
            x_axis, spectrum, self.low_x, self.high_x, **gaussian_settings
        )
        # except Exception as e:
        #     self.gaussian = None
        #     UI_logger.error(f"Gaussian fit failed on ROI {self.low_x}-{self.high_x}: {e}")

    def __lt__(self, other):
        if not isinstance(other, ROI):
            return NotImplemented
        return self.mid_E < other.mid_E

    def __repr__(self):
        return f"ROI({self.low_x}, {self.high_x}, mid_E={self.mid_E})"


# ===================== MATPLOTLIB CANVAS =====================
class MplCanvas(FigureCanvasQTAgg):
    def __init__(self, title="", xlabel="", ylabel="", height=3):
        fig = Figure(figsize=(5, height), dpi=100)
        self.ax = fig.add_subplot(111)
        self.ax.set_title(title)
        self.ax.set_xlabel(xlabel)
        self.ax.set_ylabel(ylabel)
        super().__init__(fig)
        self.user_scaled = False  # Track if user adjusted view manually


# ===================== MAIN WINDOW =====================
class MainWindow(QMainWindow):
    restAcc = Signal()

    def __init__(self):
        super().__init__()
        self.setWindowTitle("Gamma Spectroscopy")

        self.window_seconds = 20
        self.cps_buf = deque([0.0] * self.window_seconds, maxlen=self.window_seconds)
        self.dr_buf = deque([0.0] * self.window_seconds, maxlen=self.window_seconds)

        central = QWidget()
        grid = QGridLayout(central)
        grid.setColumnStretch(0, 3)
        grid.setColumnStretch(1, 1)

        # ---------- OPTIONS ----------
        options = QGroupBox("Options & Controls")
        opt_layout = QHBoxLayout(options)
        self.btn_start = QPushButton("Start 📦")
        self.btn_stop = QPushButton("Stop")
        self.btn_reset = QPushButton("Reset")
        self.btn_reset.clicked.connect(self.restAcc.emit)

        self.btn_toggle_scale = QPushButton("Log Scale")
        self.btn_toggle_scale.setCheckable(True)
        self.btn_roi = QPushButton("Enable ROI ")
        self.btn_clear_roi = QPushButton("Clear ROI")
        opt_layout.addWidget(self.btn_start)
        opt_layout.addWidget(self.btn_stop)
        opt_layout.addWidget(self.btn_reset)
        opt_layout.addWidget(self.btn_toggle_scale)

        self.btn_toggle_scale.clicked.connect(self.toggle_spectrum_scale)

        # ---------- LARGE SPECTRUM ----------
        spectrum_box = QGroupBox("Spectrum")
        spectrum_layout = QVBoxLayout(spectrum_box)
        self.spectrum_canvas = MplCanvas(xlabel="Channel", ylabel="Counts", height=4)
        self.spectrum_canvas.figure.tight_layout()
        self.spec_x = np.arange(1800)
        self.spectrum_data = np.zeros_like(self.spec_x)
        (self.spec_line,) = self.spectrum_canvas.ax.step(
            self.spec_x, self.spectrum_data, where="post"
        )
        spectrum_layout.addWidget(self.spectrum_canvas)
        self.spectrum_toolbar = NavigationToolbar2QT(self.spectrum_canvas, self)
        spectrum_layout.addWidget(self.spectrum_toolbar)

        self.gaussian_line = None

        # ---------- ROI PANEL ----------
        roi_box = QGroupBox("ROIs / Gaussians")
        roi_layout = QVBoxLayout(roi_box)
        self.roi_dropdown = QComboBox()
        self.btn_enable_roi = QPushButton("Enable ROI Selection")
        self.btn_remove_roi = QPushButton("Remove Selected ROI")
        self.btn_clear_roi = QPushButton("Clear All ROIs")
        self.txt_gaussian_info = QTextEdit()
        self.txt_gaussian_info.setReadOnly(True)

        roi_layout.addWidget(QLabel("Select ROI:"))
        roi_layout.addWidget(self.roi_dropdown)
        roi_layout.addWidget(self.btn_enable_roi)
        roi_layout.addWidget(self.btn_remove_roi)
        roi_layout.addWidget(self.btn_clear_roi)
        roi_layout.addWidget(QLabel("Gaussian info:"))
        roi_layout.addWidget(self.txt_gaussian_info)
        roi_layout.addStretch()

        # ---------- STATUS ----------
        status = QGroupBox("Status")
        status_layout = QVBoxLayout(status)
        self.lbl_battery = QLabel("Battery: -- %")
        self.lbl_charging = QLabel("Charging: --")
        self.lbl_temp = QLabel("Temperature: -- °C")
        for lbl in (self.lbl_battery, self.lbl_charging, self.lbl_temp):
            status_layout.addWidget(lbl)
        status_layout.addStretch()

        # ---------- CPS / DOSE PLOT ----------
        cps_plot_box = QGroupBox("Live CPS / Dose (10 s)")
        cps_plot_layout = QVBoxLayout(cps_plot_box)
        self.cps_canvas = MplCanvas(xlabel="Seconds ago", ylabel="CPS", height=2.5)
        x = np.arange(self.window_seconds) / 2
        self.ax_cps = self.cps_canvas.ax
        (self.cps_line,) = self.ax_cps.plot(
            x, list(self.cps_buf), color="tab:blue", label="CPS"
        )
        self.ax_cps.set_ylabel("CPS", color="tab:blue")
        self.ax_cps.tick_params(axis="y", labelcolor="tab:blue")
        self.ax_dr = self.ax_cps.twinx()
        (self.dr_line,) = self.ax_dr.plot(
            x, list(self.dr_buf), color="tab:orange", label="Dose Rate"
        )
        self.ax_dr.set_ylabel("Dose Rate (µSv/h)", color="tab:orange")
        self.ax_dr.tick_params(axis="y", labelcolor="tab:orange")
        self.ax_cps.invert_xaxis()
        lines = [self.cps_line, self.dr_line]
        labels = [l.get_label() for l in lines]
        self.ax_cps.legend(lines, labels, loc="upper left")
        cps_plot_layout.addWidget(self.cps_canvas)

        # ---------- CPS / DOSE VALUES ----------
        values_box = QGroupBox("Rates")
        values_layout = QVBoxLayout(values_box)
        self.lbl_cps = QLabel("CPS: 0.0")
        self.lbl_dr = QLabel("Dose: 0.000 µSv/h")
        for lbl in (self.lbl_cps, self.lbl_dr):
            lbl.setAlignment(Qt.AlignCenter)
            lbl.setStyleSheet("font-size: 16px; font-weight: bold;")
            values_layout.addWidget(lbl)
        self.btn_avg = QPushButton("Show Averages")
        self.btn_avg.setCheckable(True)
        self.btn_avg.clicked.connect(self.toggle_average_display)
        values_layout.addWidget(self.btn_avg)
        values_layout.addStretch()

        # ---------- LOG ----------
        log = QGroupBox("Log")
        log_layout = QVBoxLayout(log)
        self.log_edit = QTextEdit()
        self.log_edit.setReadOnly(True)
        log_layout.addWidget(self.log_edit)

        # Connect loggers to UI
        text_handler = QTextEditLogger(self.log_edit)
        text_handler.setFormatter(
            logging.Formatter("%(name)s - %(levelname)s: %(message)s")
        )
        UI_logger.addHandler(text_handler)
        raysid_logger.addHandler(text_handler)

        # ---------- BOTTOM ROW ----------
        bottom_row = QHBoxLayout()
        bottom_row.addWidget(cps_plot_box, 3)
        bottom_row.addWidget(values_box, 1)
        bottom_row.addWidget(log, 2)
        bottom_container = QWidget()
        bottom_container.setLayout(bottom_row)

        # ---------- GRID ----------
        grid.addWidget(options, 0, 0)
        grid.addWidget(status, 0, 1)
        grid.addWidget(spectrum_box, 1, 0)
        grid.addWidget(roi_box, 1, 1)
        grid.addWidget(bottom_container, 2, 0, 1, 2)
        self.setCentralWidget(central)

        # ---------- ROI management ----------
        self.roi_selector = None
        self.ROIs = []
        self.show_average = False

        self.btn_enable_roi.clicked.connect(self.enable_roi)
        self.btn_remove_roi.clicked.connect(self.remove_selected_roi)
        self.btn_clear_roi.clicked.connect(self.clear_all_rois)
        self.roi_dropdown.currentIndexChanged.connect(self.update_gaussian_info)

    # ---------- UPDATE METHODS ----------
    def update_current(self, pkg: CurrentValuesPackage):
        self.cps_buf.append(pkg.CPS)
        self.dr_buf.append(pkg.DR)
        self.cps_line.set_ydata(list(self.cps_buf)[::-1])
        self.dr_line.set_ydata(list(self.dr_buf)[::-1])
        self.ax_cps.relim()
        self.ax_cps.autoscale_view()
        self.ax_dr.relim()
        self.ax_dr.autoscale_view()
        self.cps_canvas.draw_idle()
        if self.show_average:
            avg_cps = np.mean(self.cps_buf)
            avg_dr = np.mean(self.dr_buf)
            self.lbl_cps.setText(f"CPS (avg): {avg_cps:.1f}")
            self.lbl_dr.setText(f"Dose (avg): {avg_dr:.3f} µSv/h")
        else:
            self.lbl_cps.setText(f"CPS: {pkg.CPS:.1f}")
            self.lbl_dr.setText(f"Dose: {pkg.DR:.3f} µSv/h")

    def update_status(self, pkg: StatusPackage):
        self.lbl_battery.setText(f"Battery: {pkg.battery} %")
        self.lbl_charging.setText(f"Charging: {pkg.charging}")
        self.lbl_temp.setText(f"Temperature: {pkg.temperature:.1f} °C")

    def update_spectrum(self, pkg: SpectrumResult):
        self.spectrum_data = pkg.spectrum
        # self.spectrum_data[:1023] = imported_data.T[1]
        self.spec_line.set_ydata(self.spectrum_data)
        self.spectrum_canvas.ax.set_title(
            f"# {pkg.counts}, Time {pkg.uptime}, E {pkg.energy}, HE# {pkg.high_E_counts}"
        )
        if not self.spectrum_canvas.user_scaled:
            self.spectrum_canvas.ax.set_ylim(0, max(1, self.spectrum_data.max() * 1.1))
        # Update all ROIs Gaussian with new spectrum
        for roi in self.ROIs:
            roi.fit_gaussian(self.spec_x, self.spectrum_data)
        self.update_gaussian_info(self.roi_dropdown.currentIndex())
        self.spectrum_canvas.draw_idle()

    # ---------- SPECTRUM SCALE TOGGLE ----------
    def toggle_spectrum_scale(self):
        if self.btn_toggle_scale.isChecked():
            self.btn_toggle_scale.setText("Linear Scale")
            ydata = np.maximum(self.spec_line.get_ydata(), 1e-3)
            self.spec_line.set_ydata(ydata)
            self.spectrum_canvas.ax.set_yscale("log")
        else:
            self.btn_toggle_scale.setText("Log Scale")
            self.spectrum_canvas.ax.set_yscale("linear")
        self.spectrum_canvas.draw_idle()

    # ---------- ROI HANDLING ----------
    def enable_roi(self):
        if self.roi_selector is None:

            def onselect(eclick, erelease):
                x_min, x_max = sorted([int(eclick.xdata), int(erelease.xdata)])
                roi = ROI(x_min, x_max)
                roi.fit_gaussian(self.spec_x, self.spectrum_data)
                self.ROIs.append(roi)
                patch = self.spectrum_canvas.ax.axvspan(
                    x_min, x_max, color="red", alpha=0.3
                )
                roi.patch = patch
                self.roi_dropdown.addItem(f"ROI {len(self.ROIs)} ({x_min}-{x_max})")
                self.roi_dropdown.setCurrentIndex(len(self.ROIs) - 1)
                self.update_gaussian_info(len(self.ROIs) - 1)
                self.spectrum_canvas.draw_idle()
                self.roi_selector.set_active(False)
                self.roi_selector = None
                self.btn_enable_roi.setText("Select ROI")

            self.roi_selector = RectangleSelector(
                self.spectrum_canvas.ax,
                onselect,
                useblit=True,
                button=[1],
                minspanx=1,
                minspany=0,
                spancoords="data",
                interactive=False,
            )
            self.btn_enable_roi.setText("Cancel ROI selection")
        else:
            self.roi_selector.set_active(False)
            self.roi_selector = None
            self.btn_enable_roi.setText("Select ROI")

    def remove_selected_roi(self):
        idx = self.roi_dropdown.currentIndex()
        if idx < 0 or idx >= len(self.ROIs):
            return
        roi = self.ROIs.pop(idx)
        if roi.patch is not None:
            roi.patch.remove()
        self.roi_dropdown.removeItem(idx)
        self.spectrum_canvas.draw_idle()
        self.update_gaussian_info(self.roi_dropdown.currentIndex())

    def clear_all_rois(self):
        for roi in self.ROIs:
            if roi.patch is not None:
                roi.patch.remove()
        self.ROIs.clear()
        self.roi_dropdown.clear()
        self.txt_gaussian_info.clear()
        self.spectrum_canvas.draw_idle()

    def update_gaussian_info(self, idx):
        if idx < 0 or idx >= len(self.ROIs):
            self.txt_gaussian_info.clear()
            return
        roi = self.ROIs[idx]
        g = roi.gaussian
        if g is None:
            self.txt_gaussian_info.setPlainText("Gaussian not available or fit failed")
        else:
            try:
                info = str(g)
                self.txt_gaussian_info.setPlainText(info)
            except AttributeError as e:
                self.txt_gaussian_info.setPlainText(f"Error reading Gaussian: {e}")

    # ---------- TOGGLE AVERAGE DISPLAY ----------
    def toggle_average_display(self):
        self.show_average = self.btn_avg.isChecked()


# ===================== MOCK DATA TASK =====================
async def raysid_update_task(win: MainWindow):
    device = await find_device_by_name("Raysid")
    if not device:
        return

    client = RaysidClientAsync(device)
    win.raysid = client
    win.restAcc.connect(lambda: client.reset(RaysidClientAsync.EnergyRange.LOW))
    send_spect = False
    try:
        await client.start()

        while True:
            await asyncio.sleep(0.5)

            if client.LatestRealTimeData:
                win.update_current(client.LatestRealTimeData)

            if client.LatestStatusData:
                win.update_status(client.LatestStatusData)

            if send_spect:
                if client.LatestSpectrum:
                    win.update_spectrum(client.LatestSpectrum)
                send_spect = False
            else:
                send_spect = True

    except asyncio.CancelledError:
        pass
    finally:
        await client.stop()  # ✅ ALWAYS disconnect


# ===================== ENTRY =====================
def main():
    print(
        "WARNING This GUI was written by chatGPT in 10 min and FKN SUCKS! Only use for debug"
    )
    app = QApplication(sys.argv)
    loop = QEventLoop(app)
    asyncio.set_event_loop(loop)

    win = MainWindow()
    win.show()

    raysid_task = loop.create_task(raysid_update_task(win))

    def shutdown():
        print("Application quitting...")
        raysid_task.cancel()

    app.aboutToQuit.connect(shutdown)

    with loop:
        loop.run_forever()


if __name__ == "__main__":
    main()


# pyinstaller \
#   --onefile \
#   --windowed \
#   --clean \
#   --name linux_binary \
#   --hidden-import numpy \
#   --hidden-import scipy \
#   --exclude-module numpy.tests \
#   --exclude-module scipy.tests \
#   --exclude-module matplotlib.tests \
#   --exclude-module matplotlib._tkinter \
#   --exclude-module matplotlib.backends.backend_agg \
#   --exclude-module matplotlib.backends.backend_pdf \
#   --exclude-module matplotlib.backends.backend_ps \
#   --exclude-module matplotlib.backends.backend_svg \
#   --exclude-module matplotlib.toolkits.mplot3d \
#   --exclude-module PySide6.QtWebEngineWidgets \
#   --exclude-module PySide6.QtWebEngineCore \
#   --exclude-module PySide6.QtPdf \
#   --exclude-module PySide6.Qt3DCore \
#   --exclude-module PySide6.Qt3DRender \
#   QTUI.py

# pyinstaller \
#   --windowed \
#   --clean \
#   --strip \
#   --name linux_binary \
#   --hidden-import=numba.core.typing \
#   --hidden-import=numba.core.typing.templates \
#   --hidden-import=numba.core.datamodel \
#   --hidden-import=numba.core.compiler \
#   --hidden-import=numba.core.runtime \
#   --collect-submodules=numba \
#   --collect-data=numpy \
#   --collect-data=scipy \
#   --exclude-module=PySide6.QtWebEngineWidgets \
#   --exclude-module=PySide6.QtWebEngineCore \
#   --exclude-module=PySide6.QtPdf \
#   --exclude-module=PySide6.Qt3DCore \
#   --exclude-module=PySide6.Qt3DRender \
#   QTUI.py


# pyinstaller ^
#   --onefile ^
#   --windowed ^
#   --clean ^
#   --name windows_binary ^
#   --hidden-import=numba.core.typing ^
#   --hidden-import=numba.core.typing.templates ^
#   --hidden-import=numba.core.datamodel ^
#   --hidden-import=numba.core.compiler ^
#   --hidden-import=numba.core.runtime ^
#   --hidden-import=scipy.special.cython_special ^
#   --hidden-import=scipy.linalg.cython_lapack ^
#   --collect-all numpy ^
#   --collect-all scipy ^
#   --collect-all numba ^
#   GTUI.py
