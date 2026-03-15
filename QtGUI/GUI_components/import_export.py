from pathlib import Path

from PySide6.QtWidgets import QFileDialog
from PySide6.QtCore import QObject

from ..utils.file_io import xml_io, csv_io
from ..core import SpectrumManager

def io_dispatcher(file_name: Path | str):
    if isinstance(file_name, Path):
        pass
    elif isinstance(file_name, str):
        file_name = Path(file_name)
    else:
        raise ValueError(f"Unknown path type {type(file_name)}")
    
    if file_name.suffix in (".xml", ".n42"):
        return xml_io.load(file_name)



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

    def load_spectrum(self, _=None):
        parser = self.import_spectrum()
        SpectrumManager.create_spectrum(parser.kwargs["name"], parser.kwargs["foreground"].channels)
        SpectrumManager.set_foreground_spectrum(parser.kwargs["name"], parser.kwargs["foreground"])
        
        if "background" in parser.kwargs:
            SpectrumManager.set_background_spectrum(parser.kwargs["name"], parser.kwargs["background"])
            
        if "calibration" in parser.kwargs:
            SpectrumManager.calibrate_spectrum(parser.kwargs["name"], parser.kwargs["calibration"])

    def load_spectrum_as_background(self, spectrum_name):
        parser = self.import_spectrum()
        new_bkg = parser.kwargs["foreground"]
        subject_spectrum = SpectrumManager.get_spectrum(spectrum_name)

        if new_bkg.channels != subject_spectrum.channels:
            return
        
        subject_spectrum.set_background(new_bkg)
        

    

    # --- Export ---
    def export_spectrum(self, spectrum_name):
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
        spectrum = SpectrumManager.get_spectrum(spectrum_name)
        if "xml" in selected_filter.lower():
            xml_io.export(spectrum, str(file_path))
        elif "csv" in selected_filter.lower():
            csv_io.export(spectrum, str(file_path))
     


            




