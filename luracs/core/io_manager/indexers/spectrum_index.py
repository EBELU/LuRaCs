from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum

import logging

gui_logger = logging.getLogger("Application")

from luracs.core.settings import Settings
from luracs.utils.file_io import xml_writer

from .indexer_base import Indexer


class SpectrumIndex(Indexer):
    def __init__(self, parent):
        super().__init__(
            parent=parent, path=Settings.Paths.spectrum_library
        )  # Library path

    def save_to_library(self, spectrum: Spectrum):
        new_file = Settings.Paths.spectrum_library / spectrum.name
        xml_writer(spectrum, new_file)

        gui_logger.debug(f"Spectrum saved to library: {new_file}")
        return new_file.with_suffix(".xml")
