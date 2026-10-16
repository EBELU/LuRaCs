from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from datetime import datetime
    from luracs.gui.measure_plot import Measurement

from dataclasses import dataclass

@dataclass(kw_only=True, frozen=True)
class MeasurementReport:
    date: datetime
    author_name: str
    location: str
    longitude: float | None = None
    latitude: float | None = None
    weather: str
    wind_direction: str
    wind_speed: float
    air_temperature: float
    remark: str
    fg_measurement: Measurement
    bg_measurement: Measurement
