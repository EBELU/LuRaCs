from dataclasses import dataclass
from datetime import datetime
import numpy as np

@dataclass
class Instrument:
    "Stores the data associated with a detector type"
    model: str
    manufacturer: str
    detector_type: str
    detector_material: str = None
    detector_shape: str = None
    detector_volume: float = None

    resolution_fn: float = None
    resolution_param: list = None
    resolution_E_points: list = None
    resolution_FWHM_points: list = None
    
    efficiency_fn: str = None
    efficiecny_param: list = None
    efficiency_created: datetime = None
    efficiency_description: str = None

    response_matrix: np.array = None
    response_matrix_shape: list = None

@dataclass
class UniqueInstrument(Instrument):
    id: str
    calibration_poly_order: int = None
    calibration_coefficients: list = None
    note: str = None
    