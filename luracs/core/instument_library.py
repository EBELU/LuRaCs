from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass

from containers.instrument_classes import UniqueInstrument, GenericInstrument
from PySide6.QtCore import QObject, Signal


class InstrumentLibrary(QObject):
    def __init__(self, instument_class: UniqueInstrument | GenericInstrument, parent = None, ):
        super().__init__(parent)
        self.instruments: dict[str, UniqueInstrument | GenericInstrument] = {}
        self._instrument_class: UniqueInstrument | GenericInstrument = instument_class

    def get_instrument_by_name(self, name: str) -> UniqueInstrument | GenericInstrument | None:
        "Get an instrument based on name"
        for i in self.instruments.values():
            if isinstance(i, UniqueInstrument):
                if i.name == name:
                    return i
            else:
                if i.model == name:
                    return i  

    def get_instrument_names(self) -> list[str]:
        if self._instrument_class is UniqueInstrument:
            return [i.name for i in self.instruments.values()]
        else:
            return [i.model for i in self.instruments.values()]
