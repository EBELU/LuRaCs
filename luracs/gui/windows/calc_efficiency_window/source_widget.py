from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .main_window import EfficiencyWindow
import math
from datetime import timedelta

import numpy as np
from PySide6.QtCore import QDateTime, Qt
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDateTimeEdit,
    QDoubleSpinBox,
    QGridLayout,
    QLabel,
    QLineEdit,
    QSizePolicy,
    QWidget,
)
from uncertainties import ufloat

from luracs.core import SpectrumManager
from luracs.gui.windows.calc_efficiency_window.helpers import (
    ActivityUnits,
    Source,
    TimeUnits,
    datetime_to_qdatetime,
    format_activity,
    format_duration,
    qdatetime_to_datetime,
)


class SourceParametersWidget(QWidget):
    def __init__(self, parent: EfficiencyWindow =None):
        super().__init__(parent)
        self.parent_window = parent
        source_parameters_layout = QGridLayout()
        
        # Nuclide combo box
        self.nuclide_combo = QComboBox()
        self.nuclide_combo.addItems(["Other"] + SpectrumManager.NuclideLibrary.get_sorted_nuclide_names())
        self.nuclide_combo.currentTextChanged.connect(self.nuclide_changed)
        self.nuclide_combo.currentTextChanged.connect(self.reference_activity_changed)
    
        nuclide_row = 1        
        source_parameters_layout.addWidget(QLabel("Nuclide"), nuclide_row, 0)
        source_parameters_layout.addWidget(self.nuclide_combo, nuclide_row, 1)
        
        # Half life
        self.half_life_spin = QDoubleSpinBox(decimals=2, minimum=0, maximum=1e14)
        source_parameters_layout.addWidget(QLabel("Half Life", alignment=Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), nuclide_row, 2)
        source_parameters_layout.addWidget(self.half_life_spin, nuclide_row, 3)
        self.half_life_unit_combo = QComboBox()
        self.half_life_unit_combo.addItems([i.name for i in TimeUnits])
        source_parameters_layout.addWidget(self.half_life_unit_combo, nuclide_row, 4)
        
        self.activity_spin = QDoubleSpinBox(minimum=0, maximum=1e9, decimals=4)
        self.activity_spin.valueChanged.connect(self.reference_activity_changed)
        self.activity_spin.setSizePolicy(QSizePolicy.Policy.Maximum, QSizePolicy.Policy.Fixed)
        self.activity_uncert_spin = QDoubleSpinBox(minimum=0, maximum=1e9, decimals=4, prefix="± ")
        self.activity_uncert_spin.valueChanged.connect(self.reference_activity_changed)
        self.activity_unit_combo = QComboBox()
        self.activity_unit_combo.addItems([i.name for i in ActivityUnits])
        
        activity_row = 2
        source_parameters_layout.addWidget(QLabel("Ref Activity"), activity_row, 0)
        source_parameters_layout.addWidget(self.activity_spin, activity_row, 1)
        source_parameters_layout.addWidget(self.activity_uncert_spin, activity_row, 2)
        source_parameters_layout.addWidget(self.activity_unit_combo, activity_row, 3)
        
        
        self.spectrum_combo = QComboBox()
        self.spectrum_combo.addItem("None")
        self.spectrum_combo.currentTextChanged.connect(self.spectrum_changed)
        self.measurement_time_line = QLineEdit()
        self.measurement_time_line.setReadOnly(True)
        
        spectrum_row = 3
        source_parameters_layout.addWidget(QLabel("Spectrum"), spectrum_row, 0)
        source_parameters_layout.addWidget(self.spectrum_combo, spectrum_row, 1)
        source_parameters_layout.addWidget(QLabel("Measurement Duration"), spectrum_row, 2)
        source_parameters_layout.addWidget(self.measurement_time_line, spectrum_row, 3)
           
        # Dates
        min_datetime = QDateTime(1900, 1, 1, 0, 0, 0)

        self.calibration_dateTimeEdit = QDateTimeEdit()
        self.start_dateTimeEdit = QDateTimeEdit()
        self.end_dateTimeEdit = QDateTimeEdit()

        for date_edit in (
            self.calibration_dateTimeEdit,
            self.start_dateTimeEdit,
            self.end_dateTimeEdit,
        ):
            date_edit.setMinimumDateTime(min_datetime)
            date_edit.setDisplayFormat("yyyy-MM-dd HH:mm:ss")
            date_edit.timeChanged.connect(self.reference_activity_changed)
            date_edit.dateChanged.connect(self.reference_activity_changed)
            date_edit.timeChanged.connect(self.measurement_time_changed)

        dates_row = 4
        source_parameters_layout.addWidget(QLabel("Calibration"), dates_row, 0)
        source_parameters_layout.addWidget(self.calibration_dateTimeEdit, dates_row, 1)

        source_parameters_layout.addWidget(QLabel("Measurement Start", alignment=Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), dates_row, 2)
        source_parameters_layout.addWidget(self.start_dateTimeEdit, dates_row, 3)

        source_parameters_layout.addWidget(QLabel("Measurement End", alignment=Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), dates_row, 4)
        source_parameters_layout.addWidget(self.end_dateTimeEdit, dates_row, 5)
        
        # Corrected activity
        self.activity_at_start_line = QLineEdit()
        self.activity_at_start_line.setReadOnly(True)
        self.activity_at_end_line = QLineEdit()
        self.activity_at_end_line.setReadOnly(True)
        self.correct_during_meas_check = QCheckBox("Decay Correction")
        self.correct_during_meas_check.setChecked(True)
        self.correct_during_meas_check.checkStateChanged.connect(self.reference_activity_changed)

        corrected_activity_row = 5
        source_parameters_layout.addWidget(QLabel("Activity Start", alignment=Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), corrected_activity_row, 2)
        source_parameters_layout.addWidget(self.activity_at_start_line, corrected_activity_row, 3)

        source_parameters_layout.addWidget(QLabel("Activity End", alignment=Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter), corrected_activity_row, 4)
        source_parameters_layout.addWidget(self.activity_at_end_line, corrected_activity_row, 5)

        source_parameters_layout.addWidget(self.correct_during_meas_check, corrected_activity_row, 1)
    
        solid_angle_row = 6
        self.distance_spin = QDoubleSpinBox(minimum=0, maximum=1e6, decimals=2, suffix=" cm") 
        self.distance_spin.valueChanged.connect(self.distance_changed)
        self.distance_uncert_spin = QDoubleSpinBox(minimum=0, maximum=1e6, decimals=2, suffix=" cm", prefix="± ") 
        self.distance_uncert_spin.valueChanged.connect(self.distance_changed)
        self.solid_angle_spin = QDoubleSpinBox(minimum=0, maximum=1e6, decimals=7, suffix=" sr")
        self.solid_angle_spin.setEnabled(False) 
        self.solid_angle_uncert_spin = QDoubleSpinBox(minimum=0, maximum=1e6, decimals=7, suffix=" sr", prefix="± ") 
        self.solid_angle_uncert_spin.setEnabled(False)
        
        source_parameters_layout.addWidget(QLabel("Distance"), solid_angle_row, 0)
        source_parameters_layout.addWidget(self.distance_spin, solid_angle_row, 1)
        source_parameters_layout.addWidget(self.distance_uncert_spin, solid_angle_row, 2)
        self.custom_solid_angle_check = QCheckBox("Custom Ω")
        self.custom_solid_angle_check.toggled.connect(self.custom_solid_angle_changed)
        self.custom_solid_angle_check.setLayoutDirection(Qt.LayoutDirection.RightToLeft)
        source_parameters_layout.addWidget(self.custom_solid_angle_check, solid_angle_row, 3)
        source_parameters_layout.addWidget(self.solid_angle_spin, solid_angle_row, 4)
        source_parameters_layout.addWidget(self.solid_angle_uncert_spin, solid_angle_row, 5)

        self.setLayout(source_parameters_layout)

    def nuclide_changed(self, current_text: str):
        if current_text == "Other":
            self.half_life_spin.setValue(0)
            return
        
        nuclide = SpectrumManager.NuclideLibrary.get_nuclide(current_text)
        
        value, unit = format_duration(nuclide.half_life_s[0])
        self.half_life_spin.setValue(value)
        self.half_life_unit_combo.setCurrentText(unit.name)
        
        
    def reference_activity_changed(self):
        if not self.half_life_spin.value():
            self.activity_at_start_line.clear()
            self.activity_at_end_line.clear()
            return
        A0 = self.activity_spin.value() * ActivityUnits[self.activity_unit_combo.currentText()].value
        A0_u = self.activity_uncert_spin.value() * ActivityUnits[self.activity_unit_combo.currentText()].value
        
        calibration_time = self.calibration_dateTimeEdit.time()
        start_time = self.start_dateTimeEdit.time()
        end_time = self.end_dateTimeEdit.time()
        

        
        hl = self.half_life_spin.value() * TimeUnits[self.half_life_unit_combo.currentText()].value
        calibration_time = self.calibration_dateTimeEdit.dateTime()
        start_time = self.start_dateTimeEdit.dateTime()
        end_time = self.end_dateTimeEdit.dateTime()

        decay_time_since_calib_s = calibration_time.secsTo(start_time)
        decay_time_since_start_s = start_time.secsTo(end_time)

        A_start = ufloat(A0, A0_u) * math.exp(-np.log(2) / hl * decay_time_since_calib_s)

        formatted_A_start, unit = format_activity(A_start)
        self.activity_at_start_line.setText(f"{round(formatted_A_start.n, 4)}±{round(formatted_A_start.s, 4)} {unit.name}")
        
        if self.correct_during_meas_check.isChecked():
            A_end = A_start * math.exp(-np.log(2) / hl * decay_time_since_start_s)
            
            formatted_A_end, unit = format_activity(A_end)
            self.activity_at_end_line.setText(f"{round(formatted_A_end.n, 4)}±{round(formatted_A_end.s, 4)} {unit.name}")
        else:
            self.activity_at_end_line.setText(f"{round(formatted_A_start.n, 4)}±{round(formatted_A_start.s, 4)} {unit.name}")

    def distance_changed(self):
        if self.parent_window.detector_area == 0 or self.distance_spin.value() == 0 or self.custom_solid_angle_check.isChecked():
            return
        
        distance = ufloat(self.distance_spin.value(), self.distance_uncert_spin.value())
        area = ufloat(self.parent_window.detector_area.value(), self.parent_window.detector_area_unc.value())
        
        solid_angle = area / distance ** 2
        
        self.solid_angle_spin.setValue(solid_angle.n)
        self.solid_angle_uncert_spin.setValue(solid_angle.s)
        
    def custom_solid_angle_changed(self, state: bool):
        state = bool(state)
        self.solid_angle_uncert_spin.setEnabled(state)
        self.solid_angle_spin.setEnabled(state)
        
    def measurement_time_changed(self):
        start_time = self.start_dateTimeEdit.dateTime()
        end_time = self.end_dateTimeEdit.dateTime()

        decay_time_since_start_s = start_time.secsTo(end_time)
        self.measurement_time_line.setText(str(timedelta(seconds=decay_time_since_start_s)))
        
    def spectrum_changed(self, current_text: str):
        spect = SpectrumManager.get_spectrum(current_text)
        if spect is None:
            return
        
        fg = spect.foreground
        
        start_date = fg.start_date
        end_date = fg.end_date
        duration = fg.real_time if fg.real_time else fg.live_time

        if not duration:
            return
        
        if start_date is None and end_date is None:
            return
        
        if start_date is None and end_date is not None:
            start_date = end_date - timedelta(seconds=duration)
        
        self.start_dateTimeEdit.setDateTime(datetime_to_qdatetime(start_date))
        
        if start_date is not None and end_date is None:
            end_date = start_date + timedelta(seconds=duration)
        
        self.end_dateTimeEdit.setDateTime(datetime_to_qdatetime(end_date))
        

    def to_source(
        self,
    ) -> Source:

        half_life = (
            self.half_life_spin.value()
            * TimeUnits[self.half_life_unit_combo.currentText()].value
        )

        activity = (
            self.activity_spin.value()
            * ActivityUnits[self.activity_unit_combo.currentText()].value
        )

        activity_uncert = (
            self.activity_uncert_spin.value()
            * ActivityUnits[self.activity_unit_combo.currentText()].value
        )

        return Source(
            name=self.source_name,
            nuclide=self.nuclide_combo.currentText(),
            half_life_s=half_life,
            ref_activity_Bq=activity,
            ref_activity_uncert_Bq=activity_uncert,
            calibration_time=qdatetime_to_datetime(
                self.calibration_dateTimeEdit.dateTime()
            ),
            measurement_start=qdatetime_to_datetime(
                self.start_dateTimeEdit.dateTime()
            ),
            measurement_end=qdatetime_to_datetime(
                self.end_dateTimeEdit.dateTime()
            ),
            solid_angle=self.solid_angle_spin.value(),
            solid_angle_uncert=self.solid_angle_uncert_spin.value(),
            measurement_decay_compensate=self.correct_during_meas_check.isChecked(),
            distance_cm=self.distance_spin.value(),
            distance_uncert_cm=self.distance_uncert_spin.value(),
            custom_sold_angle=self.custom_solid_angle_check.isChecked(),
            spectrum=self.spectrum_combo.currentText()
        )

    def from_source(self, source: Source):

        self.source_name = source.name

        self.nuclide_combo.setCurrentText(source.nuclide)

        # Half life
        half_life, unit = format_duration(source.half_life_s)
        self.half_life_spin.setValue(half_life)
        self.half_life_unit_combo.setCurrentText(unit.name)

        # Reference activity
        activity = ufloat(
            source.ref_activity_Bq,
            source.ref_activity_uncert_Bq,
        )

        activity, unit = format_activity(activity)
        self.activity_spin.setValue(activity.n)
        self.activity_uncert_spin.setValue(activity.s)
        self.activity_unit_combo.setCurrentText(unit.name)

        # Dates
        self.calibration_dateTimeEdit.setDateTime(
            datetime_to_qdatetime(source.calibration_time)
        )

        self.start_dateTimeEdit.setDateTime(
            datetime_to_qdatetime(source.measurement_start)
        )

        self.end_dateTimeEdit.setDateTime(
            datetime_to_qdatetime(source.measurement_end)
        )
        
        self.solid_angle_spin.setValue(source.solid_angle)
        self.solid_angle_uncert_spin.setValue(source.solid_angle_uncert)
        self.distance_spin.setValue(source.distance_cm)
        self.distance_uncert_spin.setValue(source.distance_uncert_cm)
        self.custom_solid_angle_check.setChecked(source.custom_sold_angle)
        self.spectrum_combo.setCurrentText(source.spectrum)


        # Decay correction
        self.correct_during_meas_check.setChecked(
            source.measurement_decay_compensate
        )

        self.reference_activity_changed()

