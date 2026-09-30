from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import SpectrogramROI

from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QComboBox,
)

import pyqtgraph as pg
import numpy as np

from luracs.core import RunManager


class GraphWindow(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        
        self.sg1_buffer = None
        self.sg2_buffer = None
        
        main_layout = QVBoxLayout(self)
        
        self.sg1_combo = QComboBox()
        self.sg1_combo.currentTextChanged.connect(self.sg_combo_changed)
        self.sg1_roi_combo = QComboBox()
        self.sg1_roi_combo.addItem("Total Count Rate")
        self.sg1_roi_combo.currentTextChanged.connect(self.roi_combo_changed)
        
        self.operation_combo = QComboBox()
        self.operation_combo.addItems(["+", "-", "/"])
        self.operation_combo.currentTextChanged.connect(self.roi_combo_changed)
        
        self.sg2_combo = QComboBox()
        self.sg2_combo.currentTextChanged.connect(self.sg_combo_changed)
        self.sg2_roi_combo = QComboBox()
        self.sg2_roi_combo.addItem("Total Count Rate")
        self.sg2_roi_combo.currentTextChanged.connect(self.roi_combo_changed)
        
        top_combo_layout = QHBoxLayout()
        top_combo_layout.addWidget(self.sg1_combo)
        top_combo_layout.addWidget(self.sg1_roi_combo)
        top_combo_layout.addWidget(self.operation_combo)
        top_combo_layout.addWidget(self.sg2_combo)
        top_combo_layout.addWidget(self.sg2_roi_combo)
        main_layout.addLayout(top_combo_layout)
        
        self.plot_widget = pg.PlotWidget()
        main_layout.addWidget(self.plot_widget)
        
        self.plot_line = self.plot_widget.getPlotItem().plot([], [], pen=pg.mkPen(color="b", width=2))
        
        self.sg1_combo.addItems(list(RunManager.SpectrogramManager.spectrogram_registry))
        self.sg2_combo.addItems(list(RunManager.SpectrogramManager.spectrogram_registry))
        
        RunManager.Signals.spectrogramStarted.connect(self.catch_spectrogram_added)
        RunManager.Signals.spectrogramClosed.connect(self.catch_spectrogram_removed)
        RunManager.SpectrogramManager.sigAddROI.connect(self.catch_spectrogram_roi_added)
        RunManager.SpectrogramManager.sigRemoveROI.connect(self.catch_spectrogram_roi_removed)
        RunManager.SpectrogramManager.sigROICountsUpdated.connect(self.catch_roi_data_emit)
        
        
        
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
        print(sg_name)
        RunManager.SpectrogramManager.spectrogram_registry[sg_name].request_data()
        
    def roi_combo_changed(self):
        if not len(RunManager.SpectrogramManager.spectrogram_registry):
            return
        
        sg1 = self.sg1_combo.currentText()
        RunManager.SpectrogramManager.spectrogram_registry[sg1].request_data()
        
        sg2 = self.sg1_combo.currentText()
        RunManager.SpectrogramManager.spectrogram_registry[sg2].request_data()
        
    def catch_roi_data_emit(self, sg_name: str, roi_data_dict: dict):
        if sg_name == self.sg1_combo.currentText():
            roi_data = roi_data_dict.get(self.sg1_roi_combo.currentData())
            if roi_data is None:
                return
            
            self.sg1_buffer = roi_data
        
        if sg_name == self.sg2_combo.currentText():
            roi_data = roi_data_dict.get(self.sg2_roi_combo.currentData())
            if roi_data is None:
                return
            
            self.sg2_buffer = roi_data
        
        if self.sg1_buffer is None or self.sg2_buffer is None:
            return
        print(self.sg1_buffer, self.sg2_buffer)
        if len(self.sg1_buffer) != len(self.sg2_buffer):
            cutoff = min(len(self.sg1_buffer), len(self.sg2_buffer))
            self.sg1_buffer = self.sg1_combo[:cutoff]
            self.sg2_buffer = self.sg2_combo[:cutoff]
        
        if self.operation_combo.currentText() == "+":
            result = self.sg1_buffer + self.sg2_buffer
        elif self.operation_combo.currentText() == "-":
            result = self.sg1_buffer - self.sg2_buffer
        elif self.operation_combo.currentText() == "/":
            result = self.sg1_buffer / self.sg2_buffer
            
        print(result)
        self.plot_line.setData(np.arange(len(result)), result)
        

        
if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication.instance() or QApplication(sys.argv)

    window = GraphWindow()
    window.show()

    sys.exit(app.exec())
    
    