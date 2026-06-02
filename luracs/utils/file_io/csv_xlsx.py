from __future__ import annotations
import csv
import numpy as np
import openpyxl as pyxl

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum
    from pathlib import Path

class csv_writer:
    @classmethod
    def export_spectrum(cls, spectrum: Spectrum, file_name: Path | str):
        "Export a spectrum to a csv file"
        if isinstance(file_name, str):
            file_name = Path(file_name)
        acc_spectrum = spectrum.get_foreground()
        cps_spectrum = spectrum.get_foreground(cps=True)
        if cps_spectrum is None:
            cps_spectrum = np.zeros_like(acc_spectrum)

        x_axis = spectrum.x_axis

        with open(str(file_name.with_suffix(".csv")), "w", newline="") as csv_file:
            csv_writer = csv.writer(csv_file, dialect=csv.excel)
            csv_writer.writerow(["Energy/Channel [keV]", "Counts", "CPS"])
            csv_writer.writerows(zip(x_axis, acc_spectrum, cps_spectrum))
    
    @classmethod
    def export_rois(cls, rois: list, file_name: Path | str):
        "Export the roi data in list of rois to a csv file"
        if isinstance(file_name, str):
            file_name = Path(file_name)
        
        with open(str(file_name.with_suffix(".csv")), "w", newline="") as csv_file:
            csv_writer = csv.writer(csv_file, dialect=csv.excel)
            csv_writer.writerow(["Alias", "Lower Bound [keV]", "Upper Bound [keV]","Centroid [keV]", "FWHM [keV]"])
        


class xlsx_writer:
    def __init__(
        self,
        file_name: str | Path,
        spectrum: Spectrum,
        export_spectra=True,
        export_rois=True,
        export_instrument=True,
    ):
        self.wb = pyxl.open(f"{str(file_name)}.xlsx")


def export_roi_as_xlsx(spectrum: Spectrum, file_name: str) -> None:
    pass


def export_spectrum_as_xlsx(spectrum: Spectrum, file_name: str) -> None:
    pass
