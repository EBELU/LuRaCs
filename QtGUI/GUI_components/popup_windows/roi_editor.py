from PySide6.QtWidgets import (QDialog, QVBoxLayout, QFormLayout, 
    QLineEdit, QDoubleSpinBox, QComboBox, QCheckBox, 
    QDialogButtonBox, QPushButton)

class ROIEditor(QDialog):
    DELETE = 2
    def __init__(self, roi_name, low, high, fit_type, bkg_type,
                 merge, poisson_weights, movable,
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

        self.upper_bound = QDoubleSpinBox()
        self.upper_bound.setRange(0.0, 1e6)
        self.upper_bound.setDecimals(2)
        self.upper_bound.setSuffix(" keV")
        self.upper_bound.setValue(round(high))     # example default
        form.addRow("Higher Bound:", self.upper_bound)
        

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
        
        self.movable = QCheckBox("Movable")
        self.movable.setChecked(movable)
        form.addRow("\t\t",self.movable)
        
        self.poisson_weights = QCheckBox("Use Poisson Weights")
        self.poisson_weights.setChecked(poisson_weights)
        form.addRow("\t\t",self.poisson_weights)
        
        def update_button_state():
            is_gaussian = self.fit_type.currentText() == "Gaussian"
            self.poisson_weights.setEnabled(is_gaussian)
            if not is_gaussian:
                self.merge.setChecked(False)
            self.merge.setEnabled(is_gaussian)
            self.bkg_type.setEnabled(is_gaussian)

        self.fit_type.currentTextChanged.connect(update_button_state)
            
        main_layout.addLayout(form)
        
        update_button_state()
        
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
        
    def get_values(self):
        return {
            "roi_name": self.roi_name.text(),
            "lower_bound": self.lower_bound.value(),
            "upper_bound": self.upper_bound.value(),
            "fit_type": self.fit_type.currentText(),
            "bkg_type": self.bkg_type.currentText(),
            "merge": self.merge.isChecked(),
            "movable": self.movable.isChecked(),
            "poisson_weights": self.poisson_weights.isChecked()
        }