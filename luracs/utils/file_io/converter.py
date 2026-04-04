from pathlib import Path
from SpectrumClasses import Spectrum, SpectrumData
from ROIClasses import ROI, Fit
from .dispatcher import io_dispatcher

def load_spectrum(file_name: Path) -> Spectrum:
    kwargs = io_dispatcher(file_name)

    new_spectrum = Spectrum(len(kwargs["foreground"].y_axis), file_name.stem)

    new_spectrum.set_foreground(kwargs["foreground"])

    bkg = kwargs.get("background")
    if bkg is not None:
        new_spectrum.set_background(bkg)