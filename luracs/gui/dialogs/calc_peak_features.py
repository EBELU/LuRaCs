import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLineEdit,
    QVBoxLayout,
)


class PeakFeaturesDialog(QDialog):
    sigLineUpdated = Signal(bool, str, float, object) # Show state, label, energy, Qcolor
    def __init__(self, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Peak Features")

        main_layout = QVBoxLayout(self)

        form = QFormLayout()

        self.input_E_spin = QDoubleSpinBox()
        self.input_E_spin.setRange(0, 1e4)
        self.input_E_spin.setDecimals(2)
        self.input_E_spin.setSuffix(" keV")
        self.input_E_spin.valueChanged.connect(self.calculate)

        form.addRow("Input Energy:", self.input_E_spin)
        main_layout.addLayout(form)

        results_form = QFormLayout()

        self.result_rows: dict[str, QHBoxLayout] = {}

        for feature in [
            "Backscatter",
            "Compton Edge",
            "Photopeak",
            "1st Escape Peak",
            "2nd Escape Peak",
        ]:
            row_layout = QHBoxLayout()

            check = QCheckBox()
            check.setChecked(True)

            energy_line = QLineEdit()
            energy_line.setReadOnly(True)
            
            color_btn = pg.ColorButton()

            row_layout.addWidget(check)
            row_layout.addWidget(energy_line)
            row_layout.addWidget(color_btn)

            self.result_rows[feature] = row_layout

            results_form.addRow(feature, row_layout)

        main_layout.addLayout(results_form)

        button_box = QDialogButtonBox(
            QDialogButtonBox.Apply
            | QDialogButtonBox.Ok
            | QDialogButtonBox.Cancel
        )

        # Use "Accept" instead of the platform's default "OK".
        button_box.button(QDialogButtonBox.Ok)

        button_box.clicked.connect(self._button_clicked)

        main_layout.addWidget(button_box)
        
    def calculate(self, value: float):
        photopeak = value

        backscatter = photopeak / (1.0 + 2.0 * photopeak / 511.)
        compton_edge = photopeak-backscatter
        
        self.result_rows["Photopeak"].itemAt(1).widget().setText(f"{photopeak:.2f} keV")
        self.result_rows["Backscatter"].itemAt(1).widget().setText(f"{backscatter:.2f} keV")
        self.result_rows["Compton Edge"].itemAt(1).widget().setText(f"{compton_edge:.2f} keV")
        
        first_escape = second_escape = np.nan
        if value > 1022.:
            first_escape = value - 511.
            second_escape = value - 511.*2
            
        self.result_rows["1st Escape Peak"].itemAt(1).widget().setText(f"{first_escape:.2f} keV")
        self.result_rows["2nd Escape Peak"].itemAt(1).widget().setText(f"{second_escape:.2f} keV")

    def _button_clicked(self, button):
        role = self.sender().buttonRole(button)

        if role == QDialogButtonBox.ApplyRole:
            self.apply()

        elif role == QDialogButtonBox.AcceptRole:
            self.accept()

        elif role == QDialogButtonBox.RejectRole:
            self.reject()

    def apply(self):
        for layout in self.result_rows.values():
            check = layout.itemAt(0).widget().isChecked()
            value_text = layout.itemAt(1).widget().text().removesuffix(" keV")
            value = float(value_text) if value_text and value_text != "nan" else 0
            color = layout.itemAt(2).widget().color()
            
            if value:
            
                print(check, value, color)

            

    def accept(self):
        self.apply()
        super().accept()


if __name__ == "__main__":
    app = QApplication([])

    dialog = PeakFeaturesDialog()
    dialog.exec()
