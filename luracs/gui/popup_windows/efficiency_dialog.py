from PySide6.QtWidgets import (
    QApplication,
    QDialog,
    QVBoxLayout,
    QFormLayout,
    QComboBox,
    QWidget,
    QCheckBox,
    QTableWidget,
    QTableWidgetItem,
    QPushButton,
    QDoubleSpinBox,
    QHBoxLayout,
    QAbstractItemView
)
from PySide6.QtCore import Qt
import pyqtgraph as pg
import numpy as np

from core import SpectrumManager
from uncertainties import ufloat
from utils.numerics.efficiency import u_intrinsic_efficiency
from utils.numerics import curve_fit
from utils.numerics.approximation_fns import exp_atten, exp_polynomial

def value_with_uncertainty(has_unit = False):
    container = QWidget()
    layout = QHBoxLayout(container)
    layout.setContentsMargins(0, 0, 0, 0)

    value = QDoubleSpinBox()
    uncertainty = QDoubleSpinBox()
    unit = QComboBox()
    
    value.setRange(0, 1e12)
    uncertainty.setRange(0, 1e12)
    
    uncertainty.setPrefix("± ")

    layout.addWidget(value)
    layout.addWidget(uncertainty)
    if has_unit:
        layout.addWidget(unit)
        return container, value, uncertainty, unit
    else:
        return container, value, uncertainty

