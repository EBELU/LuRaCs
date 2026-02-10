import numpy as np
from dataclasses import dataclass
from .utils.gaussian_fitting import fit_gaussian, Gaussian
from PySide6.QtGui import QColor
from datetime import datetime

class ROI:
    def __init__(self, tag:str,  low: int, high: int, gaussian):
        self.tag = tag
        self.low = low
        self.high = high
        self.mid = (low + high) / 2
        self.gaussian = gaussian

    def contains(self, x):
        return self.low <= x <= self.high

    def __lt__(self, other):
        return self.mid < other.mid

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

    def __init__(self, channels: int, name: str):
        self.name: str = name
        self.channels: int = channels
        
        self.foreground: SpectrumData = None
        self.background: SpectrumData = None
        self.x_axis:np.array = np.arange(channels)
        
        self.ROIs = {}

        self.color_foreground: QColor = QColor("white")
        self.color_background: QColor = QColor("white")
        
        self.calibration_coefficients: list = None
        self.calibrated: bool = False
        self.energy_unit: str = None
        self.fit_rois: bool = True
        self.has_device: bool = False
        
    def set_foreground(self, spectrum: SpectrumData, color: QColor = None):
        assert spectrum.channels == self.channels
        self.foreground = spectrum
        if color is not None:
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
        
    def get_ROI_plots(self, ROI_tag: str, log: bool = False):
        """Return the x-axis and the gaussian curve and the background fit from a ROI"""
        roi = self.ROIs[ROI_tag]
        x = self.x_axis[(roi.low < self.x_axis) & (self.x_axis < roi.high)]
        if roi.gaussian is not None:
            lin = roi.gaussian._corr_f(x)
            gaussian = roi.gaussian.value(x) + lin

            if log:
                gaussian = np.log10(np.where(gaussian > 0, gaussian, np.nan))
                lin = np.log10(np.where(lin > 0, lin, np.nan))
                return x,gaussian, lin
            else:
                return x, gaussian, lin
        else:
            return None, None, None

        
    def update_roi(self, tag: str, x_low: float, x_high: float, cps: bool = None):
        """Refit a ROI"""
        try:
            y_data = self.get_foreground(False, cps)
            gaussian = fit_gaussian(self.x_axis, y_data, x_low, x_high)
        except Exception as e:
            print(f"Gaussian fit failed on ", e)
            gaussian = None
        
        self.ROIs[tag] = ROI(tag, x_low, x_high, gaussian)
        
    
    def _dump_state(self):
        """Export the state of the spectrum as a dict for json format"""
        return {}
        
        
        
