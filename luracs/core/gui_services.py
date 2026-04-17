from enum import Enum


class GuiServicesKeys(Enum):
    FILE_IMPORT = "file_import"
    FILE_EXPORT = "file_export"
    LIBRARY = "library"


class GuiServices:
    _registry = {}

    @classmethod
    def register(cls, key, obj):
        cls._registry[key] = obj

    @classmethod
    def get(cls, key):
        return cls._registry[key]
