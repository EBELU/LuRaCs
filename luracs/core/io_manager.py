from __future__ import annotations
from typing import TYPE_CHECKING, Callable
if TYPE_CHECKING:
    from containers.spectrum_classes import Spectrum

from pathlib import Path
from glob import glob
import os
from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import QFileDialog, QMessageBox, QWidget

from utils.file_io import db_parser, xml_parser, io_dispatcher
from utils import file_io
from gui.save_to_internal import save_instrument_to_library, save_roi_references, save_spectrum_to_library

from .settings import Settings
from .gui_logger import gui_logger
from .spectrum_manager import SpectrumManager

class _IOManager(QObject):
    """
    The io manager is a core singleton that manages the input and output of files. This includes both the loading of internally stored files for the data library and the import-export of external files.
    
    The io manager contains the following services:\n
    
    **FileIndex**: Indexes and manages datafiles saved internally for use in the data library.
    
    **Importer**: Contains components for importing external files
    
    **Exporter**: Contains components for exporting internal files
    """
    def __init__(self):
        super().__init__(parent=None)
        self.FileIndex = _FileIndex(self)
        self.Importer = _Importer(self)
        self.Exporter = _Exporter(self)
        
class _FileIndex(QObject):
    def __init__(self, parent):
        super().__init__(parent)
        
        # Build the indexes
        self.spectrum_index = Indexer(self, Settings.Paths.spectrum_library / "*.xml", {"meta_only": True}, save_spectrum_to_library, xml_parser)
        self.spectrogram_index = Indexer(self, Settings.Paths.spectrogram_library / "*.db", {}, None, db_parser)
        self.roi_index = Indexer(self, Settings.Paths.roi_library / "*.xml", {"meta_only": True}, save_roi_references, xml_parser)
        self.unique_instrument_index = Indexer(self, Settings.Paths.unique_instrument_library / "*.xml", {"meta_only": True}, save_instrument_to_library, xml_parser)
        self.generic_instrument_index = Indexer(self, Settings.Paths.generic_instrument_library / "*.xml", {"meta_only": True}, save_instrument_to_library, xml_parser)
        
        # --- Connect the instrument librates ---
        # They are special :)
        # Adding a new instrument
        self.unique_instrument_index.sigItemAdded.connect(SpectrumManager.UniqueInstrumentLibrary.set_instrument_from_parser)
        self.generic_instrument_index.sigItemAdded.connect(SpectrumManager.GenericInstrumentLibrary.set_instrument_from_parser)
        
        # Deleting
        SpectrumManager.UniqueInstrumentLibrary.sigRemoveInstrument.connect(self.unique_instrument_index.delete_file)
        SpectrumManager.GenericInstrumentLibrary.sigRemoveInstrument.connect(self.generic_instrument_index.delete_file)
        
        # Updating existing
        SpectrumManager.UniqueInstrumentLibrary.sigInstrumentUpdated.connect(self.unique_instrument_index.update_file)
        SpectrumManager.GenericInstrumentLibrary.sigInstrumentUpdated.connect(self.generic_instrument_index.update_file)
        
        # Saving a new
        SpectrumManager.UniqueInstrumentLibrary.sigNewInstrumentAdded.connect(self.unique_instrument_index.save_file)
        SpectrumManager.GenericInstrumentLibrary.sigNewInstrumentAdded.connect(self.generic_instrument_index.save_file)
        
        # Index everything at startup
        self.run_index_all()

    def run_index_all(self):
        "Re-Index all indexes"
        for indexer in [self.spectrum_index, self.spectrogram_index, self.roi_index, self.unique_instrument_index, self.generic_instrument_index]:
            indexer.run_index()
            gui_logger.debug(f"{indexer.__class__}:{len(indexer.index_registry)} files found in {indexer.path}")

class Indexer(QObject):
    sigIndexUpdated = Signal()
    sigItemAdded = Signal(str, object)
    def __init__(self, parent, glob_path: Path, parser_kwargs: dict, save_fn: Callable, parser: xml_parser | db_parser):
        super().__init__(parent)
        self.parser = parser
        self.path = glob_path
        self.parser_kwargs = parser_kwargs
        self.index_registry = {}
        self.save_fn = save_fn
        
    def run_index(self):
        "Index the given directory and store the parser objects"
        nr_changed = 0
        for file in glob(str(self.path)):
            if file not in self.index_registry:
                nr_changed += 1
                self.index_registry[file] = self.parser(file, **self.parser_kwargs)
                self.sigItemAdded.emit(file, self.index_registry[file])
                
        
        if nr_changed > 0:
            self.sigIndexUpdated.emit()
    
    def rename_file(self, old_name: str, new_name: str):
        del self.index_registry[old_name]
        os.rename(old_name, new_name)
        gui_logger.debug(f"{self.__class__} File {old_name} renamed to {new_name}")
        self.run_index()
                
    def update_file(self, key: str, new_item):
        # Not needed?
        # self.blockSignals(True)
        # self.delete_file(key)
        # self.blockSignals(False)
        
        del self.index_registry[key]
        # Save the updated file
        self.save_file(new_item)
        
    
    def save_file(self, item):
        if self.save_fn is None:
            raise NotImplementedError()
        
        self.save_fn(item)
        self.run_index()
        gui_logger.debug(f"{self.__class__} File {item} saved")
                
    def delete_file(self, key: str):
        del self.index_registry[key]
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
        for key, item in self.index_registry.items():
            try:
                if item.data[attr] == value:
                    return item
            except KeyError:
                raise KeyError(f"Key '{attr}' does no exists in {item.__class__}")
            
    def get_index(self):
        return self.index_registry.copy()
        

    
        
