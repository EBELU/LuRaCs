from __future__ import annotations

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Qt, Slot
from PySide6.QtWidgets import (
    QHBoxLayout,
    QSizePolicy,
    QSplitter,
    QWidget,
)

from luracs.core import RunManager, Settings
from luracs.gui.misc.idx_table import StrIdxTable
from luracs.utils.color_rotator import ColorRotator

pg.setConfigOptions(antialias=True)

class RealTimeValuesPlot(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)

        main_splitter = QSplitter(Qt.Horizontal)
        plot_splitter = QSplitter(Qt.Horizontal)

        self.cps_plot_widget = pg.PlotWidget()
        self.dose_plot_widget = pg.PlotWidget()
        self.color_rotation = ColorRotator(
            ColorRotator.ColorSchemes(Settings.Appearance.color_rotator_scheme)
        )

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

        plot_splitter.addWidget(self.cps_plot_widget)
        plot_splitter.addWidget(self.dose_plot_widget)

        titles = ["Device", "Count Rate\n[s⁻¹]", "Dose Rate\n[μSv/h]"]
        self.table = StrIdxTable(columns=titles)
        main_splitter.addWidget(plot_splitter)
        main_splitter.addWidget(self.table.table)
        
        plot_splitter.setStretchFactor(0, 3)
        plot_splitter.setStretchFactor(1, 3)
        main_splitter.setStretchFactor(0, 2)
        main_splitter.setStretchFactor(1, 3)
        
        layout.addWidget(main_splitter)

        self.queue_len = Settings.Advanced.real_time_values_deque_length

        self.x_axis = (
            np.arange(Settings.Advanced.real_time_values_deque_length)[::-1]
            * Settings.Advanced.update_loop_delay
        )

        self.cps_lines = {}
        self.dose_lines = {}

        self.cps_mean_lines = {}
        self.dr_mean_lines = {}
        self.showing_mean_lines: bool = False

        self.row_indicies = {}

        RunManager.Signals.realTimeBuffersUpdated.connect(self.receive_buffers)
        RunManager.Signals.deviceConnected.connect(self.device_added)
        RunManager.Signals.deviceRemoved.connect(self.device_removed)

        for plot_widget in (self.cps_plot_widget, self.dose_plot_widget):
            plot_widget.setLimits(
                xMin=0, xMax=self.queue_len * Settings.Advanced.update_loop_delay
            )

        for lgd in self.legends:
            lgd.setOffset((2, 2))

    def device_added(self, name: str):
        pen = self.color_rotation.next_pen()
        # --- CPS ---
        if name not in self.cps_lines:
            self.cps_lines[name] = self.cps_plot_widget.plot([], [], pen=pen, name=name)

        # --- Dose Rate ---
        if name not in self.dose_lines:
            self.dose_lines[name] = self.dose_plot_widget.plot(
                [], [], pen=pen, name=name
            )

        mean_pen = pg.mkPen(color=pen.color(), width=pen.width(), style=Qt.PenStyle.DashLine)
        self.cps_mean_lines[name] = pg.InfiniteLine(angle=0, pen=mean_pen)
        self.dr_mean_lines[name] = pg.InfiniteLine(angle=0, pen=mean_pen)

        self.toggle_mean_lines(self.showing_mean_lines)

    def device_removed(self, name: str):
        line = self.cps_lines.pop(name, None)
        if line is not None:
            self.cps_plot_widget.removeItem(line)

        line = self.dose_lines.pop(name, None)
        if line is not None:
            self.dose_plot_widget.removeItem(line)

        self.table.delete_row(name)

        cps_line = self.cps_mean_lines.pop(name)
        dr_line = self.dr_mean_lines.pop(name)

        self.cps_plot_widget.removeItem(cps_line)
        self.dose_plot_widget.removeItem(dr_line)

    def toggle_mean_lines(self, state: bool):
        for cps_line, dr_line in zip(
            self.cps_mean_lines.values(), self.dr_mean_lines.values()
        ):
            if state:
                self.cps_plot_widget.addItem(cps_line)
                self.dose_plot_widget.addItem(dr_line)
            else:
                self.cps_plot_widget.removeItem(cps_line)
                self.dose_plot_widget.removeItem(dr_line)

        self.showing_mean_lines = state

    @Slot(str, object, object)
    def receive_buffers(self, name: str, cps_buffer: np.ndarray, dr_buffer: np.ndarray):
        self.update_plots(name, cps_buffer, dr_buffer)
        self.update_values_text(name, cps_buffer, dr_buffer)

    def update_plots(self, name: str, cps_buffer: np.ndarray, dr_buffer: np.ndarray):
        self.cps_lines[name].setData(self.x_axis, cps_buffer)
        self.dose_lines[name].setData(self.x_axis, dr_buffer)

        self.cps_mean_lines[name].setPos(np.nanmean(cps_buffer))
        self.dr_mean_lines[name].setPos(np.nanmean(dr_buffer))

    def update_values_text(
        self, name: str, cps_array: np.ndarray, dr_array: np.ndarray
    ):
        # Last value = most recent
        current_cps = cps_array[-1]
        current_dr = dr_array[-1]

        self.table.write_row(name, [name, round(current_cps, 2), round(current_dr, 3)])
