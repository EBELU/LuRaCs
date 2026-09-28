from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import ROI
import math
from dataclasses import dataclass
from datetime import datetime
from enum import Enum

from PySide6.QtCore import QDate, QDateTime, QTime
from PySide6.QtWidgets import QComboBox, QDoubleSpinBox, QHBoxLayout, QWidget
from uncertainties import Variable, ufloat, umath


class TimeUnits(Enum):
    us = 1e-6
    ms = 1-3
    s = 1
    min = 60
    h = 60*60
    d = 60*60*24
    yr = 60*60*24*365.24
    
class ActivityUnits(Enum):
    Bq  = 1
    kBq = 1e3
    MBq = 1e6
    GBq = 1e9
    TBq = 1e12

    Ci  = 3.7e10
    mCi = 3.7e7
    uCi = 3.7e4
    nCi = 3.7e1

@dataclass(frozen=True, kw_only=True)
class Source:
    name: str = "Unused"
    nuclide: str = "Other"
    spectrum: str = "None"
    half_life_s: float = 0
    ref_activity_Bq: float = 0
    ref_activity_uncert_Bq: float = 0
    calibration_time: datetime = datetime.now()
    measurement_start: datetime = datetime.now()
    measurement_end: datetime = datetime.now()
    distance_cm: float = 0
    distance_uncert_cm: float = 0
    solid_angle: float = 0
    solid_angle_uncert: float = 0
    
    measurement_decay_compensate: bool = True
    custom_sold_angle: bool = False
    
def format_duration(seconds: float) -> tuple[float, TimeUnits]:
    if seconds == 0:
        return 0.0, TimeUnits.s

    abs_s = abs(seconds)

    units = sorted(TimeUnits, key=lambda unit: unit.value, reverse=True)

    for unit in units:
        if abs_s >= unit.value:
            return seconds / unit.value, unit

    return seconds / TimeUnits.us.value, TimeUnits.us

def format_activity(activity_Bq: float | Variable) -> tuple[float | Variable, ActivityUnits]:
    if activity_Bq == 0:
        if isinstance(activity_Bq, float):
            return 0.0, ActivityUnits.Bq
        else:
            return ufloat(0, 0), ActivityUnits.Bq

    abs_activity = abs(activity_Bq) if isinstance(activity_Bq, float) else abs(activity_Bq.n)

    units = sorted(
        [ActivityUnits.TBq, ActivityUnits.GBq, ActivityUnits.MBq,
         ActivityUnits.kBq, ActivityUnits.Bq],
        key=lambda unit: unit.value,
        reverse=True,
    )

    for unit in units:
        if abs_activity >= unit.value:
            return activity_Bq / unit.value, unit

    return activity_Bq, ActivityUnits.Bq
    
def value_with_uncertainty(has_unit=False):
    container = QWidget()
    layout = QHBoxLayout(container)
    layout.setContentsMargins(0, 0, 0, 0)

    value = QDoubleSpinBox()
    uncertainty = QDoubleSpinBox()
    unit = QComboBox()

    value.setRange(0, 1e12)
    uncertainty.setRange(0, 1e12)

    uncertainty.setPrefix("± ")

    layout.addWidget(value)
    layout.addWidget(uncertainty)
    if has_unit:
        layout.addWidget(unit)
        return container, value, uncertainty, unit
    else:
        return container, value, uncertainty
    
def datetime_to_qdatetime(value: datetime) -> QDateTime:
    return QDateTime(
        QDate(value.year, value.month, value.day),
        QTime(
            value.hour,
            value.minute,
            value.second,
            value.microsecond // 1000,
        ),
    )

def qdatetime_to_datetime(value: QDateTime) -> datetime:
    date = value.date()
    time = value.time()

    return datetime(
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.msec() * 1000,
    )
    

def number_of_decays(A0: float, half_life: float, duration: float) -> float:
    """
    Calculate the expected number of radioactive decays during a measurement.

    Parameters
    ----------
    A0 : float
        Activity at the start of the measurement, in Bq (decays/s).
    half_life : float
        Half-life, in seconds.
    duration : float
        Measurement duration, in seconds.

    Returns
    -------
    float
        Expected number of decays.
    """
    decay_constant = math.log(2) / half_life

    return A0 / decay_constant * (
        1 - math.exp(-decay_constant * duration)
    )


def decayed_activity(A0: float,
                     half_life: float,
                     duration: float) -> float:
    decay_constant = umath.log(2) / half_life

    return A0 * umath.exp(-decay_constant * duration)

def calculate_efficiency(source: Source, roi: ROI) -> tuple[float, float] | None:
    """
    Calculate the intrinsic efficiency for a configured source and ROI.

    Returns:
        tuple[float, float] | None: Intrinsic efficiency and its uncertainty.

    Raises:
        ValueError: If a required variable is not set. Required variables are
            Activity, Duration, Solid Angle, Half Life and Emission Yield.
    """

    t_calib_to_start_s = (source.measurement_start - source.calibration_time).total_seconds()
    A_calib = source.ref_activity_Bq if source.ref_activity_uncert_Bq == 0 else ufloat(source.ref_activity_Bq, source.ref_activity_uncert_Bq)
    
    if A_calib <= 0:
        raise ValueError(f"Calculation failed. Invalid value for Activity! value={A_calib}, source={source.name}, roi={roi.alias}")

    hl = source.half_life_s
    
    if hl == 0:
        raise ValueError(f"Calculation failed. Invalid value for Half Life! value={hl}, source={source.name}, roi={roi.alias}")
    
    A_start = decayed_activity(A_calib, hl, t_calib_to_start_s)
    
    measurement_duration_s = (source.measurement_end - source.measurement_start).total_seconds()
    if measurement_duration_s == 0:
        raise ValueError(f"Calculation failed. Invalid value for Measurement Duration! value={measurement_duration_s}, source={source.name}, roi={roi.alias}")

    if source.measurement_decay_compensate:
        decays_during_measurement = number_of_decays(A_start, hl, measurement_duration_s)
    else:
        decays_during_measurement = A_start * measurement_duration_s
        
    solid_angle = source.solid_angle if source.solid_angle_uncert == 0 else ufloat(source.solid_angle, source.solid_angle_uncert)
    
    if solid_angle <= 0:
        raise ValueError(f"Calculation failed. Invalid value for Solid Angle! value={solid_angle}, source={source.name}, roi={roi.alias}")
    
    cps = roi.get_count_data("N", True)
    
    if cps is None:
        return
    
    em = roi.emission
    I = em.intensity_percent / 100 if em.intensity_error_percent == 0 else ufloat(em.intensity_percent, em.intensity_error_percent) / 100
    
    if em.intensity_percent <= 0:
        raise ValueError(f"Calculation failed. Invalid value for Emission Yield! value={I}, source={source.name}, roi={roi.alias}")

    int_eff = cps / (solid_angle * I * decays_during_measurement)
    
    return int_eff.n, int_eff.s

    
     
    