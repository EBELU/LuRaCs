from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument
from luracs.containers.spectrum_classes import Spectrum
from luracs.core import Log, Settings
from luracs.utils.file_io.xml_writer import xml_writer


def save_spectrum_to_library(spectrum: Spectrum):
    new_file = Settings.Paths.spectrum_library / spectrum.name
    xml_writer(spectrum, new_file)

    Log.debug(f"Spectrum saved to library: {new_file}")
    return new_file.with_suffix(".xml")


def save_instrument_to_library(instrument: UniqueInstrument | GenericInstrument):
    # Build dummy spectrum for the xml writer
    dummy_spectrum = Spectrum(1, "Dummy")
    dummy_spectrum.instrument = instrument

    # Check so im not doing anything dumb
    if isinstance(instrument, UniqueInstrument):
        new_file = Settings.Paths.unique_instrument_library / instrument.name
    elif isinstance(instrument, GenericInstrument):
        new_file = Settings.Paths.generic_instrument_library / instrument.model
    else:
        raise TypeError(f"Invalid instrument type! {type(instrument)}")

    xml_writer(dummy_spectrum, new_file, export_spectrum=False, export_rois=False)

    Log.debug(f"Instrument saved to library: {new_file}")
    return new_file.with_suffix(".xml")


def save_rois_to_internal(dummy_spectrum: Spectrum):
    new_file = Settings.Paths.roi_library / dummy_spectrum.name
    xml_writer(dummy_spectrum, new_file, export_spectrum=False, export_instrument=False)
    Log.debug(f"ROIs saved to library: {new_file}")
    return new_file.with_suffix(".xml")
