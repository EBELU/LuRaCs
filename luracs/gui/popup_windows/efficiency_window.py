from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QFormLayout,
    QComboBox,
    QWidget,
    QTableWidget,
    QCheckBox,
    QLabel,
    QHBoxLayout,
)
import pyqtgraph as pg

from core import SpectrumManager


class ROIsWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(6)

        self.table = QTableWidget()
        self.table.setColumnCount(3)
        self.table.setHorizontalHeaderLabels(["Name", "Energy", "Probability"])

        main_layout.addWidget(self.table)

    def update_rois(self, rois):
        self.table.clearContents()
        self.table.setRowCount(len(rois))

        for i, roi in enumerate(rois.values()):
            # --- Column 0: Name + Checkbox ---
            container = QWidget()
            layout = QHBoxLayout(container)
            layout.setContentsMargins(0, 0, 0, 0)
            layout.setSpacing(6)

            checkbox = QCheckBox()
            checkbox.setStyleSheet("""
                QCheckBox::indicator {
                    border: 1px solid #ccc;
                }

                QCheckBox::indicator:checked {
                    background: lightgreen;
                }
            """)
            label = QLabel(roi.tag)

            layout.addWidget(checkbox)
            layout.addWidget(label)

            self.table.setCellWidget(i, 0, container)

            # --- Energy ---
            if roi.fit is not None:
                energy = roi.fit.mu
                energy_err = roi.fit.mu_err
                energy_item = QLabel(f"{energy:.2f} ± {energy_err:.2f}")
            else:
                energy_item = QLabel("N/A")
            self.table.setCellWidget(i, 1, energy_item)

            if roi.fit is not None and roi.fit.G != 0:
                prob = roi.fit.N / roi.fit.G
                prob_item = QLabel(f"{prob:.3f}")
            else:
                prob_item = QLabel("N/A")

            self.table.setCellWidget(i, 2, prob_item)


class EfficiencyWindow(QDialog):
    def on_spectrum_changed(self, spectrum_name):
        if spectrum_name == "No Spectrum":
            self.rois_widget.update_rois({})
            return

        rois = SpectrumManager.ROIManager.get_data_from_spectrum(spectrum_name)

        self.rois_widget.update_rois(rois)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Efficiency Window")
        self.resize(540, 640)

        self.setMinimumWidth(540)
        self.setMinimumHeight(640)
        self.setMaximumWidth(540)
        self.setMaximumHeight(640)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        self.layout = main_layout

        form = QFormLayout()
        form.setSpacing(9)

        spectrum_combo = QComboBox()
        spectrum_combo.addItems(["No Spectrum", *SpectrumManager.spectra])
        spectrum_combo.setCurrentIndex(0)

        form.addRow("Chosen Spectrum", spectrum_combo)

        self.rois_widget = ROIsWidget()
        form.addRow("ROIs", self.rois_widget)

        form.addRow("Efficiency Plot", pg.PlotWidget())

        main_layout.addLayout(form)

        spectrum_combo.currentTextChanged.connect(self.on_spectrum_changed)

        # Initial population
        self.on_spectrum_changed(spectrum_combo.currentText())
