from ..SpectrumClasses import Spectrum, SpectrumData
from PySide6.QtCore import QObject, Signal
from PySide6.QtGui import QColor

from .GUILogger import gui_logger

"""
    The Spectrum manager handles the spectra in the program.
    GUI components can request actions from the spectrum manager but should not change the state of any spectrum without going through the manager.
"""

class SpectrumColorRotator:
    def __init__(self, colors="mpl", width=2):
        if colors == "mpl":
            colors = [
                "#1f77b4", "#ff7f0e", "#2ca02c",
                "#d62728", "#9467bd", "#8c564b",
            ]
        elif colors == "lo":
            colors = [
                "#004586", "#ff420e", "#ffd320",
                "#579d1c", "#7e0021", "#83caff",
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
    
    roiCreated = Signal(str)
    roiUpdated = Signal(str)
    roiRemoved = Signal(str)

    colorUpdated = Signal(str)
    

    def __init__(self, parent=None):
        super().__init__(parent)
        
        

class SpectrumManagerBase(QObject):
    
    def __init__(self):
        super().__init__()

        self.color_rotation = SpectrumColorRotator("lo")
        
        self.spectra: dict[str, Spectrum] = {}
        self.existing_rois = []
        
        self.roi_counter = 0 # Counts rois to ensure unique tags
        
        self.Signals = EmittedSignals()
        
        
        
    # --- Spectrum manipulators ---
        
    def create_spectrum(self, name: str, channels: int):
        if name not in self.spectra:
            self.spectra[name] = Spectrum(channels, name)
            clr = self.color_rotation.next_color()
            self.set_color(name, "foreground", clr)
            self.set_color(name, "background", clr)
            self.Signals.spectrumCreated.emit(name)
            gui_logger.info(f"[Spectrum added] {name}")
            return True
        else:
            return False
        
    def set_foreground_spectrum(self, name, spectrum_data):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        y_axis = getattr(spectrum_data, "y_axis", None)

        if y_axis is None:
            return
        
        new_spectrum = SpectrumData(y_axis,
                                    len(y_axis),
                                    sum(y_axis),
                                    getattr(spectrum_data, "live_time", None),
                                    getattr(spectrum_data, "real_time", None),
                                    getattr(spectrum_data, "avg_dose_rate", None),
                                    getattr(spectrum_data, "avg_cps", None),)


        self.spectra[name].set_foreground(new_spectrum)

        self.Signals.spectrumUpdated.emit(name)
        
    def set_background_spectrum(self, name, spectrum_data):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        y_axis = getattr(spectrum_data, "y_axis", None)

        if y_axis is None:
            return
        
        new_spectrum = SpectrumData(y_axis,
                                    len(y_axis),
                                    sum(y_axis),
                                    getattr(spectrum_data, "live_time", None),
                                    getattr(spectrum_data, "real_time", None),
                                    getattr(spectrum_data, "avg_dose_rate", None),
                                    getattr(spectrum_data, "avg_cps", None),)

        self.spectra[name].set_background(new_spectrum)
        self.Signals.spectrumUpdated.emit(name)
        
    def remove_spectrum(self, name):
        if name in self.spectra:
            self.spectra.pop(name)
            self.Signals.spectrumRemoved.emit(name)
            gui_logger.info(f"[Spectrum removed] {name}")
            
    def calibrate_spectrum(self, name, coeff):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        self.spectra[name].apply_calibration(coeff)
        self.Signals.spectrumUpdated.emit(name)

    def set_color(self, name, fg_bkg: str, color: QColor):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        print(name)
        if fg_bkg.lower() == "foreground":
            self.spectra[name].color_foreground = color
        else:
            self.spectra[name].color_background = color

        self.Signals.colorUpdated.emit(name)
            
    # --- Getters ---
        
    def get_spectrum(self, name: str) -> Spectrum:
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        return self.spectra[name]
    
    def get_spectra_dict(self) -> dict[str, Spectrum]:
        return self.spectra
    
    
    # --- ROIs ---
    
    def create_ROI(self):
        roi_tag = f"ROI_{self.roi_counter}"
        self.roi_counter += 1
        self.existing_rois.append(roi_tag)
        gui_logger.info(f"[ROI added] {roi_tag}")
        return roi_tag
    
    def update_ROI(self, tag, x_min, x_max, use_cps):
        for spect_tag, spectrum in self.spectra.items():
            if spectrum.fit_rois:
                spectrum.update_roi(tag, x_min, x_max, use_cps)
        
        self.Signals.roiUpdated.emit(tag)
                
    def remove_ROI(self, tag):
        gui_logger.info(f"[ROI removed] {tag}")
        for spectrum in self.spectra.values():
            spectrum.ROIs.pop(tag, None)

        self.Signals.roiRemoved.emit(tag)
        
                
    
# Declare ONE instance
SpectrumManager = SpectrumManagerBase()



