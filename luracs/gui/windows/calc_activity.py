from __future__ import annotations

from statistics import NormalDist

import pyqtgraph as pg
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from luracs.core import core_utils


def calculate_k(confidence_level):
    return NormalDist().inv_cdf((1 + confidence_level) / 2)


class ActivityWindow(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        main_layout = QVBoxLayout(self)
        
        
        # The top row of two comboboxes for selecting spectrum and a ROI from that spectrum
        top_layout = QHBoxLayout()
        top_layout.addWidget(QLabel("Spectrum"))
        self.spectrum_combo = QComboBox()
        top_layout.addWidget(self.spectrum_combo)
        top_layout.addWidget(QLabel("ROI"))
        self.roi_combo = QComboBox()
        top_layout.addWidget(self.roi_combo)
        
        main_layout.addLayout(top_layout)
        
        spectrum_layout = QHBoxLayout()
        spectrum_info_layout = QFormLayout()
        
        self.peak_plot = pg.PlotWidget()
        
        core_utils.ThemeManager.register_plot(self.peak_plot)
        spectrum_layout.addLayout(spectrum_info_layout, 1)
        spectrum_layout.addWidget(self.peak_plot, 1)
        
        self.peak_info_line = QLineEdit(readOnly=True)
        self.emission_info = QLineEdit(readOnly=True)
        self.fg_info_line = QLineEdit(readOnly=True)
        self.bg_info_line = QLineEdit(readOnly=True)
        
        spectrum_info_layout.addRow("Peak", self.peak_info_line)
        spectrum_info_layout.addRow("Emission", self.emission_info)
        spectrum_info_layout.addRow("Foreground", self.fg_info_line)
        spectrum_info_layout.addRow("Background", self.bg_info_line)
        
        main_layout.addLayout(spectrum_layout)
        
        form = QFormLayout()
        main_layout.addLayout(form)
        
        self.instrument_combo = QComboBox()
        form.addRow("Instrument", self.instrument_combo)
        
        self.geometry_combo = QComboBox()
        self.geometry_combo.addItems(["Parallel Beam", "Manual"])
        form.addRow("Geometry", self.geometry_combo)
        
        widget_stack = QStackedWidget()
        widget_stack.addWidget(self.build_parallel_beam_widget())
        
        form.addRow(widget_stack)
        
        self.mda_combo = QComboBox()
        self.mda_combo.addItems(["Currie", "ISO 11929", "KTA"])
        form.addRow("MDA Method", self.mda_combo)
        
        calculate_btn = QPushButton("Calculate")
        form.addRow(calculate_btn)
        
        bottom_buttons = QHBoxLayout()
        bottom_buttons.addStretch()
        close_btn = QPushButton("Close")
        bottom_buttons.addWidget(close_btn)
        close_btn.clicked.connect(self.close)
        main_layout.addLayout(bottom_buttons)
        
        
    def build_parallel_beam_widget(self):
        page = QWidget()
        layout = QFormLayout(page)
        
        self.pb_int_eff_line = QDoubleSpinBox(minimum=0, maximum=1e2, decimals=2, suffix=" %")
        layout.addRow("Efficiency",self.pb_int_eff_line)
        
        area_layout = QHBoxLayout()

        self.pb_detector_area_spin = QDoubleSpinBox(
            minimum=0, maximum=1e4, decimals=2, suffix=" cm²"
        )
        self.pb_detector_area_uncertainty_spin = QDoubleSpinBox(
            minimum=0, maximum=1e4, decimals=2, suffix=" cm²", prefix="± "
        )

        area_layout.addWidget(self.pb_detector_area_spin)
        area_layout.addWidget(self.pb_detector_area_uncertainty_spin)
        layout.addRow("Detector Area", area_layout)


        distance_layout = QHBoxLayout()

        self.pb_distance_spin = QDoubleSpinBox(
            minimum=0, maximum=1e4, decimals=2, suffix=" cm"
        )
        self.pb_distance_uncertainty_spin = QDoubleSpinBox(
            minimum=0, maximum=1e4, decimals=2, suffix=" cm", prefix="± "
        )

        distance_layout.addWidget(self.pb_distance_spin)
        distance_layout.addWidget(self.pb_distance_uncertainty_spin)
        layout.addRow("Source Distance", distance_layout)
        
        def on_manual_solid_angle_toggle(state: bool):
            self.pb_detector_area_spin.setEnabled(not state)
            self.pb_detector_area_uncertainty_spin.setEnabled(not state)

            self.pb_distance_spin.setEnabled(not state)
            self.pb_distance_uncertainty_spin.setEnabled(not state)
            self.solid_angle_spin.setEnabled(state)
        
        self.pb_manual_solid_angle_check = QCheckBox("Manual Ω")
        self.pb_manual_solid_angle_check.toggled.connect(on_manual_solid_angle_toggle)
        layout.addRow(self.pb_manual_solid_angle_check)
        
        self.solid_angle_spin = QDoubleSpinBox(minimum=0, maximum=1e4, decimals=3, suffix=" sr")
        self.solid_angle_spin.setEnabled(False)
        layout.addRow("Ω", self.solid_angle_spin)
        
        return page
        
    def calculate(self):
        pass
    
        
        
        

if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = ActivityWindow()
    window.resize(800, 500)
    window.show()

    sys.exit(app.exec())