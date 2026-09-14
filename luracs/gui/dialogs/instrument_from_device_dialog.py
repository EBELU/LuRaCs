from PySide6.QtWidgets import (
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QLineEdit,
    QVBoxLayout,
    QComboBox,
    QCheckBox
)

from luracs.core import SpectrumManager
from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument


class NewInstrumentDialog(QDialog):
    def __init__(self, name="", title="New Instrument", parent=None):
        super().__init__(parent=parent)
        
        self.name=name

        self.setWindowTitle(title)
        self.setMinimumWidth(200)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- Name field ---
        self.name_edit = QLineEdit()
        self.name_edit.setText(name)
        self.name_edit.setEnabled(False)
        form.addRow("Name", self.name_edit)
        
        self.generic_instrument_combo = QComboBox()
        for instrument in SpectrumManager.GenericInstrumentLibrary.instrument_registry.values():
            self.generic_instrument_combo.addItem(instrument.model, instrument)

        form.addRow("Generic Instruments", self.generic_instrument_combo)
        
        self.save_calibration_check = QCheckBox("Save Current Calibration")
        form.addRow("", self.save_calibration_check)
        
        main_layout.addLayout(form)

        # --- Buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)

        self.ok_button = buttons.button(QDialogButtonBox.Ok)
        self.ok_button.setEnabled(bool(name))  # disable if empty initially

        buttons.accepted.connect(self.accept)
        buttons.accepted.connect(self.on_accept)
        buttons.rejected.connect(self.reject)

        # Enable OK only when text is not empty
        self.name_edit.textChanged.connect(
            lambda text: self.ok_button.setEnabled(bool(text.strip()))
        )

        main_layout.addWidget(buttons)

    def on_accept(self):
        generic_instrument: GenericInstrument = self.generic_instrument_combo.currentData()
        spectrum = SpectrumManager.get_spectrum(self.name)
        
        calib_dict = {}
        if self.save_calibration_check.isChecked() and spectrum is not None and spectrum.calibrated:
                calib_dict["calibration_coefficients"] = spectrum.calibration_coefficients
                
        new_instrument = UniqueInstrument(
            name=self.name, 
            **generic_instrument.get_copy().__dict__, 
            **calib_dict
            )
        
        SpectrumManager.UniqueInstrumentLibrary.add_instrument(new_instrument)
        SpectrumManager.set_spectrum_instrument(self.name, SpectrumManager.UniqueInstrumentLibrary.get_instrument_by_name(self.name))
        

        

