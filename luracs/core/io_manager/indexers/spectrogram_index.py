from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.core.spectrogram_manager.spectrogram import Spectrogram

import logging
import os

gui_logger = logging.getLogger("Application")

from luracs.core.settings import Settings
from luracs.utils.file_io import db_parser

from .indexer_base import Indexer


class SpectrogramIndex(Indexer):
    def __init__(self, parent):
        super().__init__(
            parent=parent, path=Settings.Paths.spectrogram_library
        )  # Library path
        self.index_registry: dict[str, Spectrogram] = {}

    def parser(self, path):
        return db_parser(path)

    def delete_file(self, key: str):
        parser = self.index_registry.pop(key)
        assert isinstance(parser, db_parser)
        parser.close()

        assert key not in self.index_registry
        os.remove(key)
        self.sigIndexUpdated.emit()
        gui_logger.debug(f"{self.__class__} File {key} deleted")