class EfficiencyWindow(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Efficiency Window")
        self.resize(640, 740)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        self.layout = main_layout

        form = QFormLayout()
        form.setSpacing(9)

        self.spectrum_combo = QComboBox()
        self.spectrum_combo.addItem("None")
        
        self.spectrum_combo.setCurrentIndex(0)  # default to "None"
        form.addRow("Spectrum", self.spectrum_combo)

        titles = ["", "ROI", "Counts", "Yield", "Energy", "Nuclide"]
        widths = [15, 130, 130, 130, 130, 100]
        
        self.data_table = QTableWidget(columnCount=len(titles))
        self.data_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.data_table.setHorizontalHeaderLabels(titles)
        for i, w in enumerate(widths):
            self.data_table.setColumnWidth(i, w)
        
        form.addRow("", self.data_table)
        
        self.instrument_combo = QComboBox()
        self.instrument_combo.addItem("None")
        form.addRow("Instrument", self.instrument_combo)
        
        # Detector area
        widget, self.detector_area, self.detector_area_unc = value_with_uncertainty()
        form.addRow("Detector area [cm²]", widget)

        # Source-detector distance
        widget, self.source_detector_distance, self.source_detector_distance_unc = value_with_uncertainty()
        form.addRow("Source-Detector\nDistance [cm]", widget)
        
        # Source activity
        widget, self.source_activity, self.source_activity_unc, self.source_activity_unit = value_with_uncertainty(has_unit = True)
        self.source_activity_unit.addItems(["Bq", "kBq", "MBq", "GBq", "TBq"])
        form.addRow("Source Activity", widget)

        self.calculate_btn = QPushButton("Calculate")
        self.calculate_btn.clicked.connect(self.calculate)
        form.addRow("", self.calculate_btn)
        
        self.demo_plot = pg.PlotWidget()
        self.demo_plot.setMaximumHeight(250)
        self.demo_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.demo_plot.setLimits(
            xMin=0,
            xMax=3500,
            yMin=0,
            yMax=1,
        )

        form.addRow("Efficiency Plot", self.demo_plot)
        

        

        main_layout.addLayout(form)
        
        bottom_buttons = QHBoxLayout()
        self.assign_to_instrument_btn = QPushButton("Assign to instrument")
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.close)
        
        bottom_buttons.addStretch()
        bottom_buttons.addWidget(self.assign_to_instrument_btn)
        bottom_buttons.addWidget(close_btn)

        
        main_layout.addLayout(bottom_buttons)
        
    def show(self):
        self.spectrum_combo.clear()
        self.spectrum_combo.addItems(sorted(list(SpectrumManager.get_spectra_dict().keys())))  
        self.spectrum_combo.currentTextChanged.connect(self.set_table) 
        self.set_table(self.spectrum_combo.currentText())     
        super().show()

    def set_table(self, spectrum_name):
        table = self.data_table
        table.setRowCount(0)  # clear properly

        rois = SpectrumManager.ROIManager.get_data_from_spectrum(spectrum_name).values()

        for row, roi in enumerate(rois):
            table.insertRow(row)

            # --- Column 0: checkbox ---
            check_box = QCheckBox()
            check_box.setChecked(True)
            check_box.setEnabled(bool(roi.fit))
            table.setCellWidget(row, 0, check_box)

            # --- Column 1: alias (always shown) ---
            roi_item = QTableWidgetItem(str(roi.alias))
            roi_item.setData(Qt.UserRole, roi)
            table.setItem(row, 1, roi_item)

            # Default values
            cps_text = "-"
            intensity_text = "-"
            energy_text = "-"
            parent_text = "-"

            if roi.fit:
                cps = roi.get_count_data("peak_counts", cps=True)
                cps_text = f"{cps:.3f} CPS"

                if roi.emission:
                    intensity_text = f"{roi.emission.intensity_percent} %"
                    energy_text = f"{roi.emission.energy_keV} keV"
                    parent_text = str(roi.emission.parent_nuclide)

            # --- Fill remaining columns ---
            table.setItem(row, 2, QTableWidgetItem(cps_text))
            table.setItem(row, 3, QTableWidgetItem(intensity_text))
            table.setItem(row, 4, QTableWidgetItem(energy_text))
            table.setItem(row, 5, QTableWidgetItem(parent_text))
    
    def calculate(self):
        energies = []
        efficiencies = []
        activity_conversion = 1
        match self.source_activity_unit.currentText():
            case "Bq":
                activity_conversion = 1
            case "kBq":
                activity_conversion = 1e3
            case "MBq":
                activity_conversion = 1e6
            case "GBq":
                activity_conversion = 1e9
            case "TBq":
                activity_conversion = 1e12
            
        
        for i in range(self.data_table.rowCount()):
            item = self.data_table.item(i, 1)
            if item is None:
                continue

            roi = item.data(Qt.UserRole)
            if roi is None or roi.emission is None or roi.fit is None:
                continue
            
            
            
            eff = u_intrinsic_efficiency(
                activity_Bq = ufloat(self.source_activity.value() * activity_conversion, self.source_activity_unc.value() * activity_conversion),
                source_detector_distance_cm = ufloat(self.source_detector_distance.value(), self.source_detector_distance_unc.value()),
                detector_area_cm2 = ufloat(self.detector_area.value(), self.detector_area_unc.value()),
                emission_yield = ufloat(roi.emission.intensity_percent*0.01, roi.emission.intensity_error_percent*0.01),
                cps = ufloat(roi.get_count_data("N", cps=True), 1e-12)
            )
            print(roi.get_count_data("peak_counts", cps=True), roi.get_count_data("N", cps=True))
            energies.append(roi.emission.energy_keV)
            efficiencies.append(eff)
            
        if len(efficiencies) > 0:
            self.demo_plot.plotItem.clear()
            x = np.asarray(energies, dtype=float)
            y = np.asarray([e.n for e in efficiencies], dtype=float)
            yerr = np.asarray([e.s for e in efficiencies], dtype=float)

            self.demo_plot.plotItem.plot(x, y, pen=None, symbol='o')
            err = pg.ErrorBarItem(
                x=x,
                y=y,
                height=yerr
            )
            self.demo_plot.plotItem.addItem(err)
            
            fit_params, _, _ = curve_fit(exp_atten, x, y, [10, 10])
            
            full_x = np.linspace(0, 3500, 1000)
            fitted_y = exp_atten(full_x, fit_params)
            
            self.demo_plot.plotItem.plot(full_x, fitted_y)
            