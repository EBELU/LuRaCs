from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout,
    QSizePolicy, QHBoxLayout, QDialog, QFormLayout, QTextEdit, QComboBox, QLineEdit, QDialogButtonBox,
    QPushButton, QCheckBox, QDoubleSpinBox, QTabWidget, QAbstractItemView, QMessageBox, QFileDialog, QLabel,QDoubleSpinBox
)
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg

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
        form_layout.addRow("Resolution: ",self.resolution)
        
        # Resolution Plot
        self.res_plot_widget = pg.PlotWidget()
        self.res_plot_widget.setMouseEnabled(x=False, y=False)
        form_layout.addRow("", self.res_plot_widget)

        # Example data
        self.res_plot_widget.plot([1, 2, 3, 4], [10, 20, 15, 30])
        
        # Efficiency
        self.efficiency = QLineEdit()
        self.efficiency.setEnabled(False)
        form_layout.addRow("Efficiency: ",self.efficiency)
        
        # Efficiency plot
        self.eff_plot_widget = pg.PlotWidget()
        self.eff_plot_widget.setMouseEnabled(x=False, y=False)
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
            "detector_dimensions": [self.height_spin.value(), self.width_spin.value(), self.depth_spin.value()],
            "remark": self.remarks.toPlainText()
        }
        return data