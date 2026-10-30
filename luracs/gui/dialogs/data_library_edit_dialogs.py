from __future__ import annotations

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QApplication,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QPushButton,
    QSpinBox,
    QTableWidget,
    QTableWidgetItem,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from luracs.containers.roi_classes import ROI
from luracs.core import SpectrumManager
from luracs.utils.numerics import EfficiencyFn, ResolutionFn


class InstrumentDialog(QDialog):
    def __init__(self, parent=None, generic_selection: bool = True, **kwargs):
        super().__init__(parent)
        self.setWindowTitle("Instrument Dialog")
        self.resize(500, 500)

        # Main layout
        main_layout = QVBoxLayout(self)

        # Form layout
        form_layout = QFormLayout()
        main_layout.addLayout(form_layout)

        # ------------------------------------------------------------------
        # Generic instrument combo box
        # ------------------------------------------------------------------
        self.generic_list = QComboBox()
        form_layout.addRow("Generic Instruments:", self.generic_list)
        self.generic_list.addItem("None", None)
        for (
            key,
            i,
        ) in SpectrumManager.GenericInstrumentLibrary.instrument_registry.items():
            self.generic_list.addItem(i.model, key)
        self.generic_list.setCurrentText("None")
        self.generic_list.currentIndexChanged.connect(self.generic_instrument_selected)
        self.generic_list.setEnabled(generic_selection)

        # ------------------------------------------------------------------
        # Row with 2 lines, name and model
        # ------------------------------------------------------------------
        model_name_row = QHBoxLayout()

        # Name
        self.name_input = QLineEdit()
        model_name_row.addWidget(self.name_input)
        model_name_row.addWidget(QLabel("Instrument Model:"))

        # Model
        self.model_input = QLineEdit()
        model_name_row.addWidget(self.model_input)
        form_layout.addRow("Instrument Name:", model_name_row)

        # ------------------------------------------------------------------
        # Row with 2 lines, Manufacturer and detector material
        # ------------------------------------------------------------------
        manufacturer_material_row = QHBoxLayout()

        # Manufacturer
        self.manufacturer = QLineEdit()
        manufacturer_material_row.addWidget(self.manufacturer)
        manufacturer_material_row.addWidget(QLabel("Detector Material:"))

        # Material
        self.detector_material = QLineEdit()
        manufacturer_material_row.addWidget(self.detector_material)
        form_layout.addRow("Manufacturer:", manufacturer_material_row)

        self.remarks = QTextEdit()
        form_layout.addRow("Remarks:", self.remarks)

        # ------------------------------------------------------------------
        # Detector Shape, with uncertainty
        # ------------------------------------------------------------------

        # --- Shape combo ---
        self.shape = QComboBox()
        self.shape.addItems(["Cuboid", "Cylinder", "Other"])
        self.shape.currentTextChanged.connect(self.update_spinboxes)
        form_layout.addRow("Shape:", self.shape)

        # --- Dimension spin boxes ---
        # Height
        self.height_spin = QDoubleSpinBox()
        self.height_spin.setRange(0, 500)
        self.height_spin.setDecimals(2)

        self.height_uncert_spin = QDoubleSpinBox()
        self.height_uncert_spin.setRange(0, 500)
        self.height_uncert_spin.setDecimals(2)
        self.height_uncert_spin.setPrefix("± ")

        # Width
        self.width_spin = QDoubleSpinBox()
        self.width_spin.setRange(0, 500)
        self.width_spin.setDecimals(2)

        self.width_uncert_spin = QDoubleSpinBox()
        self.width_uncert_spin.setRange(0, 500)
        self.width_uncert_spin.setDecimals(2)
        self.width_uncert_spin.setPrefix("± ")

        # Depth
        self.depth_spin = QDoubleSpinBox()
        self.depth_spin.setRange(0, 500)
        self.depth_spin.setDecimals(2)

        self.depth_uncert_spin = QDoubleSpinBox()
        self.depth_uncert_spin.setRange(0, 500)
        self.depth_uncert_spin.setDecimals(2)
        self.depth_uncert_spin.setPrefix("± ")

        # --- Build widgets for the spinboxes ---

        # Layout for dimensions
        dim_layout = QHBoxLayout()

        # Height
        h_widget = QWidget()
        h_layout = QHBoxLayout(h_widget)
        h_layout.setContentsMargins(0, 0, 0, 0)
        h_layout.addWidget(QLabel("Height:"))
        h_layout.addWidget(self.height_spin)
        h_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(h_widget)

        # Width
        w_widget = QWidget()
        w_layout = QHBoxLayout(w_widget)
        w_layout.setContentsMargins(0, 0, 0, 0)
        self.W_label = QLabel("Width:")
        w_layout.addWidget(self.W_label)
        w_layout.addWidget(self.width_spin)
        w_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(w_widget)

        # Depth
        d_widget = QWidget()
        d_layout = QHBoxLayout(d_widget)
        d_layout.setContentsMargins(0, 0, 0, 0)
        d_layout.addWidget(QLabel("Length:"))
        d_layout.addWidget(self.depth_spin)
        d_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(d_widget)

        form_layout.addRow("Dimensions:", dim_layout)

        # Layout for uncertainties
        unc_layout = QHBoxLayout()

        # Height uncert
        h_unc_widget = QWidget()
        h_unc_layout = QHBoxLayout(h_unc_widget)
        h_unc_layout.setContentsMargins(0, 0, 0, 0)
        h_unc_layout.addWidget(QLabel("Height:"))
        h_unc_layout.addWidget(self.height_uncert_spin)
        h_unc_layout.addWidget(QLabel("cm  "))
        unc_layout.addWidget(h_unc_widget)

        # Width uncert
        w_unc_widget = QWidget()
        w_unc_layout = QHBoxLayout(w_unc_widget)
        w_unc_layout.setContentsMargins(0, 0, 0, 0)
        self.W_uncert_label = QLabel("Width:")
        w_unc_layout.addWidget(self.W_uncert_label)
        w_unc_layout.addWidget(self.width_uncert_spin)
        w_unc_layout.addWidget(QLabel("cm  "))
        unc_layout.addWidget(w_unc_widget)

        # Length uncert
        d_unc_widget = QWidget()
        d_unc_layout = QHBoxLayout(d_unc_widget)
        d_unc_layout.setContentsMargins(0, 0, 0, 0)
        d_unc_layout.addWidget(QLabel("Length:"))
        d_unc_layout.addWidget(self.depth_uncert_spin)
        d_unc_layout.addWidget(QLabel("cm  "))
        unc_layout.addWidget(d_unc_widget)

        form_layout.addRow("Uncertainties:", unc_layout)

        # ------------------------------------------------------------------
        # Resolution
        # ------------------------------------------------------------------

        # Line
        self.resolution = QLineEdit()
        self.resolution.setReadOnly(True)
        form_layout.addRow("Resolution: ", self.resolution)

        # Resolution Plot
        self.res_plot_widget = pg.PlotWidget()
        self.res_plot_widget.setMinimumHeight(150)
        self.res_plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.res_plot_widget.setMouseEnabled(x=False, y=False)
        self.res_plot_widget.getPlotItem().setLabel(axis="bottom", text="Energy [keV]")
        self.res_plot_widget.getPlotItem().setLabel(axis="left", text="Resolution [%]")
        form_layout.addRow("", self.res_plot_widget)

        # ------------------------------------------------------------------
        # Intrinsic efficiency
        # ------------------------------------------------------------------

        # Line
        self.efficiency = QLineEdit()
        self.efficiency.setReadOnly(True)
        form_layout.addRow("Efficiency: ", self.efficiency)

        # Efficiency plot
        self.eff_plot_widget = pg.PlotWidget()
        self.eff_plot_widget.setMinimumHeight(150)
        self.eff_plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.eff_plot_widget.setMouseEnabled(x=False, y=False)
        self.eff_plot_widget.getPlotItem().setLabel(axis="bottom", text="Energy [keV]")
        self.eff_plot_widget.getPlotItem().setLabel(axis="left", text="Int. Eff. [%]")
        form_layout.addRow("", self.eff_plot_widget)

        # ------------------------------------------------------------------
        # Response matrix, not implemented
        # ------------------------------------------------------------------
        response_matrix_row = QHBoxLayout()
        self.response_matrix = QLineEdit()
        self.response_matrix.setEnabled(False)
        response_matrix_row.addWidget(self.response_matrix)

        self.load_matrix = QPushButton()
        self.load_matrix.setText("Import")
        response_matrix_row.addWidget(self.load_matrix)
        # form_layout.addRow("Response Matrix: ", response_matrix_row)

        # --- Bottom Buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

        # Initialize spinboxes
        self.update_spinboxes(self.shape.currentText())

        # Set values from an entered instrument
        self.set_values(**kwargs)

    def update_spinboxes(self, shape_text):
        """Disable depth spinboxes for Cylinder, enable otherwise."""
        if shape_text == "Cylinder":
            # Normal
            self.depth_spin.setDisabled(True)
            self.depth_uncert_spin.setDisabled(True)

            # Uncertainty
            self.W_label.setText("Diameter:")
            self.W_uncert_label.setText("Diameter:")
        else:
            # Normal
            self.depth_spin.setEnabled(True)
            self.depth_uncert_spin.setEnabled(True)

            # Uncertainty
            self.W_label.setText("Width:")
            self.W_uncert_label.setText("Width:")

    def set_values(self, **kwargs):
        # --- Basic Info ---
        self.name_input.setText(kwargs.get("name", ""))
        self.model_input.setText(kwargs.get("model", ""))
        self.manufacturer.setText(kwargs.get("manufacturer", ""))
        self.detector_material.setText(kwargs.get("detector_material", ""))
        self.remarks.setPlainText(kwargs.get("remark", ""))

        # --- Detector shape ---
        dimensions = kwargs.get("detector_dimensions_cm")
        dimensions_uncert = kwargs.get("detector_dimensions_uncert_cm")

        # Height
        if dimensions is not None:
            self.height_spin.setValue(dimensions[0])
        if dimensions_uncert is not None:
            self.height_uncert_spin.setValue(dimensions_uncert[0])

        # Width
        if dimensions is not None:
            self.width_spin.setValue(dimensions[1])
        if dimensions_uncert is not None:
            self.width_uncert_spin.setValue(dimensions_uncert[1])

        # Depth
        if self.shape.currentText() != "Cylinder" and dimensions is not None:
            self.depth_spin.setValue(dimensions[2])
        if self.shape.currentText() != "Cylinder" and dimensions_uncert is not None:
            self.depth_uncert_spin.setValue(dimensions_uncert[2])

        # --- Resolution ---
        # Line
        res_created = kwargs.get("resolution_created")
        if res_created is not None:
            res_created = res_created.strftime("%Y-%m-%d %H:%M")
        
        res_fn, res_param = kwargs.get('resolution_fn', ''), kwargs.get('resolution_params', '')
        if res_fn and res_param:
            self.resolution.setText(
                f"[{res_created}] fn = {ResolutionFn.Signatures.K_OVER_SQRT_E.value.format(*np.round(res_param, 3))}"
            )

            # Resolution plot
            self.res_plot_widget.getPlotItem().clear()
            if (
                kwargs.get("resolution_E_points") is not None
                and kwargs.get("resolution_FWHM_points") is not None
            ):
                self.res_plot_widget.plot(
                    kwargs.get("resolution_E_points"),
                    np.array(kwargs.get("resolution_FWHM_points"))
                    * 100
                    / np.array(kwargs.get("resolution_E_points")),
                    pen=None,
                    symbol="o",
                )
                res_x = np.linspace(25, max(kwargs.get("resolution_E_points")) + 500, 1000)
                res_y = ResolutionFn.lookup(ResolutionFn.Signatures.K_OVER_SQRT_E)(res_x, np.array(kwargs.get("resolution_params"))) * 100
                self.res_plot_widget.plot(res_x, res_y)

        # --- Intrinsic efficiency ---
        # Line
        eff_created = kwargs.get("int_efficiency_created")

        if eff_created is not None:
            eff_created = eff_created.strftime("%Y-%m-%d %H:%M")
        
        eff_fn, res_param = kwargs.get('int_efficiency_fn', ''), kwargs.get('int_efficiency_params', '')
        if eff_fn and res_param is not None:
            self.efficiency.setText(
                f"[{eff_created}] fn = {eff_fn.format(*np.round(res_param, 3))}"
            )

            # Efficiency plot
            self.eff_plot_widget.getPlotItem().clear()
            if (
                kwargs.get("int_efficiency_E_points") is not None
                and kwargs.get("int_efficiency_eff_points") is not None
            ):
                self.eff_plot_widget.plot(
                    kwargs.get("int_efficiency_E_points"),
                    np.array(kwargs.get("int_efficiency_eff_points")) * 100,
                    pen=None,
                    symbol="o",
                )
                if not kwargs.get("int_efficiency_E_points"):
                    return
                res_x = np.linspace(
                    25, max(kwargs.get("int_efficiency_E_points")) + 500, 1000
                )
                res_y = (
                    EfficiencyFn.lookup(EfficiencyFn.Signatures.EXP_POLY)(res_x, np.array(kwargs.get("int_efficiency_params")))
                    * 100
                )
                self.eff_plot_widget.plot(res_x, res_y)

    def generic_instrument_selected(self, index: int):
        instrument_key = self.generic_list.itemData(index)
        if instrument_key is None:
            self.set_values(
                detector_dimensions_cm=[0, 0, 0],
                detector_dimensions_uncert_cm=[0, 0, 0],
            )
        else:
            instrument = SpectrumManager.GenericInstrumentLibrary.instrument_registry[
                instrument_key
            ]
            self.set_values(**instrument.__dict__)

    def get_data(self):
        """Collect data from the dialog and return as a dictionary."""
        data = {
            "name": self.name_input.text(),
            "model": self.model_input.text(),
            "manufacturer": self.manufacturer.text(),
            "detector_material": self.detector_material.text(),
            "detector_shape": self.shape.currentText(),
            "detector_dimensions_cm": [
                self.height_spin.value(),
                self.width_spin.value(),
                self.depth_spin.value(),
            ],
            "detector_dimensions_uncert_cm": [
                self.height_uncert_spin.value(),
                self.width_uncert_spin.value(),
                self.depth_uncert_spin.value(),
            ],
            "remark": self.remarks.toPlainText(),
        }
        return data, self.generic_list.currentData()


