from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from containers.spectrum_classes import Spectrum
    from gui.popup_windows.data_store import DataLibrary

from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QHBoxLayout,
    QDialog,
    QFormLayout,
    QTextEdit,
    QComboBox,
    QLineEdit,
    QDialogButtonBox,
    QPushButton,
    QDoubleSpinBox,
    QLabel,
    QDoubleSpinBox,
    QListWidget,
    QListWidgetItem,
)

from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg
import numpy as np
from gui.import_export import FileDialogs

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

        # Generic instruments as a base
        self.generic_list = QComboBox()
        form_layout.addRow("Generic Instruments:", self.generic_list)

        # Name and model
        model_name_row = QHBoxLayout()

        self.name_input = QLineEdit()
        self.name_input.setText(kwargs.get("name"))

        model_name_row.addWidget(self.name_input)
        model_name_row.addWidget(QLabel("Instrument Model:"))

        self.model_input = QLineEdit()
        self.model_input.setText(kwargs.get("model"))
        model_name_row.addWidget(self.model_input)

        form_layout.addRow("Instrument Name:", model_name_row)

        # Manufacturer and material
        manufacturer_material_row = QHBoxLayout()

        self.manufacturer = QLineEdit()
        self.manufacturer.setText(kwargs.get("manufacturer"))
        manufacturer_material_row.addWidget(self.manufacturer)

        manufacturer_material_row.addWidget(QLabel("Detector Material:"))

        self.detector_material = QLineEdit()
        self.detector_material.setText(kwargs.get("detector_material"))
        manufacturer_material_row.addWidget(self.detector_material)

        form_layout.addRow("Manufacturer:", manufacturer_material_row)

        self.remarks = QTextEdit()
        self.remarks.setPlainText(kwargs.get("remark", ""))
        form_layout.addRow("Remarks:", self.remarks)

        # Shape combo
        self.shape = QComboBox()
        self.shape.addItems(["Cuboid", "Cylinder", "Other"])
        self.shape.currentTextChanged.connect(self.update_spinboxes)
        form_layout.addRow("Shape:", self.shape)

        dimensions = kwargs.get("detector_dimensions")
        # Dimensions spin boxes
        self.height_spin = QDoubleSpinBox()
        self.height_spin.setRange(0, 500)
        self.height_spin.setDecimals(3)
        if dimensions is not None:
            self.height_spin.setValue(dimensions[0])

        self.width_spin = QDoubleSpinBox()
        self.width_spin.setRange(0, 500)
        self.width_spin.setDecimals(3)
        if dimensions is not None:
            self.width_spin.setValue(dimensions[1])

        self.depth_spin = QDoubleSpinBox()
        self.depth_spin.setRange(0, 500)
        self.depth_spin.setDecimals(3)
        if self.shape.currentText != "Cylinder" and dimensions is not None:
            self.height_spin.setValue(dimensions[2])

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

        # Resolution
        self.resolution = QLineEdit()
        self.resolution.setEnabled(False)
        form_layout.addRow("Resolution: ", self.resolution)

        # Resolution Plot
        self.res_plot_widget = pg.PlotWidget()
        self.res_plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.res_plot_widget.setMouseEnabled(x=False, y=False)
        self.res_plot_widget.getPlotItem().setLabel(
            axis="bottom",
            text="Energy [keV]"
        )
        form_layout.addRow("", self.res_plot_widget)

        # Example data
        self.res_plot_widget.plot([1, 2, 3, 4], [10, 20, 15, 30])

        # Efficiency
        self.efficiency = QLineEdit()
        self.efficiency.setEnabled(False)
        form_layout.addRow("Efficiency: ", self.efficiency)

        # Efficiency plot
        self.eff_plot_widget = pg.PlotWidget()
        self.eff_plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.eff_plot_widget.setMouseEnabled(x=False, y=False)
        self.eff_plot_widget.getPlotItem().setLabel(
            axis="bottom",
            text="Energy [keV]"
        )
        form_layout.addRow("", self.eff_plot_widget)

        # Example data
        self.eff_plot_widget.plot([1, 2, 3, 4], [10, 20, 15, 30])

        # Response Matrix
        response_matrix_row = QHBoxLayout()
        self.response_matrix = QLineEdit()
        self.response_matrix.setEnabled(False)
        response_matrix_row.addWidget(self.response_matrix)

        self.load_matrix = QPushButton()
        self.load_matrix.setText("Import")
        response_matrix_row.addWidget(self.load_matrix)
        form_layout.addRow("Response Matrix: ", response_matrix_row)

        # Buttons
        bottom_box = QGroupBox()

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

        # Initialize spinboxes
        self.update_spinboxes(self.shape.currentText())

    def update_spinboxes(self, shape_text):
        """Disable depth spinbox for Cylinder, enable otherwise."""
        if shape_text == "Cylinder":
            self.depth_spin.setDisabled(True)
            self.W_label.setText("Diameter: ")
        else:
            self.depth_spin.setEnabled(True)
            self.W_label.setText("Width: ")

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
            "remark": self.remarks.toPlainText(),
        }
        return data


class SpectrumEditDialog(QDialog):
    def __init__(self, parent=None, 
                 spectrum: Spectrum = None, 
                 spectrum_is_connected: bool = False, 
                 spectrum_index = None,
                 instrument_index = None):
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
        
        self.background_line = QLineEdit()
        self.background_line.setText(spectrum.background.spectrum_name if spectrum and spectrum.background is not None else "")
        self.background_line.setReadOnly(True)
        form_layout.addRow("Background", self.background_line)
        
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
        
        self.spectrum_list = QListWidget()
        form_layout.addRow("", self.spectrum_list)


        if spectrum_index is not None:
            for path, parser in spectrum_index.items():
                item = QListWidgetItem(str(parser.data["name"]))
                item.setData(Qt.ItemDataRole.UserRole, path)
                self.spectrum_list.addItem(item)
        
        self.instrument_line = QLineEdit()
        self.instrument_line.setReadOnly(True)
        form_layout.addRow("Instrument", self.instrument_line)
        
        instrument_btns = QHBoxLayout()
        instrument_btn_select = QPushButton("Select")
        instrument_btn_select.clicked.connect(self.select_instrument)
        instrument_btn_clear = QPushButton("Clear Instrument")
        instrument_btn_clear.clicked.connect(self.clear_instrument)
        instrument_btns.addWidget(instrument_btn_select)
        instrument_btns.addWidget(instrument_btn_clear)
        form_layout.addRow("", instrument_btns)
        
        self.instrument_list = QListWidget()
        self.instrument_line.setText(spectrum.instrument.name if spectrum.instrument is not None else "")
        form_layout.addRow("", self.instrument_list)
        
        if instrument_index is not None:
            for path, parser in instrument_index.items():
                item = QListWidgetItem(str(parser.get_instrument_data()["name"]))
                item.setData(Qt.ItemDataRole.UserRole, path)
                self.instrument_list.addItem(item)
        
        # Buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)
        
        
        # Flags
        self.flag_clear_bkg = False
        self.flag_clear_instrument = False
        self.flag_change_bkg = False
        self.flag_change_instrument = False
        self.flag_can_change_name = not spectrum_is_connected
        
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
        file_dialog = FileDialogs()
        path, _ = file_dialog.import_file(file_dialog.import_filters["spectrum"])
        
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
            return
        name = item.text()
        path = item.data(Qt.ItemDataRole.UserRole)
        
        self.instrument_line.setText(name)
        self.selected_instrument = path
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
