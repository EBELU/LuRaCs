import logging
import time
from dataclasses import dataclass, replace

from PySide6.QtCore import QObject, Signal
from PySide6.QtSerialPort import QSerialPort


@dataclass(frozen=True, kw_only=True)
class GPSData:
    latitude: float | None = None
    longitude: float | None = None
    altitude: float | None = None
    speed: float | None = None
    course: float | None = None

    satellites: int = 0          # satellites used in fix
    satellites_in_view: int = 0  # satellites detected by GSV

    hdop: float | None = None    # horizontal position accuracy
    vdop: float | None = None    # vertical position accuracy
    pdop: float | None = None    # overall 3D position accuracy

    fix_type: int = 0            # 0=no fix, 1=2D, 2=3D, etc.
    valid: bool = False
    update_rate: float = 0

    source: str = "USB GPS"
    timestamp: int = 0



def _nmea_coord(value: str, hemisphere: str) -> float | None:
    """Convert NMEA DDMM.MMMM / DDDMM.MMMM to decimal degrees."""
    if not value or not hemisphere:
        return None

    try:
        degrees = int(float(value) / 100)
        minutes = float(value) - degrees * 100

        if not 0 <= minutes < 60:
            return None

        result = degrees + minutes / 60.0

        if hemisphere in ("S", "W"):
            result = -result

        return result

    except (ValueError, TypeError):
        return None


def parse_gps(nmea: str) -> GPSData:
    """
    Parse one or more NMEA sentences into GPSData.

    Supports:
      - GGA: position, altitude, satellites, HDOP, fix
      - RMC: position, speed, course, validity
      - GSA: fix type, satellites used, PDOP, HDOP, VDOP
      - GSV: satellites in view
    """

    latitude = None
    longitude = None
    altitude = None
    speed = None
    course = None

    satellites = 0
    satellites_in_view = 0

    hdop = None
    vdop = None
    pdop = None

    fix_type = 0
    valid = False

    for line in nmea.splitlines():
        line = line.strip()

        if not line.startswith("$"):
            continue

        sentence = line.split("*", 1)[0]
        fields = sentence.split(",")

        if not fields:
            continue

        sentence_type = fields[0]

        # ---------------------------------------------------------
        # GGA
        # ---------------------------------------------------------
        if sentence_type in ("$GPGGA", "$GNGGA"):
            if len(fields) < 10:
                continue

            latitude = _nmea_coord(fields[2], fields[3])
            longitude = _nmea_coord(fields[4], fields[5])

            try:
                fix = int(fields[6]) if fields[6] else 0
            except ValueError:
                fix = 0

            try:
                satellites = int(fields[7]) if fields[7] else 0
            except ValueError:
                satellites = 0

            try:
                hdop = float(fields[8]) if fields[8] else None
            except ValueError:
                hdop = None

            try:
                altitude = float(fields[9]) if fields[9] else None
            except ValueError:
                altitude = None

            # GGA quality:
            # 0 = invalid
            # 1 = GPS fix
            # 2 = DGPS fix
            # etc.
            valid = fix > 0

        # ---------------------------------------------------------
        # RMC
        # ---------------------------------------------------------
        elif sentence_type in ("$GPRMC", "$GNRMC"):
            if len(fields) < 9:
                continue

            valid = fields[2] == "A"

            rmc_latitude = _nmea_coord(fields[3], fields[4])
            rmc_longitude = _nmea_coord(fields[5], fields[6])

            if latitude is None:
                latitude = rmc_latitude
                longitude = rmc_longitude

            try:
                speed = float(fields[7]) * 0.514444 if fields[7] else None
            except ValueError:
                speed = None

            try:
                course = float(fields[8]) if fields[8] else None
            except ValueError:
                course = None

        # ---------------------------------------------------------
        # GSA
        # ---------------------------------------------------------
        elif sentence_type in ("$GPGSA", "$GNGSA"):
            if len(fields) < 18:
                continue

            try:
                fix_type = int(fields[2]) if fields[2] else 0
            except ValueError:
                fix_type = 0

            # Fields 3..14 contain satellite PRNs.
            satellites = sum(
                1
                for prn in fields[3:15]
                if prn
            )

            try:
                pdop = float(fields[15]) if fields[15] else None
            except ValueError:
                pdop = None

            try:
                hdop = float(fields[16]) if fields[16] else None
            except ValueError:
                hdop = None

            try:
                vdop = float(fields[17]) if fields[17] else None
            except ValueError:
                vdop = None

            valid = fix_type in (2, 3)

        # ---------------------------------------------------------
        # GSV
        # ---------------------------------------------------------
        elif sentence_type in ("$GPGSV", "$GNGSV"):
            if len(fields) < 4:
                continue

            try:
                satellites_in_view = (
                    int(fields[3])
                    if fields[3]
                    else 0
                )
            except ValueError:
                satellites_in_view = 0

    return GPSData(
        latitude=latitude,
        longitude=longitude,
        altitude=altitude,
        speed=speed,
        course=course,
        satellites=satellites,
        satellites_in_view=satellites_in_view,
        hdop=hdop,
        pdop=pdop,
        vdop=vdop,
        fix_type=fix_type,
        valid=valid,
    )

