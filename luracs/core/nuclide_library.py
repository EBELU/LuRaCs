from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from core.spectrum_manager import SpectrumManagerBase
from PySide6.QtCore import QObject, Signal
from NuclideClasses import Nuclide, Emission
import numpy as np
from collections import Counter

class NuclideLibrary(QObject):
    def __init__(self, spectrum_manager: SpectrumManagerBase):
        super().__init__(parent=spectrum_manager)
        self.nuclides: dict[str, Nuclide] = {}
        self.spectrum_manager = spectrum_manager
        self.decay_chains = {}
        
    def add_nuclide(self, nuclide: Nuclide):
        assert nuclide.nuclide not in self.nuclides, f"Nuclide with name {nuclide.nuclide} already exists in library"
        self.nuclides[nuclide.nuclide] = nuclide

    def get_nuclide(self, name: str) -> Nuclide | None:
        "Get one nuclide based on the nn-iii key"
        return self.nuclides.get(name, None)
    
    def get_sorted_nuclide_names(self) -> list[str]:
        "Get a list of all nuclides sorted by atomic mass number"
        return sorted(self.nuclides.keys(), key = lambda n: int(n.split("-")[-1].removesuffix("m")))
    
    def match_energy_to_nuclide(self, energy: float, window = 50) -> Emission:
        "Matches and energy to the closes emission within a window surrounding the given energy. All units are keV."
        emissions = [e for e in self.get_all_emissions() if abs(e.energy_keV - energy) < window]
        energies = np.array([e.energy_keV * ((e.intensity_percent * 0.01) ** 0.7) for e in emissions])
        

        idx = np.argmin((energies - energy)**2)
        closest_emission = emissions[idx]
        
        return closest_emission
    
    def match_roi_to_nuclide(self, roi_tag: str):
        "Attempts to match a roi to an emission by matching centroids to emissions and choosing the most common result."
        rois = self.spectrum_manager.ROIManager.get_data_from_roi(roi_tag)
        
        if len(rois) == 0:
            return
        
        matches = [self.match_energy_to_nuclide(r.fit.mu) for r in rois.values() if r.fit is not None]
        
        most_common = Counter(matches).most_common(1)

        if most_common:
            result, count = most_common[0]
        else:
            result = None
            
        return result
    
    
    def get_all_emissions(self) -> list[Emission]:
        energies = []
        for nuclide in self.nuclides.values():
            for emission in nuclide.emissions:
                energies.append(emission)
        return energies