from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QSizePolicy, QColorDialog, QFrame, QHBoxLayout, QDialog, QFormLayout, QTextEdit, QComboBox, QLineEdit, QDialogButtonBox,
    QPushButton, QCheckBox, QDoubleSpinBox, QTabWidget
)
from PySide6.QtCore import Qt, Signal
import sys

class ROIEditor(QDialog):
    DELETE = 2
    def __init__(self, roi_name, low, high, fit_type, bkg_type,
                 merge, poisson_weights,
                 title="", parent=None):
        super().__init__(parent=parent)
        
        self.setWindowTitle("ROI Editor")
        self.setMinimumWidth(150)
        self.setMinimumHeight(300)
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)
        
        self.layout = main_layout
        form = QFormLayout()
        form.setSpacing(9)


        
        self.roi_name = QLineEdit()
        self.roi_name.setText(roi_name)
        form.addRow("ROI Name:", self.roi_name)
        
        self.lower_bound = QDoubleSpinBox()
        self.lower_bound.setRange(0.0, 1e6)   # adjust as needed
        self.lower_bound.setDecimals(2)
        self.lower_bound.setSuffix(" keV")
        self.lower_bound.setValue(round(low))
        form.addRow("Lower Bound:", self.lower_bound)

        self.higher_bound = QDoubleSpinBox()
        self.higher_bound.setRange(0.0, 1e6)
        self.higher_bound.setDecimals(2)
        self.higher_bound.setSuffix(" keV")
        self.higher_bound.setValue(round(high))     # example default
        form.addRow("Higher Bound:", self.higher_bound)
        

        self.fit_type = QComboBox()
        self.fit_type.addItems(["None", "Gaussian"])
        self.fit_type.setCurrentText(fit_type)
        form.addRow("Peak Function:", self.fit_type)
        
        self.bkg_type = QComboBox()
        self.bkg_type.addItems(["None", "Linear", "Quadratic"])
        self.bkg_type.setCurrentText(bkg_type)
        form.addRow("Background:", self.bkg_type)
        
        self.merge = QCheckBox("Allow merging")
        self.merge.setChecked(merge)
        form.addRow("\t\t",self.merge)
        
        self.poisson_weights = QCheckBox("Use Poisson Weights")
        self.poisson_weights.setChecked(poisson_weights)
        form.addRow("\t\t",self.poisson_weights)
        
        def update_poisson_state():
            is_gaussian = self.fit_type.currentText() == "Gaussian"
            self.poisson_weights.setEnabled(is_gaussian)

        self.fit_type.currentTextChanged.connect(update_poisson_state)
            
        main_layout.addLayout(form)
        
        # --- Bottom Buttons
        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        delete_button = QPushButton("Delete")
        delete_button.clicked.connect(self.on_delete)
        buttons.addButton(delete_button, QDialogButtonBox.ActionRole)

        main_layout.addWidget(buttons)


    def on_delete(self):
        self.done(self.DELETE)
        
class GenericLibrary(QWidget):
    def __init__(self, parent = None):
        super().__init__(self, parent)
        

    
        
class DataLibrary(QDialog):
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)

        # Main layout for the dialog
        main_layout = QVBoxLayout(self)

        # Tabs
        self.tabs = QTabWidget()
        self.tabs.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

        # Create tab widgets as class members
        self.spectrum_tab = QWidget()
        self.roi_tab = QWidget()
        self.spectrogram_tab = QWidget()
        self.instruments_tab = QWidget()
        self.generic_instruments_tab = QWidget()

        # Add tabs in the desired order
        self.tabs.addTab(self.spectrum_tab, "Spectrum Library")
        self.tabs.addTab(self.spectrogram_tab, "Spectrogram Library")
        self.tabs.addTab(self.roi_tab, "ROI Library")
        self.tabs.addTab(self.instruments_tab, "Instruments")
        self.tabs.addTab(self.generic_instruments_tab, "Generic Instruments")


        main_layout.addWidget(self.tabs)
        
        self.buttons = QDialogButtonBox()
        main_layout.addWidget(self.buttons)      

        
        main_layout.addWidget(self.buttons)

        # self.resize(self.tabs.sizeHint())
        self.adjustSize()


    
        
        
app = QApplication.instance() or QApplication(sys.argv)


w = ROIEditor("ROI_0", 124, 452.425, "Gaussian", "Linear", True, False)

l = DataLibrary("Data Storage")
res = l.exec()

print(res)
