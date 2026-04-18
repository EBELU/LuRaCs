from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QPushButton,
    QHBoxLayout,
    QMessageBox,
    QComboBox,
)
from PySide6.QtCore import Qt, QObject, Signal
from PySide6.QtGui import QColor
import pyqtgraph as pg
import numpy as np

from core import SpectrumManager, Settings

from SpectrumClasses import Spectrum

from utils.numerics import multi_gaussian


class EmittedSignals(QObject):
    updateSpectumROIs = Signal(str)
    removeROI = Signal(str)
    logLinUpdated = Signal(bool)
    cpsUpdated = Signal(bool)
    bkgSubUpdated = Signal(bool)

    def __init__(self, parent=None):
        super().__init__(parent)


class SpectrumPlot(QWidget):
    """Class to control and manage the plotting of spectra.

    Does not manage the spectra, can only request operations from SpectrumManager.
    """

    def __init__(self, xlabel="Energy [keV]", ylabel="Counts", parent=None):
        super().__init__(parent)

        # --- Signals ---

        self.Signals = EmittedSignals()

        SpectrumManager.Signals.spectrumUpdated.connect(self.update_plot)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_plot)
        SpectrumManager.Signals.backgroundRemoved.connect(lambda *args: self._redraw())
        SpectrumManager.Signals.visibilityChanged.connect(lambda *args: self._redraw())
        SpectrumManager.ROIManager.sigROICreated.connect(
            lambda r: self.create_roi_lines(r.tag)
        )
        SpectrumManager.ROIManager.sigROIUpdated.connect(self.draw_roi)
        SpectrumManager.ROIManager.sigROIDeleted.connect(self.remove_roi)

        self.Signals.updateSpectumROIs.connect(
            lambda n: SpectrumManager.ROIManager.update_roi(spectrum_name=n)
        )
        self.Signals.removeROI.connect(SpectrumManager.ROIManager.remove_roi)
        self.Signals.logLinUpdated.connect(SpectrumManager.ROIManager.set_log)
        self.Signals.cpsUpdated.connect(SpectrumManager.ROIManager.set_cps)
        self.Signals.bkgSubUpdated.connect(SpectrumManager.ROIManager.set_bkg_sub)

        # --- Layout ---

        layout = QVBoxLayout(self)
        layout.setContentsMargins(1, 1, 1, 1)

        btn_layout = QHBoxLayout()
        layout.addLayout(btn_layout)
        self.plot_widget = pg.PlotWidget()
        layout.addWidget(self.plot_widget)
        SpectrumManager.ROIManager.set_plot(self.plot_widget)

        self.plot_widget.setLabel("bottom", xlabel)
        self.plot_widget.setLabel("left", ylabel)
        self.plot_widget.showGrid(x=True, y=True)
        # self.plot_widget.getAxis('left').enableAutoSIPrefix(False)
        self.plot_widget.setXRange(0, 2500, padding=0)
        self.plot_widget.setLimits(
            xMin=0,
            xMax=3500,
            yMin=0,
            yMax=1e6,
            minXRange=10,
            maxXRange=3500,
            minYRange=1e-4,
            maxYRange=1e6,
        )
        self.plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 2)

        self.plot_widget.getViewBox().setMouseEnabled(x=True, y=False)
        self.view = self.plot_widget.getViewBox()
        self.plot_widget.enableAutoRange()

        # Buttons

        self.btn_reset_zoom = QPushButton("Reset Zoom")
        self.btn_y_axis_lock = QPushButton("Unlock y-axis")
        self.btn_lin_log = QPushButton("Log")
        self.btn_cps = QPushButton("CPS")
        self.btn_mark_roi = QPushButton("Add ROI")

        self.cbox_bkg_choises = QComboBox()

        self.cbox_bkg_choises.addItems(
            ["No Background", "Background Overlay", "Background Subtract"]
        )
        self.cbox_bkg_choises.setCurrentIndex(0)

        btn_layout.addWidget(self.btn_reset_zoom)
        btn_layout.addWidget(self.btn_lin_log)
        btn_layout.addWidget(self.btn_cps)
        btn_layout.addWidget(self.btn_y_axis_lock)
        btn_layout.addWidget(self.btn_mark_roi)
        btn_layout.addWidget(self.cbox_bkg_choises)

        # --- Assign button callbacks ---
        self.btn_reset_zoom.clicked.connect(self.reset_zoom)
        self.btn_mark_roi.clicked.connect(SpectrumManager.ROIManager.add_roi)
        self.btn_lin_log.clicked.connect(self.change_lin_log)
        self.btn_cps.clicked.connect(self._set_cps)
        self.btn_y_axis_lock.clicked.connect(self.lock_y_axis)
        self.cbox_bkg_choises.currentIndexChanged.connect(self._on_bkg_option_selection)

        self.primary_lines = {}
        self.bkg_lines = {}

        self.ROIs = {}
        self.ROI_lines_gaussian = {}  # Gaussian cruve lines
        self.ROI_lines_linear = {}  # Background correction lines

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

        self.Signals.bkgSubUpdated.emit(self.bkg_sub)
        # Finish by redrawing
        self._redraw()

    def _set_cps(self, _, cps_bool=None, recalculate=True):
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
                self.plot_widget.setLimits(yMin=-10, yMax=1e6)

            else:
                self.plot_widget.setLimits(yMin=0, yMax=1e6)

            self.cps = True
            self.plot_widget.setLabel("left", "CPS")
            self.btn_cps.setText("Counts")
        else:
            self.plot_widget.setLimits(yMin=0, yMax=1e6)
            self.cps = False
            self.plot_widget.setLabel("left", "Counts")
            self.btn_cps.setText("CPS")

        self.Signals.cpsUpdated.emit(self.cps)
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

        self.ROI_lines_gaussian.clear()
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

        self.Signals.updateSpectumROIs.emit(name)

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
                fill_level = np.floor(
                    np.nanmin(spectrum.get_foreground(self.log, self.cps))
                )

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

            self.primary_lines[spectrum.name].setFillLevel(
                np.floor(min(np.nanmin(foreground), bkg) * 2) / 2
            )

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
                fill_level = (
                    np.floor(np.nanmin(spectrum.get_background(self.log, self.cps)) * 2)
                    / 2
                )

            line = self.plot_widget.plot(
                [],
                [],
                name=spectrum.name,
                pen=pen,
                brush=brush,
                fillLevel=fill_level,
                stepMode="center",
            )
            line.setZValue(1)  # Place on top

            self.bkg_lines[spectrum.name] = line

        background = spectrum.get_background(self.log, self.cps)
        if self.log:
            self.bkg_lines[spectrum.name].setFillLevel(
                np.floor(np.nanmin(background) * 2) / 2
            )
        self.bkg_lines[spectrum.name].setData(
            spectrum.x_axis,
            background[:-1],
        )

    def plot_bkg_subtract(self, spectrum: Spectrum):
        # Skip if background or live times invalid
        if (
            spectrum.get_background() is None  # No bkg
            or spectrum.foreground.live_time == 0  # No fg live time
            or spectrum.background.live_time == 0  # No bkg live time
        ):
            return

        if spectrum.name not in self.primary_lines:
            # Pen uses foreground color
            pen = pg.mkPen(spectrum.color_foreground, width=2)

            # Brush with semi-transparent alpha
            if Settings.Appearance.brush:
                brush = QColor(spectrum.color_background)
                brush.setAlpha(150)
            else:
                brush = None

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
            self.primary_lines[spectrum.name].setFillLevel(
                np.floor(np.nanmin(bkg_sub_spect) * 2) / 2
            )

        self.primary_lines[spectrum.name].setData(
            spectrum.x_axis,
            bkg_sub_spect[:-1],
        )

    def remove_plot(self, name: str):
        self.primary_lines.pop(name, None)
        self.bkg_lines.pop(name, None)
        for roi_tag in SpectrumManager.ROIManager.ROIs.keys():
            self.ROI_lines_linear[roi_tag].pop(name, None)
            self.ROI_lines_gaussian[roi_tag].pop(name, None)

        self._redraw()

    def change_lin_log(self):
        """Change lin log at data retrieval"""
        if self.log == False:
            self.log = True
            self.plot_widget.setLimits(yMin=-10, yMax=1e6)
            self.btn_lin_log.setText("Lin")

        else:
            self.log = False
            self.btn_lin_log.setText("Log")
            self.plot_widget.setLimits(
                yMin=0,
                yMax=1e6,
            )
        self.Signals.logLinUpdated.emit(self.log)
        self._redraw()

    def create_roi_lines(self, roi_tag):
        self.ROI_lines_gaussian[roi_tag] = {}
        self.ROI_lines_linear[roi_tag] = {}

    def update_all_rois(self):
        for roi in SpectrumManager.ROIManager.ROIs.values():
            if roi not in self.plot_widget.plotItem.items:
                self.plot_widget.addItem(roi)
        for spectrum_name in SpectrumManager.spectra.keys():
            for roi_tag in SpectrumManager.ROIManager.ROIs.keys():
                self.draw_roi(roi_tag, spectrum_name)

    def draw_roi(self, roi_tag: str, spectrum_name):
        spectrum = SpectrumManager.get_spectra_dict().get(spectrum_name)

        if spectrum is None or not spectrum.show_in_plot:
            return

        # Ensure dict structure exists
        if roi_tag not in self.ROI_lines_gaussian:
            self.ROI_lines_gaussian[roi_tag] = {}
            self.ROI_lines_linear[roi_tag] = {}

        # --- Gaussian line ---
        if spectrum_name not in self.ROI_lines_gaussian[roi_tag]:
            pen = pg.mkPen(color=QColor("#BAFFC9"), width=1.3)
            line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
            self.ROI_lines_gaussian[roi_tag][spectrum_name] = line

        # --- Linear background line ---
        if spectrum_name not in self.ROI_lines_linear[roi_tag]:
            pen = pg.mkPen(color="w", width=1, style=Qt.DashLine)
            line = self.plot_widget.plot([], [], pen=pen, name=roi_tag)
            self.ROI_lines_linear[roi_tag][spectrum_name] = line

        # --- Get data ---
        roi_fit = spectrum.ROIs.get(roi_tag, None)
        if roi_fit is None:
            return
        if roi_fit.fit is None:
            self.ROI_lines_gaussian[roi_tag][spectrum_name].setData([], [])
            self.ROI_lines_linear[roi_tag][spectrum_name].setData([], [])
            return

        # --- Calculate values for lines ---
        x = spectrum.x_axis[
            (roi_fit.fit.region_lower < spectrum.x_axis)
            & (spectrum.x_axis < roi_fit.fit.region_upper)
        ]
        lin = (
            np.polyval(roi_fit.fit.bkg_params, x)
            if roi_fit.bkg_type != "None"
            else np.zeros_like(x)
        )
        gaussian = multi_gaussian(x, roi_fit.fit.params) + lin

        if self.cps and spectrum.foreground.live_time is not None:
            lin /= spectrum.foreground.live_time
            gaussian /= spectrum.foreground.live_time

        if self.log:
            gaussian = np.log10(np.where(gaussian > 0, gaussian, np.nan))
            lin = np.log10(np.where(lin > 0, lin, np.nan))

        # --- Update plot ---
        if gaussian is not None:
            self.ROI_lines_gaussian[roi_tag][spectrum_name].setData(x, gaussian)

            if lin is not None:
                self.ROI_lines_linear[roi_tag][spectrum_name].setData(x, lin)
        else:
            # Clear if no data
            self.ROI_lines_gaussian[roi_tag][spectrum_name].setData([], [])
            self.ROI_lines_linear[roi_tag][spectrum_name].setData([], [])

    def remove_roi(self, roi):
        self.plot_widget.removeItem(roi)
        gauss_lines = self.ROI_lines_gaussian.get(roi.tag)
        lin_lines = self.ROI_lines_linear.get(roi.tag)
        # Skip if it doesnt exist
        if gauss_lines is not None and lin_lines is not None:
            for gaussian_line, lin_line in zip(
                gauss_lines.values(), lin_lines.values()
            ):
                self.plot_widget.removeItem(gaussian_line)
                self.plot_widget.removeItem(lin_line)