class ROIDialog(QDialog):
    def __init__(self, rois: list[ROI] = [], name: str = "", parent=None, **kwargs):
        super().__init__(parent)
        self.setWindowTitle("ROI Dialog")
        self.resize(750, 500)

        # Main layout
        main_layout = QVBoxLayout(self)

        # Form layout
        form_layout = QFormLayout()
        main_layout.addLayout(form_layout)

        self.line_name = QLineEdit()
        self.line_name.setText(name)

        form_layout.addRow("Name: ", self.line_name)

        titles = ["Alias", "Lower Bound", "Upper Bound", "Nuclide", "Energy"]
        self.table = QTableWidget()
        self.table.setColumnCount(len(titles) + 1)  # +1 hidden ROI column
        self.table.setHorizontalHeaderLabels(titles + ["__roi__"])
        self.table.setColumnHidden(len(titles), True)  # hide last column
        self.table.horizontalHeader().setSectionResizeMode(QHeaderView.Stretch)

        form_layout.addRow(self.table)

        for i, roi in enumerate(rois):
            self.table.insertRow(i)

            # Store full ROI in hidden column (5)
            roi_item = QTableWidgetItem()
            roi_item.setData(Qt.UserRole, roi)
            self.table.setItem(i, 5, roi_item)

            # Row 0: Alias
            line_alias = QLineEdit(roi.alias)
            self.table.setCellWidget(i, 0, line_alias)

            # Row 1: Lower bound
            spin_lower = QSpinBox()
            spin_lower.setRange(0, 3500)
            spin_lower.setValue(roi.roi_bound[0])
            self.table.setCellWidget(i, 1, spin_lower)

            # Row 2: Upper bound
            spin_upper = QSpinBox()
            spin_upper.setRange(0, 3500)
            spin_upper.setValue(roi.roi_bound[1])
            self.table.setCellWidget(i, 2, spin_upper)

            # Row 3: Nuclide
            combo_nuclide = QComboBox()
            combo_nuclide.addItem("None")
            combo_nuclide.addItems(
                SpectrumManager.NuclideLibrary.get_sorted_nuclide_names(True)
            )
            self.table.setCellWidget(i, 3, combo_nuclide)

            # Row 4: Emission
            combo_emission = QComboBox()
            self.table.setCellWidget(i, 4, combo_emission)

            # Signal connection (row-safe)
            combo_nuclide.currentTextChanged.connect(
                lambda text, row=i: self._update_emissions(row, text)
            )

            # Set initial nuclide
            if roi.emission is not None:
                combo_nuclide.setCurrentText(roi.emission.parent_nuclide)
            else:
                combo_nuclide.setCurrentText("None")

            # Fill emissions + select current emission
            self._update_emissions(i, combo_nuclide.currentText())

            if roi.emission is not None:
                combo_emission = self.table.cellWidget(i, 4)
                for idx in range(combo_emission.count()):
                    if combo_emission.itemData(idx) == roi.emission:
                        combo_emission.setCurrentIndex(idx)
                        break

        # --- Bottom Buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

    def _update_emissions(self, row: int, nuclide: str):
        emissions_obj = SpectrumManager.NuclideLibrary.get_nuclide(nuclide)

        combo_emission = self.table.cellWidget(row, 4)
        if combo_emission is None:
            return

        combo_emission.clear()

        if emissions_obj is None:
            combo_emission.addItem("None")
            return

        emissions = emissions_obj.emissions

        for e in emissions:
            text = f"{e.energy_keV} keV"
            combo_emission.addItem(text, e)

    def get_name(self):
        return self.line_name.text()

    def get_rois(self):
        rois = []
        for i in range(self.table.rowCount()):
            roi_item = self.table.item(i, 5)  # hidden column index
            if roi_item is None:
                continue

            old_roi: ROI = roi_item.data(Qt.UserRole)
            combo = self.table.cellWidget(i, 4)
            emission = combo.currentData()
            rois.append(
                ROI(
                    tag=f"ROI_{i}",  # Random id
                    alias=self.table.cellWidget(i, 0).text(),
                    spectrum="None",
                    roi_bound=(
                        self.table.cellWidget(i, 1).value(),
                        self.table.cellWidget(i, 2).value(),
                    ),
                    region_bound=(),
                    fit_type=old_roi.fit_type,
                    bkg_type=old_roi.bkg_type,
                    bkg_est_channels=old_roi.bkg_est_channels,
                    fit=None,
                    roi_counts=0,
                    live_time=1,
                    emission=emission,
                    meta=old_roi.meta,
                )
            )

        return rois


if __name__ == "__main__":
    import sys

    app = QApplication(sys.argv)
    sys.exit(app.exec())
