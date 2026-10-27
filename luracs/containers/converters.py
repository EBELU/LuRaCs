from copy import deepcopy

from luracs.containers.spectrum_classes import Spectrum


def convert_background_to_spectrum(original_spectrum: Spectrum) -> Spectrum:
    "Convert the background spectrum to the foreground spectrum, discarding the original_foreground. Creates a new spectrum."
    if original_spectrum.background is None:
        raise ValueError("Given spectrum has no background")
    
    new_spectrum = deepcopy(original_spectrum)
    
    new_spectrum.foreground = new_spectrum.background
    new_spectrum.background = None

    return new_spectrum