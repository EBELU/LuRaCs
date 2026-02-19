from .SpectrumManager import SpectrumManager
from .RunManager import RunManager

_ = None

RunManager.createDeviceSpectrum.connect(SpectrumManager.create_spectrum)
RunManager.removeDeviceSpectrum.connect(SpectrumManager.remove_spectrum)
RunManager.spectrumUpdated.connect(SpectrumManager.set_foreground_spectrum)