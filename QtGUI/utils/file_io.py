import csv
import xml.etree.ElementTree as ET

import numpy as np

from ..SpectrumClasses import Spectrum
class csv_io:
    def export(spectrum: Spectrum, file_name: str) -> bool:
        acc_spectrum = spectrum.get_spectrum()
        cps_spectrum = spectrum.get_spectrum(cps=True)
        if not cps_spectrum:
            cps_spectrum = np.zeros_like(acc_spectrum)

        x_axis = spectrum.x_data

        with open(f"{file_name}.csv", "w", newline="") as csv_file:
            csv_writer = csv.writer(csv_file, dialect=csv.excel)
            csv_writer.writerow(["Energy/Channel", "Counts", "CPS"])
            csv_writer.writerows(zip(x_axis, acc_spectrum, cps_spectrum))
        
        return True
    
    def load(name, x_axis_col = 0, foreground_col = 1, background_col = 2):
        pass


class xml_io:
    def export(spectrum, file_name):
        pass

    def load(file_name):
        pass

class spe_io:
    def load(file_name):
        pass