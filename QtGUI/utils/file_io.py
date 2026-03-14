import csv
import numpy as np


from ..core import SpectrumManager
from ..SpectrumClasses import Spectrum
from .xml_parser import SpectrumParser as xmlParser


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
    


class xml_io:
    def export(spectrum, file_name):
        pass
        
        

    def load(file_name):
        parser = xmlParser(file_name)

        
        SpectrumManager.create_spectrum(parser.kwargs["name"], parser.kwargs["foreground"].channels)
        SpectrumManager.set_foreground_spectrum(parser.kwargs["name"], parser.kwargs["foreground"])
        
        if "background" in parser.kwargs:
            SpectrumManager.set_background_spectrum(parser.kwargs["name"], parser.kwargs["background"])
            
        if "calibration" in parser.kwargs:
            SpectrumManager.calibrate_spectrum(parser.kwargs["name"], parser.kwargs["calibration"])
        
        
        
        
        

class spe_io:
    def load(file_name):
        pass
    
    
if __name__=="__main__":
    write_xml_spectrum()