from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from main import MainWindow
    
from PySide6.QtWidgets import QWidget, QVBoxLayout, QStackedWidget, QTabWidget, QMessageBox
from PySide6.QtCore import Signal
from .spectrum_plot import SpectrumPlot

from core import SpectrumManager, Settings

import numpy as np

class SpectrumPlotContainer(QWidget):
    sigModeChanged = Signal()
    sigRedrawRequested = Signal()
    sigTabChanged = Signal(str)
    _sigRedraw = Signal()
    
    def __init__(self, main_window: MainWindow, parent = None):
        super().__init__(parent=parent)
        self.main_window = main_window
        
        SpectrumManager.Signals.spectrumCreated.connect(self.add_tab)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_tab)
        self.main_window.main_menu_bar.sigSetSpectrumViewToCombined.connect(self.set_combined_mode)
        self.main_window.main_menu_bar.sigSetSpectrumViewToTabs.connect(self.set_tabbed_mode)
        SpectrumManager.ROIManager.sigROICreated.connect(self.add_roi_to_plot)
        
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
        self.tabs.currentChanged.connect(lambda : self.sigTabChanged.emit(self.tabs.currentWidget().owned_spectrum if self.tabs.currentWidget() else ""))
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
    
    # --- View modes ---
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
        plot_widget = self.tab_spectrum_plots.pop(spectrum_name)
        self.main_window.theme.unregister_plot(plot_widget)
        index = self.tabs.indexOf(plot_widget)
        self.tabs.removeTab(index)
        plot_widget.deleteLater()
        
        # Clean up rois
        attached_rois = [roi.tag for roi in SpectrumManager.ROIManager.roi_registry.values() if roi.owner_spectrum == spectrum_name]
        for roi_tag in attached_rois:
            SpectrumManager.ROIManager.remove_roi(roi_tag, update_state=False) # Everything is going, dont waste time on updating

    # --- Communication with plots ---
    def add_roi_to_plot(self, roi):
        if Settings.Appearance.tabbed_spectrum_view:
            if roi.owner_spectrum is None:
                current_plot = self.tabs.currentWidget()
                roi.owner_spectrum = current_plot.owned_spectrum
                current_plot.plot_widget.addItem(roi)
            else:
                for i in range(self.tabs.count()):
                    widget = self.tabs.widget(i)
                    if roi.owner_spectrum == widget.owned_spectrum:
                        widget.plot_widget.addItem(roi)
                        break
        else:
            self.single_plot.plot_widget.addItem(roi)
        
        SpectrumManager.ROIManager.on_roi_change(roi_tag=roi.tag)
        
    def request_redraw(self):
        self._sigRedraw.emit()
        
    def match_nuclide_to_rois(self):
        matches = []
        for roi in SpectrumManager.ROIManager.roi_registry.values():
            if roi.owner_spectrum is not None and roi.owner_spectrum != self.tabs.currentWidget().owned_spectrum:
                continue
            
            match = SpectrumManager.NuclideLibrary.match_roi_to_nuclide(roi.tag, energy_search_window = 75)
            
            if match is not None:
                roi.emission = match
                SpectrumManager.ROIManager.update_roi(roi_tag=roi.tag)
                matches.append((roi.alias, roi.emission.parent_nuclide, roi.emission.energy_keV, roi.emission.intensity_percent))
        
        if len(matches) > 0:
            message_strs = []
            
            for alias, nuclide, energy, intensity in matches:
                message_strs.append(f"{alias} --> {nuclide} | [{energy} keV - {intensity} %]")
            
            message = "Nuclides matched to ROIs:\n\n" + "\n".join(message_strs)
            
            QMessageBox.information(self, "Nuclide match", message)
            
        else:
            QMessageBox.information(self, "Nuclide match", "No matching nuclides were found")
                
        
