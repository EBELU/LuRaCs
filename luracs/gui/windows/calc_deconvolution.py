from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum

from copy import deepcopy
from pathlib import Path

import numpy as np
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QPushButton,
    QSpinBox,
    QStackedWidget,
    QVBoxLayout,
    QWidget,
)

from luracs.core import SpectrumManager
from luracs.utils.numerics import ml_em, process_response


class DeconvolutionWindow(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        main_layout = QVBoxLayout(self)
        
        form = QFormLayout()
        
        self.combo_algorithm = QComboBox()
        self.combo_algorithm.addItems(["Generalized Richardson-Lucy", "ML-EM"])
        self.combo_algorithm.currentIndexChanged.connect(self.set_algorithm)
        form.addRow("Algorithm", self.combo_algorithm)
        

        
        # Stacked widget
        self.algorithm_stack = QStackedWidget()
        self.algorithm_stack.setMaximumHeight(200)
        self.algorithm_stack.addWidget(self.create_rl_page())
        self.algorithm_stack.addWidget(self.create_mlem_page())
        
        form.addRow(self.algorithm_stack)
        
        self.combo_instrument = QComboBox()
        form.addRow("Instrument", self.combo_instrument)
        
        self.combo_spectra = QComboBox()
        form.addRow("Spectrum", self.combo_spectra)
        
        self.chosen_input = QLineEdit()
        self.chosen_input.setReadOnly(True)
        form.addRow("Input", self.chosen_input)
        
        
        btn_calculate = QPushButton("Calculate")
        btn_calculate.clicked.connect(self.calculate)
        
        form.addRow("", btn_calculate)
        
        # self.calculation_progress = QProgressBar()
        # self.calculation_progress.setRange(0, 100)
        # self.calculation_progress.setValue(30)
        # form.addRow("Progress", self.calculation_progress)
        
        main_layout.addLayout(form)
        # --- Bottom Buttons ---
        bottom_buttons = QHBoxLayout()
        close_btn = QPushButton("Close")
        close_btn.clicked.connect(self.close)

        bottom_buttons.addStretch()
        bottom_buttons.addWidget(close_btn)

        main_layout.addLayout(bottom_buttons)
        
        # --- Set default algorithm ---
        self.combo_algorithm.setCurrentIndex(1)
        
    def create_rl_page(self):
        page = QWidget()
        layout = QFormLayout(page)

        self.rl_iterations = QSpinBox()
        self.rl_iterations.setRange(1, 100000)
        self.rl_iterations.setValue(30)
        
        self.rl_use_resolution = QCheckBox()
        self.rl_use_resolution.setText("Use Resolution")
        self.rl_use_resolution.setChecked(True)
        self.rl_use_resolution.setEnabled(False)
        
        self.rl_use_efficiency = QCheckBox()
        self.rl_use_efficiency.setText("Use Efficiency")
        
        
        label = QLabel("Iterations")
        label.setFixedWidth(130)
        layout.addRow(label, self.rl_iterations)
        
        res_eff_row = QHBoxLayout()
        res_eff_row.addWidget(self.rl_use_resolution)
        res_eff_row.addWidget(self.rl_use_efficiency)
        layout.addRow("Priors", res_eff_row)
        
        # tikhonov_row = QHBoxLayout()
        # self.rl_use_reg_tikhonov = QCheckBox("Use Tikhonov")
        # self.rl_reg_tikhonov_lambda = QDoubleSpinBox(decimals=6, minimum=1e-6, maximum=10, value=1e-3)
        # tikhonov_row.addWidget(self.rl_use_reg_tikhonov)
        # tikhonov_row.addWidget(self.rl_reg_tikhonov_lambda)
        # layout.addRow("Tikhonov\nRegularization", tikhonov_row)

        return page



    def create_mlem_page(self):
        page = QWidget()
        layout = QFormLayout(page)

        self.mlem_iterations = QSpinBox()
        self.mlem_iterations.setRange(1, 100000)
        self.mlem_iterations.setValue(150)

        response_matrix_row = QHBoxLayout()
        btn_load_matrix = QPushButton("Import")
        btn_load_matrix.clicked.connect(self.import_response_matrix)
        
        self.mlem_line_loaded_file = QLineEdit()       
        response_matrix_row.addWidget(self.mlem_line_loaded_file)
        response_matrix_row.addWidget(btn_load_matrix)
        
        self.mlem_use_efficiency = QCheckBox("Use Efficiency")
        
        label = QLabel("Iterations")
        label.setFixedWidth(130)
        layout.addRow(label, self.mlem_iterations)
        layout.addRow("Response Matrix", response_matrix_row)
        layout.addRow("Priors", self.mlem_use_efficiency)

        return page
    
    def set_algorithm(self, idx: int):
        self.algorithm_stack.setCurrentIndex(idx)
        
    def show(self):
        self.set_instrument_combo()
        self.set_spectrum_combo()
        super().show()
        
    def set_instrument_combo(self):
        self.combo_instrument.clear()
        for key, i in sorted(
            SpectrumManager.GenericInstrumentLibrary.instrument_registry.items(),
            key=lambda x: x[1].model,
        ):
            self.combo_instrument.addItem(i.model, key)
        self.combo_instrument.insertSeparator(self.combo_instrument.count())
        for key, i in sorted(
            SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items(),
            key=lambda x: x[1].name,
        ):
            self.combo_instrument.addItem(i.name, key)
            
    def set_spectrum_combo(self):
        self.combo_spectra.clear()
        for key, s in sorted(
            SpectrumManager.spectrum_registry.items(),
            key=lambda x: x[1].name,
        ):
            self.combo_spectra.addItem(s.name, key)
            
    def import_response_matrix(self):
        file_path, _ = QFileDialog.getOpenFileName(
            self, "Import Response Matrix", "", "NPZ Files (*.npz)", options=QFileDialog.Option.DontUseNativeDialog
        )
        if file_path:
            self.mlem_line_loaded_file.setText(file_path)
            
    def calculate(self):
        algorithm_idx = self.combo_algorithm.currentIndex()
        if algorithm_idx == 0:
            deconv_y = self.calculate_rl()
        elif algorithm_idx == 1:
            deconv_y = self.calculate_mlem()
        else:
            raise ValueError("Invalid algorithm index")
        
        spectrum_copy: Spectrum = deepcopy(SpectrumManager.spectrum_registry.get(self.combo_spectra.currentData()))
        
        if spectrum_copy is None:
            QMessageBox.warning(None, "Error", "No spectrum loaded")
            return
        
        new_name = spectrum_copy.name + "_deconvolved"
        
        spectrum_copy.foreground.y_axis = deconv_y
        spectrum_copy.name = new_name
        spectrum_copy.foreground.total_counts = np.sum(deconv_y)
        
        SpectrumManager.create_spectrum(new_name, spectrum_copy.channels)
        SpectrumManager.set_spectrum(new_name, spectrum_copy)
        
    def calculate_rl(self):
        pass
        
            
    def calculate_mlem(self):
        if not self.mlem_line_loaded_file.text():
            return

        spectrum = SpectrumManager.spectrum_registry.get(self.combo_spectra.currentData())
        if spectrum is None:
            return
        
        if not Path(self.mlem_line_loaded_file.text()).is_file():
            QMessageBox.warning(None, "Error", f"{self.mlem_line_loaded_file.text()} is not a file!")
            return
            
        response_matrix = np.load(self.mlem_line_loaded_file.text(), allow_pickle=False)
        try:
            processed_response = process_response(
            response_matrix["response_matrix"],
            response_matrix["indices"].astype(np.float64),
            response_matrix["bin_centres"],
            spectrum.x_axis,
            spectrum.x_axis,
            )
        except KeyError as e:
            QMessageBox.warning(None, "Error", f"Key error in response matrix: {e}")
            return
        
        return ml_em(
            spectrum.get_foreground(),
            processed_response[0],
            processed_response[1].astype(np.int32),
            processed_response[2],
            sensitivity=None,
            iterations=self.mlem_iterations.value(),
            use_sensitivity=False,
        )
        
    
if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = DeconvolutionWindow()
    window.resize(800, 500)
    window.show()

    sys.exit(app.exec())