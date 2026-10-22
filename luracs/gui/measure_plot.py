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
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QButtonGroup,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLayout,
    QLineEdit,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QRadioButton,
    QSpinBox,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from luracs.containers.report_template import MeasurementReport
from luracs.containers.spectrum_classes import Spectrum, SpectrumData
from luracs.core import RunManager, Settings, core_utils, SpectrumManager
from luracs.utils.color_rotator import ColorRotator
from luracs.utils.file_io import xml_report_writer


class ReportFormDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        
        main_layout = QVBoxLayout(self)
        
        form = QFormLayout()
        label = QLabel(" Enter additional report information ")
        label.setStyleSheet("font-weight: bold;")
        form.addRow(label)
        self.name = QLineEdit()
        form.addRow("Name", self.name)
        self.location = QLineEdit()
        form.addRow("Location", self.location)
        
        self.longitude = QDoubleSpinBox()
        self.longitude.setRange(-180.0, 180.0)
        self.longitude.setDecimals(6)
        self.longitude.setSingleStep(0.0001)
        self.longitude.setSuffix("°")
        self.longitude.setKeyboardTracking(False)

        form.addRow("Longitude", self.longitude)

        self.latitude = QDoubleSpinBox()
        self.latitude.setRange(-90.0, 90.0)
        self.latitude.setDecimals(6)
        self.latitude.setSingleStep(0.0001)
        self.latitude.setSuffix("°")
        self.latitude.setKeyboardTracking(False)

        form.addRow("Latitude", self.latitude)
        
        form.addRow(QLabel(" "))
        self.weather = QLineEdit()
        form.addRow("Weather", self.weather)
        
        self.wind_direction = QComboBox()
        self.wind_direction.addItems(
            ["-", "N", "NW", "W", "SW", "S", "SE", "E", "NE"]
        )
        form.addRow("Wind Direction", self.wind_direction)
        
        self.wind_speed = QDoubleSpinBox(decimals=1, maximum=100, minimum=0, suffix=" m/s", value=0, singleStep=0.1)
        form.addRow("Wind Speed", self.wind_speed)
        
        self.air_temperature = QDoubleSpinBox(decimals=1, maximum=100, minimum=-100, suffix=" °C", value=0, singleStep=0.1)
        form.addRow("Air Temperature", self.air_temperature)
        
        form.addRow(QLabel(" "))
        self.remark = QTextEdit()
        form.addRow("Comments", self.remark)
        
        main_layout.addLayout(form)
        
        button_box = QDialogButtonBox(self)

        self.cancel_button = button_box.addButton(
            QDialogButtonBox.StandardButton.Cancel
        )
        self.next_button = button_box.addButton(
            "Next →",
            QDialogButtonBox.ButtonRole.AcceptRole
        )
        
        self.cancel_button.clicked.connect(self.reject)
        self.next_button.clicked.connect(self.accept)

        main_layout.addWidget(button_box)
        
        
        
    def get_results(self) -> dict:
        return {
            "author_name": self.name.text(),
            "location": self.location.text(),
            "weather": self.weather.text(),
            "remark": self.remark.toPlainText().replace("\n", "; ").strip(),
            "longitude": self.longitude.value(),
            "latitude": self.latitude.value(),
            "wind_direction": self.wind_direction.currentText(),
            "wind_speed": self.wind_speed.value(),
            "air_temperature": self.air_temperature.value(),
        }
        
        