def fmt(value: float, width: int, decimals: int) -> str:
    if value is None:
        return f"{'---':>{width}}"

    return f"{value:>{width}.{decimals}f}"


def format_gps(data: GPSData) -> str:
    return (
        f"Source: {data.source} | "
        f"Lat: {fmt(data.latitude, 10, 6)} | "
        f"Lon: {fmt(data.longitude, 10, 6)} | "
        f"Alt: {fmt(data.altitude, 7, 1)} m | "
        f"Speed: {fmt(data.speed, 6, 2)} m/s | "
        f"Course: {fmt(data.course, 6, 1)}° | "
        f"Sats: {data.satellites:2d}/{data.satellites_in_view:2d} | "
        f"HDOP: {fmt(data.hdop, 4, 1)} | "
        f"Rate: {fmt(data.update_rate, 3, 1)} Hz | "
        f"Fix: {'YES' if data.valid else 'NO '}"
    )


class GPS(QObject):
    sigUpdated = Signal(object)

    log = logging.getLogger("GPS")

    def __init__(
        self,
        port_name: str,
        baud_rate: QSerialPort.BaudRate,
        parent=None,
    ):
        super().__init__(parent)

        self.port = QSerialPort(self)
        self.port.setPortName(port_name)
        self.port.setBaudRate(baud_rate)
        self.connection_time = None
        self.packets_received = 0

        self._buffer = bytearray()

        # Complete current GPS state
        self.data = GPSData()

        self.port.readyRead.connect(self._read)

        self.log.info(
            "GPS Created: port=%s, baud_rate=%s",
            port_name,
            baud_rate,
        )
        


    def open(self) -> bool:
        if self.port.open(QSerialPort.OpenModeFlag.ReadOnly):
            self.log.info("GPS opened: %s", self.port.portName())
            self.connection_time = time.monotonic()
            return True

        self.log.error(
            "Failed to open GPS: %s",
            self.port.errorString(),
        )
        return False

    def close(self):
        if self.port.isOpen():
            self.port.close()

    def _read(self):
        self._buffer.extend(self.port.readAll().data())

        while b"\r\n" in self._buffer:
            line, self._buffer = self._buffer.split(b"\r\n", 1)

            try:
                sentence = line.decode("ascii").strip()
            except UnicodeDecodeError:
                continue

            if sentence:
                self._parse(sentence)

    def _parse(self, sentence: str):
        """Parse one NMEA sentence and merge it into the current state."""

        new_data = parse_gps(sentence)
        self.packets_received += 1
        
        self.data = replace(
            self.data,

            latitude=(
                new_data.latitude
                if new_data.latitude is not None
                else self.data.latitude
            ),

            longitude=(
                new_data.longitude
                if new_data.longitude is not None
                else self.data.longitude
            ),

            altitude=(
                new_data.altitude
                if new_data.altitude is not None
                else self.data.altitude
            ),

            speed=(
                new_data.speed
                if new_data.speed is not None
                else self.data.speed
            ),

            course=(
                new_data.course
                if new_data.course is not None
                else self.data.course
            ),

            satellites=(
                new_data.satellites
                if new_data.satellites != 0
                else self.data.satellites
            ),

            satellites_in_view=(
                new_data.satellites_in_view
                if new_data.satellites_in_view != 0
                else self.data.satellites_in_view
            ),

            hdop=(
                new_data.hdop
                if new_data.hdop is not None
                else self.data.hdop
            ),

            pdop=(
                new_data.pdop
                if new_data.pdop is not None
                else self.data.pdop
            ),

            vdop=(
                new_data.vdop
                if new_data.vdop is not None
                else self.data.vdop
            ),

            fix_type=(
                new_data.fix_type
                if new_data.fix_type != 0
                else self.data.fix_type
            ),
            # Estimate the number of rate of packages received
            update_rate = round(self.packets_received / max(time.monotonic() - self.connection_time, 1)),
            # Data is emitted even if it is not valid but the spectrogram only stores the new value
            # if valid = True
            valid=new_data.valid,
            # A new timestamp is only collected if the position data updated
            timestamp = time.time() if new_data.latitude is not None else self.data.timestamp
        )


        self.sigUpdated.emit(self.data)

