from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass
from datetime import datetime
from PySide6.QtWidgets import (
    QDialog,
    QWidget,
    QHBoxLayout,
    QVBoxLayout,
    QFormLayout,
    QTableWidget,
    QTableWidgetItem,
    QCheckBox,
    QPushButton,
    QAbstractItemView,
    QDoubleSpinBox,
    QSpinBox,
    QComboBox,
    QMessageBox,
    QLineEdit
)
from PySide6.QtCore import Qt
import pyqtgraph as pg
import numpy as np

from luracs.core import SpectrumManager
from luracs.utils.numerics import calibrate_x_axis


class CalibrationWindow(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Calibration Window")
        self.resize(1000, 700)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- Store Calculation Results ---
        self.current_new_coeff = None
        self.current_ref_points = None

        # --- Spectrum Combo Box ---
        self.combo_spectrum = QComboBox()
        self.combo_spectrum.currentTextChanged.connect(self.set_table)

        form.addRow("Spectrum", self.combo_spectrum)

        # --- ROI Table ---
        titles = ["", "ROI", "Nuclide", "Centroid", "Reference", "Difference"]
        self.roi_table = QTableWidget(columnCount=len(titles))
        self.roi_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.roi_table.setHorizontalHeaderLabels(titles)

        form.addRow("ROI Peaks", self.roi_table)

        # --- Polynomial degree selection ---
        self.spin_poly_degree = QSpinBox()
        self.spin_poly_degree.setRange(0, 3)
        self.spin_poly_degree.setValue(2)
        self.spin_poly_degree.valueChanged.connect(self.poly_degree_changed)

        form.addRow("Degree", self.spin_poly_degree)

        # --- Polynomial parameters ---
        parameter_layout = QFormLayout()
        parameter_layout.setContentsMargins(1, 0, 1, 0)
        self.poly_spin_list = [QDoubleSpinBox() for i in range(4)]

        for i, sb in enumerate(self.poly_spin_list):
            sb.setDecimals(6)
            parameter_layout.addRow(f"a{i} =", sb)
            if i > self.spin_poly_degree.value():
                sb.setEnabled(False)

        form.addRow("", parameter_layout)
        
        # --- Instrument attached to spectrum ---
        self.line_detected_instrument = QLineEdit()
        self.line_detected_instrument.setReadOnly(True)
        form.addRow("Instrument", self.line_detected_instrument)

        # --- Calculations ---
        btn_calculate = QPushButton("Calculate")
        btn_calculate.clicked.connect(self.calculate)

        form.addRow("", btn_calculate)

        self.calibration_plot = pg.PlotWidget()
        self.calibration_plot.setMaximumHeight(250)
        self.calibration_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.calibration_plot.getPlotItem().setLabel("bottom", "Channels")
        self.calibration_plot.getPlotItem().setLabel("left", "Energy [keV]")
        self.calibration_plot.setLimits(
            xMin=0,
            yMin=0,
        )

        form.addRow("Plot", self.calibration_plot)

        main_layout.addLayout(form)

        bottom_buttons = QHBoxLayout()
        self.assign_to_instrument_btn = QPushButton("Assign to instrument")
        self.assign_to_instrument_btn.clicked.connect(self.assign_to_instrument)

        self.assign_to_spectrum_btn = QPushButton("Assign to spectrum")
        self.assign_to_spectrum_btn.clicked.connect(self.assign_to_spectrum)
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.close)

        bottom_buttons.addStretch()
        bottom_buttons.addWidget(self.assign_to_spectrum_btn)
        bottom_buttons.addWidget(self.assign_to_instrument_btn)
        bottom_buttons.addWidget(close_btn)

        main_layout.addLayout(bottom_buttons)

    def show(self):
        self.set_spectrum_combo()
        self.set_table(self.combo_spectrum.currentText())

        super().show()

    def poly_degree_changed(self, value: int):
        for i, sb in enumerate(self.poly_spin_list):
            if i <= value:
                sb.setEnabled(True)
            else:
                sb.setEnabled(False)

    def set_spectrum_combo(self):
        self.combo_spectrum.clear()
        self.combo_spectrum.addItems(SpectrumManager.get_spectra_dict().keys())

    def set_table(self, spectrum_name: str):
        self.current_new_coeff = None
        if not spectrum_name:
            return
        table = self.roi_table
        table.setRowCount(0)  # clear properly

        rois = SpectrumManager.ROIManager.get_data_from_spectrum(
            self.combo_spectrum.currentText()
        )
        
        spectrum = SpectrumManager.get_spectrum(self.combo_spectrum.currentText())
        instrument = spectrum.instrument
        if instrument is not None:
            self.line_detected_instrument.setText(instrument.name)

        skipped = 0
        for row, roi in enumerate(rois.values()):
            if not roi.fit:
                skipped += 1
                continue

            row -= skipped
            table.insertRow(row)

            # --- Column 0: checkbox ---
            check_box = QCheckBox()
            check_box.setChecked(True)

            container = QWidget()
            layout = QHBoxLayout(container)
            layout.addWidget(check_box)
            layout.setAlignment(check_box, Qt.AlignCenter)
            layout.setContentsMargins(0, 0, 0, 0)

            table.setCellWidget(row, 0, container)

            # --- Column 1: alias (always shown) ---
            roi_item = QTableWidgetItem(str(roi.alias))
            roi_item.setData(Qt.UserRole, roi)
            table.setItem(row, 1, roi_item)

            # --- Column 2: Nuclide ---
            table.setItem(
                row,
                2,
                QTableWidgetItem(
                    str(roi.emission.parent_nuclide if roi.emission else "None")
                ),
            )

            # --- Column 3: Centroid ---
            table.setItem(row, 3, QTableWidgetItem(str(round(roi.fit.mu, 2))))

            # --- Column 4: Ref photopeak ---
            ref_box = QDoubleSpinBox()
            ref_box.setRange(0, 1e5)
            ref_box.setValue(roi.emission.energy_keV if roi.emission else 0)
            ref_box.valueChanged.connect(
                lambda _, row=row: self.recalculate_difference(row)
            )
            table.setCellWidget(row, 4, ref_box)

            # --- Column 5: Diff ---
            table.setItem(
                row,
                5,
                QTableWidgetItem(
                    str(round(ref_box.value() - roi.fit.mu, 2))
                    if roi.emission
                    else None
                ),
            )

        calib_coeff = SpectrumManager.get_spectrum(
            spectrum_name
        ).calibration_coefficients

        for i, display in enumerate(self.poly_spin_list):
            if i >= len(calib_coeff):
                break
            display.setValue(calib_coeff[-(i + 1)])

    def calculate(self):
        spectrum = SpectrumManager.get_spectrum(self.combo_spectrum.currentText())

        centroids = []
        reference_energies = []

        for i in range(self.roi_table.rowCount()):
            if (
                not self.roi_table.cellWidget(i, 0)
                .layout()
                .itemAt(0)
                .widget()
                .isChecked()
            ):
                continue

            # Column 3 = centroid item
            centroid_item = self.roi_table.item(i, 3)

            if centroid_item is None:
                continue

            centroid = float(centroid_item.text())

            # Column 4 = QDoubleSpinBox
            ref_box = self.roi_table.cellWidget(i, 4)

            reference_energy = ref_box.value()
            if reference_energy == 0:
                continue

            centroids.append(centroid)
            reference_energies.append(reference_energy)

        if len(centroids) == 0:
            return
        new_x_axis, new_coeff, ref_points = calibrate_x_axis(
            centroids,
            reference_energies,
            self.spin_poly_degree.value(),
            spectrum.channels,
            spectrum.x_axis if spectrum.calibrated else None,
            return_reference_points=True,
        )

        self.calibration_plot.clear()
        self.calibration_plot.getPlotItem().plot(np.arange(len(new_x_axis)), new_x_axis)
        self.calibration_plot.plotItem.plot(*ref_points, pen=None, symbol="o")

        for i, display in enumerate(self.poly_spin_list):
            if i >= len(new_coeff):
                display.setValue(0)
                continue
            display.setValue(new_coeff[-(i + 1)])

        self.current_new_coeff = new_coeff
        self.current_ref_points = ref_points

    def assign_to_spectrum(self):
        if self.current_new_coeff is None:
            QMessageBox.warning(
                self, "Error", "No new calibration points calculated to assign"
            )
            return

        spectrum_name = self.combo_spectrum.currentText()
        SpectrumManager.calibrate_spectrum(spectrum_name, self.current_new_coeff)

        self.set_table(spectrum_name)

    def assign_to_instrument(self):
        if self.current_new_coeff is None:
            QMessageBox.warning(
                self, "Error", "No new calibration points calculated to assign"
            )
            return

        spectrum_name = self.combo_spectrum.currentText()
        instrument = SpectrumManager.get_spectrum(spectrum_name).instrument
        if instrument is None:
            QMessageBox.warning(
                self, "Error", "No instrument attached"
            )
            return

        SpectrumManager.UniqueInstrumentLibrary.update_instrument_data(SpectrumManager.UniqueInstrumentLibrary.get_key_from_attr("name", instrument.name),
              {
                "calibration_poly_order": 1,
                "calibration_coefficients": list(self.current_new_coeff),
                "calibration_channel_points": list(self.current_ref_points[0]),
                "calibration_energy_points": list(self.current_ref_points[1]),
                "calibration_date": datetime.now()
              }
        )
        self.set_table(spectrum_name)

    def recalculate_difference(self, row_index: int):
        centre_value = float(self.roi_table.item(row_index, 3).text())
        ref_box = self.roi_table.cellWidget(row_index, 4)
        self.roi_table.setItem(
            row_index,
            5,
            QTableWidgetItem(str(round(ref_box.value() - centre_value, 2))),
        )


if __name__ == "__main__":
    import sys
    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = CalibrationWindow()
    window.resize(800, 500)
    window.show()

    sys.exit(app.exec())
