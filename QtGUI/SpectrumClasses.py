import numpy as np
from .gaussian_fitting import fit_gaussian, Gaussian

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

    
    
class Spectrum:
    """
    A class to hold and do operations on an energy spectrum. 
    """

    def __init__(self, channels, name):
        self.name = name
        self.channels = channels
        self.y_data = np.zeros(channels)
        self.x_data = np.arange(channels)
        self.bkg_y_data = None
        self.total_counts = 0
        self.primary_uptime = None
        self.bkg_uptime = None
        
        self.ROIs = {}
        
        self.calibration_coefficients = None
        self.calibrated = False
        
    def set_y_data(self, spectrum, uptime = None):
        assert len(spectrum) == self.channels
        self.y_data = spectrum
        self.total_counts = np.sum(spectrum)
        self.primary_uptime = uptime
        
    def set_y_bkg(self, spectrum, uptime = None):
        assert len(spectrum) == self.channels
        self.bkg_y_data = spectrum
        self.bkg_uptime = uptime
        
    def apply_calibration(self, coefficients):
        """
            coeffs = [a_n, a_{n-1}, ..., a_0]
        """
        self.x_data = np.polyval(coefficients, np.arange(self.channels))
        self.calibration_coefficients = coefficients
        self.calibrated = True
        
    def get_spectrum(self, log = False, cps = False):
        """
         Returns the channel counts for the primary spectrum.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        :param cps: Converts the channel counts to cps by dividing by uptime. If the uptime is 0 it returns None.
        """

        if cps and self.primary_uptime > 0:
            y_data = self.y_data / self.primary_uptime
        elif cps and not self.primary_uptime > 0:
            return
        else:
            y_data = self.y_data

        if log:
            return np.log10(np.where(y_data > 0, y_data, np.nan))
        else:
            return y_data
        
    def get_bkg(self, log = False, cps = False):
        """
         Returns the channel counts for the background spectrum. If the background is empty it returns None.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        :param cps: Converts the channel counts to cps by dividing by uptime. If the uptime is 0 it returns the normal specturm.
        """
        if self.bkg_y_data is None:
            return

        if cps and self.bkg_uptime != 0:
            bkg_y_data = self.bkg_y_data / self.bkg_uptime
        else:
            bkg_y_data = self.bkg_y_data

        if log:
            return np.log10(np.where(bkg_y_data > 0, bkg_y_data, np.nan))
        else:
            return bkg_y_data
        
    def get_bkg_sub(self, log = False):        
        """
         Returns the channel cps, never counts, for a background subtracted spectrum. If the background is empty it returns the normal spectrum.
        
        :param log: If True applies log10 before returning spectrum. Counts of 0 is set to NaN.
        """
        if self.primary_uptime is None:
            return
        
        if self.bkg_uptime is None or self.bkg_y_data is None:
            return self.get_spectrum(False, True)

        data = self.get_spectrum(False, True) - self.get_bkg(False, True)
        if log:
            return np.log10(np.where(data > 0, data, np.nan))
        else:
            return data
        
    def get_ROI_plots(self, ROI_tag, log = False):
        roi = self.ROIs[ROI_tag]
        x = self.x_data[(roi.low < self.x_data) & (self.x_data < roi.high)]
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

        
    def update_roi(self, tag, x_low, x_high, cps = None):
        try:
            y_data = self.get_spectrum(False, cps)
            gaussian = fit_gaussian(self.x_data, y_data, x_low, x_high)
        except Exception as e:
            print(f"Gaussian fit failed on ", e)
            gaussian = None
        
        self.ROIs[tag] = ROI(tag, x_low, x_high, gaussian)
        
        
        
