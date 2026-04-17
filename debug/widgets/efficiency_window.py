from PySide6.QtWidgets import (
    QApplication,
    QDialog,
    QVBoxLayout,
    QFormLayout,
    QComboBox,
    QWidget,
    QCheckBox,
    QTableWidget,
    QTableWidgetItem,
)
import pyqtgraph as pg

pg.setConfigOptions(antialias=True)
import sys


app = QApplication.instance() or QApplication(sys.argv)


class ROIsWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)

        table = QTableWidget()
        table.setColumnCount(4)
        table.setHorizontalHeaderLabels(["Name", "Include", "Energy", "Probability"])
        table.setRowCount(3)  # dummy number of ROIs
        for i in range(3):
            table.setItem(i, 0, QTableWidgetItem(f"ROI {i + 1}"))
            checkbox = QCheckBox()
            checkbox.setChecked(False)  # default to excluded
            table.setCellWidget(i, 1, checkbox)
        layout.addWidget(table)


class EfficiencyWindow(QDialog):
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
        spectrum_combo.addItems(
            ["No Spectrum", "Spectrum 1", "Spectrum 2", "Spectrum 3"]
        )  # dummy items
        spectrum_combo.setCurrentIndex(0)  # default to "No Spectrum"
        form.addRow("Chosen Spectrum", spectrum_combo)

        rois_widget = ROIsWidget()
        form.addRow("ROIs", rois_widget)

        form.addRow("Efficiency Plot", pg.PlotWidget())  # placeholder for the plot

        main_layout.addLayout(form)


e_w = EfficiencyWindow()

res = e_w.exec()

print(res)
