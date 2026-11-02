from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import ROI
    from luracs.containers.spectrum_classes import Spectrum

import logging

gui_logger = logging.getLogger("Application")

from luracs.core.settings import Settings
from luracs.utils.file_io import xml_writer

from .indexer_base import Indexer


class ROIIndex(Indexer):
    def __init__(self, parent):
        super().__init__(parent=parent, path=Settings.Paths.roi_library)  # Library path
        self.index_registry: dict[str, ROI] = {}

    def save_to_library(dummy_spectrum: Spectrum):
        new_file = Settings.Paths.roi_library / dummy_spectrum.name
        xml_writer(
            dummy_spectrum, new_file, export_spectrum=False, export_instrument=False
        )
        gui_logger.debug(f"ROIs saved to library: {new_file}")
        return new_file.with_suffix(".xml")
