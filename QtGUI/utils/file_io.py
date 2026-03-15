import csv
from pathlib import Path
import numpy as np


from ..core import SpectrumManager
from ..SpectrumClasses import Spectrum
from .xml_parser import SpectrumParser as xmlParser
from .xml_writer import write_xml_spectrum

    
class csv_io:
    def export(spectrum: Spectrum, file_name: str) -> bool:
        acc_spectrum = spectrum.get_foreground()
        cps_spectrum = spectrum.get_foreground(cps=True)
        if cps_spectrum is None:
            cps_spectrum = np.zeros_like(acc_spectrum)

        x_axis = spectrum.x_axis

        with open(f"{file_name}.csv", "w", newline="") as csv_file:
            csv_writer = csv.writer(csv_file, dialect=csv.excel)
            csv_writer.writerow(["Energy/Channel", "Counts", "CPS"])
            csv_writer.writerows(zip(x_axis, acc_spectrum, cps_spectrum))
        
        return True
    


class xml_io:
    def export(spectrum: Spectrum, file_name: str):
        write_xml_spectrum(spectrum, file_name)
        
        
    def load(file_name):
        return xmlParser(file_name)

        

class spe_io:
    def load(file_name):
        pass
    
    
if __name__=="__main__":
    write_xml_spectrum()