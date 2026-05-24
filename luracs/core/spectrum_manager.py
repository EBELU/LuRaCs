from PySide6.QtCore import QObject, Signal
from PySide6.QtGui import QColor
from clients.DeviceWrappers import WrappedSpectrumPackage
from datetime import datetime, timedelta

from containers.instrument_classes import GenericInstrument, UniqueInstrument
from containers.spectrum_classes import Spectrum, SpectrumData

from .gui_logger import gui_logger
from .settings import Settings
from .roi_manager import ROIManager
from .instument_library import InstrumentLibrary
from .nuclide_library import NuclideLibrary
from utils.color_rotator import ColorRotator
from utils.file_io import xml_writer

"""
    The Spectrum manager handles the spectra in the program.
    GUI components can request actions from the spectrum manager but should not change the state of any spectrum without going through the manager.
"""


class EmittedSignals(QObject):
    spectrumCreated = Signal(str)
    spectrumUpdated = Signal(str)
    spectrumRemoved = Signal(str)

    backgroundRemoved = Signal(str, str)
    visibilityChanged = Signal(bool)

    roiCreated = Signal(str)
    roiUpdated = Signal(str)
    roiRemoved = Signal(str)
    
    newInstrumentLoaded = Signal(object) # Signal if an unknown instrument from a file

    colorUpdated = Signal(str)
    spectrumNameChanged = Signal(str, str)

    def __init__(self, parent=None):
        super().__init__(parent)


