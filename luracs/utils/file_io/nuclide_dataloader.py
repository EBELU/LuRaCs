from containers.nuclide_classes import Nuclide, Emission
from core import SpectrumManager, Settings
import json
import glob


def load_nuclide_data():
    for file in glob.glob(str(Settings.Paths.nuclide_data / "*.json")):
        with open(file) as f:
            data = json.load(f)
            nuclide = nuclide_from_json(data)
            SpectrumManager.NuclideLibrary.add_nuclide(nuclide)


def nuclide_from_json(data: dict) -> Nuclide:
    emissions = [
        Emission(
            parent_nuclide=data["Nuclide"],
            energy_keV=e["Energy (keV)"],
            energy_error_keV=e["Energy error (keV)"],
            intensity_percent=e["I (%)"],
            intensity_error_percent=e["I error (%)"],
            type=e["Type"],
            origin=e["Origin"],
        )
        for e in data.get("Emissions", [])
    ]

    return Nuclide(
        nuclide=data["Nuclide"],
        element=data["Element"],
        Z=int(data["Z"]),
        daughters=[(d[0], d[1], float(d[2])) for d in data["Daughters"]],
        citation_ref=data["citation_ref"],
        half_life_s=(
            float(data["Half-life (s)"][0]),
            float(data["Half-life (s)"][1]),
        ),
        specific_activity_Bq_per_g=(
            float(data["Specific activity (Bq/g)"][0]),
            float(data["Specific activity (Bq/g)"][1]),
        ),
        emissions=emissions,
    )
