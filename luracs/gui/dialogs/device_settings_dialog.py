from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.clients.DeviceWrappers import DeviceWrapper
    from luracs.containers.instrument_classes import UniqueInstrument

from PySide6.QtWidgets import (
    QVBoxLayout,
    QHBoxLayout,
    QFormLayout,
    QDialog,
    QPushButton,
    QDoubleSpinBox,
    QTabWidget,
    QWidget,
    QComboBox,
    QDialogButtonBox,
    QMessageBox,
)

from pathlib import Path
from luracs.core import SpectrumManager


class DeviceSettingsDialog(QDialog):
    def __init__(self, device_wrapper: DeviceWrapper, parent=None):
        super().__init__(parent=parent)
        self.device_wrapper = device_wrapper
        self.current_coeffs: list | None = None

        main_layout = QVBoxLayout(self)
        tab_layout = QTabWidget()

        self.combo_instruments = QComboBox()
        for (
            key,
            instr,
        ) in SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items():
            self.combo_instruments.addItem(Path(key).stem, instr)

        main_layout.addWidget(self.combo_instruments)
        main_layout.addWidget(tab_layout)

        # --- Basic Settings ---
        calibration_settings_widget = QWidget()
        calibration_settings_form = QFormLayout(calibration_settings_widget)

        btn_pull_calibration = QPushButton("Pull Calibration")
        btn_pull_calibration.clicked.connect(self.get_calibration)
        btn_push_calibration = QPushButton("Push Calibration")
        btn_push_calibration.clicked.connect(self.push_calibration)

        calib_btn_layout = QHBoxLayout()
        calib_btn_layout.addWidget(btn_pull_calibration)
        calib_btn_layout.addWidget(btn_push_calibration)

        calibration_settings_form.addRow("Device IO", calib_btn_layout)

        self.spinboxes_coeff = [
            QDoubleSpinBox(),
            QDoubleSpinBox(),
            QDoubleSpinBox(),
            QDoubleSpinBox(),
        ]

        spin_layout = QFormLayout()
        for i, dsb in enumerate(self.spinboxes_coeff):
            dsb.setRange(-1e6, 1e6)
            dsb.setDecimals(7)
            spin_layout.addRow(f"a{i}", dsb)

        nr_of_calibration_coefficients = getattr(
            self.device_wrapper, "calibration_polynomial_order", None
        )
        if nr_of_calibration_coefficients is not None:
            for i, dsb in enumerate(self.spinboxes_coeff):
                if i >= nr_of_calibration_coefficients:
                    dsb.setEnabled(False)

        calibration_settings_form.addRow("Coefficients", spin_layout)

        btn_get_from_spectrum = QPushButton("From Spectrum")
        btn_get_from_spectrum.clicked.connect(self.set_calibration_list_from_spectrum)
        btn_get_from_instrument = QPushButton("From Instrument")
        btn_get_from_instrument.clicked.connect(
            self.set_calibration_list_from_instrument
        )

        get_calib_btn_layout = QHBoxLayout()
        get_calib_btn_layout.addWidget(btn_get_from_spectrum)
        get_calib_btn_layout.addWidget(btn_get_from_instrument)

        calibration_settings_form.addRow("Get Coefficients", get_calib_btn_layout)

        tab_layout.addTab(calibration_settings_widget, "Calibration")
        tab_layout.setTabToolTip(
            0, "Basic settings like device calibration and alarm limits"
        )

        # --- Advanced Settings ---
        high_voltage_settings_widget = QWidget()
        high_voltage_settings_form = QFormLayout(high_voltage_settings_widget)

        tab_layout.addTab(high_voltage_settings_widget, "HV and Amp")
        tab_layout.setTabToolTip(1, "High Voltage and Amplifier settings")

        tab_layout.setTabEnabled(0, device_wrapper.has_calibration_settings)
        tab_layout.setTabEnabled(1, device_wrapper.has_hv_settings)

        buttons = QDialogButtonBox(QDialogButtonBox.Close)
        buttons.rejected.connect(self.close)

        main_layout.addWidget(buttons)

    def set_calibration_list_from_instrument(self):
        instr: UniqueInstrument = self.combo_instruments.currentData()
        coeffs = instr.calibration_coefficients
        if coeffs is not None:
            self.set_spinbox_values(reversed(coeffs))
        else:
            self.set_spinbox_values(coeffs)

    def set_calibration_list_from_spectrum(self):
        connection = None
        for spect in SpectrumManager.spectrum_registry.values():
            if spect.connection == self.device_wrapper.name:
                connection = spect
                break

        if connection is None or connection.calibration_coefficients is None:
            self.set_spinbox_values(connection.calibration_coefficients)
            return

        self.set_spinbox_values(reversed(connection.calibration_coefficients))
        self.current_coeffs = connection.calibration_coefficients

    def set_spinbox_values(self, coeffs: list):
        if coeffs is None:
            for dsb in self.spinboxes_coeff:
                dsb.setValue(0)
            return

        coeffs = list(coeffs)
        for i, dsb in enumerate(self.spinboxes_coeff):
            if i < len(coeffs):
                dsb.setValue(coeffs[i])
            else:
                dsb.setValue(0)

    def push_calibration(self):
        if self.current_coeffs is not None:
            return
        try:
            self.device_wrapper.set_calibration(
                [c for c in self.current_coeffs if c != 0]
            )
        except (AttributeError, NotImplementedError):
            QMessageBox.warning(
                self,
                "Error",
                f"'{self.device_wrapper.name}' does not implement a method for setting device calibration",
            )

    def get_calibration(self):
        try:
            self.current_coeffs = self.device_wrapper.get_calibration()
        except (AttributeError, NotImplementedError):
            QMessageBox.warning(
                self,
                "Error",
                f"'{self.device_wrapper.name}' does not implement a method for getting device calibration",
            )

        if self.current_coeffs is not None:
            self.set_spinbox_values(reversed(self.current_coeffs))
        else:
            self.set_spinbox_values(self.current_coeffs)
