from pathlib import Path

from PySide6.QtWidgets import QFileDialog, QMessageBox, QWidget
from PySide6.QtCore import Signal

import utils.file_io as file_io
from core import SpectrumManager, Settings
from containers.roi_classes import ROI
from containers.spectrum_classes import Spectrum
from .popup_windows.save_dialog import SaveNamingDialog
from containers.instrument_classes import GenericInstrument, UniqueInstrument
from utils.file_io import xml_parser


class FileDialogs(QWidget):
    sigImportSpectrum = Signal(dict)
    sigImportSpectrumAsBackground = Signal(str, dict)

    import_filters = {
        "spectrum": "Spectrum Files (*.xml *.n42 *.tke *.spe)",
        "spectrogram": "LuRaCs Spectrogram Database File (*.db)",
        "rois": "LuRaCs ROIs File (*.xml)",
        "instrument": "LuRaCs Instrument File (*xml)",
    }
    export_filters = {
        "spectrum": "XML/n42 (*xml);; CSV (*.csv);; Excel Workbook (*.xlsx)",
        "spectrogram": "Spectrogram Sqlite (.db);; Excel Workbook (*.xlsx)",
        "rois": "XML (*xml);; Excel Workbook (*.xlsx)",
        "instrument": "XML (*xml)",
    }

    def __init__(self, parent=None):
        super().__init__(parent=parent)

        print(Settings.Paths.last_opened_dir)
        
        self.sigImportSpectrum.connect(SpectrumManager.import_spectrum)
        self.sigImportSpectrumAsBackground.connect(
            SpectrumManager.import_spectrum_as_background
        )

    # --- Import ---
    def import_files(self, filter=None) -> tuple[list[Path], list[str]]:
        "Import multiple files with filters"
        if filter is None:
            filter = ";;".join(self.import_filters.values())
        file_paths, selected_filter = QFileDialog.getOpenFileNames(
            self, "Import File", str(Settings.Paths.last_opened_dir), filter
        )

        if file_paths is not None and len(file_paths) > 0:
            file_paths = [Path(fp) for fp in file_paths]
            Settings.Paths.last_opened_dir = file_paths[0].parent
            return file_paths, selected_filter

        else:
            return None, None
        
    def import_file(self, filter=None) -> tuple[Path, str]:
        "Import a single file with filters"
        if filter is None:
            filter = ";;".join(self.import_filters.values())
        file_path, selected_filter = QFileDialog.getOpenFileName(
            self, "Import File", str(Settings.Paths.last_opened_dir), filter
        )

        if file_path is not None:
            return Path(file_path), selected_filter

        else:
            return None, None

    def import_generic(self, filter=None):
        file_paths, selected_filter = self.import_files(filter)
        if file_paths is None:
            return

        # --- Spectrum Import ---
        if selected_filter == self.import_filters["spectrum"]:
            for file_path in file_paths:
                spectrum_parser = file_io.io_dispatcher(file_path)
                if isinstance(spectrum_parser, xml_parser):
                    self.sigImportSpectrum.emit(spectrum_parser.data)

    def import_spectrum(self, _=None):
        file_paths = self.import_generic(self.import_filters["spectrum"])

    def load_spectrum_as_background(self, spectrum_name: str):
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

    # --- Export ---
    def export_spectrum(self, spectrum_name: str):
        "Export a single spectrum from outside the data store"
        file_path, selected_filter = QFileDialog.getSaveFileName(
            self,
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

        if "xml" in selected_filter.lower():
            file_io.xml_writer(spectrum, str(file_path))
        elif "csv" in selected_filter.lower():
            file_io.export_csv(spectrum, str(file_path))


def save_roi_references():
    "Export reference rois for the library to be loaded on any spectrum"
    save_diag = SaveNamingDialog()
    if len(SpectrumManager.ROIManager.ROIs) < 1:
        QMessageBox.warning(save_diag, "Warning Message", "No ROIs set")
        return
    res = save_diag.exec()

    if res == SaveNamingDialog.Accepted:
        new_file = Settings.Paths.roi_library / str(save_diag.get_name())

        # Check if the file already exists
        if new_file.exists():
            reply = QMessageBox.question(
                None,
                "Overwrite File?",
                f"The file '{new_file.name}' already exists. Do you want to overwrite it?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )

            if reply == QMessageBox.No:
                return

    else:
        return

    # I cant be bothered to change the functional writer, just hack it
    # Create a dummy spectrum, only give it rois and the export it using the normal xml_writer
    dummy_spectrum = Spectrum(1, "Dummy")

    for r in SpectrumManager.ROIManager.ROIs.values():
        # Same as for the spectrum, make a dummy ROI
        dummy_roi = ROI(
            r.tag,
            r.alias,
            r.getRegion(),
            (None, None),
            r.fit_type,
            r.bkg_type,
            None,
            0,
            1,
            r.emission,
            meta={
                "movable": r.movable,
                "poisson_weights": r.poisson_weights,
                "merge": r.merge,
            },
        )

        dummy_spectrum.set_roi(dummy_roi)

    file_io.xml_writer(
        dummy_spectrum, new_file, export_spectrum=False, export_instrument=False
    )


def save_spectrum_to_library(spectrum: Spectrum):
    save_diag = SaveNamingDialog(spectrum.name)
    save_diag.remark_edit.setText(spectrum.remark)
    res = save_diag.exec()
    
    spectrum.remark = save_diag.get_remark()

    if res == SaveNamingDialog.Accepted:
        new_file = (Settings.Paths.spectrum_library / save_diag.get_name()).with_suffix(".xml")

        # Check if the file already exists
        if new_file.exists():
            reply = QMessageBox.question(
                None,
                "Overwrite File?",
                f"The file '{new_file.name}' already exists. Do you want to overwrite it?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )

            if reply == QMessageBox.No:
                return

    else:
        return
    
    SpectrumManager.rename_spectrum(spectrum.name, save_diag.get_name())
    
    file_io.xml_writer(
        spectrum, new_file
    )
    
def save_instrument(instrument: UniqueInstrument | GenericInstrument, file_name: Path):
    dummy_spectrum = Spectrum(1, "Dummy")
    dummy_spectrum.instrument = instrument
    if isinstance(instrument, UniqueInstrument):
        file_path = Settings.Paths.unique_instrument_library / file_name
    elif isinstance(instrument, GenericInstrument):
        file_path = Settings.Paths.generic_instrument_library / file_name
    else:
        raise ValueError(f"Invalid instrument type! {type(instrument)}")
    
    file_io.xml_writer(dummy_spectrum, file_path, export_spectrum=False, export_rois=False)