from __future__ import annotations

from PySide6.QtCore import QObject

from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument

from ..gui_logger import gui_logger
from .indexers import _InstrumentIndex, _ROIIndex, _SpectrogramIndex, _SpectrumIndex


class _FileIndex(QObject):
    def __init__(self, parent):
        super().__init__(parent)

        self.spectrum_index = _SpectrumIndex(self)
        self.spectrogram_index = _SpectrogramIndex(self)
        self.roi_index = _ROIIndex(self)
        self.unique_instrument_index = _InstrumentIndex(self, UniqueInstrument)
        self.generic_instrument_index = _InstrumentIndex(self, GenericInstrument)

        # Index everything at startup
        self.run_index_all()

    def run_index_all(self):
        "Re-Index all indexes"
        for indexer in [
            self.spectrum_index,
            self.spectrogram_index,
            self.roi_index,
            self.unique_instrument_index,
            self.generic_instrument_index,
        ]:
            indexer.run_index()
            gui_logger.debug(
                f"{indexer.__class__}: {len(indexer.index_registry)} files found in {indexer.path}"
            )
