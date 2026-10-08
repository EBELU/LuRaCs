from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.clients import (
        WrappedRealTimePackage,
        WrappedSpectrumPackage,
        WrappedStatusPackage,
    )

from datetime import datetime, timedelta
from pathlib import Path

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import QObject
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QComboBox,
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QSpinBox,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from luracs.containers.spectrum_classes import SpectrumData
from luracs.core import RunManager, Settings, core_utils
from luracs.utils.color_rotator import ColorRotator


class Measurement(QObject):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.detector_name = ""
        self.remaining_time = 0  # in seconds
        self.duration_limit = 0  # in seconds
        self.measurement_time = 0  # in seconds
        self.accumulated_counts = 0
        self.accumulated_dose = 0.0
        self.mean_count_rate = 0.0
        self.mean_dose_rate = 0.0
        self.previous_spectrum = None
        self.previous_real_time = None
        self.spectrum: np.ndarray = None
        self.count_rate_data: list = []
        self.dose_rate_data: list = []
        self.rate_axis: list = []
        self.x_axis: np.ndarray | None = None
        
        
    def set_detector_name(self, name: str):
        self.detector_name = name
    
    def set_duration_limit(self, duration: int):
        self.duration_limit = duration
        
    def update_real_time(self, real_time_data: WrappedRealTimePackage):
        if self.previous_spectrum is None:
            return
        
        self.count_rate_data.append(real_time_data.CPS)
        self.dose_rate_data.append(real_time_data.DR)
        self.rate_axis.append(self.measurement_time)
        self.mean_count_rate = np.mean(self.count_rate_data) if self.count_rate_data else real_time_data.CPS
        self.mean_dose_rate = np.mean(self.dose_rate_data) if self.dose_rate_data else real_time_data.DR
        self.accumulated_dose += real_time_data.DR * Settings.Advanced.spectrum_update_delay / 3600
        
    def set_calibration(self, coeffs: list, channels: int):
        if coeffs is not None:
            self.x_axis = np.polyval(coeffs, np.arange(channels))
        else:
            self.x_axis = np.arange(channels)    
        
    def set_spectrum(self, spectrum: SpectrumData):
        if self.previous_spectrum is None:
            self.previous_spectrum = spectrum
            if self.spectrum is None:
                self.spectrum = np.zeros_like(spectrum.y_axis)
            return

        self.spectrum += spectrum.y_axis - self.previous_spectrum.y_axis
        self.accumulated_counts = np.sum(self.spectrum)
        self.measurement_time += spectrum.live_time - self.previous_spectrum.live_time
        
        self.previous_spectrum = spectrum

