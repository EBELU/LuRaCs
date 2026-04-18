from pathlib import Path

from PySide6.QtWidgets import QFileDialog, QMessageBox
from PySide6.QtCore import QObject, Signal

import utils.file_io as file_io
from core import SpectrumManager, Settings
from ROIClasses import ROI
from SpectrumClasses import Spectrum
from .popup_windows.save_dialog import SaveNamingDialog


class FileDialogs(QObject):
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
        super().__init__(parent)
        self.parent = parent
        self.last_spect_dir = Path.home()

        self.sigImportSpectrum.connect(SpectrumManager.import_spectrum)
        self.sigImportSpectrumAsBackground.connect(
            SpectrumManager.import_spectrum_as_background
        )

    # --- Import ---
    def import_files(self, filter=None):
        "Import multiple files with filters"
        if filter is None:
            filter = ";;".join(self.import_filters.values())
        file_paths, selected_filter = QFileDialog.getOpenFileNames(
            self.parent, "Import File", str(self.last_spect_dir), filter
        )

        if file_paths is not None and len(file_paths) > 0:
            file_paths = [Path(fp) for fp in file_paths]
            self.last_spect_dir = file_paths[0].parent
            return file_paths, selected_filter

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
                if spectrum_parser is not None:
                    self.sigImportSpectrum.emit(spectrum_parser.data)

    def import_spectrum(self, _=None):
        file_paths = self.import_generic(self.import_filters["spectrum"])

    def load_spectrum_as_background(self, spectrum_name: str):
        file_path, _ = QFileDialog.getOpenFileName(
            self.parent,
            "Import File",
            str(self.last_spect_dir),
            self.import_filters["spectrum"],
        )
        if file_path is not None:
            file_path = Path(file_path)
            self.last_spect_dir = file_path.parent
        else:
            return

        spectrum_parser = file_io.io_dispatcher(file_path)
        if spectrum_parser is not None:
            self.sigImportSpectrumAsBackground.emit(spectrum_name, spectrum_parser.data)

    # --- Export ---
    def export_spectrum(self, spectrum_name: str):
        "Export a single spectrum from outside the data store"
        file_path, selected_filter = QFileDialog.getSaveFileName(
            self.parent,
            "Export File",
            str(self.last_spect_dir),
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
            {
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
    res = save_diag.exec()
    
    spectrum.name = save_diag.get_name()

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
    
    
    file_io.xml_writer(
        spectrum, new_file
    )