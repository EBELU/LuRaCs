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
    QPushButton,
    QDoubleSpinBox,
    QHBoxLayout,
)
import pyqtgraph as pg

pg.setConfigOptions(antialias=True)
import sys


app = QApplication.instance() or QApplication(sys.argv)


def value_with_uncertainty():
    container = QWidget()
    layout = QHBoxLayout(container)
    layout.setContentsMargins(0, 0, 0, 0)

    value = QDoubleSpinBox()
    uncertainty = QDoubleSpinBox()

    # Optional: make uncertainty smaller / styled differently
    uncertainty.setPrefix("± ")

    layout.addWidget(value)
    layout.addWidget(uncertainty)

    return container, value, uncertainty


class EfficiencyWindow(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Efficiency Window")
        self.resize(640, 740)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        self.layout = main_layout

        form = QFormLayout()
        form.setSpacing(9)

        self.spectrum_combo = QComboBox()
        self.spectrum_combo.addItem("None")

        self.spectrum_combo.setCurrentIndex(0)  # default to "None"
        form.addRow("Spectrum", self.spectrum_combo)

        titles = ["", "ROI", "Counts", "Yield", "Nuclide"]
        widths = [25, 130, 130, 130, 100]
        self.data_table = QTableWidget(columnCount=len(titles))
        self.data_table.setHorizontalHeaderLabels(titles)
        for i, w in enumerate(widths):
            self.data_table.setColumnWidth(i, w)

        form.addRow("", self.data_table)

        self.instrument_combo = QComboBox()
        self.instrument_combo.addItem("None")
        form.addRow("Instrument", self.instrument_combo)

        # Detector area
        widget, self.detector_area, self.detector_area_unc = value_with_uncertainty()
        form.addRow("Detector area [cm²]", widget)

        # Source-detector distance
        widget, self.source_detector_distance, self.source_detector_distance_unc = (
            value_with_uncertainty()
        )
        form.addRow("Source-Detector\nDistance [cm]", widget)

        self.calculate_button = QPushButton("Calculate")
        form.addRow("", self.calculate_button)

        self.demo_plot = pg.PlotWidget()
        self.demo_plot.setMaximumHeight(250)
        self.demo_plot.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
        self.demo_plot.setLimits(
            xMin=0,
            xMax=3500,
            yMin=0,
            yMax=1,
        )

        form.addRow("Efficiency Plot", self.demo_plot)

        main_layout.addLayout(form)

        bottom_buttons = QHBoxLayout()
        self.assign_to_instrument_btn = QPushButton("Assign to instrument")
        self.close_btn = QPushButton("Close")

        bottom_buttons.addStretch()
        bottom_buttons.addWidget(self.assign_to_instrument_btn)
        bottom_buttons.addWidget(self.close_btn)

        main_layout.addLayout(bottom_buttons)

    def show(self):

        super().show()


e_w = EfficiencyWindow()

res = e_w.exec()

print(res)
