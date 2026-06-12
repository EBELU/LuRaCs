from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum

from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QVBoxLayout,
    QHBoxLayout,
    QDialog,
    QFormLayout,
    QTextEdit,
    QComboBox,
    QLineEdit,
    QDialogButtonBox,
    QPushButton,
    QLabel,
    QDoubleSpinBox,
    QListWidget,
    QListWidgetItem,
)

from PySide6.QtCore import Qt
import pyqtgraph as pg
import numpy as np

from luracs.core import SpectrumManager, IOManager
from luracs.utils.numerics import resolution, exp_polynomial

class InstrumentDialog(QDialog):
    def __init__(self, parent=None, **kwargs):
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
        for key, i in SpectrumManager.GenericInstrumentLibrary.instrument_registry.items():
            self.generic_list.addItem(i.model, key)
        self.generic_list.setCurrentText("None")
        self.generic_list.currentIndexChanged.connect(self.generic_instrument_selected)
        if len(kwargs) != 0:
            self.generic_list.setEnabled(False)
            
            
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
        self.res_plot_widget.getPlotItem().setLabel(
            axis="bottom",
            text="Energy [keV]"
        )
        self.res_plot_widget.getPlotItem().setLabel(
            axis="left",
            text="Resolution [%]"
        )
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
        self.eff_plot_widget.getPlotItem().setLabel(
            axis="bottom",
            text="Energy [keV]"
        )
        self.res_plot_widget.getPlotItem().setLabel(
            axis="left",
            text="Int. Eff. [%]"
        )
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
        self.resolution.setText(f"[{res_created}] fn = {kwargs.get('resolution_fn', '')}, params = {kwargs.get('resolution_params', '')}")
        
        # Resolution plot
        self.res_plot_widget.getPlotItem().clear()
        if kwargs.get("resolution_E_points") is not None and kwargs.get("resolution_FWHM_points") is not None:
            self.res_plot_widget.plot(kwargs.get("resolution_E_points"), np.array(kwargs.get("resolution_FWHM_points")) * 100 / np.array(kwargs.get("resolution_E_points")), pen=None, symbol = "o")
            res_x = np.linspace(25, max(kwargs.get("resolution_E_points")) + 500, 1000)
            res_y = resolution(res_x, np.array(kwargs.get("resolution_params"))) * 100
            self.res_plot_widget.plot(res_x, res_y)
        
        # --- Intrinsic efficiency ---
        # Line
        eff_created = kwargs.get("int_efficiency_created")
        if eff_created is not None:
            eff_created = eff_created.strftime("%Y-%m-%d %H:%M")
        self.efficiency.setText(f"[{eff_created}] fn = {kwargs.get('int_efficiency_fn', '')}, params = {kwargs.get('int_efficiency_params', '')}")

        
        # Efficiency plot
        self.eff_plot_widget.getPlotItem().clear()
        if kwargs.get("int_efficiency_E_points") is not None and kwargs.get("int_efficiency_eff_points") is not None:
            self.eff_plot_widget.plot(kwargs.get("int_efficiency_E_points"), np.array(kwargs.get("int_efficiency_eff_points")) * 100, pen=None, symbol = "o")
            res_x = np.linspace(25, max(kwargs.get("int_efficiency_E_points")) + 500, 1000)
            res_y = exp_polynomial(res_x, np.array(kwargs.get("int_efficiency_params"))) * 100
            self.eff_plot_widget.plot(res_x, res_y)
            
    def generic_instrument_selected(self, index: int):
        instrument_key = self.generic_list.itemData(index)
        if instrument_key is None:
            self.set_values(detector_dimensions_cm = [0, 0, 0], detector_dimensions_uncert_cm = [0, 0, 0],)
        else:
            instrument = SpectrumManager.GenericInstrumentLibrary.instrument_registry[instrument_key]
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


