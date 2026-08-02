import copy
from dataclasses import dataclass
from datetime import datetime

import numpy as np


@dataclass
class GenericInstrument:
    "Stores the data associated with a detector type"

    model: str
    manufacturer: str
    detector_type: str
    detector_material: str = None
    detector_shape: str = None
    detector_dimensions_cm: list = None  # cm
    detector_dimensions_uncert_cm: list = None  # cm

    # --- Resolution ---
    resolution_fn: str = None
    resolution_params: list = None
    resolution_E_points: list = None
    resolution_FWHM_points: list = None
    resolution_FWHM_uncert_points: list = None
    resolution_created: datetime = None

    # --- Efficiency ---
    int_efficiency_fn: str = None
    int_efficiency_params: list = None
    int_efficiency_E_points: list = None
    int_efficiency_eff_points: list = None
    int_efficiency_uncert_points: list = None
    int_efficiency_created: datetime = None
    int_efficiency_description: str = None

    # --- Response Matrix ---
    response_matrix: np.ndarray = None
    response_matrix_shape: list = None

    remark: str = ""

    def get_copy(self):
        return copy.deepcopy(self)


@dataclass(kw_only=True)
class UniqueInstrument(GenericInstrument):
    "Stores the data for a unique instrument, should be created from a generic instrument UniqueInstrument('Name', **template.get_copy().__dict__())"

    name: str = ""  # Important

    calibration_poly_order: int = None
    calibration_coefficients: list = None
    calibration_energy_points: list = None  # known energies (keV)
    calibration_channel_points: list = None  # corresponding channels
    calibration_date: datetime = None
