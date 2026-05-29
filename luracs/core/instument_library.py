from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass

from containers.instrument_classes import UniqueInstrument, GenericInstrument
from PySide6.QtCore import QObject, Signal

from .gui_logger import gui_logger


class InstrumentLibrary(QObject):
    sigInstrumentUpdated = Signal(object)
    
    def __init__(self, instument_class: UniqueInstrument | GenericInstrument, parent = None, ):
        super().__init__(parent)
        self.instrument_registry: dict[str, UniqueInstrument | GenericInstrument] = {} # Shared with datastore index
        self._instrument_class: UniqueInstrument | GenericInstrument = instument_class

    def get_instrument_by_name(self, name: str) -> UniqueInstrument | GenericInstrument | None:
        "Get an instrument based on name"
        for i in self.instrument_registry.values():
            if isinstance(i, UniqueInstrument):
                if i.name == name:
                    return i
            else:
                if i.model == name:
                    return i  

    def get_instrument_names(self) -> list[str]:
        if self._instrument_class is UniqueInstrument:
            return [i.name for i in self.instrument_registry.values()]
        else:
            return [i.model for i in self.instrument_registry.values()]
    
    def get_instrument_by_name(self, name: str):
        if self._instrument_class is UniqueInstrument:
            for i in self.instrument_registry.values():
                if i.name == name:
                    return i
        else:
            for i in self.instrument_registry.values():
                if i.model == name:
                    return i
                
    def update_instrument_data(self, instrument_key: str, data_dict: dict):
        instr = self.instrument_registry[instrument_key]
        name = instr.name if isinstance(instr, UniqueInstrument) else instr.model
        changes = []
        for key, value in data_dict.items():
            if not hasattr(instr, key):
                raise ValueError(f"{type(instr)} does not have attribute {key}!")
            setattr(instr, key, value)
            changes.append(f"{key}={value}")
        
        gui_logger.debug(f"Instrument updated: name={name},{", ".join(changes)}")
        self.sigInstrumentUpdated.emit(instrument_key)