class MeasurePlot(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        color_rotator = ColorRotator(Settings.Appearance.color_rotator_scheme)
        
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(5, 5, 5, 5)

        # --- Status Layout at the top ---
        status_layout = QHBoxLayout()
        
        status_layout.addWidget(QLabel("Detector"))
        self.detector_combo = QComboBox()
        self.detector_combo.setFixedWidth(200)
        status_layout.addWidget(self.detector_combo)
        
        status_layout.addWidget(QLabel("Battery"))
        self.battery_edit = QLineEdit()
        self.battery_edit.setReadOnly(True)
        status_layout.addWidget(self.battery_edit)
        
        status_layout.addWidget(QLabel("Detector Status"))
        self.status_edit = QLineEdit()
        self.status_edit.setReadOnly(True)
        status_layout.addWidget(self.status_edit)
        
        status_layout.addWidget(QLabel("Connection Type"))
        self.connection_type_edit = QLineEdit()
        self.connection_type_edit.setReadOnly(True)
        status_layout.addWidget(self.connection_type_edit)
        
        self.reset_detector_button = QPushButton("Reset Detector")
        # status_layout.addWidget(self.reset_detector_button)
        
        main_layout.addLayout(status_layout)
        
        # --- Control Layout for measurement controls ---
        main_layout.addWidget(QLabel("Measurement Controls"))
        control_layout = QHBoxLayout()
        self.hours_spin = QSpinBox()
        self.hours_spin.setRange(0, 100)
        self.hours_spin.valueChanged.connect(lambda : self.confirm_time_button.setChecked(True))
        control_layout.addWidget(QLabel("Hours"))
        control_layout.addWidget(self.hours_spin)
        self.minutes_spin = QSpinBox()
        self.minutes_spin.valueChanged.connect(lambda : self.confirm_time_button.setChecked(True))
        self.minutes_spin.setRange(0, 59)
        control_layout.addWidget(QLabel("Minutes"))
        control_layout.addWidget(self.minutes_spin)
        self.seconds_spin = QSpinBox()
        self.seconds_spin.valueChanged.connect(lambda : self.confirm_time_button.setChecked(True))
        self.seconds_spin.setRange(0, 59)
        control_layout.addWidget(QLabel("Seconds"))
        control_layout.addWidget(self.seconds_spin)
        self.confirm_time_button = QPushButton("Confirm Time", checkable=True)
        self.confirm_time_button.clicked.connect(self.set_time)
        control_layout.addWidget(self.confirm_time_button)
        self.progress_bar = QProgressBar()
        self.progress_bar.setRange(0, 100)
        self.progress_bar.setValue(0)
        self.progress_bar.setFormat("Remaining: 00:00:00")
        control_layout.addWidget(self.progress_bar)
        main_layout.addLayout(control_layout)
        
        start_stop_layout = QHBoxLayout()
        self.start_button = QPushButton("Start Measurement")
        self.start_button.clicked.connect(self.start_measurement)
        start_stop_layout.addWidget(self.start_button)
        self.stop_button = QPushButton("Stop Measurement")
        self.stop_button.clicked.connect(self.stop_measurement)
        start_stop_layout.addWidget(self.stop_button)
        self.clear_measurement_button = QPushButton("Clear Measurement")
        self.clear_measurement_button.clicked.connect(self.clear_measurement)
        start_stop_layout.addWidget(self.clear_measurement_button)
        self.make_report_button = QPushButton("Make Report", checkable=True)
        self.make_report_button.clicked.connect(self.make_report)
        start_stop_layout.addWidget(self.make_report_button)
        main_layout.addLayout(start_stop_layout, 7)

        
        spectrum_layout = QHBoxLayout()
        self.info_text = QTextEdit()
        self.info_text.setReadOnly(True)
        spectrum_layout.addWidget(self.info_text)
        spectrum_layout.addLayout(spectrum_layout, 1)
        
        # --- Spectrum Plot ---
        self.spectrum_plot = pg.PlotWidget()
        self.spectrum_plot.setTitle("Measured Spectrum")
        self.spectrum_plot.setLabel("bottom", "Energy [keV]")
        self.spectrum_plot.setLabel("left", "Counts per Channel")
        self.spectrum_plot.getViewBox().setMouseEnabled(x=True, y=False)
        self.spectrum_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.spectrum_plot.setLimits(xMin=-25, xMax=3000)
        
        # Make data line
        spect_color = color_rotator.next_color()
        brush_color = QColor(spect_color)
        brush_color.setAlpha(80)

        self.spectrum_detector_line = self.spectrum_plot.plot(
            [],
            [],
            pen=pg.mkPen(spect_color, width = 1),
            brush=pg.mkBrush(brush_color),
            fillLevel=0,
        )
        
        
        spectrum_layout.addWidget(self.spectrum_plot)
        main_layout.addLayout(spectrum_layout, 2)
        
        # --- Layout for rate plots bellow the spectrum ---
        rate_layout = QHBoxLayout()
        
        self.count_rate_plot = pg.PlotWidget()
        self.count_rate_plot.setTitle("Count Rate")
        self.count_rate_plot.setLabel("bottom", "Measurement Time [s]")
        self.count_rate_plot.setLabel("left", "Count Rate [/s]")
        self.count_rate_plot.getViewBox().setMouseEnabled(x=True, y=False)
        
        self.count_rate_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.count_rate_line = self.count_rate_plot.plot([], [], pen=color_rotator.next_pen())
        rate_layout.addWidget(self.count_rate_plot)
        
        
        self.dose_rate_plot = pg.PlotWidget()
        self.dose_rate_plot.setTitle("Dose Rate")
        self.dose_rate_plot.setLabel("bottom", "Measurement Time [s]")
        self.dose_rate_plot.setLabel("left", "Dose Rate [uSv/s]")
        self.dose_rate_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.dose_rate_plot.getViewBox().setMouseEnabled(x=True, y=False)
        self.dose_rate_line = self.dose_rate_plot.plot([], [], pen=color_rotator.next_pen())
        rate_layout.addWidget(self.dose_rate_plot)
        main_layout.addLayout(rate_layout, 1)
        
        core_utils.ThemeManager.register_plot(self.spectrum_plot, self.count_rate_plot, self.dose_rate_plot)
        
        self.connected: bool = False
        RunManager.Signals.deviceConnected.connect(self.catch_detector_added)
        RunManager.Signals.deviceRemoved.connect(self.catch_detector_removed)
        RunManager.Signals.statusUpdated.connect(self.catch_status_update)


        self.buffer_real_time = None
        self.current_measurement = Measurement()
        self.time_limit_seconds = 0
        self.plot_nr = 0
        
        self.update_text()
        
    def start_measurement(self):
        if not self.detector_combo.currentText():
            QMessageBox.warning(self, "No Detector", "The measurement could not be started because no detector is connected.")
        
        if self.time_limit_seconds == 0:
            result = QMessageBox.question(
                self,
                "Confirm",
                "No time limit for the measurement has been set. Are you sure you want to continue?",
                QMessageBox.StandardButton.Ok | QMessageBox.StandardButton.Cancel,
            )
            if result == QMessageBox.StandardButton.Cancel:
                return
        
        RunManager.Signals.spectrumUpdated.connect(self.catch_spectrum_update)
        RunManager.Signals.currentUpdated.connect(self.catch_real_time_update)
        
    def stop_measurement(self):
        RunManager.Signals.spectrumUpdated.disconnect(self.catch_spectrum_update)
        RunManager.Signals.currentUpdated.disconnect(self.catch_real_time_update)
        self.current_measurement.detector_name = self.detector_combo.currentText()
        self.current_measurement.previous_spectrum = None
        
    def clear_measurement(self):
        result = QMessageBox.question(
            self,
            "Confirm",
            "Do you want clear the current measurement? All acquired data will be lost.",
            QMessageBox.StandardButton.Ok | QMessageBox.StandardButton.Cancel,
        )
        if result == QMessageBox.StandardButton.Cancel:
            return
        
        self.make_report_button.setChecked(False)
        self.current_measurement.deleteLater()
        self.current_measurement = Measurement()
        self.spectrum_detector_line.setData([], [])
        self.count_rate_line.setData([], [])    
        self.dose_rate_line.setData([], [])    
        self.update_text()
    
    def set_time(self):
        self.confirm_time_button.setChecked(False)
        hours = self.hours_spin.value()
        minutes = self.minutes_spin.value()
        seconds = self.seconds_spin.value()
        if hours == 0 and minutes == 0 and seconds == 0:
            self.progress_bar.setRange(0, 100)
            self.progress_bar.setValue(0)
            self.progress_bar.setFormat("Remaining: 00:00:00")
            return
        
        total_seconds = hours * 3600 + minutes * 60 + seconds
        self.time_limit_seconds = total_seconds
        self.progress_bar.setRange(0, total_seconds)
        self.progress_bar.setValue(0)
        self.progress_bar.setFormat(f"Remaining: {hours:02d}:{minutes:02d}:{seconds:02d}")
        
    def catch_detector_added(self, detector_name: str):
        self.detector_combo.addItem(detector_name)
        
    def catch_detector_removed(self, detector_name: str):
        idx = self.detector_combo.findText(detector_name)
        self.detector_combo.removeItem(idx)
        
    def catch_real_time_update(self, detector_name: str, real_time_data: WrappedRealTimePackage):
        if detector_name != self.detector_combo.currentText():
            return
        
        self.buffer_real_time = real_time_data
    
    def catch_spectrum_update(self, detector_name: str, spectrum: WrappedSpectrumPackage):
        if detector_name != self.detector_combo.currentText():
            return
                
        if 0.05 > (self.time_limit_seconds - self.current_measurement.measurement_time) and self.time_limit_seconds > 0:
            self.stop_button.click()
            QMessageBox.information(self, "Info", "Measurement time reached. Increase the measurement duration to continue.")
            return
        
        if self.current_measurement.x_axis is None and spectrum.calib_coeff is not None:
            self.current_measurement.set_calibration(spectrum.calib_coeff, len(spectrum.y_axis))
        
        self.current_measurement.update_real_time(self.buffer_real_time)
        self.current_measurement.set_spectrum(spectrum)
        if self.time_limit_seconds > 0:
            self.update_progress()

        
        self.update_plots()
        self.update_text()

        
        if 0.05 > (self.time_limit_seconds - self.current_measurement.measurement_time) and self.time_limit_seconds > 0:
            self.update_progress()
            self.stop_button.click()
            QMessageBox.information(
                self, 
                "Measurement Completed",
                "The measurement has completed successfully.\n"
                "Data collection has stopped because the configured collection time was reached."
            )
            self.make_report_button.setChecked(True)
            
            
        
    def catch_status_update(self, detector_name: str, status: WrappedStatusPackage):
        if detector_name != self.detector_combo.currentText():
            return
        
        self.battery_edit.setText(f"{status.battery:.1f}%")
        self.status_edit.setText(str(status.device_state))
        self.connection_type_edit.setText(str(status.connection_type))
        
    def update_progress(self):
        diff = self.time_limit_seconds - self.current_measurement.measurement_time
        td = timedelta(seconds=max(round(diff), 0))
        self.progress_bar.setValue(round(self.current_measurement.measurement_time))
        self.progress_bar.setFormat(f"Remaining: {td}")
        
    def update_text(self):
        text = (
            " --- Measurement Summary --- \n\n"
            f"Detector: {self.current_measurement.detector_name}\n"
            f"Measurement Rate: {1 / Settings.Advanced.spectrum_update_delay}/s \n"
            f"Measurement Time: {timedelta(seconds=round(self.current_measurement.measurement_time))}\n"
            "\n"
            f"Total Counts: {round(self.current_measurement.accumulated_counts)}\n"
            f"Mean Count Rate: {round(self.current_measurement.mean_count_rate,2)} /s \n"
            f"Total Dose: {round(self.current_measurement.accumulated_dose, 4)} uSv\n"
            f"Mean Dose Rate: {round(self.current_measurement.mean_dose_rate, 3)} uSv/h\n"
        )
        
        self.info_text.setText(text)
        
    def make_report(self):
        self.make_report_button.setChecked(False)
        if self.current_measurement.measurement_time == 0:
            QMessageBox.information(self, "No Measurement", "No measurement to report on.")
            return
        
        timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        file, used_filter = QFileDialog.getSaveFileName(
            self,
            "Export File",
            str(Settings.Paths.last_opened_dir / f"MeasurementReport_{timestamp}"),
            "Text Report (.txt);; JSON Report (.json)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )
        if not file:
            return
        file = Path(file)
        Settings.Paths.last_opened_dir = file.parent
        
        if file.exists():
            result = QMessageBox.question(
                self,
                "File Exists",
                f"{file.name} already exists. Do you want to replace it?",
                QMessageBox.StandardButton.Ok | QMessageBox.StandardButton.Cancel,
            )
            if result == QMessageBox.StandardButton.Cancel:
                return
            
        if ".txt" in used_filter:
            print("txt")
        elif ".json" in used_filter:
            print(".json")

            
    
    def update_plots(self):       
        self.count_rate_line.setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.count_rate_data)
        self.dose_rate_line.setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.dose_rate_data)
        
        self.spectrum_detector_line.setData(self.current_measurement.x_axis, self.current_measurement.spectrum)

if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = MeasurePlot()
    window.resize(800, 500)
    window.show()

    sys.exit(app.exec())