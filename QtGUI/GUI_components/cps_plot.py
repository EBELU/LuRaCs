from collections import deque

from PySide6.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QHBoxLayout, QMessageBox, QComboBox, QTextEdit, QSizePolicy, QLabel
from PySide6.QtGui import QFont
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
            plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
            plot_widget.setLabel('bottom', "Time [s]")
            plot_widget.invertX(True)
            plot_widget.getAxis('left').enableAutoSIPrefix(False)

        self.cps_plot_widget.setLabel("left", "CPS")
        self.dose_plot_widget.setLabel("left", "Dose Rate [μSv/h]")

        plot_box.addWidget(self.cps_plot_widget)
        plot_box.addWidget(self.dose_plot_widget)

        layout.addLayout(plot_box, 3)

        # Layout for labels
        self.label_box = QVBoxLayout()

        # Fonts
        large_font = QFont()
        large_font.setPointSize(16)
        large_font.setBold(True)

        normal_font = QFont()
        normal_font.setPointSize(12)

        # Dictionary to hold all labels
        self.labels = {}

        # Define label texts and keys
        label_definitions = {
            "cps": ("CPS: 0.0", large_font),
            "dr": ("Dose: 0.000 µSv/h", large_font),
            "cps_5s": ("CPS 5s avg: 0.0", normal_font),
            "dr_5s": ("Dose 5s avg: 0.000 µSv/h", normal_font),
            "cps_30s": ("CPS 30s avg: 0.0", normal_font),
            "dr_30s": ("Dose 30s avg: 0.000 µSv/h", normal_font),
        }

        # Create labels and add them to the layout and dictionary
        for key, (text, font) in label_definitions.items():
            lbl = QLabel(text, alignment=Qt.AlignCenter)
            lbl.setFont(font)
            self.label_box.addWidget(lbl)
            self.labels[key] = lbl

        # Add the label layout to the main layout
        layout.addLayout(self.label_box, 1)


        self.queue_len = 60
        self.cps_queues = {}
        self.dose_queues = {}

        self.cps_lines = {}
        self.dose_lines = {}


    def receive_data_packet(self, packet, name):
        cps = packet.CPS
        dose_rate = packet.DR

        if name not in self.cps_queues:
            self.cps_queues[name] = deque([np.nan]*self.queue_len, self.queue_len)
            pen = pg.mkPen(color="g", width=2)
            self.cps_lines[name] = self.cps_plot_widget.plot([], [], pen=pen, name=name)
        
        self.cps_queues[name].append(cps)

        if name not in self.dose_queues:
            self.dose_queues[name] = deque([np.nan]*self.queue_len, self.queue_len)
            pen = pg.mkPen(color="r", width=2)
            self.dose_lines[name] = self.dose_plot_widget.plot([], [], pen=pen, name=name)

        self.dose_queues[name].append(dose_rate)

        self.update_plots()
        self.update_values_text()
        return True
    

    def update_plots(self):
        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setLimits(xMin=0, xMax=self.queue_len)

        for key, queue in self.cps_queues.items():
            x_axis = np.arange(len(queue))[::-1]
            self.cps_lines[key].setData(x_axis, queue)

        for key, queue in self.dose_queues.items():
            x_axis = np.arange(len(queue))[::-1]
            self.dose_lines[key].setData(x_axis, queue)


    def update_values_text(self):
        # Convert deques to numpy arrays for easy calculations
        cps_array = np.array(self.cps_queues["Raysid"], dtype=float)
        dr_array  = np.array(self.dose_queues["Raysid"], dtype=float)

        # Replace None with np.nan if needed
        cps_array = np.nan_to_num(cps_array, nan=0.0)
        dr_array  = np.nan_to_num(dr_array, nan=0.0)

        # Current values = most recent
        current_cps = cps_array[-1]
        current_dr  = dr_array[-1]

        # 5-second averages (assume 1 value per second)
        avg_len_5s = min(5, len(cps_array))
        cps_5s_avg = np.mean(cps_array[-avg_len_5s:])
        dr_5s_avg  = np.mean(dr_array[-avg_len_5s:])

        # 30-second averages
        avg_len_30s = min(30, len(cps_array))
        cps_30s_avg = np.mean(cps_array[-avg_len_30s:])
        dr_30s_avg  = np.mean(dr_array[-avg_len_30s:])

        # Update labels
        self.labels["cps"].setText(f"CPS: {current_cps:.2f}")
        self.labels["dr"].setText(f"Dose: {current_dr:.3f} µSv/h")

        self.labels["cps_5s"].setText(f"CPS 5s avg: {cps_5s_avg:.2f}")
        self.labels["dr_5s"].setText(f"Dose 5s avg: {dr_5s_avg:.3f} µSv/h")

        self.labels["cps_30s"].setText(f"CPS 30s avg: {cps_30s_avg:.2f}")
        self.labels["dr_30s"].setText(f"Dose 30s avg: {dr_30s_avg:.3f} µSv/h")







