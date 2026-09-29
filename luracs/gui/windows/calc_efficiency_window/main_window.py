import warnings
from dataclasses import replace
from datetime import datetime

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QAbstractItemView,
    QCheckBox,
    QComboBox,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
    QTableWidget,
    QTableWidgetItem,
    QVBoxLayout,
    QWidget,
)
from uncertainties import Variable, ufloat

from luracs.core import SpectrumManager
from luracs.gui.windows.calc_efficiency_window.helpers import (
    Source,
    calculate_efficiency,
    value_with_uncertainty,
)
from luracs.gui.windows.calc_efficiency_window.source_widget import (
    SourceParametersWidget,
)
from luracs.utils.numerics import curve_fit
from luracs.utils.numerics.approximation_fns import exp_polynomial


class EfficiencyWindow(QWidget):
    sigUpdateGenericInstrument = Signal(object, dict)
    sigUpdateUniqueInstrument = Signal(object, dict)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Efficiency Window")
        self.resize(1400, 700)
        
        self.source_num = 0
        self.source_name = "Unused"
        self.current_source_item = None

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- ROI data ---
        # Data from fitted ROIs in loaded spectra
        titles = ["", "Source", "ROI", "Spectrum", "Counts", "Yield", "Energy", "Nuclide"]

        self.data_table = QTableWidget(columnCount=len(titles))
        self.data_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.data_table.setHorizontalHeaderLabels(titles)
        self.data_table.horizontalHeader().setSectionResizeMode(
            QHeaderView.ResizeToContents
        )

        form.addRow("Peaks", self.data_table)

        # --- Source activity ---
        # Handles information about the source
        self.btn_add_source = QPushButton("Add Source")
        self.btn_add_source.clicked.connect(lambda :self.add_source(None))
        self.btn_remove_source = QPushButton("Remove Source")
        self.btn_remove_source.clicked.connect(self.remove_source)
        self.btn_from_json = QPushButton("From JSON")
        self.btn_to_json = QPushButton("To JSON")
        
        btn_layout = QHBoxLayout()
        btn_layout.addWidget(self.btn_add_source)
        btn_layout.addWidget(self.btn_remove_source)
        btn_layout.addStretch()
        btn_layout.addWidget(self.btn_from_json)
        btn_layout.addWidget(self.btn_to_json)
        
        form.addRow("", btn_layout)
        
        source_layout = QHBoxLayout()
        self.source_list = QListWidget()
        self.source_list.currentRowChanged.connect(self.list_item_selected)
        self.source_parameters_widget = SourceParametersWidget(self)
        self.source_parameters_widget.setEnabled(False)
        self.source_parameters_widget.nuclide_combo.currentTextChanged.connect(self.nuclide_changed)
        
        source_layout.addWidget(self.source_list)
        source_layout.addWidget(self.source_parameters_widget)
        
        form.addRow("Sources", source_layout)

        # Instrument Combo Box
        self.instrument_combo = QComboBox()
        form.addRow("Instrument", self.instrument_combo)
        # Detector area
        widget, self.detector_area, self.detector_area_unc = value_with_uncertainty()
        self.detector_area.setSuffix(" cm²")
        self.detector_area_unc.setSuffix(" cm²")
        self.detector_area.valueChanged.connect(self.source_parameters_widget.distance_changed)
        self.detector_area_unc.valueChanged.connect(self.source_parameters_widget.distance_changed)
        form.addRow("Detector area", widget)

        # --- Results ---
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
        )

        form.addRow("Efficiency Plot", self.demo_plot)

        main_layout.addLayout(form)

        # --- Bottom Buttons ---
        bottom_buttons = QHBoxLayout()
        self.assign_to_instrument_btn = QPushButton("Assign to Selected Instrument")
        self.assign_to_instrument_btn.setToolTip(
            "Assign the calculation to only the selected Generic Instrument"
        )
        self.assign_to_instrument_btn.clicked.connect(
            lambda: self.assign_to_instruments(False)
        )
        self.assign_to_instruments_of_same_model_btn = QPushButton(
            "Assign to Instrument Model"
        )
        self.assign_to_instruments_of_same_model_btn.setToolTip(
            "Assign the calculation to all Unique Instruments of the same model as the selected Generic Instrument"
        )
        self.assign_to_instruments_of_same_model_btn.clicked.connect(
            lambda: self.assign_to_instruments(True)
        )
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.close)

        bottom_buttons.addStretch()
        bottom_buttons.addWidget(self.assign_to_instruments_of_same_model_btn)
        bottom_buttons.addWidget(self.assign_to_instrument_btn)
        bottom_buttons.addWidget(close_btn)

        main_layout.addLayout(bottom_buttons)

        self.sigUpdateGenericInstrument.connect(
            SpectrumManager.GenericInstrumentLibrary.update_instrument_data
        )
        self.sigUpdateUniqueInstrument.connect(
            SpectrumManager.UniqueInstrumentLibrary.update_instrument_data
        )
        
    def show(self):
        self.set_data_table()
        self.set_instrument_combo()
        self.set_spectrum_combo()
        self.auto_assign_sources()
        super().show()

    def set_instrument_combo(self):
        self.instrument_combo.clear()
        for key, i in sorted(
            SpectrumManager.GenericInstrumentLibrary.instrument_registry.items(),
            key=lambda x: x[1].model,
        ):
            self.instrument_combo.addItem(i.model, key)
        self.instrument_combo.insertSeparator(self.instrument_combo.count())
        for key, i in sorted(
            SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items(),
            key=lambda x: x[1].name,
        ):
            self.instrument_combo.addItem(i.name, key)
            
        for spectrum in SpectrumManager.get_spectra_dict().values():
            if spectrum.instrument is not None:
                self.instrument_combo.setCurrentText(spectrum.instrument.name)

    def add_source(self, source: Source | None = None):
        if source is not None:
            new_source = replace(source, name=f"Source {self.source_num}")
        else:
            new_source = Source(name=f"Source {self.source_num}")
            
        self.source_num += 1

        new_item = QListWidgetItem(new_source.name)
        new_item.setData(Qt.ItemDataRole.UserRole, new_source)

        self.source_list.addItem(new_item)
        self.source_parameters_widget.setEnabled(True)

        self.source_list.setCurrentItem(new_item)
        index = self.source_list.model().index(
            self.source_list.model().rowCount() - 1, 0
        )
        self.source_list.setCurrentIndex(index)

        if isinstance(source, Source):
            self.source_parameters_widget.nuclide_combo.setCurrentText(source.nuclide)
        
        for ti in range(self.data_table.rowCount()):
            combo = self.data_table.cellWidget(ti, 1)
            combo.addItem(
                new_source.name + f" - [{source.nuclide}]" if isinstance(source, Source) else new_source.name)
        
        self.source_parameters_widget.nuclide_changed(new_source.nuclide)

        
    def remove_source(self):
        item = self.source_list.currentItem()

        if item is None:
            return

        row = self.source_list.row(item)
        self.source_list.takeItem(row)

        if self.source_list.count() == 0:
            self.source_parameters_widget.setEnabled(False)
            
        for ti in range(self.data_table.rowCount()):
            combo = self.data_table.cellWidget(ti, 1)
            combo.removeItem(row)
            
    def list_item_selected(self, idx: int):
        # Save the previously selected source
        if self.current_source_item is not None:
            source = self.source_parameters_widget.to_source()

            self.current_source_item.setData(
                Qt.ItemDataRole.UserRole,
                source,
            )

        # Get the newly selected source
        item = self.source_list.item(idx)

        if item is None:
            self.current_source_item = None
            return

        source = item.data(Qt.ItemDataRole.UserRole)

        # Load the new source
        self.source_parameters_widget.from_source(source)

        self.current_source_item = item


    def nuclide_changed(self, new_name: str):
        item = self.source_list.currentItem()

        if item is None:
            return

        source_name = item.text().split(" - [")[0]

        if new_name == "Other":
            item.setText(source_name)
        else:
            item.setText(f"{source_name} - [{new_name}]")
        
        idx = self.source_list.currentIndex().row()
        for ti in range(self.data_table.rowCount()):
            combo: QComboBox = self.data_table.cellWidget(ti, 1)
            combo.setItemText(idx, item.text())
            
    def set_spectrum_combo(self):
        self.source_parameters_widget.spectrum_combo.clear()
        self.source_parameters_widget.spectrum_combo.addItems(["None"] + list(SpectrumManager.get_spectra_dict()))
        
                

    def set_data_table(self):
        table = self.data_table
        table.setRowCount(0)  # clear properly

        rois = []

        for spectrum_name in SpectrumManager.get_spectra_dict():
            rois.extend(
                SpectrumManager.ROIManager.get_data_from_spectrum(
                    spectrum_name
                ).values()
            )
            

        for row, roi in enumerate(rois):
            table.insertRow(row)

            # --- Column 0: checkbox ---
            check_box = QCheckBox()
            check_box.setChecked(bool(roi.fit) and bool(roi.emission))
            check_box.setEnabled(bool(roi.fit) and bool(roi.emission))
            container = QWidget()
            layout = QHBoxLayout(container)
            layout.addWidget(check_box)
            layout.setAlignment(check_box, Qt.AlignCenter)
            layout.setContentsMargins(0, 0, 0, 0)
            table.setCellWidget(row, 0, container)
            
            source_combo = QComboBox()
            source_combo.setSizeAdjustPolicy(QComboBox.SizeAdjustPolicy.AdjustToContents)
            source_combo.currentTextChanged.connect(
                lambda: self.data_table.resizeColumnToContents(1)
            )
            for i in range(self.source_list.count()):
                item = self.source_list.item(i)
                assert item is not None
                source_combo.addItem(item.text())
                
            table.setCellWidget(row, 1, source_combo)
            

            # --- Column 2: alias (always shown) ---
            roi_item = QTableWidgetItem(str(roi.alias))
            roi_item.setData(Qt.UserRole, roi)
            table.setItem(row, 2, roi_item)

            spectrum_item = QTableWidgetItem(str(roi.spectrum))
            spectrum_item.setData(Qt.UserRole, roi)
            table.setItem(row, 3, spectrum_item)

            # Default values
            cps_text = "-"
            intensity_text = "-"
            energy_text = "-"
            parent_text = "-"

            if roi.fit:
                cps = roi.get_count_data("peak_area", cps=True)
                cps_text = f"{cps:.3f} CPS"

                if roi.emission:
                    intensity_text = f"{roi.emission.intensity_percent} %"
                    energy_text = f"{roi.emission.energy_keV} keV"
                    parent_text = str(roi.emission.parent_nuclide)

            # --- Fill remaining columns ---
            table.setItem(row, 4, QTableWidgetItem(cps_text))
            table.setItem(row, 5, QTableWidgetItem(intensity_text))
            table.setItem(row, 6, QTableWidgetItem(energy_text))
            table.setItem(row, 7, QTableWidgetItem(parent_text))
            
    
    def auto_assign_sources(self):
        for spect_name in SpectrumManager.get_spectra_dict():
            rois = SpectrumManager.ROIManager.get_data_from_spectrum(spect_name)
            
            found_nuclides = set()
            for r in rois.values():
                if r.emission is not None and r.emission.parent_nuclide is not None:
                    found_nuclides.add(r.emission.parent_nuclide)
            
            for nuc in found_nuclides:
                nuclide = SpectrumManager.NuclideLibrary.get_nuclide(nuc)
                assert nuclide is not None
                new_source = Source(
                    nuclide=nuc,
                    spectrum=spect_name
                )
                
                found_source = False
                for li in range(self.source_list.count()):
                    if (self.source_list.item(li).data(Qt.UserRole).spectrum == spect_name
                        and self.source_list.item(li).data(Qt.UserRole).nuclide == nuc):
                        found_source = True
                        break   
                
                if not found_source:
                    self.add_source(new_source)

                for ti in range(self.data_table.rowCount()):
                    row_roi = self.data_table.item(ti, 2).data(Qt.UserRole)
                    if row_roi.emission is None:
                        # If the roi does not have an emission skip it
                        continue
                    
                    if row_roi.spectrum != spect_name or row_roi.emission.parent_nuclide != nuc:
                        # Ensure we have found the correct row by matching nuclide and spectrum
                        # Each spectrum should only have one source of nuclide X
                        # If its a combination of sources you have to solve the your self!
                        continue
                    
                    src = None
                    for li in range(self.source_list.count()):
                        if (self.source_list.item(li).data(Qt.UserRole).spectrum == spect_name
                            and self.source_list.item(li).data(Qt.UserRole).nuclide == nuc):
                            src = self.source_list.item(li).text()
                            break   
                    
                    if src is not None:          
                        row_combo = self.data_table.cellWidget(ti, 1)
                        row_combo.setCurrentText(src)

                

    def calculate(self):
        self.energies: list[float] = []
        self.efficiencies: list[Variable] = []
        self.demo_plot.clear()
        
        self.list_item_selected(self.source_list.currentIndex().row())

        # Gather the sources
        sources = {self.source_list.item(i).text(): self.source_list.item(i).data(Qt.UserRole) for i in range(self.source_list.count())}
        
        for name, src in sources.items():
            for ti in range(self.data_table.rowCount()):
                combo: QComboBox = self.data_table.cellWidget(ti, 1)
                if combo.currentText().strip() == name.strip():
                    roi = self.data_table.item(ti, 2).data(Qt.UserRole)
                    try:
                        eff = calculate_efficiency(src, roi)
                    except ValueError as e:
                        QMessageBox.warning(self, "Error", str(e))
                        return
                    if eff is not None:
                        self.energies.append(roi.fit.mu)
                        self.efficiencies.append(ufloat(*eff))
                    
        # Plot the data
        if len(self.efficiencies) > 0:
            self.demo_plot.plotItem.clear()
            x = np.asarray(self.energies, dtype=float)
            y = np.asarray([e.n for e in self.efficiencies], dtype=float)
            yerr = np.asarray([e.s for e in self.efficiencies], dtype=float)

            self.demo_plot.plotItem.plot(x, y, pen=None, symbol="o")
            err = pg.ErrorBarItem(x=x, y=y, height=yerr)
            self.demo_plot.plotItem.addItem(err)

            # Suppress warnings during optimization
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                self.fit_params, _, _ = curve_fit(exp_polynomial, x, y, [1, -1, 0, 0])

            full_x = np.linspace(
                25, max(self.energies) + 500, 1000
            )  # Very few detectors work bellow 25keV
            fitted_y = exp_polynomial(full_x, self.fit_params)

            self.demo_plot.plotItem.plot(full_x, fitted_y)

    def assign_to_instruments(self, include_all_of_model: bool = False):
        data_dict = {
            "int_efficiency_fn": "exp_polynomial",
            "int_efficiency_params": list(self.fit_params),
            "int_efficiency_E_points": list(self.energies),
            "int_efficiency_eff_points": [v.n for v in self.efficiencies],
            "int_efficiency_uncert_points": [v.s for v in self.efficiencies],
            "int_efficiency_created": datetime.now(),
        }

        # Get the instrument key
        instrument_key = self.instrument_combo.currentData()

        # Check if it matches a generic instrument, if so, update it
        if (
            instrument_key
            in SpectrumManager.GenericInstrumentLibrary.instrument_registry
        ):
            self.sigUpdateGenericInstrument.emit(instrument_key, data_dict)
            base_instrument = (
                SpectrumManager.GenericInstrumentLibrary.instrument_registry[
                    instrument_key
                ]
            )

        # Check if it matches a unique instrument, if so, update it
        elif (
            instrument_key
            in SpectrumManager.UniqueInstrumentLibrary.instrument_registry
        ):
            self.sigUpdateUniqueInstrument.emit(instrument_key, data_dict)
            base_instrument = (
                SpectrumManager.UniqueInstrumentLibrary.instrument_registry[
                    instrument_key
                ]
            )

        else:
            # Bugger
            raise KeyError(f"No instrument matches {instrument_key}")

        if include_all_of_model:
            # Find all instruments of the same model
            for (
                key,
                instr,
            ) in SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items():
                if instr.model == base_instrument.model:
                    self.sigUpdateUniqueInstrument.emit(key, data_dict)

            for (
                key,
                instr,
            ) in SpectrumManager.GenericInstrumentLibrary.instrument_registry.items():
                if instr.model == base_instrument.model:
                    self.sigUpdateGenericInstrument.emit(key, data_dict)


if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = EfficiencyWindow()
    window.show()

    sys.exit(app.exec())