from SpectrumClasses import Spectrum, SpectrumData
from PySide6.QtCore import QObject, Signal
from PySide6.QtGui import QColor
from clients.DeviceWrappers import WrappedSpectrumPackage
from datetime import datetime, timedelta

from InstrumentClasses import GenericInstrument, UniqueInstrument

from .gui_logger import gui_logger
from .roi_manager import ROIManager
from .instument_library import InstrumentLibrary
from .nuclide_library import NuclideLibrary

"""
    The Spectrum manager handles the spectra in the program.
    GUI components can request actions from the spectrum manager but should not change the state of any spectrum without going through the manager.
"""


class SpectrumColorRotator:
    def __init__(self, colors="mpl", width=2):
        if colors == "mpl":  # Matplotlib
            colors = [
                "#1f77b4",
                "#ff7f0e",
                "#2ca02c",
                "#d62728",
                "#9467bd",
                "#8c564b",
            ]
        elif colors == "lo":  # LibreOffice
            colors = [
                "#004586",
                "#ff420e",
                "#ffd320",
                "#579d1c",
                "#7e0021",
                "#83caff",
            ]

        # Normalize everything to QColor
        self.colors = [QColor(c) for c in colors]

        self.width = width
        self._i = 0

    def next_color(self) -> QColor:
        color = self.colors[self._i % len(self.colors)]
        self._i += 1
        return QColor(color)  # return a copy (safe to modify)

    def reset(self):
        self._i = 0


class EmittedSignals(QObject):
    spectrumCreated = Signal(str)
    spectrumUpdated = Signal(str)
    spectrumRemoved = Signal(str)

    backgroundRemoved = Signal(str, str)
    visibilityChanged = Signal(bool)

    roiCreated = Signal(str)
    roiUpdated = Signal(str)
    roiRemoved = Signal(str)

    colorUpdated = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)


class SpectrumManagerBase(QObject):
    def __init__(self):
        super().__init__()
        self.color_rotation = SpectrumColorRotator("mpl")

        self.spectra: dict[str, Spectrum] = {}
        self.existing_rois = []

        self.Signals = EmittedSignals()

        self.ROIManager = ROIManager(self)

        self.UniqueInstrumentLibrary = InstrumentLibrary(self, UniqueInstrument)
        self.GenericInstrumentLibrary = InstrumentLibrary(self, GenericInstrument)

        self.NuclideLibrary = NuclideLibrary(self)

    # --- Spectrum manipulators ---

    def create_spectrum(self, name: str, channels: int, device: str = None):
        if name not in self.spectra:
            # Create spectrum and add a possible connection
            new_spect = Spectrum(channels, name)
            new_spect.connected_device = device
            self.spectra[name] = new_spect

            # Set colors
            clr = self.color_rotation.next_color()
            self.set_color(name, "foreground", clr)
            self.set_color(name, "background", clr)

            # Emit done
            self.Signals.spectrumCreated.emit(name)
            gui_logger.info(f"[Spectrum added] {name}")
            return True
        else:
            return False

    def set_foreground_spectrum(
        self, name: str, spectrum_data: WrappedSpectrumPackage | SpectrumData
    ):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        if isinstance(spectrum_data, WrappedSpectrumPackage):
            start_date = datetime.now() - timedelta(seconds=spectrum_data.uptime)
            new_spectrum = SpectrumData(
                spectrum_data.y_axis,
                len(spectrum_data.y_axis),
                sum(spectrum_data.y_axis),
                spectrum_data.uptime,
                None,
                sum(spectrum_data.y_axis) / max(spectrum_data.uptime, 1),
                None,
                start_date,
                None,
                name,
                None,
            )
            calib_coeff = spectrum_data.calib_coeff

        elif isinstance(spectrum_data, SpectrumData):
            new_spectrum = spectrum_data
            calib_coeff = None
        else:
            gui_logger.warning(f"Invalid spectrum data type {type(spectrum_data)}")
            return

        self.spectra[name].set_foreground(new_spectrum)
        if not self.spectra[name].calibrated and calib_coeff is not None:
            self.calibrate_spectrum(name, spectrum_data.calib_coeff)

        self.Signals.spectrumUpdated.emit(name)

    def set_background_spectrum(self, name: str, spectrum_data: SpectrumData):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        y_axis = getattr(spectrum_data, "y_axis", None)

        if y_axis is None:
            return

        new_spectrum = SpectrumData(
            y_axis,
            len(y_axis),
            sum(y_axis),
            getattr(spectrum_data, "live_time", None),
            getattr(spectrum_data, "real_time", None),
            getattr(spectrum_data, "avg_dose_rate", None),
            getattr(spectrum_data, "avg_cps", None),
        )

        self.spectra[name].set_background(new_spectrum)
        self.Signals.spectrumUpdated.emit(name)

    def clear_background(self, name: str):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        self.spectra[name].background = None
        self.Signals.backgroundRemoved.emit(name, "bkg")
        gui_logger.info(f"[Background removed] {name}")

    def remove_spectrum(self, name: str):
        if name in self.spectra:
            self.spectra.pop(name)
            self.Signals.spectrumRemoved.emit(name)
            gui_logger.info(f"[Spectrum removed] {name}")

    def calibrate_spectrum(self, name: str, coeff: list):
        """Apply a polynomial calibration of the x-axis."""
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        self.spectra[name].apply_calibration(coeff)
        self.Signals.spectrumUpdated.emit(name)

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

                self.ROIManager.add_roi(*peak.roi_bound, **extented_kwargs)

    def import_spectrum_as_background(self, spectrum_name: str, data_dict: dict):
        if "foreground" not in data_dict:
            gui_logger.warning("File contains no spectrum")
            return
        self.set_background_spectrum(spectrum_name, data_dict["foreground"])


# Declare ONE instance
SpectrumManager = SpectrumManagerBase()