class Measurement:
    def __init__(self):
        self.detector_name = ""
        self.start_time: datetime | None = None
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
        self.calibration_coefficients: list | None = None
        self.stop_timestamps: list[datetime] = []
        
        
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
            self.calibration_coefficients = coeffs  
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
        self.detector_combo.currentTextChanged.connect(
            self.on_detector_change
        )
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
        label = QLabel(" Timer ")
        label.setStyleSheet("font-weight: bold;")
        control_layout = QHBoxLayout()
        control_layout.addWidget(label)
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
        
        
        label = QLabel(" Controls ")
        label.setMaximumWidth(100)
        label.setStyleSheet("font-weight: bold;")
        start_stop_layout = QHBoxLayout()
        start_stop_layout.addWidget(label)

        fg_bg_button_layout = QHBoxLayout()
        self.measurement_type = QButtonGroup(self)
        self.measurement_type.buttonToggled.connect(
            self.on_measurement_type_changed
        )
        
        self.measure_fg = QRadioButton("Foreground")
        self.measure_fg.setChecked(True)
        self.measure_bg = QRadioButton("Background")

        self.measurement_type.addButton(self.measure_fg)
        self.measurement_type.addButton(self.measure_bg)


        fg_bg_button_layout.addWidget(self.measure_fg)
        fg_bg_button_layout.addWidget(self.measure_bg)
        fg_bg_button_layout.setSizeConstraint(QLayout.SizeConstraint.SetFixedSize)
        start_stop_layout.addLayout(fg_bg_button_layout)
        
        self.start_button = QPushButton("Start Measurement", checkable=True)
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
        
        # --- Spectrum Plot ---
        self.spectrum_plot = pg.PlotWidget()
        self.spectrum_plot.setTitle("Measured Spectrum")
        self.spectrum_plot.setLabel("bottom", "Energy [keV]")
        self.spectrum_plot.setLabel("left", "Counts per Channel per Second")
        self.spectrum_plot.getViewBox().setMouseEnabled(x=True, y=False)
        self.spectrum_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.spectrum_plot.setLimits(xMin=-25, xMax=3000, minXRange=5)
    
        legend = self.spectrum_plot.addLegend()
        legend.setOffset((-1, 1))
        legend.setLabelTextSize(f"{Settings.Appearance.font_size}pt")

        self.spectrum_lines: dict[str, pg.PlotDataItem] = {}
        fg_color = color_rotator.next_color()
        color_rotator.next_color() # I like the green more than orange :)
        bg_color = color_rotator.next_color()
        
        for key in ("fg", "bg"):
            spect_color = fg_color if key == "fg" else bg_color
            brush_color = QColor(spect_color)
            brush_color.setAlpha(80)

            self.spectrum_lines[key] = self.spectrum_plot.plot(
                [],
                [],
                name = "Foreground" if key == "fg" else "Background",
                pen=pg.mkPen(spect_color, width=1),
                brush=pg.mkBrush(brush_color),
                fillLevel=0,
                stepMode="right",
                antialias=False,
            )
            

        
        spectrum_layout.addWidget(self.spectrum_plot)
        main_layout.addLayout(spectrum_layout, 2)
        
        # --- Layout for rate plots bellow the spectrum ---
        fg_pen = pg.mkPen(fg_color)
        bg_pen = pg.mkPen(bg_color)
        rate_layout = QHBoxLayout()
        
        self.count_rate_plot = pg.PlotWidget()
        self.count_rate_plot.setTitle("Count Rate")
        self.count_rate_plot.setLabel("bottom", "Measurement Time [s]")
        self.count_rate_plot.setLabel("left", "Count Rate [/s]")
        self.count_rate_plot.getViewBox().setMouseEnabled(x=True, y=False)
        
        self.count_rate_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.count_rate_lines: dict[str, pg.PlotDataItem] = {
            "fg": self.count_rate_plot.plot([], [], pen=fg_pen),
            "bg": self.count_rate_plot.plot([], [], pen=bg_pen)
        }
        rate_layout.addWidget(self.count_rate_plot)
        
        # --- Count Rate and Dose Rate Plots ---
        rate_layout = QHBoxLayout()

        # Count Rate Plot
        self.count_rate_plot = pg.PlotWidget()
        self.count_rate_plot.setTitle("Count Rate")
        self.count_rate_plot.setLabel("bottom", "Measurement Time [s]")
        self.count_rate_plot.setLabel("left", "Count Rate [/s]")
        self.count_rate_plot.getViewBox().setMouseEnabled(x=True, y=False)
        self.count_rate_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)

        self.count_rate_lines: dict[str, pg.PlotDataItem] = {
            "fg": self.count_rate_plot.plot(
                [], [], pen=fg_pen
            ),
            "bg": self.count_rate_plot.plot(
                [], [], pen=bg_pen
            ),
        }

        rate_layout.addWidget(self.count_rate_plot)


        # Dose Rate Plot
        self.dose_rate_plot = pg.PlotWidget()
        self.dose_rate_plot.setTitle("Dose Rate")
        self.dose_rate_plot.setLabel("bottom", "Measurement Time [s]")
        self.dose_rate_plot.setLabel("left", "Dose Rate [uSv/s]")
        self.dose_rate_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        self.dose_rate_plot.getViewBox().setMouseEnabled(x=True, y=False)

        self.dose_rate_lines: dict[str, pg.PlotDataItem] = {
            "fg": self.dose_rate_plot.plot(
                [], [], pen=fg_pen
            ),
            "bg": self.dose_rate_plot.plot(
                [], [], pen=bg_pen
            ),
        }

        rate_layout.addWidget(self.dose_rate_plot)

        main_layout.addLayout(rate_layout, 1)
        
        core_utils.ThemeManager.register_plot(self.spectrum_plot, self.count_rate_plot, self.dose_rate_plot)
        core_utils.ThemeManager.register_legend(legend)
                
        self.connected: bool = False
        RunManager.Signals.deviceConnected.connect(self.catch_detector_added)
        RunManager.Signals.deviceRemoved.connect(self.catch_detector_removed)
        RunManager.Signals.statusUpdated.connect(self.catch_status_update)


        self.buffer_real_time = None
        self.fg = Measurement()
        self.bg = Measurement()
        self.current_measurement = self.fg
        self.time_limit_seconds = 0
        self.plot_nr = 0
        self._measurement_signals_connected = False
        
        self.update_text()
        
    def start_measurement(self):
        self.start_button.setChecked(False)
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
        
        self.connect_measurement_signals()
        
    def stop_measurement(self):
        self.disconnect_measurement_signals()
        if self.current_measurement.previous_spectrum is not None:
            self.current_measurement.stop_timestamps.append(datetime.now())
        self.current_measurement.detector_name = self.detector_combo.currentText()
        self.current_measurement.previous_spectrum = None

    def connect_measurement_signals(self):
        if not self._measurement_signals_connected:
            RunManager.Signals.spectrumUpdated.connect(
                self.catch_spectrum_update
            )
            RunManager.Signals.currentUpdated.connect(
                self.catch_real_time_update
            )
            self._measurement_signals_connected = True


    def disconnect_measurement_signals(self):
        if self._measurement_signals_connected:
            RunManager.Signals.spectrumUpdated.disconnect(
                self.catch_spectrum_update
            )
            RunManager.Signals.currentUpdated.disconnect(
                self.catch_real_time_update
            )
            self._measurement_signals_connected = False
            
    def clear_measurement(self):
        result = QMessageBox.question(
            self,
            "Confirm",
            "Do you want clear the current measurement? All acquired data will be lost.",
            QMessageBox.StandardButton.Ok | QMessageBox.StandardButton.Cancel,
        )
        if result == QMessageBox.StandardButton.Cancel:
            return
        
        if self.measure_fg.isChecked():
            key = "fg"
            self.fg = Measurement()
            self.current_measurement = self.fg
        else:
            key = "bg"
            self.bg = Measurement()
            self.current_measurement = self.bg
        
        self.make_report_button.setChecked(False)
        self.spectrum_lines[key].setData([], [])
        self.count_rate_lines[key].setData([], [])    
        self.dose_rate_lines[key].setData([], [])    
        self.update_text()
        
    def on_measurement_type_changed(self, button, checked):
        self.stop_button.click()
        if checked:
            if button == self.measure_fg:
                self.current_measurement = self.fg
            elif button == self.measure_bg:
                self.current_measurement = self.bg
        
        self.set_time()
        
    def on_detector_change(self):
        if self.fg.measurement_time != 0 or self.bg.measurement_time != 0:
            res = QMessageBox.question(self, "Change Detector", "Changing detector will reset all previous measurements and all unsaved data will be lost!", QMessageBox.StandardButton.Ok | QMessageBox.StandardButton.Cancel,)
            if res == QMessageBox.StandardButton.Cancel:
                self.detector_combo.blockSignals(True)
                detector_name = self.fg.detector_name if self.fg.detector_name else self.bg.detector_name
                self.detector_combo.setCurrentText(detector_name)
                self.detector_combo.blockSignals(False)
                return

        self.fg = Measurement()
        self.current_measurement = self.fg

        self.bg = Measurement()

        self.make_report_button.setChecked(False)
        for key in ["fg", "bg"]:
            self.spectrum_lines[key].setData([], [])
            self.count_rate_lines[key].setData([], [])    
            self.dose_rate_lines[key].setData([], [])    
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
        
        self.start_button.setChecked(True)
        
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
        
        if self.current_measurement.x_axis is None:
            self.current_measurement.start_time = datetime.now()
            if spectrum.calib_coeff is None and SpectrumManager.get_spectrum(detector_name):
                spect = SpectrumManager.get_spectrum(detector_name)
                if spect.calibrated:
                    self.current_measurement.set_calibration(spect.calibration_coefficients, spect.channels)
                
            else:
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
            f"Detector: {self.fg.detector_name}\n"
            " --- Foreground Summary --- \n"
            f"Measurement Rate: {1 / Settings.Advanced.spectrum_update_delay}/s \n"
            f"Measurement Time: {timedelta(seconds=round(self.fg.measurement_time))}\n"
            "\n"
            f"Total Counts: {round(self.fg.accumulated_counts)}\n"
            f"Mean Count Rate: {round(self.fg.mean_count_rate,2)} /s \n"
            f"Total Dose: {round(self.fg.accumulated_dose, 4)} uSv\n"
            f"Mean Dose Rate: {round(self.fg.mean_dose_rate, 3)} uSv/h\n"
            "\n"
            " --- Background Summary --- \n"
            f"Measurement Rate: {1 / Settings.Advanced.spectrum_update_delay}/s \n"
            f"Measurement Time: {timedelta(seconds=round(self.bg.measurement_time))}\n"
            "\n"
            f"Total Counts: {round(self.bg.accumulated_counts)}\n"
            f"Mean Count Rate: {round(self.bg.mean_count_rate,2)} /s \n"
            f"Total Dose: {round(self.bg.accumulated_dose, 4)} uSv\n"
            f"Mean Dose Rate: {round(self.bg.mean_dose_rate, 3)} uSv/h\n"
        )
        
        self.info_text.setText(text)
        
    def make_report(self):
        self.make_report_button.setChecked(False)
        if self.current_measurement.measurement_time == 0:
            QMessageBox.information(self, "No Measurement", "No measurement to report on.")
            return
        
        report_form = ReportFormDialog()
        res = report_form.exec()
        if res != QDialog.DialogCode.Accepted:
            return
        
        timestamp = datetime.now().strftime("%Y-%m-%d_%H-%M-%S")
        file, used_filter = QFileDialog.getSaveFileName(
            self,
            "Export File",
            str(Settings.Paths.last_opened_dir / f"MeasurementReport_{timestamp}"),
            "Report (.xml)",
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
            
        form_data = report_form.get_results()
            

        new_spectrum = Spectrum(
            len(self.fg.spectrum), 
            file.stem, 
            instrument_id = self.fg.detector_name
            )
        
        fg = SpectrumData(
            y_axis=self.fg.spectrum,
            channels=len(self.fg.spectrum),
            total_counts=self.fg.accumulated_counts,
            live_time=self.fg.measurement_time,
            start_date=self.fg.start_time,
            spectrum_name=file.stem
        )
        new_spectrum.set_foreground(fg)
        
        if self.bg.measurement_time > 0:
            bg = SpectrumData(
                y_axis=self.bg.spectrum,
                channels=len(self.bg.spectrum),
                total_counts=self.bg.accumulated_counts,
                live_time=self.bg.measurement_time,
                start_date=self.bg.start_time,
                spectrum_name=file.stem
            )
            new_spectrum.set_background(bg)
        
        if self.fg.calibration_coefficients is not None:
            new_spectrum.apply_calibration(self.fg.calibration_coefficients)
        elif self.bg.calibration_coefficients is not None:
            new_spectrum.apply_calibration(self.bg.calibration_coefficients)

        
        report = MeasurementReport(
            date=datetime.now(),
            **form_data,
            fg_measurement=self.fg,
            bg_measurement=self.bg
        )
        
        xml_report_writer(new_spectrum, file, report)

    
    def update_plots(self):
        if self.measure_fg.isChecked():
            self.count_rate_lines["fg"].setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.count_rate_data)
            self.dose_rate_lines["fg"].setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.dose_rate_data)
            if self.current_measurement.x_axis is not None and self.current_measurement.spectrum is not None:
                self.spectrum_lines["fg"].setData(self.current_measurement.x_axis, self.current_measurement.spectrum  / max(self.current_measurement.measurement_time, 1))
        else:
            self.count_rate_lines["bg"].setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.count_rate_data)
            self.dose_rate_lines["bg"].setData(np.asarray(self.current_measurement.rate_axis) + Settings.Advanced.spectrum_update_delay, self.current_measurement.dose_rate_data)
            if self.current_measurement.x_axis is not None and self.current_measurement.spectrum is not None:
                self.spectrum_lines["bg"].setData(self.current_measurement.x_axis, self.current_measurement.spectrum / max(self.current_measurement.measurement_time, 1))

if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = MeasurePlot()
    window.resize(800, 500)
    window.show()

    sys.exit(app.exec())