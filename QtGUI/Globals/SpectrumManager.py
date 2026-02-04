from ..SpectrumClasses import Spectrum
from PySide6.QtCore import QObject, Signal

class EmittedSignals(QObject):
    spectrumCreated = Signal(str)
    spectrumUpdated = Signal(str)
    spectrumRemoved = Signal(str)
    
    roiCreated = Signal(str)
    roiUpdated = Signal(str)
    roiRemoved = Signal(str)
    

    def __init__(self, parent=None):
        super().__init__(parent)
        
        

class SpectrumManagerBase(QObject):
    
    def __init__(self):
        super().__init__()
        
        self.spectra = {}
        self.existing_rois = []
        
        self.roi_counter = 0 # Counts rois to ensure unique tags
        
        self.Signals = EmittedSignals()
        
    # --- Spectrum manipulators ---
        
    def create_spectrum(self, name, channels):
        if name not in self.spectra:
            self.spectra[name] = Spectrum(channels, name)
            self.Signals.spectrumCreated.emit(name)
            return True
        else:
            return False
        
    def set_primary_spectrum(self, name, spectrum_data, uptime = None):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        self.spectra[name].set_y_data(spectrum_data, uptime)
        self.Signals.spectrumUpdated.emit(name)
        
    def set_background_spectrum(self, name, spectrum_data, uptime = None):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")

        self.spectra[name].set_y_bkg(spectrum_data, uptime)
        self.Signals.spectrumUpdated.emit(name)
        
    def remove_spectrum(self, name):
        if name in self.spectra:
            self.spectra.pop(name)
            self.Signals.spectrumRemoved.emit(name)
            
    def calibrate_spectrum(self, name, coeff):
        if name not in self.spectra:
            raise ValueError(f"Spectrum {name} does not exist")
        
        self.spectra[name].apply_calibration(coeff)
        self.Signals.spectrumUpdated.emit(name)
            
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
        
        return roi_tag
    
    def update_ROI(self, tag, x_min, x_max, use_cps):
        for spect_tag, spectrum in self.spectra.items():
            if spectrum.fit_rois:
                spectrum.update_roi(tag, x_min, x_max, use_cps)
        
        self.Signals.roiUpdated.emit(tag)
                
    def remove_ROI(self, tag):
        for spectrum in self.spectra.values():
            spectrum.ROIs.pop(tag, None)
                
    
# Declare ONE instance
SpectrumManager = SpectrumManagerBase()