from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import SpectrogramROI

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import QTimer, Signal
from PySide6.QtWidgets import (
    QComboBox,
    QHBoxLayout,
    QMessageBox,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from luracs.core import Log, Settings, SpectrogramManager, core_utils


class _ScrollablePlotWidget(pg.PlotWidget):
    def wheelEvent(self, event):
        event.ignore()


class CalcROI(QWidget):
    sigDeleteRequested = Signal(str)

    def __init__(self, parent=None, label=""):
        super().__init__(parent=parent)

        self.label = label
        self.sg1_buffer = None
        self.sg2_buffer = None

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        self.delete_btn = QPushButton("Remove")
        self.delete_btn.clicked.connect(self.remove_self)

        self.sg1_combo = QComboBox()
        self.sg1_combo.setMaximumWidth(200)
        self.sg1_combo.currentTextChanged.connect(self.sg_combo_changed)
        self.sg1_roi_combo = QComboBox()
        self.sg1_roi_combo.addItem("Total Count Rate", "Total Count Rate")
        self.sg1_roi_combo.currentTextChanged.connect(self.roi_combo_changed)

        self.operation_combo = QComboBox()
        self.operation_combo.setMaximumWidth(50)
        self.operation_combo.addItems(["+", "-", "/"])
        self.operation_combo.currentTextChanged.connect(self.roi_combo_changed)

        self.sg2_combo = QComboBox()
        self.sg2_combo.setMaximumWidth(200)
        self.sg2_combo.currentTextChanged.connect(self.sg_combo_changed)
        self.sg2_roi_combo = QComboBox()
        self.sg2_roi_combo.addItem("Total Count Rate", "Total Count Rate")
        self.sg2_roi_combo.currentTextChanged.connect(self.roi_combo_changed)

        top_combo_layout = QHBoxLayout()
        top_combo_layout.addWidget(self.delete_btn)
        top_combo_layout.addWidget(self.sg1_combo)
        top_combo_layout.addWidget(self.sg1_roi_combo)
        top_combo_layout.addWidget(self.operation_combo)
        top_combo_layout.addWidget(self.sg2_combo)
        top_combo_layout.addWidget(self.sg2_roi_combo)
        main_layout.addLayout(top_combo_layout)

        self.plot_widget = _ScrollablePlotWidget()
        self.plot_widget.setContentsMargins(0, 0, 0, 0)
        self.plot_widget.plotItem.setContentsMargins(0, 0, 0, 0)
        self.plot_widget.getViewBox().setMouseEnabled(x=False, y=False)
        self.plot_widget.invertX(True)
        self.plot_widget.setMinimumHeight(150)
        main_layout.addWidget(self.plot_widget)

        plot_item = self.plot_widget.getPlotItem()
        right_axis = plot_item.getAxis("right")
        right_axis.setStyle(showValues=False, tickLength=0)
        self.plot_widget.setLabel(
            "right",
            label,
            siPrefixEnableRanges=((0.0, 0.0), (1e20, 1e20)),
            **{"font-size": f"{Settings.Appearance.font_size}pt"},
        )

        core_utils.ThemeManager.register_plot(self.plot_widget)
        core_utils.ThemeManager.apply_to_plot(self.plot_widget)

        self.plot_line = self.plot_widget.getPlotItem().plot(
            [], [], pen=pg.mkPen(color="b", width=2)
        )

        self.sg1_combo.addItems(list(SpectrogramManager.spectrogram_registry))
        self.sg2_combo.addItems(list(SpectrogramManager.spectrogram_registry))

        for roi in SpectrogramManager.ROIHandler.roi_registry.values():
            self.sg1_roi_combo.addItem(roi.alias, roi.tag)
            self.sg2_roi_combo.addItem(roi.alias, roi.tag)

        SpectrogramManager.sigSpectrogramStarted.connect(self.catch_spectrogram_added)
        SpectrogramManager.sigSpectrogramClosed.connect(self.catch_spectrogram_removed)
        SpectrogramManager.ROIHandler.sigAddROI.connect(
            self.catch_spectrogram_roi_added
        )
        SpectrogramManager.ROIHandler.sigRemoveROI.connect(
            self.catch_spectrogram_roi_removed
        )

        SpectrogramManager.sigDataUpdated.connect(self.calculate)

        sg1 = SpectrogramManager.spectrogram_registry.get(self.sg1_combo.currentText())
        if sg1 is not None:
            sg1.request_data()

        self.calc_timer = QTimer()
        self.calc_timer.setSingleShot(True)
        self.calc_timer.setInterval(250)

    def remove_self(self):
        reply = QMessageBox.question(
            self,
            "Confirm",
            f"Remove Calc ROI {self.label}",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,  # default button
        )
        if reply == QMessageBox.Yes:
            self.sigDeleteRequested.emit(self.label)

    def catch_spectrogram_removed(self, sg_name: str):
        for i in range(self.sg1_combo.count()):
            sg1_item = self.sg1_combo.itemText(i)
            if sg1_item == sg_name:
                self.sg1_combo.removeItem(i)

            sg2_item = self.sg2_combo.itemText(i)
            if sg2_item == sg_name:
                self.sg2_combo.removeItem(i)

    def catch_spectrogram_added(self, sg_name: str):
        self.sg1_combo.addItem(sg_name)
        self.sg2_combo.addItem(sg_name)

    def catch_spectrogram_roi_removed(self, roi: SpectrogramROI):
        for i in range(self.sg1_roi_combo.count()):
            sg1_item = self.sg1_roi_combo.itemData(i)
            if sg1_item == roi.tag:
                self.sg1_roi_combo.removeItem(i)

            sg2_item = self.sg2_roi_combo.itemData(i)
            if sg2_item == roi.tag:
                self.sg2_roi_combo.removeItem(i)

    def catch_spectrogram_roi_added(self, roi: SpectrogramROI):
        self.sg1_roi_combo.addItem(roi.alias, roi.tag)
        self.sg2_roi_combo.addItem(roi.alias, roi.tag)

    def sg_combo_changed(self, sg_name: str):
        if not sg_name:
            return
        SpectrogramManager.spectrogram_registry[sg_name].request_data()

    def roi_combo_changed(self):
        if not len(SpectrogramManager.spectrogram_registry):
            return

        sg1 = self.sg1_combo.currentText()
        SpectrogramManager.spectrogram_registry[sg1].request_data()

        sg2 = self.sg1_combo.currentText()
        SpectrogramManager.spectrogram_registry[sg2].request_data()

    def calculate(self):
        # # To avoid unnecessary computation the calculation is limited to once every 250ms
        # # So when two detectors are connected is only calculates for the first reading so its trailing
        # if self.calc_timer.isActive():
        #     return

        # self.calc_timer.start()

        sg1_name = self.sg1_combo.currentText()
        sg2_name = self.sg2_combo.currentText()

        sg1_roi = self.sg1_roi_combo.currentData()
        sg2_roi = self.sg2_roi_combo.currentData()

        if sg1_roi == "Total Count Rate":
            self.sg1_buffer = SpectrogramManager.get_count_rate_data(sg1_name)
        else:
            self.sg1_buffer = SpectrogramManager.get_roi_data(sg1_name, sg1_roi)

        if sg2_roi == "Total Count Rate":
            self.sg2_buffer = SpectrogramManager.get_count_rate_data(sg2_name)
        else:
            self.sg2_buffer = SpectrogramManager.get_roi_data(sg2_name, sg2_roi)

        Log.debug(
            f"SpectrogramManager.spectrogram_registry.keys(): {SpectrogramManager.spectrogram_registry.keys()}"
        )
        Log.debug(
            f"SG1: {sg1_name}, ROI: {sg1_roi}, Buffer Length: {len(self.sg1_buffer) if self.sg1_buffer is not None else 'None'}"
        )
        Log.debug(
            f"SG2: {sg2_name}, ROI: {sg2_roi}, Buffer Length: {len(self.sg2_buffer) if self.sg2_buffer is not None else 'None'}"
        )

        if self.sg1_buffer is None or self.sg2_buffer is None:
            return

        if len(self.sg1_buffer) != len(self.sg2_buffer):
            cutoff = min(len(self.sg1_buffer), len(self.sg2_buffer))
            sg1_data = self.sg1_buffer[:cutoff][::-1]
            sg2_data = self.sg2_buffer[:cutoff][::-1]
        else:
            sg1_data = self.sg1_buffer[::-1]
            sg2_data = self.sg2_buffer[::-1]

        sg1 = SpectrogramManager.spectrogram_registry.get(self.sg1_combo.currentText())
        sg2 = SpectrogramManager.spectrogram_registry.get(self.sg2_combo.currentText())
        if sg1 is None or sg2 is None:
            return

        sg1_ti = sg1.save_interval
        sg2_ti = sg2.save_interval

        longest_ti = max(sg1_ti, sg2_ti)

        if self.operation_combo.currentText() == "+":
            result = sg1_data + sg2_data
        elif self.operation_combo.currentText() == "-":
            result = sg1_data - sg2_data
        elif self.operation_combo.currentText() == "/":
            result = sg1_data / sg2_data

        x_axis = np.arange(len(result)) * longest_ti
        region = x_axis <= Settings.Advanced.spectrogram_roi_display_length_s

        self.plot_line.setData(x_axis[region], result[region])


if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = CalcROI()
    window.show()

    sys.exit(app.exec())
