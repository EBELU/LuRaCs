from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.core.spectrum_manager import SpectrumManagerBase
    from PySide6.QtGui import QColor
from PySide6.QtCore import QObject, Signal
from luracs.containers.nuclide_classes import Nuclide, Emission
import numpy as np
from collections import Counter
from luracs.utils.file_io.nuclide_dataloader import load_nuclide_data
from .settings import Settings


class NuclideLibrary(QObject):
    sigViewCheckChanged = Signal(str, bool, object)  # Propagated from the ui tab

    def __init__(self, spectrum_manager: SpectrumManagerBase):
        super().__init__(parent=spectrum_manager)
        self.nuclides: dict[str, Nuclide] = {}
        self.spectrum_manager = spectrum_manager  # Reference
        self.decay_chains: dict[str, list[str]] = {
            "Th-232 -- Chain": [
                "Th-232",
                "Ra-228",
                "Ac-228",
                "Th-228",
                "Ra-224",
                "Rn-220",
                "Po-216",
                "Pb-212",
                "Bi-212",
                # branch point:
                "Po-212",  # (alpha decay to Pb-208, stable)
                "Tl-208",  # alternative beta branch from Bi-212
                "Pb-208",  # stable end product (via Tl-208)
            ],
            "U-238 -- Chain": [
                "U-238",
                "Th-234",
                "Pa-234",
                "U-234",
                "Th-230",
                "Ra-226",
                "Rn-222",
                "Po-218",
                "Pb-214",
                "Bi-214",
                # branch point:
                "Po-214",  # alpha decay -> Pb-210
                "Tl-210",  # rare beta branch from Bi-214
                "Pb-210",
                "Bi-210",
                # branch point:
                "Po-210",  # alpha decay -> Pb-206 (stable)
                "Tl-206",  # rare beta branch from Bi-210
                "Pb-206",  # stable end product
            ],
            # "U-235 -- Chain": [
            #     "U-235",
            #     "Th-231",
            #     "Pa-231",
            #     "Ac-227",
            #     "Th-227",
            #     "Ra-223",
            #     "Rn-219",
            #     "Po-215",
            #     "Pb-211",
            #     "Bi-211",
            #     # branch point:
            #     "Po-211",  # -> Pb-207 (stable)
            #     "Tl-207",  # -> Pb-207 (stable)
            #     "Pb-207"   # stable end product
            # ]
        }
        self.selected_nuclides: set[str] = set()

        for nuclide in load_nuclide_data(str(Settings.Paths.nuclide_data / "*.json")):
            self.add_nuclide(nuclide)

    def add_nuclide(self, nuclide: Nuclide):
        assert nuclide.nuclide not in self.nuclides, (
            f"Nuclide with name {nuclide.nuclide} already exists in library"
        )
        self.nuclides[nuclide.nuclide] = nuclide

    def get_nuclide(self, name: str) -> Nuclide | None:
        "Get one nuclide based on the nn-iii key"
        return self.nuclides.get(name, None)

    def get_sorted_nuclide_names(
        self, require_photon_emissions: bool = False
    ) -> list[str]:
        "Get a list of all nuclides sorted by atomic mass number"
        if require_photon_emissions:
            photon_nuclides = [
                k for k, v in self.nuclides.items() if len(v.emissions) > 0
            ]
            return sorted(
                photon_nuclides, key=lambda n: int(n.split("-")[-1].removesuffix("m"))
            )

        else:
            return sorted(
                self.nuclides.keys(),
                key=lambda n: int(n.split("-")[-1].removesuffix("m")),
            )

    def match_energy_to_nuclide(
        self,
        energy: float,
        match_only_shown=True,
        window: float = 50,
        weight_by_intensity: bool = True,
        filter_by_intensity_precent: int = -np.inf,
    ) -> Emission | None:
        "Matches and energy to the closes emission within a window surrounding the given energy. All units are keV."
        if not match_only_shown or len(self.selected_nuclides) == 0:
            emissions = [
                e
                for e in self.get_all_emissions()
                if abs(e.energy_keV - energy) < window
                and e.intensity_percent > filter_by_intensity_precent
            ]
        else:
            emissions = []
            for nuclide_name in self.selected_nuclides:
                emissions.extend(
                    [
                        e
                        for e in self.get_nuclide(nuclide_name).emissions
                        if abs(e.energy_keV - energy) < window
                        and e.intensity_percent > filter_by_intensity_precent
                    ]
                )

        energies = np.fromiter(
            (
                e.energy_keV * ((e.intensity_percent * 0.01) ** 0.7)
                if weight_by_intensity
                else e.energy_keV
                for e in emissions
            ),
            dtype=float,
        )

        if len(energies) == 0:
            return

        idx = np.argmin((energies - energy) ** 2)
        closest_emission = emissions[idx]

        return closest_emission

    def match_roi_to_nuclide(self, roi_tag: str, energy_search_window: float = 50):
        "Attempts to match a roi to an emission by matching centroids to emissions and choosing the most common result."
        rois = self.spectrum_manager.ROIManager.get_data_from_roi(roi_tag)

        if len(rois) == 0:
            return

        matches = [
            self.match_energy_to_nuclide(r.fit.mu, window=energy_search_window)
            for r in rois.values()
            if r.fit is not None
        ]

        most_common = Counter(matches).most_common(1)

        if most_common:
            result, count = most_common[0]
        else:
            result = None

        return result

    def _track_selected_nuclies(self, nuclide: str, show_status: bool, color: QColor):
        if show_status:
            self.selected_nuclides.add(nuclide)
        else:
            self.selected_nuclides.remove(nuclide)

        self.sigViewCheckChanged.emit(nuclide, show_status, color)

    def get_all_emissions(self) -> list[Emission]:
        energies = []
        for nuclide in self.nuclides.values():
            for emission in nuclide.emissions:
                energies.append(emission)
        return energies
