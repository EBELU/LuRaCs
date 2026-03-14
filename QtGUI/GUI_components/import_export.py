from pathlib import Path

from PySide6.QtWidgets import QFileDialog
from PySide6.QtCore import QObject

from ..utils.file_io import io_dispatcher



class FileDialogs(QObject):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.parent = parent
        self.last_spect_dir = Path.home()

    # --- Import ---
    def import_file(self, filter="All Files (*)"):
        file_path, _ = QFileDialog.getOpenFileName(
            self.parent,
            "Import File",
            str(self.last_spect_dir),
            filter
        )
        return file_path

    def import_spectrum(self, _=None):
        filter = "Spectrum Files (*.xml *.n42 *.tke *.spe)"
        file_path_str = self.import_file(filter)

        if not file_path_str:
            return None

        file_path = Path(file_path_str)

        self.last_spect_dir = file_path.parent

        return io_dispatcher(file_path)
    

    # --- Export ---
    def export_file(self):
        filters = "CSV (*.csv);;XML (*.xml, *.n42);;Excel (*xlsx)"

        file_path, selected_filter = QFileDialog.getSaveFileName(
            self.parent,
            "Export File",
            str(self.last_spect_dir),
            filters
        )

        if not file_path:
            return None

        file_path = Path(file_path)

        print(file_path, selected_filter)

        return file_path, selected_filter
    


            




