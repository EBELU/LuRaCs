from pathlib import Path

from PySide6.QtWidgets import QFileDialog
from PySide6.QtCore import QObject, Signal

import utils.file_io as file_io
from core import SpectrumManager

def io_dispatcher(file_name: Path | str):
    if isinstance(file_name, Path):
        pass
    elif isinstance(file_name, str):
        file_name = Path(file_name)
    else:
        raise ValueError(f"Unknown path type {type(file_name)}")
    
    if file_name.suffix in (".xml", ".n42"):
        try: 
            parsed = file_io.xml_parser(file_name)
        except Exception as e:
            print("Failed to parse XML file {file_name}, exeption raised {e}")
            return
        
        return parsed.kwargs



class FileDialogs(QObject):
    sigImportSpectrum = Signal(dict)
    sigImportSpectrumAsBackground = Signal(str, dict)

    import_filters = {
        "spectrum": "Spectrum Files (*.xml *.n42 *.tke *.spe)",
        "spectrogram": "LuRaCs Spectrogram Database File (*.db)",
        "rois": "LuRaCs ROIs File (*.xml)",
        "instrument": "LuRaCs Instrument File (*xml)"
    }
    export_filters = {
        "spectrum": "XML/n42 (*xml);; CSV (*.csv);; Excel Workbook (*.xlsx)",
        "spectrogram": "Spectrogram Sqlite (.db);; Excel Workbook (*.xlsx)",
        "rois": "XML (*xml);; Excel Workbook (*.xlsx)",
        "instrument": "XML (*xml)"
    }

    def __init__(self, parent=None):
        super().__init__(parent)
        self.parent = parent
        self.last_spect_dir = Path.home()

        self.sigImportSpectrum.connect(SpectrumManager.import_spectrum)
        self.sigImportSpectrumAsBackground.connect(SpectrumManager.import_spectrum_as_background)


    # --- Import ---
    def import_files(self, filter=None):
        "Import multiple files with filters"
        if filter is None:
            filter = ";;".join(self.import_filters.values())
        file_paths, selected_filter = QFileDialog.getOpenFileNames(
            self.parent,
            "Import File",
            str(self.last_spect_dir),
            filter
        )
        
        if file_paths is not None and len(file_paths) > 0:
            file_paths = [Path(fp) for fp in  file_paths]
            self.last_spect_dir = file_paths[0].parent
            return file_paths, selected_filter
        
        else:
           return None, None
        
    def import_generic(self, filter = None):
        file_paths, selected_filter = self.import_files(filter)
        if file_paths is None:
            return
        

        # --- Spectrum Import ---
        if selected_filter == self.import_filters["spectrum"]:
            for file_path in file_paths:
                spectrum_kwargs = io_dispatcher(file_path)
                if spectrum_kwargs is not None:
                    self.sigImportSpectrum.emit(spectrum_kwargs)

    def import_spectrum(self, _=None):
        file_paths = self.import_generic(self.import_filters["spectrum"])

    def load_spectrum_as_background(self, spectrum_name: str):
        file_path, _ = QFileDialog.getOpenFileName(
            self.parent,
            "Import File",
            str(self.last_spect_dir),
            self.import_filters["spectrum"]
        )
        if file_path is not None:
            file_path = Path(file_path)
            self.last_spect_dir = file_path.parent
        else:
            return
        
        spectrum_kwargs = io_dispatcher(file_path)
        if spectrum_kwargs is not None:
            self.sigImportSpectrumAsBackground.emit(spectrum_kwargs)
        
        


    # --- Export ---
    def export_spectrum(self, spectrum_name: str):
        file_path, selected_filter = QFileDialog.getSaveFileName(
            self.parent,
            "Export File",
            str(self.last_spect_dir),
            self.export_filters["spectrum"]
        )

        if not file_path:
            return None

        file_path = Path(file_path)
        if file_path is None:
            raise RuntimeWarning(f"Invalid file path {file_path}")
        spectrum = SpectrumManager.get_spectrum(spectrum_name)

        if "xml" in selected_filter.lower():
            file_io.xml_writer(spectrum, str(file_path))
        elif "csv" in selected_filter.lower():
            file_io.export_csv(spectrum, str(file_path))
     


            