class _Importer(QObject):
    """ 
    All functionality needed for importing data to the application gathered in place. 
    """
    sigImportSpectrum = Signal(dict, bool)
    sigImportSpectrumAsBackground = Signal(str, dict)

    import_filters = {
        "spectrum": "Spectrum Files (*.xml *.n42 *.tke *.spe)",
        "spectrogram": "LuRaCs Spectrogram Database File (*.db)",
        "rois": "LuRaCs ROIs File (*.xml)",
        "instrument": "LuRaCs Instrument File (*xml)",
    }
    def __init__(self, parent):
        super().__init__(parent)
        
        self.sigImportSpectrum.connect(SpectrumManager.import_spectrum)
        self.sigImportSpectrumAsBackground.connect(
            SpectrumManager.import_spectrum_as_background
        )
        
    # --- Generic Importers ---
    def import_files(self, filter=None, dialog_parent = None) -> tuple[list[Path], list[str]]:
        "Import multiple files with filters"
        # If no filter use all
        if filter is None:
            filter = ";;".join(self.import_filters.values())
            
        
        file_paths, selected_filter = QFileDialog.getOpenFileNames(
            dialog_parent, "Import File", str(Settings.Paths.last_opened_dir), filter
        )

        # Clean up files and make them Paths
        if file_paths is not None and len(file_paths) > 0:
            file_paths = [Path(fp) for fp in file_paths]
            Settings.Paths.last_opened_dir = file_paths[0].parent # New last opened
            
            return file_paths, selected_filter

        else:
            return None, None

    def import_file(self, filter=None) -> tuple[Path, str]:
        "Import a single file with filters"
        # If no filter use all
        if filter is None:
            filter = ";;".join(self.import_filters.values())
            
        file_path, selected_filter = QFileDialog.getOpenFileName(
            self, "Import File", str(Settings.Paths.last_opened_dir), filter
        )

        # Clean up the path
        if file_path is not None:
            return Path(file_path), selected_filter

        else:
            return None, None
        
    def import_generic(self, filter=None):
        "Import anything supported"
        file_paths, selected_filter = self.import_files(filter)
        if file_paths is None:
            return

        # --- Spectrum Import ---
        if selected_filter == self.import_filters["spectrum"]:
            for file_path in file_paths:
                spectrum_parser = io_dispatcher(file_path)
                if isinstance(spectrum_parser, xml_parser):
                    self.sigImportSpectrum.emit(spectrum_parser.data, True) # Bool is to signal external import

    def import_spectrum_as_background(self, spectrum_name: str):
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "Import File",
            str(Settings.Paths.last_opened_dir),
            self.import_filters["spectrum"],
        )
        if file_path is not None:
            file_path = Path(file_path)
            Settings.Paths.last_opened_dir = file_path.parent
        else:
            return

        spectrum_parser = file_io.io_dispatcher(file_path)
        if spectrum_parser is not None:
            self.sigImportSpectrumAsBackground.emit(spectrum_name, spectrum_parser.data)
    
class _Exporter(QObject):
    """ 
    All functionality needed for export data from the application gathered in place. 
    """
    export_filters = {
        "spectrum": "XML/n42 (*xml);; CSV (*.csv);; Excel Workbook (*.xlsx)",
        "spectrogram": "Spectrogram Sqlite (.db);; Excel Workbook (*.xlsx)",
        "rois": "XML (*xml);; Excel Workbook (*.xlsx)",
        "instrument": "XML (*xml)",
    }
    def __init__(self, parent):
        super().__init__(parent)
    
    # --- Spectrum ---
    def export_spectrum_dialog(self, spectrum_name: str):
        "Export a single spectrum from outside the data store"
        file_path, selected_filter = QFileDialog.getSaveFileName(
            None,
            "Export File",
            str(Settings.Paths.last_opened_dir),
            self.export_filters["spectrum"],
        )

        if not file_path:
            return None

        file_path = Path(file_path).with_suffix("")
        if file_path is None:
            raise RuntimeWarning(f"Invalid file path {file_path}")
        
        spectrum = SpectrumManager.get_spectrum(spectrum_name)
        
        self.export_spectrum(spectrum, selected_filter, file_path)

    def export_spectrum(self, spectrum: Spectrum, filter: str, file_path: Path):
        if "xml" in filter.lower():
            file_io.xml_writer(spectrum, file_path)
        elif "csv" in filter.lower():
            file_io.export_csv(spectrum, str(file_path))
            
    # --- ROIs ---
    def export_roi_dialog(self):
        "Export roi data to CSV"
        if len(SpectrumManager.ROIManager.roi_registry) == 0:
            QMessageBox.warning(None , "Error", "No ROIs to export")
            return
        file_path, _ = QFileDialog.getSaveFileName(
            None,
            "Export File",
            str(Settings.Paths.last_opened_dir),
            "Comma Separated Values (*.csv)",
        )

        if not file_path:
            return None

        file_path = Path(file_path).with_suffix("")
        if file_path is None:
            raise RuntimeWarning(f"Invalid file path {file_path}")
        
IOManager = _IOManager()