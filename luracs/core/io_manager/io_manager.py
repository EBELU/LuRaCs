from __future__ import annotations

from PySide6.QtCore import QObject

from .exporter import _Exporter
from .file_index import _FileIndex
from .importer import _Importer


class _IOManager(QObject):
    """
    The io manager is a luracs.core singleton that manages the input and output of files. This includes both the loading of internally stored files for the data library and the import-export of external files.

    The io manager contains the following services:\n

    **FileIndex**: Indexes and manages datafiles saved internally for use in the data library.

    **Importer**: Contains components for importing external files

    **Exporter**: Contains components for exporting internal files
    """

    def __init__(self):
        super().__init__(parent=None)
        self.FileIndex = _FileIndex(self)
        self.Importer = _Importer(self, self)
        self.Exporter = _Exporter(self, self)