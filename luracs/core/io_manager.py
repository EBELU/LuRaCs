from __future__ import annotations
from typing import TYPE_CHECKING, Callable
if TYPE_CHECKING:
    pass
from luracs.containers.spectrum_classes import Spectrum

from pathlib import Path
from glob import glob
import os
from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import QFileDialog, QMessageBox

from luracs.utils.file_io import db_parser, xml_parser, io_dispatcher, spe_parser, tka_parser
from luracs.utils import file_io
from luracs.utils.save_to_internal import save_instrument_to_library, save_spectrum_to_library, save_rois_to_internal

from .settings import Settings
from .gui_logger import gui_logger
from .spectrum_manager import SpectrumManager

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
        self.Importer = _Importer(self)
        self.Exporter = _Exporter(self)
        
class _FileIndex(QObject):
    def __init__(self, parent):
        super().__init__(parent)
        
        # Build the indexes
        self.spectrum_index = Indexer(self, Settings.Paths.spectrum_library / "*.xml", {"meta_only": True}, save_spectrum_to_library, xml_parser)
        self.spectrogram_index = Indexer(self, Settings.Paths.spectrogram_library / "*.db", {}, None, db_parser)
        self.roi_index = Indexer(self, Settings.Paths.roi_library / "*.xml", {"meta_only": True}, save_rois_to_internal, xml_parser)
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
        
        # Rename
        SpectrumManager.UniqueInstrumentLibrary.sigInstrumentRenamed.connect(self.unique_instrument_index.rename_file)
        SpectrumManager.GenericInstrumentLibrary.sigInstrumentRenamed.connect(self.generic_instrument_index.rename_file)
        
        # Index everything at startup
        self.run_index_all()

    def run_index_all(self):
        "Re-Index all indexes"
        for indexer in [self.spectrum_index, self.spectrogram_index, self.roi_index, self.unique_instrument_index, self.generic_instrument_index]:
            indexer.run_index()
            gui_logger.debug(f"{indexer.__class__}: {len(indexer.index_registry)} files found in {indexer.path}")

class Indexer(QObject):
    """
    Tracks the storage, updates and indexing of internally stored .xml-files for access through the data store. Requires a special connection with the instrument library since they are always loaded and mutable.
    """
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
        if new_name in self.index_registry:
            del self.index_registry[new_name]
            os.remove(new_name)
            gui_logger.debug(f"{self.__class__} File {new_name} removed to rename {old_name}")
        os.rename(old_name, new_name)
        gui_logger.debug(f"{self.__class__} File {old_name} renamed to {new_name}")
        self.run_index()
                
    def update_file(self, key: str, new_item):
        # Not needed?
        assert isinstance(key, str)
        # self.blockSignals(True)
        # self.delete_file(key)
        # self.blockSignals(False)
        gui_logger.debug(f"In Update {key}" )
        gui_logger.debug(f"Current Registry {self.index_registry.keys()}")
        if key in self.index_registry:
            del self.index_registry[key]
        # Save the updated file
        self.save_file(new_item)
        gui_logger.debug(f"{self.__class__} Update completed! Key: {key}, item {new_item}")
        self.sigIndexUpdated.emit()
        
        
    
    def save_file(self, item):
        if self.save_fn is None:
            raise NotImplementedError()
        
        new_file = str(self.save_fn(item))
        gui_logger.debug(f"New file created {new_file}")
        if new_file in self.index_registry:
            del self.index_registry[new_file]
            gui_logger.debug(f"File removed from registry {new_file}")
            
        self.run_index()
        gui_logger.debug(f"{self.__class__} File {item} saved")
        return new_file

                
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
    Provides functionality needed for importing data to the application gathered in place. 
    """
    sigImportSpectrum = Signal(dict, bool)
    sigImportSpectrumAsBackground = Signal(str, dict)

    import_filters = {
        "spectrum": "Spectrum Files (*.xml *.n42 *.TKA *.Spe *.spe)",
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
            dialog_parent, "Import File", str(Settings.Paths.last_opened_dir), filter, options=QFileDialog.Option.DontUseNativeDialog
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
            self, "Import File", str(Settings.Paths.last_opened_dir), filter, options=QFileDialog.Option.DontUseNativeDialog
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
                if isinstance(spectrum_parser, (xml_parser, spe_parser, tka_parser)):
                    self.sigImportSpectrum.emit(spectrum_parser.data, True) # Bool is to signal external import

    def import_spectrum_as_background(self, spectrum_name: str):
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "Import File",
            str(Settings.Paths.last_opened_dir),
            self.import_filters["spectrum"],
            options=QFileDialog.Option.DontUseNativeDialog
        )
        if file_path is not None:
            file_path = Path(file_path)
            Settings.Paths.last_opened_dir = file_path.parent
        else:
            return

        spectrum_parser = file_io.io_dispatcher(file_path)
        if spectrum_parser is not None:
            self.sigImportSpectrumAsBackground.emit(spectrum_name, spectrum_parser.data)
            
    def build_spectrum_from_parser_data(self, data_dict: dict) -> Spectrum:
        if "foreground" not in data_dict:
            gui_logger.warning("File contains no spectrum")
            return
        new_spectrum = Spectrum(data_dict["foreground"].channels, data_dict["name"])
        new_spectrum.set_foreground(data_dict["foreground"])

        if "background" in data_dict:
            new_spectrum.set_background(data_dict["background"])

        if "calibration" in data_dict:
            new_spectrum.apply_calibration(data_dict["calibration"])
        

        if "peaks" in data_dict:
            for i, peak in enumerate(data_dict["peaks"]):
                peak.tag = f"ROI_{i}"
                new_spectrum.set_roi(peak)
                
        if "instrument" in data_dict:
            new_spectrum.instrument = data_dict["instrument"]
        
        if "remark" in data_dict:
            new_spectrum.remark = data_dict["remark"]
            
        return new_spectrum
    
    def import_library(self):
        zip_file_path, _ = QFileDialog.getSaveFileName(
            None,
            "Import zipped data store",
            str(Path.home()),
            "Zip-File (*.zip)",
            options=QFileDialog.Option.DontUseNativeDialog
        )
        
        if not zip_file_path:
            return
        
        file_io.unzip_library(Settings.Paths.appdata, Path(zip_file_path))
    
class _Exporter(QObject):
    """ 
    Provides functionality needed for export data from the application gathered in place. 
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
            options=QFileDialog.Option.DontUseNativeDialog
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
            file_io.csv_writer.export_spectrum(spectrum, str(file_path))
            
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
            options=QFileDialog.Option.DontUseNativeDialog
        )

        if not file_path:
            return None

        file_path = Path(file_path).with_suffix(".csv")
        
        if file_path.exists():
            reply = QMessageBox.question(
                None,
                "Overwrite File?",
                f"The file '{file_path.name}' already exists. Do you want to overwrite it?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )

            if reply == QMessageBox.No:
                return
        
        rois = []
        
        for spectrum in SpectrumManager.spectrum_registry:
            rois.extend(SpectrumManager.ROIManager.get_data_from_spectrum(spectrum).values())
        
        file_io.csv_writer.export_rois(rois, file_path)
        
    def export_library(self):
        zip_file_path, _ = QFileDialog.getSaveFileName(
            None,
            "Import zipped data store",
            str(Path.home()),
            "Zip-File (*.zip)",
            options=QFileDialog.Option.DontUseNativeDialog
        )
        
        if not zip_file_path:
            return
        
        file_io.zip_library(Settings.Paths.appdata, Path(zip_file_path))
        
IOManager = _IOManager()