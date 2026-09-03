from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import SpectrogramROI

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Slot
from PySide6.QtGui import QFont, QPen
from PySide6.QtWidgets import (
    QHBoxLayout,
    QScrollArea,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from luracs.core import RunManager, Settings, core_utils
from luracs.utils.color_rotator import ColorRotator


class _ScrollablePlotWidget(pg.PlotWidget):
    def wheelEvent(self, event):
        event.ignore()

class PlotContainer(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        
        self.main_layout = QVBoxLayout()

        self.scroll_content = QWidget()
        self.scroll_content.setLayout(self.main_layout)
        self.scroll_content.setContentsMargins(0, 0, 0, 0)

        self.scroll_area = QScrollArea()
        self.scroll_area.setWidgetResizable(True)
        self.scroll_area.setWidget(self.scroll_content)

        outer_layout = QVBoxLayout(self)
        outer_layout.addWidget(self.scroll_area)
        
        self.plot_registry: dict[str, pg.PlotWidget] = {}
        self.line_registry: dict[str, dict[str, pg.PlotDataItem]] = {}
        self.legend_registry: dict[str, pg.LegendItem] = {}
        self.color_rotator = ColorRotator(ColorRotator.ColorSchemes(Settings.Appearance.color_rotator_scheme))
        self.color_buffer: dict[str, QPen] = {}
        
    def add_plot(self, roi: str):
        new_plot = _ScrollablePlotWidget()
        new_plot.setContentsMargins(2, 13, 13, 2)
        self.plot_registry[roi] = new_plot
        self.main_layout.addWidget(new_plot)
        
        new_plot.setLabel("bottom", "Time [s]")
        new_plot.setLabel("left", "Count Rate [s⁻¹]")
        new_plot.getViewBox().setMouseEnabled(x=False, y=False)
        new_plot.invertX(True)
        new_plot.setMinimumHeight(150)
        
        
        plot_item = new_plot.getPlotItem()
        plot_item.setTitle(RunManager.SpectrogramManager.roi_registry[roi].alias)
        
        legend = new_plot.addLegend()
        legend.setOffset((2, 2))
        self.legend_registry[roi] = legend
        
        core_utils.ThemeManager.register_plot(new_plot)
        core_utils.ThemeManager.register_legend(legend)
        core_utils.ThemeManager.apply_to_plot(new_plot)
        core_utils.ThemeManager.apply_to_legend(legend)
        
        self.line_registry[roi] = {}
        
        
    def plot_line(self, db_name: str, roi_data: dict):
        time_interval = RunManager.SpectrogramManager.spectrogram_registry[db_name].save_interval        
        for roi, data in roi_data.items():
            if roi not in self.plot_registry:
                self.add_plot(roi)
            
            if db_name not in self.line_registry[roi]:
                plot_item = self.plot_registry[roi].getPlotItem()
                if db_name not in self.color_buffer:
                    self.color_buffer[db_name] = self.color_rotator.next_pen()
                
                self.line_registry[roi][db_name] = plot_item.plot([], [], pen = self.color_buffer[db_name], name=db_name)
            
            x_axis = np.arange(len(data)) * time_interval
            region = x_axis < 60
            
            self.line_registry[roi][db_name].setData(x_axis[region], data[::-1][region])
            
    def db_removed(self, db_name: str):
        for roi_name, plot in self.plot_registry.items():
            plot.getPlotItem().removeItem(self.line_registry[roi_name][db_name])
            
    def roi_removed(self, roi_name: str):
        plot_widget = self.plot_registry.pop(roi_name)
        self.main_layout.removeWidget(plot_widget)
        plot_widget.deleteLater()
        self.main_layout.activate()
        del self.line_registry[roi_name]
        core_utils.ThemeManager.unregister_plot(plot_widget)
        core_utils.ThemeManager.unregister_legend(self.legend_registry.pop(roi_name))
    

    def resize_plot(self, roi_name: str, new_size: int):
        plot = self.plot_registry[roi_name]
        plot.setMinimumHeight(new_size)
        
    def move_plot(self, roi_name: str, new_index: int):
        widget = self.plot_registry[roi_name]
        old_index = self.main_layout.indexOf(widget)

        if old_index == -1 or new_index == old_index:
            return

        self.main_layout.removeWidget(widget)

        # Account for the index shifting after removal
        if new_index > old_index:
            new_index -= 1

        new_index = max(0, min(new_index, self.main_layout.count()))

        self.main_layout.insertWidget(new_index, widget)
        
class StatsTextContainer:
    def __init__(self, roi_name: str):
        self.roi_name = roi_name
        self.buffers: dict[str, tuple[float, float]] = {}
    
    def set_values(self, db_name: str, last_value: float, mean_value: float):
        self.buffers[db_name] = (last_value, mean_value)        
        
    def get_text(self):
        longest_text = 0
        for name in self.buffers:
            longest_text = max(longest_text, len(name))
        
        db_stats = [f"|{name:<{longest_text}}| {self.buffers[name][0]}, ({round(self.buffers[name][1], 2)})" for name in self.buffers]  
        return f"== {self.roi_name} ==\n" + "\n".join(db_stats)
    
    def db_removed(self, db_name: str):
        del self.buffers[db_name]

class SpectrogramROITab(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        
        RunManager.SpectrogramManager.sigROICountsUpdated.connect(self.receive_update)
        RunManager.SpectrogramManager.sigRemoveROI.connect(self.roi_removed)
        RunManager.Signals.spectrogramClosed.connect(self.db_removed)
        
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)

        self.plot_container = PlotContainer(self)

        scroll.setWidget(self.plot_container)

        main_layout = QHBoxLayout(self)
        main_layout.addWidget(scroll, 2)
        
        font = QFont("Monospace")
        font.setStyleHint(QFont.Monospace)

        self.statistics_edit = QTextEdit(readOnly=True)
        self.statistics_edit.setFont(font)
        
        main_layout.addWidget(self.statistics_edit, 1)
        
        self.stats_text_containers: dict[str, StatsTextContainer] = {}
    
    @Slot(str, dict)
    def receive_update(self, db_name: str, roi_data: dict):
        self.plot_container.plot_line(db_name, roi_data)
        for roi, data in roi_data.items():
            if roi not in self.stats_text_containers:
                self.stats_text_containers[roi] = StatsTextContainer(RunManager.SpectrogramManager.roi_registry[roi].alias)
            
            self.stats_text_containers[roi].set_values(db_name, data[-1].astype(float), float(np.nanmean(data)))
            
        self.set_stats_text()
            
    def set_stats_text(self):
        roi_sections = [container.get_text() for container in self.stats_text_containers.values()]
        
        scrollbar = self.statistics_edit.verticalScrollBar()
        position = scrollbar.value()

        self.statistics_edit.setText("\n\n".join(roi_sections))

        scrollbar.setValue(position)
        
    def roi_removed(self, roi: SpectrogramROI):
        roi_name = roi.tag
        self.plot_container.roi_removed(roi_name)
        del self.stats_text_containers[roi_name]
        
        self.set_stats_text()
        
    def db_removed(self, db_name: str):
        self.plot_container.db_removed(db_name)
        for text_container in self.stats_text_containers.values():
            text_container.db_removed(db_name)
            
        self.set_stats_text()

        
if __name__ == "__main__":
    from PySide6.QtWidgets import QApplication
    app = QApplication()

    window = SpectrogramROITab()
    window.resize(800, 600)
    window.show()
    
    window.receive_update("Db", {"SG_ROI_1": np.arange(10), "SG_ROI_2": np.arange(10) / 2,"SG_ROI_3": np.arange(10) / 3,})
    window.receive_update("Db2", {"SG_ROI_1": np.arange(10)*1.2, "SG_ROI_2": np.arange(10) / 2.5, "SG_ROI_3": np.arange(10) / 3,})
    
    # input()
    
    # window.plot_container.roi_removed("SG_ROI_1")

    app.exec()
    
        
