from PySide6.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QHBoxLayout, QMessageBox, QComboBox, QTextEdit, QSizePolicy
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg
import numpy as np

class CurrentValuesPlot(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)

        plot_box = QHBoxLayout()

        self.cps_plot_widget = pg.PlotWidget()
        self.dose_plot_widget = pg.PlotWidget()

        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setSizePolicy(
                QSizePolicy.Expanding, 
                QSizePolicy.Expanding,
            )
            plot_widget.getViewBox().setMouseEnabled(x=False, y=False)

        plot_box.addWidget(self.cps_plot_widget)
        plot_box.addWidget(self.dose_plot_widget)

        layout.addLayout(plot_box, 3)

        self.text_box = QTextEdit()
        self.text_box.setReadOnly(True)
        self.text_box.setText("Program output goes here")
        self.text_box.setSizePolicy(
            QSizePolicy.Expanding,
            QSizePolicy.Expanding,
        )

        layout.addWidget(self.text_box, 1)
