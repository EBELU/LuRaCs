from collections import deque
from itertools import cycle

from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QPushButton,
    QHBoxLayout,
    QMessageBox,
    QComboBox,
    QTextEdit,
    QSizePolicy,
    QLabel,
    QTableWidget,
    QTableWidgetItem,
)
from PySide6.QtGui import QFont
from PySide6.QtCore import Qt, Signal
from PySide6 import QtGui
import pyqtgraph as pg

pg.setConfigOptions(antialias=True)
import numpy as np


def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))


from utils.color_rotator import ColorRotator
from core import RunManager


class CurrentValuesPlot(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)

        plot_box = QHBoxLayout()

        self.cps_plot_widget = pg.PlotWidget()
        self.dose_plot_widget = pg.PlotWidget()
        self.color_rotation = ColorRotator("lo")

        self.legends = []

        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setSizePolicy(
                QSizePolicy.Expanding,
                QSizePolicy.Expanding,
            )
            plot_widget.getViewBox().setMouseEnabled(x=False, y=False)
            plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)
            plot_widget.setLabel("bottom", "Time [s]")
            plot_widget.invertX(True)
            plot_widget.getAxis("left").enableAutoSIPrefix(False)

            legend = plot_widget.addLegend()
            legend.setOffset((2, 2))

            self.legends.append(legend)

        self.cps_plot_widget.setLabel("left", "CPS")
        self.dose_plot_widget.setLabel("left", "Dose Rate [μSv/h]")

        plot_box.addWidget(self.cps_plot_widget)
        plot_box.addWidget(self.dose_plot_widget)

        layout.addLayout(plot_box, 7.5)

        titles = ["Device", "Count Rate\n[s⁻¹]", "Does Rate\n[μSv/h]"]
        self.table = QTableWidget(0, len(titles))
        self.table.setColumnCount(len(titles))
        self.table.setHorizontalHeaderLabels(titles)

        # self.table.setMaximumWidth(
        #     self.table.verticalHeader().width()
        #     + self.table.horizontalHeader().length()
        #     + self.table.frameWidth() * 2 + 13
        # )
        self.table.setSizePolicy(
            QSizePolicy.Expanding,  # vertical
            QSizePolicy.Expanding,  # horizontal
        )
        layout.addWidget(self.table, 2.5)

        self.queue_len = 60
        self.cps_queues = {}
        self.dose_queues = {}

        self.cps_lines = {}
        self.dose_lines = {}

        self.row_indicies = {}

        RunManager.currentUpdated.connect(self.receive_data_packet)

    def receive_data_packet(self, name, packet):
        cps = packet.CPS
        dose_rate = packet.DR

        pen = None
        # --- CPS ---
        if name not in self.cps_queues:
            self.cps_queues[name] = deque([np.nan] * self.queue_len, self.queue_len)
            pen = self.color_rotation.next_pen()
            self.cps_lines[name] = self.cps_plot_widget.plot([], [], pen=pen, name=name)

        self.cps_queues[name].append(cps)
        # --- Dose Rate ---
        if name not in self.dose_queues:
            self.dose_queues[name] = deque([np.nan] * self.queue_len, self.queue_len)
            if pen is None:
                pen = self.color_rotation.next_pen()
            self.dose_lines[name] = self.dose_plot_widget.plot(
                [], [], pen=pen, name=name
            )

        self.dose_queues[name].append(dose_rate)

        self.update_plots()
        self.update_values_text()
        return True

    def update_plots(self):
        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setLimits(xMin=0, xMax=self.queue_len / 2)

        for key, queue in self.cps_queues.items():
            x_axis = np.arange(len(queue))[::-1] / 2
            self.cps_lines[key].setData(x_axis, queue)

        for key, queue in self.dose_queues.items():
            x_axis = np.arange(len(queue))[::-1] / 2
            self.dose_lines[key].setData(x_axis, queue)

        for lgd in self.legends:
            lgd.setOffset((2, 2))

    def update_values_text(self):
        for device in self.cps_queues.keys():
            # Convert deques to numpy arrays for easy calculations
            cps_array = np.array(self.cps_queues[device], dtype=float)
            dr_array = np.array(self.dose_queues[device], dtype=float)

            # Replace None with np.nan if needed
            cps_array = np.nan_to_num(cps_array, nan=0.0)
            dr_array = np.nan_to_num(dr_array, nan=0.0)

            # Current values = most recent
            current_cps = cps_array[-1]
            current_dr = dr_array[-1]

            # 5-second averages (assume 1 value per second)
            avg_len_5s = min(5, len(cps_array))
            cps_5s_avg = np.mean(cps_array[-avg_len_5s:])
            dr_5s_avg = np.mean(dr_array[-avg_len_5s:])

            # 30-second averages
            avg_len_30s = min(30, len(cps_array))
            cps_30s_avg = np.mean(cps_array[-avg_len_30s:])
            dr_30s_avg = np.mean(dr_array[-avg_len_30s:])

            if device not in self.row_indicies:
                row_index = self.table.rowCount()
                self.table.insertRow(row_index)
                self.row_indicies[device] = row_index
            else:
                row_index = self.row_indicies[device]

            row = [device, round(current_cps, 2), round(current_dr, 3)]
            write_row(self.table, row_index, row)
