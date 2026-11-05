import logging
import math
import time

from PySide6.QtCore import QObject, QTimer, Signal

from .gps import GPSData


class MockGPS(QObject):
    """Simulated GPS client compatible with the real GPS interface."""

    sigUpdated = Signal(object)

    log = logging.getLogger("GPS")
    
    class port:
        @classmethod
        def portName(cls):
            return "Mock GPS"

    def __init__(
        self,
        center_lat: float = 55.58944353363139,
        center_lon: float = 13.001056311555528,
        radius: float = 25.0,
        speed: float = 1.0,
        update_interval_ms: int = 1000,
        parent=None,
    ):
        super().__init__(parent)

        self.center_lat = center_lat
        self.center_lon = center_lon

        self.radius = radius
        self.speed = speed
        self.update_interval_ms = update_interval_ms

        self.timer = QTimer(self)
        self.timer.timeout.connect(self._update_position)

        self.start_time = None
        self.angle = 0.0
        self.packets_received = 0

        self.data = GPSData(
            latitude=center_lat,
            longitude=center_lon,
            altitude=35.0,
            speed=0.0,
            course=0.0,
            satellites=12,
            satellites_in_view=16,
            hdop=0.8,
            vdop=1.2,
            pdop=1.5,
            fix_type=3,
            valid=True,
            update_rate=1.0,
            source="Mock GPS",
            timestamp=int(time.time()),
        )

    def open(self) -> bool:
        """Start GPS simulation."""

        if self.timer.isActive():
            return True

        self.start_time = time.monotonic()
        self.packets_received = 0
        self.angle = 0.0

        self._update_position()
        self.timer.start(self.update_interval_ms)

        return True

    def close(self):
        """Stop GPS simulation."""
        self.timer.stop()

    def _update_position(self):
        """Calculate the next GPS position."""

        now = time.monotonic()

        elapsed = now - self.start_time if self.start_time is not None else 0.0

        # Angular velocity (radians/second)
        angular_velocity = self.speed / self.radius

        # Circular movement
        self.angle = angular_velocity * elapsed

        # Position relative to circle center
        east = self.radius * math.cos(self.angle)
        north = self.radius * math.sin(self.angle)

        # Convert meters to geographic coordinates
        latitude = self.center_lat + north / 111320.0

        longitude = self.center_lon + (
            east / (111320.0 * math.cos(math.radians(self.center_lat)))
        )

        # Calculate direction of movement (degrees from north)
        course = (
            math.degrees(math.atan2(-math.sin(self.angle), math.cos(self.angle)))
            % 360.0
        )

        self.packets_received += 1

        update_rate = self.packets_received / max(elapsed, 1.0)

        # Small simulated altitude variation
        altitude = 35.0 + 1.5 * math.sin(self.angle * 2)

        self.data = GPSData(
            latitude=latitude,
            longitude=longitude,
            altitude=altitude,
            speed=self.speed,
            course=course,
            satellites=12,
            satellites_in_view=16,
            hdop=0.8,
            vdop=1.2,
            pdop=1.5,
            fix_type=3,
            valid=True,
            update_rate=update_rate,
            source="Mock GPS",
            timestamp=int(time.time()),
        )

        self.sigUpdated.emit(self.data)