class SpectrumManagerBase(QObject):
    def __init__(self):
        super().__init__()
        self.color_rotation = ColorRotator("mpl")

        self.spectra: dict[str, Spectrum] = {}

        self.Signals = EmittedSignals()

        self.ROIManager = ROIManager(self)

        self.UniqueInstrumentLibrary = InstrumentLibrary(UniqueInstrument)
        self.GenericInstrumentLibrary = InstrumentLibrary(GenericInstrument)

        self.NuclideLibrary = NuclideLibrary(self)

    # --- Spectrum manipulators ---

    def create_spectrum(self, name: str, channels: int, device: str = None):
        if name not in self.spectra:
            # Create spectrum and add a possible connection
            new_spect = Spectrum(channels, name)
            new_spect.connection = device
            self.spectra[name] = new_spect

            # Set colors
            fg_clr, bkg_clr = self.color_rotation.get_color_pair()
            self.set_color(name, "foreground", fg_clr)
            self.set_color(name, "background", bkg_clr)

            # Emit done
            self.Signals.spectrumCreated.emit(name)
            gui_logger.info(f"Spectrum added: name={name}, channels={channels}, connection={str(device)}")
            return True
        else:
            return False

    def set_foreground_spectrum(
        self, name: str, spectrum_data: WrappedSpectrumPackage | SpectrumData, connection: str = None
    ):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        if isinstance(spectrum_data, WrappedSpectrumPackage):
            # Re-Wrap data from a connected instrument
            start_date = datetime.now() - timedelta(seconds=spectrum_data.uptime)
            new_spectrum = SpectrumData(
                y_axis=spectrum_data.y_axis,
                channels=len(spectrum_data.y_axis),
                total_counts=sum(spectrum_data.y_axis),
                live_time=spectrum_data.uptime,
                real_time=None,
                avg_cps=sum(spectrum_data.y_axis) / max(spectrum_data.uptime, 1),
                avg_dose_rate=None,
                start_date=start_date,
                end_date=None,
                spectrum_name=name,
                instrument=connection,
            )
            calib_coeff = spectrum_data.calib_coeff

        elif isinstance(spectrum_data, SpectrumData):
            new_spectrum = spectrum_data
            calib_coeff = None
            
        else:
            gui_logger.warning(f"Invalid spectrum data type {type(spectrum_data)}")
            return

        self.spectra[name].set_foreground(new_spectrum)
        
        # If the instrument provides a calibration is is sent by the WrappedSpectrumData
        # If the spectrum is already calibrated it will not be recalibrated
        if not self.spectra[name].calibrated and calib_coeff is not None:
            self.calibrate_spectrum(name, spectrum_data.calib_coeff)

        self.Signals.spectrumUpdated.emit(name)

    def set_background_spectrum(self, foreground_name: str, spectrum_data: SpectrumData):
        if foreground_name not in self.spectra:
            raise ValueError(f"Spectrum {foreground_name} does not exist")
        
        self.spectra[foreground_name].set_background(spectrum_data)
        self.Signals.spectrumUpdated.emit(foreground_name)
        gui_logger.info(f"Background set: name={foreground_name}")

    def clear_background(self, name: str):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        self.spectra[name].background = None
        self.Signals.backgroundRemoved.emit(name, "bkg")
        gui_logger.info(f"Background removed: {name}")

    def remove_spectrum(self, name: str):
        if name in self.spectra:
            del self.spectra[name]
            self.Signals.spectrumRemoved.emit(name)
            gui_logger.info(f"Spectrum removed: {name}")
            
    def rename_spectrum(self, current_name: str, new_name: str):
        if current_name not in self.spectra:
            raise ValueError(f"Spectrum {current_name} does not exist")
        
        try:
            self.blockSignals(True)
            self.spectra[new_name] = self.spectra.pop(current_name)
            self.spectra[new_name].name = new_name
            # Assign the ROIs to the renamed owner
            for roi in self.ROIManager.ROIs.values():
                if roi.owner_spectrum == current_name:
                    roi.owner_spectrum = new_name
        finally:
            self.blockSignals(False)
            self.Signals.spectrumRemoved.emit(current_name)
            self.Signals.spectrumCreated.emit(new_name)
            self.Signals.spectrumUpdated.emit(new_name)
            gui_logger.info(f"Spectrum renamed: old_name={current_name}, new_name={new_name}")

    def calibrate_spectrum(self, name: str, coeff: list):
        """Apply a polynomial calibration of the x-axis."""
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        self.spectra[name].apply_calibration(coeff)
        self.Signals.spectrumUpdated.emit(name)
        gui_logger.info(f"Spectrum calibrated: name={name}")

    def set_color(self, name: str, fg_bkg: str, color: QColor):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        if fg_bkg.lower() == "foreground":
            self.spectra[name].color_foreground = color
        else:
            self.spectra[name].color_background = color

        self.Signals.colorUpdated.emit(name)

    def update_visibility(self, name: str):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        if self.spectra[name].show_in_plot:
            self.spectra[name].show_in_plot = False
            self.spectra[name].fit_rois = False
            self.Signals.visibilityChanged.emit(False)

        else:
            self.spectra[name].show_in_plot = True
            self.spectra[name].fit_rois = True
            self.Signals.visibilityChanged.emit(True)

    # --- Getters ---

    def get_spectrum(self, name: str) -> Spectrum | None:
        return self.spectra.get(name)

    def get_spectra_dict(self) -> dict[str, Spectrum]:
        return self.spectra

    # --- IO ---
    def import_spectrum(self, data_dict: dict):
        if "foreground" not in data_dict:
            gui_logger.warning("File contains no spectrum")
            return
        self.create_spectrum(data_dict["name"], data_dict["foreground"].channels)
        self.set_foreground_spectrum(data_dict["name"], data_dict["foreground"])

        if "background" in data_dict:
            self.set_background_spectrum(data_dict["name"], data_dict["background"])

        if "calibration" in data_dict:
            self.calibrate_spectrum(data_dict["name"], data_dict["calibration"])

        if "peaks" in data_dict:
            for peak in data_dict["peaks"]:
                extented_kwargs = {
                    "alias": peak.alias,
                    "fit_type": peak.fit_type,
                    "bkg_type": peak.bkg_type,
                    "emission": peak.emission,
                    **peak.meta,
                }

                self.ROIManager.add_roi(*peak.roi_bound, **extented_kwargs, owner_spectrum = data_dict["name"] if Settings.Appearance.tabbed_spectrum_view else None)
                
        if "instrument" in data_dict and data_dict["instrument"] is not None:
            instr = data_dict["instrument"]
            if instr.name in self.UniqueInstrumentLibrary.get_instrument_names():
                # Attach the stored instrument if possible
                # Otherwise it will not share the reference to the same instrument
                # If so changes will not propagate
                self.set_spectrum_instrument(data_dict["name"], self.UniqueInstrumentLibrary.get_instrument_by_name(instr.name))
            
            else:
                # If the instrument does not exist, save the new instrument and let the data library know
                dummy_spectrum = Spectrum(1, "Dummy")
                dummy_spectrum.instrument = instr
                file_path = (Settings.Paths.unique_instrument_library / instr.name).with_suffix(".xml")
                self.UniqueInstrumentLibrary.instruments[file_path] = instr
                xml_writer(dummy_spectrum, file_path, export_spectrum=False, export_rois=False)
                
                self.set_spectrum_instrument(data_dict["name"], self.UniqueInstrumentLibrary.get_instrument_by_name(instr.name))
                self.Signals.newInstrumentLoaded.emit(file_path)
        
        if "remark" in data_dict:
            self.spectra[data_dict["name"]].remark = data_dict["remark"]
            
        self.Signals.spectrumUpdated.emit(data_dict["name"])

    def import_spectrum_as_background(self, spectrum_name: str, data_dict: dict):
        if "foreground" not in data_dict:
            gui_logger.warning("File contains no spectrum")
            return
        self.set_background_spectrum(spectrum_name, data_dict["foreground"])
        
    def set_spectrum_instrument(self, spectrum_name: str, instrument: UniqueInstrument):
        assert isinstance(instrument, UniqueInstrument), f"Instrument must be UniqueInstrument, is {type(instrument)}"
        self.spectra[spectrum_name].instrument = instrument
        self.Signals.spectrumUpdated.emit(spectrum_name)
        
    def clear_spectrum_instrument(self, spectrum_name: str):
        self.spectra[spectrum_name].instrument = None
        self.Signals.spectrumUpdated.emit(spectrum_name)
        
# Declare ONE instance
SpectrumManager = SpectrumManagerBase()
