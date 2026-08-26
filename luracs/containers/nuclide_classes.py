from dataclasses import dataclass


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
    daughters: list[tuple[str, float]]
    half_life_s: tuple[float, float]
    specific_activity_Bq_per_g: tuple[float, float]
    citation_ref: str
    emissions: list[Emission]


EmptyEmission = Emission(
    parent_nuclide="",
    energy_keV=None,
    energy_error_keV=None,
    intensity_percent=None,
    intensity_error_percent=None,
    origin="",
    type="",
)

AnnihilationEmission = Emission(
    parent_nuclide="Annihilation",
    energy_keV=511,
    energy_error_keV=1,
    intensity_percent=100,
    intensity_error_percent=1,
    type="annih.",
    origin="Annih."
)
