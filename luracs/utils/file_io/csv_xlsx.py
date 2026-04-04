from __future__ import annotations
import csv
import numpy as np
import openpyxl as pyxl

from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from SpectrumClasses import Spectrum
    from pathlib import Path
    
def export_csv(spectrum: Spectrum, file_name: str) -> bool:
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

class xlsx_exporter:
    def __init__(self, file_name: str | Path, spectrum: Spectrum, export_spectra = True, export_rois = True, export_instrument = True):
        self.wb = pyxl.open(f"{str(file_name)}.xlsx")
        

def export_roi_as_xlsx(spectrum: Spectrum, file_name: str) -> None:
    pass

def export_spectrum_as_xlsx(spectrum: Spectrum, file_name: str) -> None:
    pass