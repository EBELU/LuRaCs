from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    pass

from PySide6.QtWidgets import (QDialog, 
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
                               QComboBox)
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg

from core import SpectrumManager

class CalibrationWindow(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Calibration Window")
        self.resize(1000, 700)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        self.layout = main_layout

        form = QFormLayout()
        form.setSpacing(9)
        
        self.combo_spectrum = QComboBox()

        form.addRow("Spectrum", self.combo_spectrum)
        
        titles = ["", "ROI", "Nuclide","Centroid", "Reference", "Difference"]
        self.roi_table = QTableWidget(columnCount = len(titles))
        self.roi_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.roi_table.setHorizontalHeaderLabels(titles)
        
        form.addRow("ROI Peaks", self.roi_table)
        
        spin_poly_degree = QSpinBox()
        spin_poly_degree.setRange(0, 3)
        spin_poly_degree.setValue(2)
        spin_poly_degree.valueChanged.connect(self.poly_degree_changed)
        form.addRow("Degree", spin_poly_degree)
        
        parameter_layout = QFormLayout()
        parameter_layout.setContentsMargins(1,0,1,0)
        self.poly_spin_list = [QDoubleSpinBox() for i in range(4)]
        
        for i, sb in enumerate(self.poly_spin_list):
            parameter_layout.addRow(f"a{i} =", sb)
            if i > spin_poly_degree.value():
                sb.setEnabled(False)
        
        form.addRow("", parameter_layout)
        
        btn_calculate = QPushButton("Calculate")
        
        form.addRow("", btn_calculate)
        
        self.calibration_plot = pg.PlotWidget()
        self.calibration_plot.setMaximumHeight(250)
        self.calibration_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.calibration_plot.getPlotItem().setLabel("bottom", "Channels")
        self.calibration_plot.getPlotItem().setLabel("left", "Energy [keV]")
        
        form.addRow("Plot", self.calibration_plot)
        
        main_layout.addLayout(form)
        
        bottom_buttons = QHBoxLayout()
        self.assign_to_instrument_btn = QPushButton("Assign to instrument")
        self.assign_to_spectrum_btn = QPushButton("Assign to spectrum")
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
        if not spectrum_name:
            return
        table = self.roi_table
        table.setRowCount(0)  # clear properly

        rois = SpectrumManager.ROIManager.get_data_from_spectrum(self.combo_spectrum.currentText())

        for row, roi in enumerate(rois.values()):
            if not roi.fit:
                continue
            
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
            table.setItem(row, 2, QTableWidgetItem(str(roi.emission.parent_nuclide if roi.emission else "None")))
            
            # --- Column 3: Centroid ---
            table.setItem(row, 3, QTableWidgetItem(str(round(roi.fit.mu, 2))))
            # --- Column 4: Ref photopeak ---
            ref_box = QDoubleSpinBox()
            ref_box.setRange(0, 1e5)
            ref_box.setValue(roi.emission.energy_keV if roi.emission else 0)
            table.setCellWidget(row, 4, ref_box)
            # --- Column 5: Diff ---

            table.setItem(row, 5, QTableWidgetItem(str(round(ref_box.value() - roi.fit.mu, 2)) if roi.emission else None))
            
if __name__ == "__main__":
    import sys
    from PySide6.QtWidgets import QApplication
    app = QApplication.instance() or QApplication(sys.argv)

    window = CalibrationWindow()
    window.resize(800, 500)
    window.show()


    sys.exit(app.exec())