from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.clients import DeviceWrapper, WrappedStatusPackage
    from luracs.containers.instrument_classes import UniqueInstrument

from pathlib import Path

from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from luracs.clients import SupportedSettings
from luracs.core import SpectrumManager, RunManager


class DeviceSettingsDialog(QDialog):
    sigCalibrationSetter = Signal(list)
    
    def __init__(self, device_wrapper: DeviceWrapper, parent=None):
        super().__init__(parent=parent)
        self.device_wrapper = device_wrapper
        self.current_coeffs: list | None = None

        self.mca_initialized = False
        
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
        
        # --- Calibration ---
        if SupportedSettings.CALIBRATION in self.device_wrapper.get_supported_settings():
            calibration_tab = self.build_calibration_tab()
            has_calib_settings = True
        else:
            calibration_tab = QWidget()
            has_calib_settings = False
        
        tab_layout.addTab(calibration_tab, "Calibration")
        tab_layout.setTabToolTip(
            0, "Basic settings like device calibration and alarm limits"
        )
        tab_layout.setTabEnabled(0, has_calib_settings)
        
        # --- High voltage and amplifiers ---
        if SupportedSettings.HV_AND_AMP in self.device_wrapper.get_supported_settings():
            hv_amp_widget = self.build_hv_and_amp_tab()
            has_hv_settings = True
        else:
            hv_amp_widget = QWidget()
            has_hv_settings = False
        
        tab_layout.addTab(hv_amp_widget, "MCA")
        tab_layout.setTabToolTip(
            1, "High Voltage and Amplifier settings"
            )
        tab_layout.setTabEnabled(1, has_hv_settings)

        if has_hv_settings and not has_calib_settings:
            tab_layout.setCurrentIndex(1)

        # --- Bottom buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Close)
        buttons.rejected.connect(self.close)

        main_layout.addWidget(buttons)
        
    def build_calibration_tab(self) -> QWidget:

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
        
        return calibration_settings_widget
    
    def build_hv_and_amp_tab(self) -> QWidget:
        main_widget = QWidget()
        form = QFormLayout(main_widget)
        
        
        hv_on_off_row = QHBoxLayout()
        self.hv_on_btn = QPushButton("Enable")
        self.hv_off_btn = QPushButton("Disable")
        hv_on_off_row.addWidget(self.hv_on_btn)
        hv_on_off_row.addWidget(self.hv_off_btn)
        
        form.addRow("High Voltage Switch", hv_on_off_row)
        
        hv_row = QHBoxLayout()
        
        self.hv_requested_spin = QDoubleSpinBox(suffix=" V")
        self.hv_requested_spin.setRange(0, 1e6)
        self.hv_requested_spin.setMaximumWidth(150)
        self.hv_acutal_line = QLineEdit()
        self.hv_acutal_line.setMaximumWidth(150)
        self.hv_acutal_line.setReadOnly(True)
        self.hv_apply_btn = QPushButton("Apply")
        
        hv_row.addWidget(self.hv_requested_spin)
        hv_row.addWidget(QLabel("Actual:"))
        hv_row.addWidget(self.hv_acutal_line)
        hv_row.addWidget(self.hv_apply_btn)
        hv_row.addStretch()
        
        form.addRow("High Voltage", hv_row)

        # Fine Gain
        fine_gain_row = QHBoxLayout()

        self.fine_gain_requested_spin = QDoubleSpinBox()
        self.fine_gain_requested_spin.setRange(0, 1e6)
        self.fine_gain_requested_spin.setMaximumWidth(150)

        self.fine_gain_actual_line = QLineEdit()
        self.fine_gain_actual_line.setReadOnly(True)
        self.fine_gain_actual_line.setMaximumWidth(150)

        self.fine_gain_apply_btn = QPushButton("Apply")

        fine_gain_row.addWidget(self.fine_gain_requested_spin)
        fine_gain_row.addWidget(QLabel("Actual:"))
        fine_gain_row.addWidget(self.fine_gain_actual_line)
        fine_gain_row.addWidget(self.fine_gain_apply_btn)
        fine_gain_row.addStretch()

        form.addRow("Fine Gain", fine_gain_row)


        # Lower Level Discriminator
        lld_row = QHBoxLayout()

        self.lld_requested_spin = QDoubleSpinBox()
        self.lld_requested_spin.setRange(0, 1e6)
        self.lld_requested_spin.setMaximumWidth(150)

        self.lld_actual_line = QLineEdit()
        self.lld_actual_line.setReadOnly(True)
        self.lld_actual_line.setMaximumWidth(150)

        self.lld_apply_btn = QPushButton("Apply")

        lld_row.addWidget(self.lld_requested_spin)
        lld_row.addWidget(QLabel("Actual:"))
        lld_row.addWidget(self.lld_actual_line)
        lld_row.addWidget(self.lld_apply_btn)
        lld_row.addStretch()

        form.addRow("LLD", lld_row)


        # Upper Level Discriminator
        uld_row = QHBoxLayout()

        self.uld_requested_spin = QDoubleSpinBox()
        self.uld_requested_spin.setRange(0, 1e6)
        self.uld_requested_spin.setMaximumWidth(150)

        self.uld_actual_line = QLineEdit()
        self.uld_actual_line.setReadOnly(True)
        self.uld_actual_line.setMaximumWidth(150)

        self.uld_apply_btn = QPushButton("Apply")

        uld_row.addWidget(self.uld_requested_spin)
        uld_row.addWidget(QLabel("Actual:"))
        uld_row.addWidget(self.uld_actual_line)
        uld_row.addWidget(self.uld_apply_btn)
        uld_row.addStretch()

        form.addRow("ULD", uld_row)


        RunManager.Signals.statusUpdated.connect(self.mca_readback_setter)
        RunManager.Signals.statusUpdated.connect(self.mca_readback_initializer)
        
        self.hv_on_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_hv_enabled(True)))
        self.hv_off_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_hv_enabled(False)))
        
        self.hv_apply_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_hv(self.hv_requested_spin.value())))
        self.uld_apply_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_uld(self.uld_requested_spin.value())))
        self.lld_apply_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_lld(self.lld_requested_spin.value())))
        self.fine_gain_apply_btn.clicked.connect(lambda :RunManager.submit_to_thread(self.device_wrapper.set_fine_gain(self.fine_gain_requested_spin.value())))
        
        return main_widget
    
    def mca_readback_setter(self, name: str, status: WrappedStatusPackage):
        if name == self.device_wrapper.name:
            self.hv_acutal_line.setText(f"{round(status.voltage, 1)} V")
            self.lld_actual_line.setText(f"{round(status.lower_level_discriminator, 4)}")
            self.uld_actual_line.setText(f"{round(status.upper_level_discriminator, 4)}")
            self.fine_gain_actual_line.setText(f"{round(status.fine_gain, 4)}")
            
    def mca_readback_initializer(self, name: str, status: WrappedStatusPackage):
        if name == self.device_wrapper.name and not self.mca_initialized:
            self.hv_requested_spin.setValue(status.desired_voltage)
            self.lld_requested_spin.setValue(status.lower_level_discriminator)
            self.uld_requested_spin.setValue(status.upper_level_discriminator)
            self.fine_gain_requested_spin.setValue(status.fine_gain)
            
            RunManager.Signals.statusUpdated.disconnect(self.mca_readback_initializer)
            
            


    def set_calibration_list_from_instrument(self):
        instr: UniqueInstrument = self.combo_instruments.currentData()
        coeffs = instr.calibration_coefficients
        if coeffs is not None:
            self.set_spinbox_clibration_values(reversed(coeffs))
        else:
            self.set_spinbox_clibration_values(coeffs)

    def set_calibration_list_from_spectrum(self):
        connection = None
        for spect in SpectrumManager.spectrum_registry.values():
            if spect.connection == self.device_wrapper.name:
                connection = spect
                break

        if connection is None or connection.calibration_coefficients is None:
            self.set_spinbox_clibration_values(connection.calibration_coefficients)
            return

        self.set_spinbox_clibration_values(reversed(connection.calibration_coefficients))
        self.current_coeffs = connection.calibration_coefficients

    def set_spinbox_clibration_values(self, coeffs: list):
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
            self.client.set_calibration(
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
            self.set_spinbox_clibration_values(reversed(self.current_coeffs))
        else:
            self.set_spinbox_clibration_values(self.current_coeffs)
