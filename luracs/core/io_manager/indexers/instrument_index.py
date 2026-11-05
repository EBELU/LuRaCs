from __future__ import annotations

import logging

from PySide6.QtCore import Signal

from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument
from luracs.containers.spectrum_classes import Spectrum

gui_logger = logging.getLogger("Application")

from luracs.core.settings import Settings
from luracs.utils.file_io import xml_parser, xml_writer

from .indexer_base import Indexer


class InstrumentIndex(Indexer):
    sigInstrumentUpdated = Signal(str, object)
    sigInstrumentRemoved = Signal(object)
    sigRemoveInstrument = Signal(str)
    sigNewInstrumentAdded = Signal(object)
    sigRegistryUpdated = Signal()
    sigInstrumentRenamed = Signal(str, object)

    def __init__(self, parent, instrument_type: UniqueInstrument | GenericInstrument):
        if instrument_type is UniqueInstrument:
            lib_path = Settings.Paths.unique_instrument_library
        else:
            lib_path = Settings.Paths.generic_instrument_library
        
        super().__init__(
            parent=parent, path=lib_path
        )  # Library path
        self._instrument_class = instrument_type

        self.index_registry: dict[str, UniqueInstrument | GenericInstrument] = {}
        
    def run_index(self):
        "Index the given directory and store the parsed instruments"
        nr_changed = 0
        for file in self.path.glob("*"):
            if file not in self.index_registry:
                nr_changed += 1
                self.index_registry[str(file)] = self.parser(file).get_instrument()
                self.sigItemAdded.emit(str(file), self.index_registry[str(file)])

        if nr_changed > 0:
            self.sigIndexUpdated.emit()

    def save_to_library(self, instrument: GenericInstrument):
        # Build dummy spectrum for the xml writer
        dummy_spectrum = Spectrum(1, "Dummy")
        dummy_spectrum.instrument = instrument

        new_file = self.path / instrument.model

        xml_writer(dummy_spectrum, new_file, export_spectrum=False, export_rois=False)

        gui_logger.debug(f"Instrument saved to library: {new_file}")

    def set_instrument_from_parser(self, key: str, parser: xml_parser):
        instrument = parser.get_instrument()

        assert isinstance(instrument, self._instrument_class), (
            f"Instrument is class {type(instrument)}"
        )
        self.index_registry[key] = instrument

    def remove_instrument(self, key: str):
        self.sigInstrumentRemoved.emit(self.index_registry[key])
        del self.index_registry[key]
        self.sigRemoveInstrument.emit(key)

    def get_instrument_by_name(
        self, name: str
    ) -> UniqueInstrument | GenericInstrument | None:
        "Get an instrument based on the name for UniqueInstruments and model for GenericInstruments"
        for i in self.index_registry.values():
            if isinstance(i, UniqueInstrument):
                if i.name == name:
                    return i
            else:
                if i.model == name:
                    return i

    def get_instrument_names(self) -> list[str]:
        if self._instrument_class is UniqueInstrument:
            return [i.name for i in self.index_registry.values()]
        else:
            return [i.model for i in self.index_registry.values()]

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
        instr = self.index_registry[instrument_key]
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
        for key, item in self.index_registry.items():
            if getattr(item, attr) == value:
                return key
