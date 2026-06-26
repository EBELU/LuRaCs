from PySide6.QtWidgets import (
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
    QAbstractItemView,
    QDateTimeEdit,
    QLabel,
    QHeaderView,
    QMessageBox,
)

from PySide6.QtCore import Qt, QDate, Signal
import pyqtgraph as pg
import numpy as np
import warnings
from datetime import datetime

from luracs.core import SpectrumManager
from uncertainties import ufloat
import uncertainties.umath as umath
from luracs.utils.numerics.efficiency import u_intrinsic_efficiency
from luracs.utils.numerics import curve_fit
from luracs.utils.numerics.approximation_fns import exp_polynomial


def value_with_uncertainty(has_unit=False):
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
    sigUpdateGenericInstrument = Signal(object, dict)
    sigUpdateUniqueInstrument = Signal(object, dict)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Efficiency Window")
        self.resize(1000, 700)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- ROI data ---
        # Data from fitted ROIs in loaded spectra
        titles = ["", "ROI", "Spectrum", "Counts", "Yield", "Energy", "Nuclide"]
        widths = [15, 130, 130, 130, 130, 100]

        self.data_table = QTableWidget(columnCount=len(titles))
        self.data_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.data_table.setHorizontalHeaderLabels(titles)
        for i, w in enumerate(widths):
            self.data_table.setColumnWidth(i, w)

        form.addRow("Peaks", self.data_table)

        # --- Source activity ---
        # Handles information about the source
        titles = [
            "Detector-Source\nDistance [cm]",
            "Detector-Source\nDistance Uncert [cm]",
            "Nuclide",
            "Activity 0",
            "Activity Uncertainty",
            "Unit",
            "Calibration Data",
            "Measurement Date",
            "Corrected Activity",
        ]

        self.source_activity_table = QTableWidget(columnCount=len(titles))
        self.source_activity_table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        self.source_activity_table.setHorizontalHeaderLabels(titles)
        self.source_activity_table.horizontalHeader().setSectionResizeMode(
            QHeaderView.ResizeToContents
        )  # Good?
        # for i, w in enumerate(widths):
        #     self.data_table.setColumnWidth(i, w)

        form.addRow("Source Activity", self.source_activity_table)

        self.instrument_combo = QComboBox()
        self.instrument_combo.addItem("None")
        form.addRow("Instrument", self.instrument_combo)

        # Detector area
        widget, self.detector_area, self.detector_area_unc = value_with_uncertainty()
        form.addRow("Detector area [cm²]", widget)

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
        # Set the tables each time the dialog is shown
        self.set_instrument_combo()
        self.set_data_table()
        self.set_source_activity_table()
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

    def set_source_activity_table(self):
        table = self.source_activity_table

        # Collect which nuclides are used in the spectra
        used_nuclides = set()
        for r in SpectrumManager.ROIManager.roi_registry.values():
            if r.emission is not None:
                used_nuclides.add(r.emission.parent_nuclide)

        # Check which nuclides have already been set
        nuclides_in_table = set()
        for i in range(table.rowCount()):
            nuclide_name = self.source_activity_table.item(i, 2)
            if nuclide_name is not None:
                nuclide_name = nuclide_name.text()
            else:
                continue
            if nuclide_name not in used_nuclides:
                # Remove nuclides no longer present
                self.source_activity_table.removeRow(i)
            else:
                nuclides_in_table.add(nuclide_name)

        for i, nuclide in enumerate(used_nuclides):
            # Skip nuclides already set in the table
            # Otherwise opening the window would always reset the table
            # Which is annoying when making small changes
            if nuclide in nuclides_in_table:
                continue

            widget, source_activity, source_activity_unc, source_activity_unit = (
                value_with_uncertainty(has_unit=True)
            )
            source_activity_unit.addItems(["Bq", "kBq", "MBq", "GBq", "TBq"])

            self.source_activity_table.insertRow(i)

            # Distances
            dist_spin = QDoubleSpinBox()
            dist_spin.setRange(0, 10000)
            self.source_activity_table.setCellWidget(i, 0, dist_spin)
            dist_spin_uncert = QDoubleSpinBox()
            dist_spin_uncert.setRange(0, 10000)
            dist_spin_uncert.setPrefix("±")

            # Activity
            self.source_activity_table.setCellWidget(i, 1, dist_spin_uncert)
            self.source_activity_table.setItem(i, 2, QTableWidgetItem(str(nuclide)))
            self.source_activity_table.setCellWidget(i, 3, source_activity)
            self.source_activity_table.setCellWidget(i, 4, source_activity_unc)
            self.source_activity_table.setCellWidget(i, 5, source_activity_unit)

            # Decay correction
            start_edit = QDateTimeEdit()
            start_edit.setMinimumDate(QDate(1900, 1, 1))

            end_edit = QDateTimeEdit()
            end_edit.setMinimumDate(QDate(1900, 1, 1))

            self.source_activity_table.setCellWidget(i, 6, start_edit)
            self.source_activity_table.setCellWidget(i, 7, end_edit)
            self.source_activity_table.setCellWidget(i, 8, QLabel("-"))

    def set_data_table(self):
        table = self.data_table
        table.setRowCount(0)  # clear properly

        rois = []
        for spectrum_name in SpectrumManager.get_spectra_dict().keys():
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

            # --- Column 1: alias (always shown) ---
            roi_item = QTableWidgetItem(str(roi.alias))
            roi_item.setData(Qt.UserRole, roi)
            table.setItem(row, 1, roi_item)

            spectrum_item = QTableWidgetItem(str(roi.spectrum))
            spectrum_item.setData(Qt.UserRole, roi)
            table.setItem(row, 2, spectrum_item)

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
            table.setItem(row, 3, QTableWidgetItem(cps_text))
            table.setItem(row, 4, QTableWidgetItem(intensity_text))
            table.setItem(row, 5, QTableWidgetItem(energy_text))
            table.setItem(row, 6, QTableWidgetItem(parent_text))

    def calculate(self):
        self.energies = []
        self.efficiencies = []
        self.demo_plot.clear()

        # Loop through the nuclides in the source-activity table
        # Each nuclide is assumed to have only one source
        for i_sa in range(self.source_activity_table.rowCount()):
            activity_conversion = 1
            A_unit = self.source_activity_table.cellWidget(i_sa, 5).currentText()
            match A_unit:
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

            # Extract data from the source-activity table
            source_detector_dist = self.source_activity_table.cellWidget(
                i_sa, 0
            ).value()
            source_detector_dist_uncert = self.source_activity_table.cellWidget(
                i_sa, 1
            ).value()

            A0 = self.source_activity_table.cellWidget(i_sa, 3).value()
            A0_unc = self.source_activity_table.cellWidget(i_sa, 4).value()

            # Dates for decay
            calibration_date_widget = self.source_activity_table.cellWidget(
                i_sa, 6
            ).dateTime()
            measurment_date_widget = self.source_activity_table.cellWidget(
                i_sa, 7
            ).dateTime()

            # Decay the source
            decay_time_s = calibration_date_widget.secsTo(measurment_date_widget)
            if decay_time_s == 0:
                # No decay required
                A = ufloat(A0, A0_unc) * activity_conversion

            elif decay_time_s < 0:
                QMessageBox.warning(
                    self,
                    "Invalid time",
                    "Negative decay time encountered\nNo time travelling! >:(",
                )
                return

            else:
                # Fetch the half-life from nuclide library
                nuclide_name = self.source_activity_table.item(i_sa, 2).text()
                nuclide = SpectrumManager.NuclideLibrary.get_nuclide(nuclide_name)

                A0_u = ufloat(A0, A0_unc) * activity_conversion

                A = A0_u * umath.exp(
                    -np.log(2) / ufloat(*nuclide.half_life_s) * decay_time_s
                )

            # Display the new activity
            self.source_activity_table.cellWidget(i_sa, 8).setText(f"{A} Bq")

            # Loop through the rois and get data
            for i_dt in range(self.data_table.rowCount()):
                if (
                    not self.data_table.cellWidget(i_dt, 0)
                    .layout()
                    .itemAt(0)
                    .widget()
                    .isChecked()
                ):
                    continue  # If the check box is not checked

                item = self.data_table.item(i_dt, 1)
                if item is None:
                    continue  # If the item has not been set correctly

                roi = item.data(Qt.UserRole)
                if roi is None or roi.emission is None or roi.fit is None:
                    continue  # If the roi has no nuclide of the fit failed

                # Check detector area
                if self.detector_area.value() == 0:
                    QMessageBox.warning(self, "Error", "Detector area can not be 0")
                    return

                # Perform calculation
                # Assumes a lowest uncertainty of 1%
                # Uncertainties warns to much about things that are not my problem
                with warnings.catch_warnings():
                    warnings.simplefilter("ignore")
                    eff = u_intrinsic_efficiency(
                        activity_Bq=A,
                        source_detector_distance_cm=ufloat(
                            source_detector_dist,
                            max(
                                source_detector_dist_uncert,
                                source_detector_dist_uncert * 0.01,
                            ),
                        ),
                        detector_area_cm2=ufloat(
                            self.detector_area.value(),
                            max(
                                self.detector_area_unc.value(),
                                self.detector_area_unc.value() * 0.01,
                            ),
                        ),
                        emission_yield=ufloat(
                            roi.emission.intensity_percent * 0.01,
                            roi.emission.intensity_error_percent * 0.01,
                        ),
                        cps=ufloat(roi.get_count_data("N", cps=True), 1e-12),
                    )
                # print(roi.get_count_data("peak_counts", cps=True), roi.get_count_data("N", cps=True))
                self.energies.append(roi.emission.energy_keV)
                self.efficiencies.append(eff)

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