class SpectrumEditDialog(QDialog):
    def __init__(self, parent=None, 
                 spectrum: Spectrum = None, 
                 spectrum_is_connected: bool = False):
        super().__init__(parent)
        self.setWindowTitle("Spectrum Edit Dialog")
        self.resize(400, 300)

        main_layout = QVBoxLayout(self)

        # Form layout
        form_layout = QFormLayout()
        main_layout.addLayout(form_layout)

        # Name
        self.name_input = QLineEdit()
        self.name_input.setEnabled(not spectrum_is_connected)
        self.name_input.setText(spectrum.name if spectrum else "")
        form_layout.addRow("Spectrum Name:", self.name_input)

        # Remark
        self.remark_input = QTextEdit()
        self.remark_input.setPlainText(spectrum.remark if spectrum else "")
        form_layout.addRow("Remark:", self.remark_input)
        
        self.calibration_text = QTextEdit()
        self.calibration_text.setReadOnly(True)
        if spectrum is not None and spectrum.calibration_coefficients is not None:
            calib_str = "\n".join([f"a{i} = {a}" for i, a in enumerate(reversed(spectrum.calibration_coefficients))])
        else:
            calib_str = ""
        self.calibration_text.setText(calib_str)
        form_layout.addRow("Calibration", self.calibration_text)
        
        # self.calibration_plot = pg.PlotWidget()
        # self.calibration_plot.setMouseEnabled(x=False, y=False)
        # self.calibration_plot.getPlotItem().setLabel(
        #     axis="left",
        #     text="Energy [keV]"
        # )

        # self.calibration_plot.getPlotItem().setLabel(
        #     axis="bottom",
        #     text="Channel"
        # )
        
        # self.calibration_plot.getPlotItem().plot(np.arange(len(spectrum.x_axis)), spectrum.x_axis)
        
        # self.calibration_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        
        # form_layout.addRow("", self.calibration_plot)
        
        # --- Background ---
        # Background name
        self.background_line = QLineEdit()
        self.background_line.setText(spectrum.background.spectrum_name if spectrum and spectrum.background is not None else "")
        self.background_line.setReadOnly(True)
        form_layout.addRow("Background", self.background_line)
        
        # Buttons
        bkg_btns = QHBoxLayout()
        
        bkg_btn_select = QPushButton("Select")
        bkg_btn_select.clicked.connect(self.select_background)
        bkg_btn_import = QPushButton("Import")
        bkg_btn_import.clicked.connect(self.import_background)
        bkg_btn_clear = QPushButton("Clear Bkg")
        bkg_btn_clear.clicked.connect(self.clear_bkg)
        
        bkg_btns.addWidget(bkg_btn_select)
        bkg_btns.addWidget(bkg_btn_import)
        bkg_btns.addWidget(bkg_btn_clear)
        
        form_layout.addRow("", bkg_btns)
        
        # Internal spectra list
        self.spectrum_list = QListWidget()
        form_layout.addRow("", self.spectrum_list)
        
        for path, parser in sorted(IOManager.FileIndex.spectrum_index.get_index().items(), key=lambda item: item[0]):
            item = QListWidgetItem(str(parser.data["name"]))
            item.setData(Qt.ItemDataRole.UserRole, path)
            self.spectrum_list.addItem(item)
        
        # --- Instrument ---
        # Name
        self.instrument_line = QLineEdit()
        self.instrument_line.setReadOnly(True)
        form_layout.addRow("Instrument", self.instrument_line)
        
        # Buttons
        instrument_btns = QHBoxLayout()
        instrument_btn_select = QPushButton("Select")
        instrument_btn_select.clicked.connect(self.select_instrument)
        instrument_btn_clear = QPushButton("Clear Instrument")
        instrument_btn_clear.clicked.connect(self.clear_instrument)
        instrument_btns.addWidget(instrument_btn_select)
        instrument_btns.addWidget(instrument_btn_clear)
        form_layout.addRow("", instrument_btns)
        
        # Instrument list from instrument libraries
        self.instrument_list = QListWidget()
        self.instrument_line.setText(spectrum.instrument.name if spectrum.instrument is not None else "")
        form_layout.addRow("", self.instrument_list)
        
        for path, instr in sorted(SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items(), key=lambda item: item[0]):
            item = QListWidgetItem(instr.name)
            item.setData(Qt.ItemDataRole.UserRole, instr)
            self.instrument_list.addItem(item)
        
        # --- Bottom buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)
        
        
        # --- Flags ---
        # These set the response of things handled outside of this dialog 
        self.flag_clear_bkg = False
        self.flag_clear_instrument = False
        self.flag_change_bkg = False
        self.flag_change_instrument = False
        self.flag_can_change_name = not spectrum_is_connected # A spectrum with a connection is not allowed to change its name, it would mess with receiving now data from the the connected device
        
        self.selected_background = None
        self.selected_instrument = None

    def get_data(self):
        """Collect data from the dialog and return as a dictionary."""
        data = {
            "name": self.name_input.text(),
            "remark": self.remark_input.toPlainText(),
            "background_pth": self.selected_background,
            "instrument": self.selected_instrument,
            "flag_clear_bkg": self.flag_clear_bkg,
            "flag_clear_instrument": self.flag_clear_instrument,
            "flag_can_change_name": self.flag_can_change_name,
            "flag_change_bkg": self.flag_change_bkg,
            "flag_change_instrument": self.flag_change_instrument
        }
        return data
    
    def select_background(self):
        item = self.spectrum_list.selectedItems()
        if len(item) > 0:
            item = item[0]
        else:
            return
        name = item.text()
        path = item.data(Qt.ItemDataRole.UserRole)
        
        self.background_line.setText(name)
        self.selected_background = path
        self.flag_clear_bkg = False
        self.flag_change_bkg = True
    
    def import_background(self):
        path, _ = IOManager.Importer.import_file(IOManager.Importer.import_filters["spectrum"])
        
        if path is not None:
            self.background_line.setText(path.name)
            self.selected_background = path
            self.flag_clear_bkg = False
            self.flag_change_bkg = True
            
    def clear_bkg(self):
        self.flag_clear_bkg = True
        self.background_line.setText("")
        
    def select_instrument(self):
        item = self.instrument_list.selectedItems()
        if len(item) > 0:
            item = item[0]
        else:
            # If none is selected sod off
            return
        name = item.text()
        instr = item.data(Qt.ItemDataRole.UserRole)
        
        self.instrument_line.setText(name)
        self.selected_instrument = instr
        self.flag_clear_instrument = False
        self.flag_change_instrument = True
        
    def clear_instrument(self):
        self.flag_clear_instrument = True
        self.instrument_line.setText("")

if __name__ == "__main__":
    import sys

    app = QApplication(sys.argv)
    dialog = SpectrumEditDialog()
    dialog.show()
    sys.exit(app.exec())
