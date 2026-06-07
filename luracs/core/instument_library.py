from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from utils.file_io import xml_parser

from containers.instrument_classes import UniqueInstrument, GenericInstrument
from PySide6.QtCore import QObject, Signal

from .gui_logger import gui_logger


class InstrumentLibrary(QObject):
    sigInstrumentUpdated = Signal(str, object)
    sigRemoveInstrument = Signal(str)
    sigNewInstrumentAdded = Signal(object)
    sigRegistryUpdated = Signal()
    sigInstrumentRenamed = Signal(str, object)

    def __init__(
        self,
        instrument_class: UniqueInstrument | GenericInstrument,
        parent=None,
    ):
        super().__init__(parent)
        self.instrument_registry: dict[str, UniqueInstrument | GenericInstrument] = {}
        self._instrument_class: UniqueInstrument | GenericInstrument = instrument_class

    def set_instrument_from_parser(self, key: str, parser: xml_parser):
        instrument = parser.get_instrument()

        assert isinstance(instrument, self._instrument_class), (
            f"Instrument is class {type(instrument)}"
        )
        self.instrument_registry[key] = instrument

    def remove_instrument(self, key: str):
        del self.instrument_registry[key]
        self.sigRemoveInstrument.emit(key)

    def get_instrument_by_name(
        self, name: str
    ) -> UniqueInstrument | GenericInstrument | None:
        "Get an instrument based on the name for UniqueInstruments and model for GenericInstruments"
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

    def add_instrument(self, new_instrument: UniqueInstrument | GenericInstrument):
        self.sigNewInstrumentAdded.emit(new_instrument)

    def rename_instrument(
        self,
        old_instrument_key: str,
        new_instrument: UniqueInstrument | GenericInstrument,
    ):
        "Removes and re-adds an instrument to rename the file containing the spectrum"
        self.remove_instrument(old_instrument_key)
        self.sigInstrumentUpdated.emit(old_instrument_key, new_instrument)

    def update_instrument_data(self, instrument_key: str, data_dict: dict):
        "Update the data of an instrument but not renaming. Renaming must be made with rename_instrument to keep the file index and file names consistent."
        instr = self.instrument_registry[instrument_key]
        name = instr.name if isinstance(instr, UniqueInstrument) else instr.model
        changes = []
        for key, value in data_dict.items():
            if not hasattr(instr, key):
                raise ValueError(f"'{type(instr)}' does not have attribute '{key}'!")
            setattr(instr, key, value)
            changes.append(f"{key}={value}")

        gui_logger.debug(f"Instrument updated: name={name}, {', '.join(changes)}")
        self.sigInstrumentUpdated.emit(instrument_key, instr)
        
    def get_key_from_attr(self, attr: str, value):
        "Get an idex key from an attribute of the stored instrument"
        for key, item in self.instrument_registry.items():
            if getattr(item, attr) == value:
                return key
