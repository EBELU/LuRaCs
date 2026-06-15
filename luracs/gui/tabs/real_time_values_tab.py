from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from core.run_manager import CurrentValuesPackage

from PySide6.QtWidgets import (
    QWidget,
    QHBoxLayout,
    QSizePolicy,
    QTableWidget,
    QTableWidgetItem,
)
import pyqtgraph as pg
import numpy as np
from collections import deque


from luracs.gui.misc.idx_table import StrIdxTable
from luracs.utils.color_rotator import ColorRotator
from luracs.core import RunManager, Settings

pg.setConfigOptions(antialias=True)


def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))


class RealTimeValuesPlot(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)

        plot_box = QHBoxLayout()

        self.cps_plot_widget = pg.PlotWidget()
        self.dose_plot_widget = pg.PlotWidget()
        self.color_rotation = ColorRotator(ColorRotator.ColorSchemes(Settings.Appearance.color_rotator_scheme))

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
        self.table = StrIdxTable(columns=titles)
        layout.addWidget(self.table.table, 2.5)

        self.queue_len = Settings.Advanced.real_time_values_deque_length
        self.cps_queues = {}
        self.dose_queues = {}

        self.cps_lines = {}
        self.dose_lines = {}

        self.row_indicies = {}

        RunManager.currentUpdated.connect(self.receive_data_packet)
        RunManager.deviceRemoved.connect(self.handle_device_removal)
        
        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setLimits(xMin=0, xMax=self.queue_len * Settings.Advanced.update_loop_delay)
            
        for lgd in self.legends:
            lgd.setOffset((2, 2))

    def receive_data_packet(self, name: str, packet: CurrentValuesPackage):
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

        self.update_plots(name)
        self.update_values_text(name)

    def update_plots(self, name: str):
        time_interval = Settings.Advanced.update_loop_delay

        x_axis = np.arange(Settings.Advanced.real_time_values_deque_length)[::-1] * time_interval
        
        self.cps_lines[name].setData(x_axis, self.cps_queues[name])
        self.dose_lines[name].setData(x_axis, self.dose_queues[name])


    def update_values_text(self, name: str):
        # Convert deques to numpy arrays for easy calculations
        cps_array = np.array(self.cps_queues[name], dtype=float)
        dr_array = np.array(self.dose_queues[name], dtype=float)

        # Replace None with np.nan if needed
        cps_array = np.nan_to_num(cps_array, nan=0.0)
        dr_array = np.nan_to_num(dr_array, nan=0.0)

        # Current values = most recent
        current_cps = cps_array[-1]
        current_dr = dr_array[-1]
        
        self.table.write_row(name, [name, round(current_cps, 2), round(current_dr, 3)])
        
    def handle_device_removal(self, name: str):
        # Remove CPS plot and data
        if name in self.cps_lines:
            self.cps_plot_widget.removeItem(self.cps_lines[name])
            del self.cps_lines[name]

        self.cps_queues.pop(name, None)

        # Remove Dose Rate plot and data
        if name in self.dose_lines:
            self.dose_plot_widget.removeItem(self.dose_lines[name])
            del self.dose_lines[name]

        self.dose_queues.pop(name, None)
        
        self.table.delete_row(name)

