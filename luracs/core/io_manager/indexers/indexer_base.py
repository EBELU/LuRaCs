from __future__ import annotations

import logging

gui_logger = logging.getLogger("Application")


import os
from pathlib import Path

from PySide6.QtCore import QObject, Signal

from luracs.utils.file_io import xml_parser, db_parser


class Indexer(QObject):
    """
    Tracks the storage, updates and indexing of internally stored .xml-files for access through the data library. Requires a special connection with the instrument library since they are always loaded and mutable.
    """

    sigIndexUpdated = Signal()
    sigItemAdded = Signal(str, object)

    def __init__(self, parent: QObject, path: Path):
        super().__init__(parent)
        self.path = path
        self.index_registry = {}

    def parser(self, path):
        return xml_parser(path, meta_only=True)

    def save_to_library(self, *_):
        raise NotImplementedError

    def run_index(self):
        "Index the given directory and store the parser objects"
        nr_changed = 0
        for file in self.path.glob("*"):
            if file not in self.index_registry:
                nr_changed += 1
                self.index_registry[str(file)] = self.parser(file)
                self.sigItemAdded.emit(str(file), self.index_registry[str(file)])

        if nr_changed > 0:
            self.sigIndexUpdated.emit()

    def rename_file(self, old_name: str, new_name: str):
        del self.index_registry[old_name]
        if new_name in self.index_registry:
            del self.index_registry[new_name]
            os.remove(new_name)
            gui_logger.debug(
                f"{self.__class__} File {new_name} removed to rename {old_name}"
            )
        os.rename(old_name, new_name)
        gui_logger.debug(f"{self.__class__} File {old_name} renamed to {new_name}")
        self.run_index()

    def update_file(self, key: str, new_item):
        # Not needed?
        assert isinstance(key, str), (
            f"Key must be a string! Given object was {type(key)}"
        )
        # self.blockSignals(True)
        # self.delete_file(key)
        # self.blockSignals(False)
        gui_logger.debug(f"In Update {key}")
        gui_logger.debug(f"Current Registry {self.index_registry.keys()}")
        if key in self.index_registry:
            gui_logger.debug(f"Removed index key {key}")
            del self.index_registry[key]
        # Save the updated file
        self.save_file(new_item)
        gui_logger.debug(
            f"{self.__class__} Update completed! Key: {key}, item {new_item}"
        )
        self.sigIndexUpdated.emit()

    def save_file(self, item):
        new_file = str(self.save_to_library(item))
        gui_logger.debug(f"New file created {new_file}")
        if new_file in self.index_registry:
            del self.index_registry[new_file]
            gui_logger.debug(f"File removed from registry {new_file}")

        self.run_index()
        gui_logger.debug(f"{self.__class__} File {item} saved")
        return new_file

    def delete_file(self, key: str):
        del self.index_registry[key]
        assert key not in self.index_registry
        os.remove(key)
        self.sigIndexUpdated.emit()
        gui_logger.debug(f"{self.__class__} File {key} deleted")

    # --- Getters ---
    def get_key_from_attr(self, attr: str, value):
        "Get an idex key from an attribute of the stored parser"
        for key, item in self.index_registry.items():
            try:
                if item.data[attr] == value:
                    return key
            except KeyError:
                raise KeyError(f"Key '{attr}' does no exists in {item.__class__}")

    def get_item_from_attr(self, attr: str, value):
        "Get a value from an attribute of the stored parser"
        for item in self.index_registry.values():
            try:
                if item.data[attr] == value:
                    return item
            except KeyError:
                raise KeyError(f"Key '{attr}' does no exists in {item.__class__}")

    def get_index(self) -> dict[str, xml_parser | db_parser]:
        return self.index_registry.copy()
