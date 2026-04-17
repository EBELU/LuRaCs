from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    pass
from PySide6.QtCore import QObject, Signal
from NuclideClasses import Nuclide, Emission


class NuclideLibrary(QObject):
    def __init__(self, parent):
        super().__init__(parent)
        self.nuclides: dict[str, Nuclide] = {}

    def add_nuclide(self, nuclide: Nuclide):
        assert nuclide.nuclide not in self.nuclides, f"Nuclide with name {nuclide.nuclide} already exists in library"
        self.nuclides[nuclide.nuclide] = nuclide

    def get_nuclide(self, name: str) -> Nuclide | None:
        return self.nuclides.get(name, None)
    
    def get_nuclide_names(self) -> list[str]:
        return list(self.nuclides.keys())
    
    def get_all_energies(self) -> list[float]:
        energies = []
        for nuclide in self.nuclides.values():
            for emission in nuclide.emissions:
                energies.append(emission.energy_keV)
        return energies