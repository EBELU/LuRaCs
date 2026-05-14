from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass

from containers.instrument_classes import UniqueInstrument, GenericInstrument
from PySide6.QtCore import QObject, Signal


class InstrumentLibrary(QObject):
    def __init__(self, parent, instument_class):
        super().__init__(parent)
        self.instruments: dict[str, UniqueInstrument | GenericInstrument] = {}
        self._instrument_class = instument_class

    def add_instrument(self, instrument: UniqueInstrument | GenericInstrument):
        assert instrument.name not in self.instruments, (
            f"Instrument with name {instrument.name} already exists in library"
        )
        assert isinstance(instrument, self._instrument_class), (
            f"Instrument must be of type {self._instrument_class.__name__}, got {type(instrument)}"
        )
        instrument_name = getattr(instrument, "name", None)
        if instrument_name is None:
            instrument_name = instrument.model

        assert instrument_name is not None, (
            "Instrument must have a name or model attribute"
        )
        self.instruments[instrument_name] = instrument

    def remove_instrument(self, name: str):
        if name in self.instruments:
            self.instruments.pop(name, None)

    def get_instrument(self, name: str) -> UniqueInstrument | GenericInstrument | None:
        "Get a loaded instrument, the name is not loaded "
        return self.instruments.get(name, None)
    
    def fetch_instrument(self, name: str):
        pass
    
    def get_unique_instruments(self):
        return {key: item for key, item in self.instruments.items() if isinstance(item, UniqueInstrument)}
    

    def get_instrument_names(self) -> list[str]:
        return list(self.instruments.keys())
