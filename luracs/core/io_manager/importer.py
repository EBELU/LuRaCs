from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .io_manager import _IOManager
    
import shutil
from enum import Enum
from pathlib import Path

from PySide6.QtCore import QObject, Signal
from PySide6.QtWidgets import QFileDialog

from luracs.containers.spectrum_classes import Spectrum
from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument
from luracs.utils import file_io
from luracs.utils.file_io import (
    io_dispatcher,
    spe_parser,
    tka_parser,
    xml_parser,
)

from ..gui_logger import gui_logger
from ..settings import Settings
from ..spectrum_manager import SpectrumManager


class _Importer(QObject):
    """
    Provides functionality needed for importing data to the application gathered in place.
    """

    sigImportSpectrum = Signal(dict, bool)
    sigImportSpectrumAsBackground = Signal(str, dict)
    sigImportSpectrogram = Signal(str)

    class ImportFilters(Enum):
        SPECTRUM = "Spectrum Files (*.xml *.n42 *.TKA *.Spe *.spe)"
        SPECTROGRAM = "LuRaCs Spectrogram Database File (*.db)"
        ROIS = "LuRaCs ROIs File (*.xml)"
        INSTRUMENT = "LuRaCs Instrument File (*xml)"

    def __init__(self, parent, io_manager: _IOManager):
        super().__init__(parent)

        self.io_manager = io_manager

        self.sigImportSpectrum.connect(SpectrumManager.import_spectrum)
        self.sigImportSpectrumAsBackground.connect(
            SpectrumManager.import_spectrum_as_background
        )

    # --- Generic Importers ---
    def import_files(
        self, filter=None, dialog_parent=None
    ) -> tuple[list[Path], list[str]]:
        "Import multiple files with filters"
        # If no filter use all
        if filter is None:
            filter = ";;".join([v.value for v in self.ImportFilters])

        file_paths, selected_filter = QFileDialog.getOpenFileNames(
            dialog_parent,
            "Import File",
            str(Settings.Paths.last_opened_dir),
            filter,
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        # Clean up files and make them Paths
        if file_paths is not None and len(file_paths) > 0:
            file_paths = [Path(fp) for fp in file_paths]
            Settings.Paths.last_opened_dir = file_paths[0].parent  # New last opened

            return file_paths, selected_filter

        else:
            return None, None

    def import_file(self, filter=None) -> tuple[Path, str]:
        "Import a single file with filters"
        # If no filter use all
        if filter is None:
            filter = ";;".join([v.value for v in self.ImportFilters])

        file_path, selected_filter = QFileDialog.getOpenFileName(
            None,
            "Import File",
            str(Settings.Paths.last_opened_dir),
            filter,
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        # Clean up the path
        if file_path is not None:
            return Path(file_path), selected_filter

        else:
            return None, None

    def import_generic(self, import_filter=None):
        "GUI Helper for importing multiple files. Opens a QFileDialog and handles dynamic import."
        file_paths, selected_filter = self.import_files(import_filter)
        if file_paths is not None:
            self.import_generic_paths(*file_paths, selected_filter=selected_filter)

    def import_spectrograms(self, *file_paths: Path):
        self.import_generic_paths(
            *file_paths, selected_filter=self.ImportFilters.SPECTROGRAM.value
        )

    def import_generic_paths(self, *file_paths: Path, selected_filter: str):
        "Import anything supported based on file path based on selected filter"

        file_path = None
        for file_path in file_paths:
            if isinstance(file_path, str):
                file_path = Path(file_path)
            # --- Spectrum Import ---
            if selected_filter == self.ImportFilters.SPECTRUM.value:
                parser = io_dispatcher(file_path)
                if isinstance(parser, (xml_parser, spe_parser, tka_parser)):
                    self.sigImportSpectrum.emit(
                        parser.data, True
                    )  # Bool is to signal external import

            # --- Spectrogram Import ---
            # An imported spectrogram is copied to the data library and then loaded for use
            elif selected_filter == self.ImportFilters.SPECTROGRAM.value:
                new_name = (
                    Settings.Paths.spectrogram_library / file_path.name
                ).with_suffix(".db")
                shutil.copy2(str(file_path.resolve()), str(new_name))
                self.sigImportSpectrogram.emit(str(new_name.stem))

            # --- ROIs import ---
            # A ROI-file is like a normal spectrum but only the roi info is used
            elif selected_filter == self.ImportFilters.ROIS.value:
                parser = io_dispatcher(file_path)
                if isinstance(parser, xml_parser):
                    name = parser.get_header().name
                    SpectrumManager.ROIManager.block_roi_updates(True)
                    for peak in parser.get_rois():
                        extended_kwargs = {
                            "alias": peak.alias,
                            "fit_type": peak.fit_type,
                            "bkg_type": peak.bkg_type,
                            "emission": peak.emission,
                            **peak.meta,
                        }

                        SpectrumManager.ROIManager.add_roi(
                            *peak.roi_bound,
                            **extended_kwargs,
                            owner_spectrum=name
                            if Settings.Appearance.tabbed_spectrum_view
                            else None,
                        )
                    SpectrumManager.ROIManager.block_roi_updates(False)
                    SpectrumManager.ROIManager.update_roi()

            # --- Instrument Import ---
            # Importing an instrument only copies it to the data library for later use
            elif selected_filter == self.ImportFilters.INSTRUMENT.value:
                parser = io_dispatcher(file_path)
                if isinstance(parser, xml_parser):
                    instrument = parser.get_instrument()

                    if isinstance(instrument, GenericInstrument):
                        self.io_manager.FileIndex.generic_instrument_index.save_to_library(instrument)
                    elif isinstance(instrument, UniqueInstrument):
                        self.io_manager.FileIndex.unique_instrument_index.save_to_library(instrument)
                    elif instrument is None:
                        return
                    else:
                        gui_logger.warning(
                            f"Instrument {instrument} is not a valid instrument type"
                        )

        if file_path is not None:
            Settings.Paths.last_opened_dir = file_path.parent

    def import_spectrum_as_background(self, spectrum_name: str):
        file_path, _ = QFileDialog.getOpenFileName(
            self,
            "Import File",
            str(Settings.Paths.last_opened_dir),
            self.ImportFilters.SPECTRUM.value,
            options=QFileDialog.Option.DontUseNativeDialog,
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
            "Import zipped data library",
            str(Path.home()),
            "Zip-File (*.zip)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        if not zip_file_path:
            return

        file_io.unzip_library(Settings.Paths.appdata, Path(zip_file_path))
