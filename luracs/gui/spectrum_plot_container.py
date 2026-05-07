from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from main import MainWindow
    
from PySide6.QtWidgets import QWidget, QVBoxLayout, QStackedWidget, QTabWidget
from PySide6.QtCore import Signal
from .SpectrumPlot import SpectrumPlot

from core import SpectrumManager

class SpectrumPlotContainer(QWidget):
    sigModeChanged = Signal()
    sigRedrawRequested = Signal()
    _sigRedraw = Signal()
    
    def __init__(self, main_window: MainWindow, parent = None):
        super().__init__(parent=parent)
        self.main_window = main_window
        
        SpectrumManager.Signals.spectrumCreated.connect(self.add_tab)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_tab)
        self.main_window.menu_bar.sigSetSpectrumViewToCombined.connect(self.set_combined_mode)
        self.main_window.menu_bar.sigSetSpectrumViewToTabs.connect(self.set_tabbed_mode)
        
        self.sigModeChanged.connect(SpectrumManager.ROIManager.clear_all)

        self.stack = QStackedWidget()

        # --- Single plot mode ---
        self.single_plot = SpectrumPlot()
        
        self.single_plot.sigRedrawRequested.connect(lambda : self.sigRedrawRequested.emit())
        self._sigRedraw.connect(self.single_plot._redraw)
        
        self.main_window.theme.register_plot(self.single_plot.plot_widget)
        
        self.single_page = QWidget()
        layout = QVBoxLayout(self.single_page)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.single_plot)

        # --- Multi plot mode ---
        self.tabs = QTabWidget()
        self.tabs.setTabPosition(QTabWidget.North)
        self.tabs.setContentsMargins(0,0,0,0)
        self.multi_page = QWidget()
        layout2 = QVBoxLayout(self.multi_page)
        layout2.setContentsMargins(0, 0, 0, 0)
        layout2.addWidget(self.tabs)
        
        self.tab_spectrum_plots: dict[str, SpectrumPlot] = {}

        # Add to stack
        self.stack.addWidget(self.single_page)
        self.stack.addWidget(self.multi_page)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.stack)
        
    def set_combined_mode(self):
        self.stack.setCurrentWidget(self.single_page)
        self.single_plot.reset_zoom()
        self.sigModeChanged.emit()

    def set_tabbed_mode(self):
        self.stack.setCurrentWidget(self.multi_page)
        self.sigModeChanged.emit()
        
    def add_tab(self, spectrum_name):
        plot_widget = SpectrumPlot(owned_spectrum=spectrum_name)
        
        
        self._sigRedraw.connect(plot_widget._redraw)
        plot_widget.sigRedrawRequested.connect(lambda : self.sigRedrawRequested.emit())
        
        self.main_window.theme.register_plot(plot_widget.plot_widget)
        self.tab_spectrum_plots[spectrum_name] = plot_widget
        self.tabs.addTab(plot_widget, spectrum_name)
        
    def remove_tab(self, spectrum_name):
        plot_widget = self.tab_spectrum_plots[spectrum_name]
        
        self.main_window.theme.unregister_plot(plot_widget)
        index = self.tabs.indexOf(plot_widget)
        self.tabs.removeTab(index)
        plot_widget.deleteLater()
        
    def request_redraw(self):
        self._sigRedraw.emit()
        
