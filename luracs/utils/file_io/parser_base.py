from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument
    from luracs.containers.roi_classes import ROI
    from luracs.containers.spectrum_classes import Spectrum
    
from abc import ABC, abstractmethod


class SpectrumParserBase(ABC):    
    @abstractmethod
    def get_spectrum(self) -> Spectrum:
        pass

    def get_rois(self) -> list[ROI]:
        return []
    
    def get_instrument(self) -> UniqueInstrument | GenericInstrument | None:
        return