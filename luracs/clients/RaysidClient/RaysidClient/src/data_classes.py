from dataclasses import dataclass
import numpy as np


@dataclass(frozen=True)
class CurrentValuesPackage:
    CPS: float
    DR: float
    timestamp: float


@dataclass(frozen=True)
class StatusPackage:
    battery: int
    temperature: float
    temp_ok: bool
    charging: bool
    channel_full: bool
    ch239: int
    ch239keV: int
    timestamp: float


@dataclass(frozen=True)
class SpectrumResult:
    spectrum: np.ndarray
    counts: int
    uptime: float
    energy: float
    high_E_counts: int
    timestamp: float
