from PySide6.QtWidgets import QDialog, QPushButton, QFormLayout, QHBoxLayout, QVBoxLayout, QLineEdit, QLabel, QDoubleSpinBox
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg

class SpectrumEditDialog(QDialog):
    def __init__(self, from_datastore:bool, parent = None, title="Spectrum Edit", **kwargs):
        super().__init__(parent=parent)

        self.setWindowTitle(title)
        self.setMinimumWidth(200)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)
        
        self.name_edit = QLineEdit()
        self.name_edit.setText(kwargs.get("name", ""))
        
        label = QLabel("Calibration")
        label.setStyleSheet("font-weight: bold; margin-top: 10px;")
        form.addRow(label)
        
        self.clear_calibration = QPushButton("Clear Calibration")
        