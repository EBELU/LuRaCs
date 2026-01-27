import csv
import numpy as np

from .SpectrumClasses import Spectrum
class csv_io:
    def export(spectrum: Spectrum):
        acc_spectrum = spectrum.get_spectrum()
        cps_spectrum = spectrum.get_spectrum(cps=True)