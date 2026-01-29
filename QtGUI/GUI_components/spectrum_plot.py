from PySide6.QtWidgets import QWidget, QVBoxLayout, QPushButton, QHBoxLayout, QMessageBox, QComboBox
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg
import numpy as np


from ..gaussian_fitting import Gaussian


class DeletableROI(pg.LinearRegionItem):
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

    def mouseClickEvent(self, ev):
        if ev.button() == Qt.RightButton:
            ev.accept()
            self.sigDeleteRequested.emit(self.tag)
        else:
            super().mouseClickEvent(ev)


class SpectrumPlot(QWidget):
    """Plot multiple lines from a Spectrum object with optional ROIs."""
    roi_signal_sender = Signal(object)
    def __init__(self, xlabel="Channel", ylabel="Counts", parent=None):
        super().__init__(parent)
        

        
        layout = QVBoxLayout(self)
        layout.setContentsMargins(1,1,1,1)        
        
        btn_layout = QHBoxLayout()
        layout.addLayout(btn_layout)
        self.plot_widget = pg.PlotWidget()
        layout.addWidget(self.plot_widget)
        
        
        
        self.plot_widget.setLabel('bottom', xlabel)
        self.plot_widget.setLabel('left', ylabel)
        self.plot_widget.showGrid(x=True, y=True)
        self.plot_widget.setXRange(0, 2500, padding=0) 
        self.plot_widget.setLimits(
            xMin=0, xMax=3500,
            yMin=0, yMax=1e6,
            minXRange=10, maxXRange=3500,
            minYRange=1, maxYRange=1e6
        )
        self.plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)

        self.plot_widget.getViewBox().setMouseEnabled(x=True, y=False)
        self.view = self.plot_widget.getViewBox()

        # Buttons
        

        self.btn_reset_zoom = QPushButton("Reset Zoom")
        self.btn_y_axis_lock   = QPushButton("Unlock y-axis")
        self.btn_lin_log = QPushButton("Log")
        self.btn_cps =  QPushButton("CPS")
        self.btn_mark_roi   = QPushButton("Add ROI")
        
        self.y_axis_locked = True

        self.cbox_bkg_choises = QComboBox()

        self.cbox_bkg_choises.addItems(["No Background", "Background Overlay", "Background Subtract"])
        self.cbox_bkg_choises.setCurrentIndex(0)
        
        btn_layout.addWidget(self.btn_reset_zoom)
        btn_layout.addWidget(self.btn_lin_log)
        btn_layout.addWidget(self.btn_cps)
        btn_layout.addWidget(self.btn_y_axis_lock)
        btn_layout.addWidget(self.btn_mark_roi)
        btn_layout.addWidget(self.cbox_bkg_choises)


        self.btn_reset_zoom.clicked.connect(self.reset_zoom)
        self.btn_mark_roi.clicked.connect(self.add_roi)
        self.btn_lin_log.clicked.connect(self.change_lin_log)
        self.btn_cps.clicked.connect(self._set_cps)
        self.btn_y_axis_lock.clicked.connect(self.lock_y_axis)
        self.cbox_bkg_choises.currentIndexChanged.connect(self._on_bkg_option_selection)

        self.primary_lines = {}
        self.bkg_lines = {}
        self.spectra = {}
        self.primary_spectrum = None
        self.ROIs = {}
        self.ROI_lines_gasussian = {}
        self.ROI_lines_linear = {}
        self.roi_counter = 0
        self.user_scaled = False
        self.log = False
        self.cps = False
        self.show_bkg = False
        self.bkg_sub = False

        self.plot_widget.sigRangeChanged.connect(self._on_range_change)

    def _on_range_change(self, view_box, range):
        self.user_scaled = True
        # enforce X limits
        x_min, x_max = self.plot_widget.viewRange()[0]
        if x_min < 0 or x_max > 3500:
            self.plot_widget.setXRange(max(0,x_min), min(3500,x_max), padding=0)

    def _on_bkg_option_selection(self, option):
        if option == 0:
            self.show_bkg = False
            self.bkg_sub = False
            self.btn_cps.setEnabled(True)
            self._set_cps(False, self.cps, False)
            
            self.plot_widget.clear()
            self.primary_lines.clear()
            self.bkg_lines.clear()
            self._redraw()


        elif option == 1:
            self.show_bkg = True
            self.bkg_sub = False
            self._set_cps(False, True, False)
            self.btn_cps.setEnabled(False)

            self._redraw()

        elif option == 2:
            self.cps = True
            self.show_bkg = True
            self.bkg_sub = True
            self._set_cps(False, True, False)
            self.btn_cps.setEnabled(False)
            self._redraw()



    def _set_cps(self,_ , cps_bool = None, recalculate = True):

        if cps_bool is None:
            if self.cps is True:
                cps_bool = False
                self.cps = False
            else:
                cps_bool = True
                self.cps = True

        if cps_bool:
            self.cps = True
            self.plot_widget.setLabel('left', "CPS")
            self.btn_cps.setText("Counts")
        else:
            self.cps = False
            self.plot_widget.setLabel('left', "Counts")
            self.btn_cps.setText("CPS")
        if recalculate:
            self._redraw()


    def lock_y_axis(self):
        if self.y_axis_locked:
            self.plot_widget.getViewBox().setMouseEnabled(x=True, y=True)
            self.y_axis_locked = False
            self.btn_y_axis_lock.setText("Lock y-axis")
        else:
            self.plot_widget.getViewBox().setMouseEnabled(x=True, y=False)
            self.y_axis_locked = True
            self.btn_y_axis_lock.setText("Unlock y-axis")

    def _redraw(self):
        self.plot_widget.clear()
        self.primary_lines.clear()
        self.bkg_lines.clear()
        for spect in self.spectra.values():
            if not self.bkg_sub:
                self.plot_primary(spect)
                self.plot_bkg(spect)
            else:
                self.plot_bkg_subtract(spect)

        self.ROI_lines_gasussian.clear()
        self.ROI_lines_linear.clear()
        self.update_all_rois()
    def reset_zoom(self):
        self.user_scaled = False
        self.plot_widget.enableAutoRange()
        self.plot_widget.setXRange(0, 2500, padding=0)

    def handle_spectrum(self, spectrum, primary = False):
        self.spectra[spectrum.name] = spectrum
        if primary:
            self.primary = spectrum.name
        if self.bkg_sub:
            self.plot_bkg_subtract(spectrum)
        else:
            self.plot_primary(spectrum)
            self.plot_bkg(spectrum)

    def plot_primary(self, spectrum, color="b"):
        if spectrum.name not in self.primary_lines:
            pen = pg.mkPen(color=color, width=2)
            line = self.plot_widget.plot([], [], pen=pen, name=spectrum.name, stepMode=True, fillLevel=0, brush=(0, 0, 255, 150))
            self.primary_lines[spectrum.name] = line

        self.primary_lines[spectrum.name].setData(spectrum.x_data, spectrum.get_spectrum(self.log, self.cps)[:-1])

    def plot_bkg(self, spectrum, color = "r"):
        if spectrum.bkg_y_data is None or not self.show_bkg:
            return
        
        if spectrum.name not in self.bkg_lines:
            pen = pg.mkPen(color=color, width=2)
            line = self.plot_widget.plot([], [], pen=pen, name=spectrum.name, stepMode=True, fillLevel=0, brush = color)
            self.bkg_lines[spectrum.name] = line
        self.bkg_lines[spectrum.name].setData(spectrum.x_data, spectrum.get_bkg(self.log, self.cps)[:-1])
        
            
    def plot_bkg_subtract(self, spectrum, color = "m"):
        if spectrum.bkg_y_data is None or spectrum.primary_uptime == 0 or spectrum.bkg_uptime == 0:
            return
        if spectrum.name not in self.primary_lines:
            pen = pg.mkPen(color=color, width=2)
            line = self.plot_widget.plot([], [], pen=pen, name=spectrum.name, stepMode=True, fillLevel=0,brush = color)
            self.primary_lines[spectrum.name] = line

        self.primary_lines[spectrum.name].setData(spectrum.x_data, spectrum.get_bkg_sub(self.log)[:-1])
            
    def change_lin_log(self):
        if self.log == False:
            self.log = True
            self._redraw()
            self.btn_lin_log.setText("Lin")
            self.plot_widget.setLimits(
            xMin=0, xMax=3500,
            yMin=-5, yMax=1e6,
            minXRange=10, maxXRange=3500,
            minYRange=-10, maxYRange=1e6
        )

        else:
            self.log = False
            self._redraw()
            self.btn_lin_log.setText("Log")
            self.plot_widget.setLimits(
            xMin=0, xMax=3500,
            yMin=0, yMax=1e6,
            minXRange=10, maxXRange=3500,
            minYRange=1, maxYRange=1e6
        )
            
        self._redraw()
            
    

    def add_roi(self):
        """Enable interactive ROI marking with LinearRegionItem."""
        roi_tag = f"ROI_{self.roi_counter}"
        self.roi_counter += 1
        new_roi = DeletableROI(roi_tag,[100, 500], movable=True)
        self.plot_widget.addItem(new_roi)
        self.ROIs[roi_tag] = new_roi
        
        new_roi.sigDeleteRequested.connect(self.remove_roi)
        new_roi.sigRegionChangeFinished.connect(
            lambda: self.on_roi_dropped(new_roi)
        )
        
    def on_roi_dropped(self, roi_selection):
        self.update_rois(roi_selection)
    
    def update_all_rois(self):
        for roi in self.ROIs.values():
            if roi not in self.plot_widget.plotItem.items:
                self.plot_widget.addItem(roi)
            self.update_rois(roi)

    def update_rois(self, roi_selection):
        x_min, x_max = roi_selection.getRegion()
        x_min, x_max = int(x_min), int(x_max)

        for spectrum_tag, spectrum in self.spectra.items():
            if not self.primary == spectrum.name:
                continue
            spectrum.update_roi(roi_selection.tag, x_min, x_max, self.cps)
            
        for spectrum_tag, spectrum in self.spectra.items():
            if not self.primary == spectrum.name:
                continue
            for roi_tag, roi in spectrum.ROIs.items():
                if spectrum_tag+roi_tag not in self.ROI_lines_gasussian:
                    pen = pg.mkPen(color="w", width=2)
                    line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
                    self.ROI_lines_gasussian[spectrum_tag+roi_tag] = line
                
                if spectrum_tag+roi_tag not in self.ROI_lines_linear:
                    pen = pg.mkPen(color="w", width=2)
                    line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
                    self.ROI_lines_linear[spectrum_tag+roi_tag] = line



                x, gaussian, lin = spectrum.get_ROI_plots(roi_tag, self.log)
                if gaussian is not None:
                    self.ROI_lines_gasussian[spectrum_tag+roi_tag].setData(x, gaussian)
                    self.ROI_lines_linear[spectrum_tag+roi_tag].setData(x, lin)

                    self.roi_signal_sender.emit(spectrum.ROIs[roi_tag])
                


                
                

        

            
    def remove_roi(self, roi):
        reply = QMessageBox.question(
            self,
            "Delete ROI",
            "Delete this ROI?",
            QMessageBox.Yes | QMessageBox.No
        )

        if reply == QMessageBox.Yes:
            for spect_tag, spectrum in self.spectra.items():
                if spect_tag != self.primary:
                    continue
                self.plot_widget.removeItem(self.ROI_lines_gasussian[spect_tag+roi])
                self.plot_widget.removeItem(self.ROI_lines_linear[spect_tag+roi])

                self.ROI_lines_gasussian.pop(spect_tag+roi)
                self.ROI_lines_linear.pop(spect_tag+roi)

                spectrum.ROIs.pop(roi)

            self.plot_widget.removeItem(self.ROIs[roi])
            self.ROIs.pop(roi)

            print(self.ROIs, self.ROI_lines_gasussian, self.ROI_lines_linear)


class CPSDosePlot(QWidget):
    """Twin Y-axis plot for CPS / Dose with no zooming."""
    def __init__(self, parent=None):
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0,0,0,0)

        self.plot_widget = pg.PlotWidget()
        layout.addWidget(self.plot_widget)

        self.cps_line = self.plot_widget.plot([], [], pen=pg.mkPen('b', width=2), name="CPS")
        self.dr_line  = self.plot_widget.plot([], [], pen=pg.mkPen('orange', width=2), name="Dose Rate")
        self.x_data, self.cps_data, self.dr_data = [], [], []

        # Disable zoom/pan entirely
        self.plot_widget.setMouseEnabled(x=False, y=False)
        self.plot_widget.showGrid(x=True, y=True)
        self.plot_widget.setLabel('bottom', "Seconds ago")
        self.plot_widget.setLabel('left', "CPS / Dose")

    def update(self, x, cps, dr):
        self.x_data = x
        self.cps_data = cps
        self.dr_data = dr
        self.cps_line.setData(self.x_data, self.cps_data)
        self.dr_line.setData(self.x_data, self.dr_data)
