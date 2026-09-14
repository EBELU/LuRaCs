from dataclasses import dataclass

from PySide6.QtCore import QObject, Signal
from PySide6.QtSerialPort import QSerialPort

@dataclass(frozen=True, kw_only=True)
class GPSData:
    latitude: float | None = None
    longitude: float | None = None
    altitude: float | None = None
    speed: float | None = None
    course: float | None = None
    satellites: int = 0
    hdop: float | None = None
    valid: bool = False
    source: str = "USB GPS"
    
def fmt(value, width, decimals):
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
        f"Sats: {data.satellites:2d} | "
        f"HDOP: {fmt(data.hdop, 4, 1)} | "
        f"Fix: {'YES' if data.valid else 'NO '}"
    )
    
class GPS(QObject):
    updated = Signal(object)

    def __init__(self, port_name: str, parent=None):
        super().__init__(parent)

        self.port = QSerialPort(self)
        self.port.setPortName(port_name)
        self.port.setBaudRate(QSerialPort.BaudRate.Baud4800)

        self._buffer = bytearray()
        self.data = GPSData()

        self.port.readyRead.connect(self._read)

    def open(self) -> bool:
        return self.port.open(QSerialPort.OpenModeFlag.ReadOnly)

    def _read(self):
        self._buffer.extend(self.port.readAll().data())

        while b"\r\n" in self._buffer:
            line, self._buffer = self._buffer.split(b"\r\n", 1)

            try:
                sentence = line.decode("ascii")
            except UnicodeDecodeError:
                continue

            self._parse(sentence)

    def _parse(self, sentence: str):
        # Parse GGA/RMC/etc.
        # Update self.data
        self.updated.emit(self.data)