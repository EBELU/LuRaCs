from dataclasses import dataclass
from typing import List, Tuple


@dataclass(frozen=True, kw_only=True)
class Emission:
    parent_nuclide: str
    energy_keV: float
    energy_error_keV: float
    intensity_percent: float
    intensity_error_percent: float
    type: str
    origin: str


@dataclass(frozen=True, kw_only=True)
class Nuclide:
    nuclide: str
    element: str
    Z: int
    daughters: List[Tuple[str, float]]
    half_life_s: Tuple[float, float]
    specific_activity_Bq_per_g: Tuple[float, float]
    citation_ref: str
    emissions: List[Emission]
    
EmptyEmission = Emission(
    parent_nuclide="", 
    energy_keV=None, 
    energy_error_keV=None, 
    intensity_percent=None,
    intensity_error_percent=None,
    origin="",
    type="")


