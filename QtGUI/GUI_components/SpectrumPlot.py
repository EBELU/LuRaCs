from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton, QHBoxLayout, QMessageBox, QComboBox
from PySide6.QtCore import Qt, QObject, Signal
from PySide6.QtGui import QColor
import pyqtgraph as pg
import numpy as np

from core import SpectrumManager, Settings

from SpectrumClasses import Spectrum

class EmittedSignals(QObject):
    updateROI = Signal(str, float, float, bool)
    removeROI = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)

class DeletableROI(pg.LinearRegionItem):
    """Visual ROI selector modified to have a 'tag' and can be deleted by right clicking"""
    sigDeleteRequested = Signal(str) 

    def __init__(
        self,
        tag: str,
        region,
        *,
        orientation='vertical',
        movable=True,
        parent=None,
    ):
        super().__init__(
            values=region,
            orientation=orientation,
            movable=movable
        )
        self.tag = tag

        self.setToolTip(f"ROI: {self.tag}\nRight-click to delete")

    def mouseClickEvent(self, ev):
        if ev.button() == Qt.RightButton:
            ev.accept()
            self.sigDeleteRequested.emit(self.tag)
        else:
            super().mouseClickEvent(ev)


class SpectrumPlot(QWidget):
    """Class to control and manage the plotting of spectra.
    
        Does not manage the spectra, can only request operations from SpectrumManager.
    """
    def __init__(self, xlabel="Channel", ylabel="Energy [keV]", parent=None):
        super().__init__(parent)
        
        # --- Signals ---
        
        self.Signals = EmittedSignals()
        
        SpectrumManager.Signals.spectrumUpdated.connect(self.update_plot)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_plot)
        SpectrumManager.Signals.backgroundRemoved.connect(lambda *args: self._redraw())
        SpectrumManager.Signals.visibilityChanged.connect(lambda *args: self._redraw())
        
        self.Signals.updateROI.connect(SpectrumManager.update_ROI)
        self.Signals.removeROI.connect(SpectrumManager.remove_ROI)
        
        
        # --- Layout ---
        
        layout = QVBoxLayout(self)
        layout.setContentsMargins(1,1,1,1)        
        
        btn_layout = QHBoxLayout()
        layout.addLayout(btn_layout)
        self.plot_widget = pg.PlotWidget()
        layout.addWidget(self.plot_widget)
        
        
        
        self.plot_widget.setLabel('bottom', xlabel)
        self.plot_widget.setLabel('left', ylabel)
        self.plot_widget.showGrid(x=True, y=True)
        # self.plot_widget.getAxis('left').enableAutoSIPrefix(False)
        self.plot_widget.setXRange(0, 2500, padding=0) 
        self.plot_widget.setLimits(
            xMin=0, xMax=3500,
            yMin=0, yMax=1e6,
            minXRange=10, maxXRange=3500,
            minYRange=1e-4, maxYRange=1e6
        )
        self.plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)

        self.plot_widget.getViewBox().setMouseEnabled(x=True, y=False)
        self.view = self.plot_widget.getViewBox()
        self.plot_widget.enableAutoRange()

        # Buttons
        

        self.btn_reset_zoom = QPushButton("Reset Zoom")
        self.btn_y_axis_lock   = QPushButton("Unlock y-axis")
        self.btn_lin_log = QPushButton("Log")
        self.btn_cps =  QPushButton("CPS")
        self.btn_mark_roi   = QPushButton("Add ROI")
        


        self.cbox_bkg_choises = QComboBox()

        self.cbox_bkg_choises.addItems(["No Background", "Background Overlay", "Background Subtract"])
        self.cbox_bkg_choises.setCurrentIndex(0)
        
        btn_layout.addWidget(self.btn_reset_zoom)
        btn_layout.addWidget(self.btn_lin_log)
        btn_layout.addWidget(self.btn_cps)
        btn_layout.addWidget(self.btn_y_axis_lock)
        btn_layout.addWidget(self.btn_mark_roi)
        btn_layout.addWidget(self.cbox_bkg_choises)

        # --- Assign button callbacks ---
        self.btn_reset_zoom.clicked.connect(self.reset_zoom)
        self.btn_mark_roi.clicked.connect(self.add_roi)
        self.btn_lin_log.clicked.connect(self.change_lin_log)
        self.btn_cps.clicked.connect(self._set_cps)
        self.btn_y_axis_lock.clicked.connect(self.lock_y_axis)
        self.cbox_bkg_choises.currentIndexChanged.connect(self._on_bkg_option_selection)

        self.primary_lines = {}
        self.bkg_lines = {}
        self.primary_spectrum = None

        self.ROIs = {}  # ROI slection objects, spectra track their own rois
        self.ROI_lines_gasussian = {}   # Gaussian cruve lines
        self.ROI_lines_linear = {}  # Background correction lines
        self.roi_counter = 0    #Increments for each added roi, ensures unique tags

        self.y_axis_locked = True
        self.user_scaled = False
        
        # --- Plotting states ---
        self.log = False
        self.cps = False
        self.show_bkg = False
        self.bkg_sub = False

        self.plot_widget.enableAutoRange(axis=pg.ViewBox.YAxis)
        self.plot_widget.setAutoVisible(y=True)
        SpectrumManager.Signals.colorUpdated.connect(self._redraw)




    def _on_bkg_option_selection(self, option):
        """Change how the background is handeled"""
        if option == 0:
            self.show_bkg = False
            self.bkg_sub = False
            self.btn_cps.setEnabled(True)
            self._set_cps(False, self.cps, False)


        elif option == 1:
            self.show_bkg = True
            self.bkg_sub = False
            self._set_cps(False, True, False)
            self.btn_cps.setEnabled(False)

        elif option == 2:
            self.cps = True
            self.show_bkg = True
            self.bkg_sub = True
            self._set_cps(False, True, False)
            self.btn_cps.setEnabled(False)

        # Finish by redrawing
        self._redraw()


    def _set_cps(self,_ , cps_bool = None, recalculate = True):
        if cps_bool is None:
            # If the change is not made explicitly
            if self.cps is True:
                cps_bool = False
                self.cps = False
            else:
                cps_bool = True
                self.cps = True

        if cps_bool:
            if self.log:
                self.plot_widget.setLimits(
                yMin=-10, yMax=1e6)
                
            else:
                self.plot_widget.setLimits(
                yMin=0, yMax=1e6)
                
            self.cps = True
            self.plot_widget.setLabel('left', "CPS")
            self.btn_cps.setText("Counts")
        else:
            self.plot_widget.setLimits(
                yMin=0, yMax=1e6)
            self.cps = False
            self.plot_widget.setLabel('left', "Counts")
            self.btn_cps.setText("CPS")
        if recalculate:
            self._redraw()
        


    def lock_y_axis(self):
        """Lock or unlock if the y-axis can be zoomed in the spectrum plot"""
        if self.y_axis_locked:
            self.plot_widget.getViewBox().setMouseEnabled(x=True, y=True)
            self.y_axis_locked = False
            self.btn_y_axis_lock.setText("Lock y-axis")
        else:
            self.plot_widget.getViewBox().setMouseEnabled(x=True, y=False)
            self.y_axis_locked = True
            self.btn_y_axis_lock.setText("Unlock y-axis")

    def _redraw(self):
        """Redraw everything on the plot"""
        self.plot_widget.clear()
        self.primary_lines.clear()
        self.bkg_lines.clear()
        for spect_name in SpectrumManager.get_spectra_dict().keys():
            self.update_plot(spect_name)

        self.ROI_lines_gasussian.clear()
        self.ROI_lines_linear.clear()
        self.update_all_rois()

    
    def reset_zoom(self):
        self.user_scaled = False
        self.plot_widget.setXRange(0, 2500, padding=0)
        self.plot_widget.enableAutoRange()

        
        
    def update_plot(self, name):
        """Primary method for updating a spectrum plot"""
        spect = SpectrumManager.get_spectrum(name)
        if not spect.show_in_plot:
            return        
        
        if self.bkg_sub:
            self.plot_bkg_subtract(spect)
        else:
            self.plot_primary(spect)
            self.plot_bkg(spect)


    def plot_primary(self, spectrum: Spectrum):
        """Plots the foreground spectrum"""
        if spectrum.name not in self.primary_lines:
            pen = pg.mkPen(spectrum.color_foreground, width=2)

            if Settings.Appearance.brush:
                brush = QColor(spectrum.color_foreground)
                brush.setAlpha(150)
            else:
                brush = None
                
            fill_level = 0
            if self.log:
                fill_level = np.floor(np.nanmin(spectrum.get_foreground(self.log, self.cps)))


            self.primary_lines[spectrum.name] = self.plot_widget.plot(
                [],
                [],
                name=spectrum.name,
                pen=pen,
                brush=brush,
                fillLevel=fill_level,
                stepMode="center",
            )

        foreground = spectrum.get_foreground(self.log, self.cps)
        if foreground is None:
            return

        if self.log:
            bkg = np.inf
            if self.show_bkg and spectrum.get_background() is not None:
                bkg = np.nanmin(spectrum.get_background(self.log, self.cps))
                
            self.primary_lines[spectrum.name].setFillLevel(np.floor(min(np.nanmin(foreground), bkg) * 2) / 2)

        
        self.primary_lines[spectrum.name].setData(
            spectrum.x_axis,
            foreground[:-1],
        )


    def plot_bkg(self, spectrum: Spectrum):
        if not self.show_bkg or spectrum.get_background() is None:
            return

        if spectrum.name not in self.bkg_lines:
            pen = pg.mkPen(spectrum.color_background, width=2)

            if Settings.Appearance.brush:
                brush = QColor(spectrum.color_background)
                brush.setAlpha(150)
            else:
                brush = None
                
            fill_level = 0
            if self.log:
                fill_level = np.floor(np.nanmin(spectrum.get_background(self.log, self.cps)) * 2) / 2

            line = self.plot_widget.plot(
                [],
                [],
                name=spectrum.name,
                pen=pen,
                brush=brush,
                fillLevel=fill_level,
                stepMode="center",
            )
            line.setZValue(1) # Place on top

            self.bkg_lines[spectrum.name] = line
            
        background = spectrum.get_background(self.log, self.cps)
        if self.log:
            self.bkg_lines[spectrum.name].setFillLevel(np.floor(np.nanmin(background) * 2) / 2)
        self.bkg_lines[spectrum.name].setData(
            spectrum.x_axis,
            background[:-1],
        )

        
                
    def plot_bkg_subtract(self, spectrum: Spectrum):
        # Skip if background or live times invalid
        if (
            spectrum.get_background() is None # No bkg
            or spectrum.foreground.live_time == 0   # No fg live time
            or spectrum.background.live_time == 0   # No bkg live time
        ):
            return

        if spectrum.name not in self.primary_lines:
            # Pen uses foreground color
            pen = pg.mkPen(spectrum.color_foreground, width=2)

            # Brush with semi-transparent alpha
            brush = QColor(spectrum.color_foreground)
            brush.setAlpha(150)
            
            fill_level = 0
            if self.log:
                fill_level = np.floor(np.nanmin(spectrum.get_bkg_sub(self.log)) * 2) / 2

            # Create the plot line
            self.primary_lines[spectrum.name] = self.plot_widget.plot(
                [],
                [],
                name=spectrum.name,
                pen=pen,
                brush=brush,
                fillLevel=fill_level,
                stepMode="center",
            )

        # Update the data with background-subtracted spectrum
        bkg_sub_spect = spectrum.get_bkg_sub(self.log)
        if self.log:
            self.primary_lines[spectrum.name].setFillLevel(np.floor(np.nanmin(bkg_sub_spect) * 2) / 2)

        self.primary_lines[spectrum.name].setData(
            spectrum.x_axis,
            bkg_sub_spect[:-1],
        )

    def remove_plot(self, name:str):
        self.primary_lines.pop(name, None)
        self.bkg_lines.pop(name, None)
        for roi in self.ROIs:
            try:
                tag = roi.tag
            except AttributeError as e:
                print(f"Remove plot failed for {name} with {e}")
                raise
            self.ROI_lines_linear.pop(name + tag, None)
            self.ROI_lines_gasussian.pop(name + tag, None)
        
        self._redraw()
            
            
    def change_lin_log(self):
        """Change lin log at data retrieval"""
        if self.log == False:
            self.log = True
            self.plot_widget.setLimits(
            yMin=-10, yMax=1e6)
            self.btn_lin_log.setText("Lin")

        else:
            self.log = False
            self.btn_lin_log.setText("Log")
            self.plot_widget.setLimits(
            yMin=0, yMax=1e6,

        )
        self._redraw()
            
    

    def add_roi(self, x_low = None, x_high = None):
        """Enable interactive ROI marking with LinearRegionItem."""
        roi_tag = SpectrumManager.create_ROI()
        
        # Pick a good position in the plit to spawn the new roi
        x_min, x_max = self.plot_widget.viewRange()[0]


        diff = float(x_max) - float(x_min)
        if diff > 400: diff = 400
        if not x_low: 
            x_low = float(x_min) + diff * 0.15 
        if not x_high: 
            x_high = float(x_min) + diff * 0.45



        new_roi = DeletableROI(roi_tag,[x_low, x_high], movable=True)
        self.plot_widget.addItem(new_roi)
        self.ROIs[roi_tag] = new_roi
        
        new_roi.sigDeleteRequested.connect(self.remove_roi)
        new_roi.sigRegionChangeFinished.connect(
            lambda: self.update_roi(new_roi)
        )
        
        self.update_roi(new_roi)
    
    def update_all_rois(self):
        for roi in self.ROIs.values():
            if roi not in self.plot_widget.plotItem.items:
                self.plot_widget.addItem(roi)
            self.update_roi(roi)

    def update_roi(self, roi_selection):
        x_min, x_max = roi_selection.getRegion()
        x_min, x_max = float(x_min), float(x_max)

        self.Signals.updateROI.emit(roi_selection.tag, x_min, x_max, self.cps)
            
        for spectrum_tag, spectrum in SpectrumManager.get_spectra_dict().items():
            if not spectrum.show_in_plot:
                continue
            for roi_tag, roi in spectrum.ROIs.items():
                if spectrum_tag+roi_tag not in self.ROI_lines_gasussian:
                    pen = pg.mkPen(color="w", width=1.3)
                    line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
                    self.ROI_lines_gasussian[spectrum_tag+roi_tag] = line
                
                if spectrum_tag+roi_tag not in self.ROI_lines_linear:
                    pen = pg.mkPen(color="w", width=1, style=Qt.DashLine)
                    line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
                    self.ROI_lines_linear[spectrum_tag+roi_tag] = line



                x, gaussian, lin = spectrum.get_ROI_plots(roi_tag, self.log)
                if gaussian is not None:
                    self.ROI_lines_gasussian[spectrum_tag+roi_tag].setData(x, gaussian)
                    self.ROI_lines_linear[spectrum_tag+roi_tag].setData(x, lin)

                
            
    def remove_roi(self, roi, force = False):
        if not force:
            reply = QMessageBox.question(
                self,
                "Delete ROI",
                "Delete this ROI?",
                QMessageBox.Yes | QMessageBox.No
            )
        else:
            reply = None

        if force or reply == QMessageBox.Yes:
            for spect_tag, spectrum in SpectrumManager.get_spectra_dict().items():

                self.plot_widget.removeItem(self.ROI_lines_gasussian.pop(spect_tag+roi, None))
                self.plot_widget.removeItem(self.ROI_lines_linear.pop(spect_tag+roi, None))

                # self.ROI_lines_gasussian.pop(spect_tag+roi)
                # self.ROI_lines_linear.pop(spect_tag+roi)

                spectrum.ROIs.pop(roi, None)

            self.plot_widget.removeItem(self.ROIs[roi])
            self.ROIs.pop(roi)
            self.Signals.removeROI.emit(roi)
            
    def _clear_rois(self):
        rois = SpectrumManager.existing_rois.copy()
        for roi in rois:
            self.remove_roi(roi, True)
