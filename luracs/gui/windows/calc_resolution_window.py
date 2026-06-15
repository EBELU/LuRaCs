from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    pass

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
                            QComboBox,
                            QGroupBox,
                            QRadioButton,
                            QButtonGroup,
                            QLineEdit)
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg
import numpy as np
import warnings
from datetime import datetime

from luracs.core import SpectrumManager
from luracs.utils.numerics import resolution, curve_fit, r_squared

class ResolutionWindow(QDialog):
    sigUpdateGenericInstrument = Signal(object, dict)
    sigUpdateUniqueInstrument = Signal(object, dict)
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Resolution Window")
        self.resize(1000, 700)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)
        
        self.current_energy_points = None
        self.current_fwhm_points = None
        self.current_resolution_points = None
        self.current_params = None
        
        # --- ROI data ---
        # Data from fitted ROIs in loaded spectra
        titles = ["", "ROI", "Spectrum", "Nuclide", "Energy", "FWHM", "Resolution"]
        widths = [15, 130, 130, 130, 130, 100, 100]
        
        self.data_table = QTableWidget(columnCount=len(titles))
        self.data_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.data_table.setHorizontalHeaderLabels(titles)
        for i, w in enumerate(widths):
            self.data_table.setColumnWidth(i, w)
        
        form.addRow("Peaks", self.data_table)
        
        self.instrument_combo = QComboBox()
        self.instrument_combo.addItem("None")
        form.addRow("Instrument", self.instrument_combo)
        
        # --- Results ---
        self.calculate_btn = QPushButton("Calculate")
        self.calculate_btn.clicked.connect(self.calculate)
        form.addRow("", self.calculate_btn)
        
        # --- Plot option buttons ---
        group_box = QGroupBox()

        # Layout with smaller margins
        layout = QHBoxLayout()
        layout.setContentsMargins(4, 0, 4, 0)   # left, top, right, bottom
        layout.setSpacing(2)

        # Radio buttons
        fwhm_to_E = QRadioButton("FWHM / E")
        R_to_E = QRadioButton("R / E")
        R_to_sqrtE = QRadioButton("R / (1 / √(E))")

        # Add to layout
        layout.addWidget(fwhm_to_E)
        layout.addWidget(R_to_E)
        layout.addWidget(R_to_sqrtE)
        group_box.setLayout(layout)

        # Exclusive button group
        self.button_group = QButtonGroup()
        self.button_group.addButton(fwhm_to_E, 0)
        self.button_group.addButton(R_to_E, 1)
        self.button_group.addButton(R_to_sqrtE, 2)
        self.button_group.idToggled.connect(self.plot_data)

        # Default selection
        fwhm_to_E.setChecked(True)
        
        form.addRow("Plot option", group_box)
        
        # --- Results ---
        self.result_line = QLineEdit()
        self.result_line.setReadOnly(True)
        
        form.addRow("Result fn", self.result_line)
        
        self.res_plot = pg.PlotWidget()
        self.res_plot.setMaximumHeight(250)
        self.res_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.res_plot.setLimits(
            xMin=0,
            xMax=3500,
            yMin=0,
        )

        form.addRow("Efficiency Plot", self.res_plot)

        main_layout.addLayout(form)
        
        # --- Bottom Buttons ---
        bottom_buttons = QHBoxLayout()
        assign_to_instrument_btn = QPushButton("Assign to instrument")
        assign_to_instrument_btn.clicked.connect(self.assign_to_instruments)
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.close)
        
        bottom_buttons.addStretch()
        bottom_buttons.addWidget(assign_to_instrument_btn)
        bottom_buttons.addWidget(close_btn)

        main_layout.addLayout(bottom_buttons)
        
        self.sigUpdateGenericInstrument.connect(SpectrumManager.GenericInstrumentLibrary.update_instrument_data)
        self.sigUpdateUniqueInstrument.connect(SpectrumManager.UniqueInstrumentLibrary.update_instrument_data)
    
    def show(self):
        self.set_instrument_combo()
        self.set_table()
        super().show()
        
    def set_instrument_combo(self):
        self.instrument_combo.clear()
        for key, i in sorted(SpectrumManager.GenericInstrumentLibrary.instrument_registry.items(), key=lambda x: x[1].model):
            self.instrument_combo.addItem(i.model, key)
        self.instrument_combo.insertSeparator(self.instrument_combo.count())
        for key, i in sorted(SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items(), key=lambda x: x[1].name):
            self.instrument_combo.addItem(i.name, key)
        
    def set_table(self):
        table = self.data_table
        table.setRowCount(0)
        
        rois = []
        for roi_key in SpectrumManager.ROIManager.roi_registry.keys():
            rois.extend(SpectrumManager.ROIManager.get_data_from_roi(roi_key).values())

        for row, roi in enumerate(sorted(rois, key = lambda r: r.fit.fwhm if r.fit else -1)):
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
            
            # --- Column 2: Spectrum ---
            table.setItem(row, 2, QTableWidgetItem(str(roi.meta.get("spectrum_name"))))
            
            # --- Column 3: Nuclide ---
            table.setItem(row, 3, QTableWidgetItem(str(roi.emission.parent_nuclide if roi.emission else "None")))
            
            if roi.fit:
                # --- Column 4: Centroid ---
                centroid_item = QTableWidgetItem(f"{round(roi.fit.mu, 2)} keV")
                centroid_item.setData(Qt.UserRole, roi.fit.mu)   # real value
                table.setItem(row, 4, centroid_item)

                # --- Column 5: FWHM ---
                fwhm_item = QTableWidgetItem(f"{round(roi.fit.fwhm, 2)} keV")
                fwhm_item.setData(Qt.UserRole, (roi.fit.fwhm, roi.fit.fwhm_err))
                table.setItem(row, 5, fwhm_item)

                # --- Column 6: Resolution ---
                resolution = roi.fit.fwhm / roi.fit.mu * 100

                resolution_item = QTableWidgetItem(f"{round(resolution, 2)} %")
                resolution_item.setData(Qt.UserRole, resolution)
                table.setItem(row, 6, resolution_item)

            else:
                table.setItem(row, 4, QTableWidgetItem("-"))
                table.setItem(row, 5, QTableWidgetItem("-"))
                table.setItem(row, 6, QTableWidgetItem("-"))
            
    def calculate(self):
        centroids = []
        fwhms = []
        fwhm_errors = []
        
        if self.data_table.rowCount() == 0:
            return
        
        for i in range(self.data_table.rowCount()):
            if not self.data_table.cellWidget(i, 0).layout().itemAt(0).widget().isChecked() or  self.data_table.item(i, 4).text() == "-":
                continue
            
            centroid = self.data_table.item(i, 4).data(Qt.UserRole)
            fwhm, fwhm_error = self.data_table.item(i, 5).data(Qt.UserRole)

            centroids.append(centroid)
            fwhms.append(fwhm)
            fwhm_errors.append(fwhm_error)
        
        # Save current data
        self.current_energy_points = np.array(centroids)
        self.current_fwhm_points = np.array(fwhms)
        self.current_fwhm_error_points = np.array(fwhm_errors)
        self.current_resolution_points = np.array([fwhm / e for fwhm, e in zip(fwhms, centroids)])
        
        # Fit
        with warnings.catch_warnings():
            warnings.simplefilter("ignore")
            self.current_params, _, _ = curve_fit(resolution, centroids, self.current_resolution_points, [2])
        
        # Calculate error
        r2 = r_squared(self.current_resolution_points, resolution(self.current_energy_points, self.current_params))
        
        # Display results
        self.result_line.setText(f"R(E) = {round(self.current_params[0], 4)} / √(E),\t R² = {round(r2, 4)}")
        self.plot_data(self.button_group.checkedId())
    
    def plot_data(self, mode: int):
        if getattr(self, "res_plot", None) is None: # Survive startup
            return
        if self.current_energy_points is None or len(self.current_energy_points) == 0:
            return
        
        self.res_plot.clear()
        x_axis = np.linspace(25, max(self.current_energy_points) + 500, 1000)
        y_axis = resolution(x_axis, self.current_params)
        
        if mode == 0:
            self.res_plot.getPlotItem().plot(self.current_energy_points, self.current_fwhm_points, pen=None, symbol = "o")
            self.res_plot.getPlotItem().plot(x_axis, y_axis * x_axis)
            self.res_plot.getPlotItem().setLabel("left", "FWHM [keV]")
            self.res_plot.getPlotItem().setLabel("bottom", "E [keV]")
        elif mode == 1:
            self.res_plot.getPlotItem().plot(self.current_energy_points, self.current_resolution_points * 100, pen=None, symbol = "o")
            self.res_plot.getPlotItem().plot(x_axis, y_axis * 100)
            self.res_plot.getPlotItem().setLabel("left", "Resolution [%]")
            self.res_plot.getPlotItem().setLabel("bottom", "E [keV]")
        elif mode == 2:
            self.res_plot.getPlotItem().plot(1 / np.sqrt(self.current_energy_points), self.current_resolution_points * 100, pen=None, symbol = "o")
            self.res_plot.getPlotItem().plot(1 / np.sqrt(x_axis), y_axis * 100)
            self.res_plot.getPlotItem().setLabel("left", "Resolution [%]")
            self.res_plot.getPlotItem().setLabel("bottom", "1 / √(E) [1/√(keV)]")
            
    def assign_to_instruments(self, include_all_of_model: bool = False):
        data_dict = {
            "resolution_fn": "k/sqrt(E)",
            "resolution_params": list(self.current_params),
            "resolution_E_points": list(self.current_energy_points),
            "resolution_FWHM_points": list(self.current_fwhm_points),
            "resolution_FWHM_uncert_points": list(self.current_fwhm_error_points),
            "resolution_created": datetime.now()
            }
        
        # Get the instrument key
        instrument_key = self.instrument_combo.currentData()
        
        # Check if it matches a generic instrument, if so, update it
        if instrument_key in SpectrumManager.GenericInstrumentLibrary.instrument_registry:
            self.sigUpdateGenericInstrument.emit(instrument_key, data_dict)
            base_instrument = SpectrumManager.GenericInstrumentLibrary.instrument_registry[instrument_key]
        
        # Check if it matches a unique instrument, if so, update it
        elif instrument_key in SpectrumManager.UniqueInstrumentLibrary.instrument_registry:
            self.sigUpdateUniqueInstrument.emit(instrument_key, data_dict)
            base_instrument = SpectrumManager.UniqueInstrumentLibrary.instrument_registry[instrument_key]
            
        else:
            # Bugger
            raise KeyError(f"No instrument matches {instrument_key}")
        
        if include_all_of_model:
            # Find all instruments of the same model
            for key, instr in SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items():
                if instr.model == base_instrument.model:
                    self.sigUpdateUniqueInstrument.emit(key, data_dict)
                    
            for key, instr in SpectrumManager.GenericInstrumentLibrary.instrument_registry.items():
                if instr.model == base_instrument.model:
                    self.sigUpdateGenericInstrument.emit(key, data_dict)
if __name__ == "__main__":
    import sys
    from PySide6.QtWidgets import QApplication
    app = QApplication.instance() or QApplication(sys.argv)

    window = ResolutionWindow()
    window.resize(800, 500)
    window.show()


    sys.exit(app.exec())