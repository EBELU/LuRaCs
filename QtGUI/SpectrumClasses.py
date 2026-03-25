import numpy as np
from dataclasses import dataclass
from PySide6.QtGui import QColor
from datetime import datetime

from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from ROIClasses import ROI

@dataclass
class SpectrumData:
    y_axis: np.array
    channels: int
    total_counts: int
    live_time:float
    real_time:float = None
    avg_dose_rate: float = None
    avg_cps: float = None
    start_date: datetime = None
    end_date: datetime = None
    spectrum_name:str = None
    instrument: str = None
    
class Spectrum:
    """
    A class to hold and do operations on an energy spectrum. 
    """

    def __init__(self, channels: int, name: str, **kwargs):
        self.name: str = name
        self.channels: int = channels
        
        self.foreground: SpectrumData = None
        self.background: SpectrumData = None
        self.x_axis:np.array = np.arange(channels)
        
        self.ROIs: dict[str, ROI] = {}

        self.color_foreground: QColor = QColor("white")
        self.color_background: QColor = QColor("white")
        
        self.calibration_coefficients: list = None
        self.calibrated: bool = False
        self.energy_unit: str = None
        
        self.fit_rois: bool = True
        self.show_in_plot: bool = True
        
        self.connection: str = kwargs.get("connection", None)
        self.instrument_id: str = kwargs.get("instrument_id", None)
        self.instrument_model: str = kwargs.get("instrument_model", None)
        
    def set_foreground(self, spectrum: SpectrumData, color: QColor = None):
        assert spectrum.channels == self.channels
        self.foreground = spectrum

        if spectrum.instrument is None and self.connection is not None:
            spectrum.instrument = self.connection

        if color is not None and self.foreground:
            self.color_background = color
        
    def set_background(self, spectrum: SpectrumData, color: QColor = None):
        assert spectrum.channels == self.channels
        self.background = spectrum
        if color is not None:
            self.color_background = color
        
    def apply_calibration(self, coefficients: list):
        """
            coeffs = [a_n, a_{n-1}, ..., a_0]
        """
        self.x_axis = np.polyval(coefficients, np.arange(self.channels))
        self.calibration_coefficients = coefficients
        self.calibrated = True
        
    def get_foreground(self, log: bool = False, cps:bool = False):
        """
         Returns the channel counts for the primary spectrum.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        :param cps: Converts the channel counts to cps by dividing by uptime. If the uptime is 0 it returns None.
        """
        
        if self.foreground is None:
            return np.zeros(self.channels)

        if cps and self.foreground.live_time > 0:
            y_data = self.foreground.y_axis / self.foreground.live_time
        elif cps and not self.foreground.live_time > 0:
            return
        else:
            y_data = self.foreground.y_axis

        if log:
            return np.log10(np.where(y_data > 0, y_data, np.nan))
        else:
            return y_data
        
    def get_background(self, log: bool = False, cps: bool = False):
        """
         Returns the channel counts for the background spectrum. If the background is empty it returns None.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        :param cps: Converts the channel counts to cps by dividing by uptime. If the uptime is 0 it returns None
        """
        if self.background is None:
            return

        if cps and self.background.live_time != 0:
            bkg_y_data = self.background.y_axis / self.background.live_time
        elif cps and not self.background.live_time > 0:
            return
        else:
            bkg_y_data = self.background

        if log:
            return np.log10(np.where(bkg_y_data > 0, bkg_y_data, np.nan))
        else:
            return bkg_y_data
        
    def get_bkg_sub(self, log: bool = False):        
        """
         Returns the channel cps, never counts, for a background subtracted spectrum. If the background is empty it returns the normal spectrum.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        """
        if self.foreground.live_time is None:
            return
        
        if self.background.live_time is None or self.background is None:
            return self.get_foreground(False, True)

        data = self.get_foreground(False, True) - self.get_background(False, True)
        if log:
            return np.log10(np.where(data > 0, data, np.nan))
        else:
            return data
        
    def set_roi(self, roi_data):
        self.ROIs[roi_data.tag] = roi_data   
        
    def remove_roi(self, roi_tag):
        self.ROIs.pop(roi_tag, None)                 
    
    def _dump_state(self):
        """Export the state of the spectrum as a dict for json format"""
        return {}
        
        
        
