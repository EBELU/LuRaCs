from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .io_manager import _IOManager

from datetime import datetime
from enum import Enum
from pathlib import Path

from PySide6.QtCore import QObject
from PySide6.QtWidgets import QFileDialog, QMessageBox

from luracs.containers.spectrum_classes import Spectrum
from luracs.utils import file_io

from ..gui_logger import gui_logger
from ..run_manager import RunManager
from ..settings import Settings
from ..spectrum_manager import SpectrumManager


class _Exporter(QObject):
    """
    Provides functionality needed for export data from the application gathered in place.
    """

    class ExportFilters(Enum):
        SPECTRUM = "XML/n42 (*xml);; CSV (*.csv);; Excel Workbook (*.xlsx)"
        SPECTROGRAM = "Spectrogram Sqlite (.db);; Excel Workbook (*.xlsx)"
        ROIS = "XML (*xml);; Excel Workbook (*.xlsx)"
        INSTRUMENT = "XML (*xml)"

    def __init__(self, parent, io_manager: _IOManager):
        super().__init__(parent)
        self.io_manager = io_manager

    # --- Spectrum ---
    def export_spectrum_dialog(self, spectrum_name: str):
        "Export a single spectrum from outside the data library"
        file_path, selected_filter = QFileDialog.getSaveFileName(
            None,
            "Export File",
            str(Settings.Paths.last_opened_dir),
            self.ExportFilters.SPECTRUM.value,
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        if not file_path:
            return

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
        elif "xlsx" in filter.lower():
            file_io.xlsx_writer.export_spectrum(spectrum, str(file_path))

    # --- Spectrogram ---
    def export_spectrogram_to_spectrum(
        self,
        spectrogram_is_loaded: bool,
        spectrogram_name: str,
        spectrum_name: str,
        spectrum_remark: str,
        start_time: datetime | None = None,
        stop_time: datetime | None = None,
    ):
        """
        Create a spectrum from a spectrogram and add it to the Data Library.
        """

        assert isinstance(spectrogram_name, str)
        if spectrogram_is_loaded:
            current_spectrogram = (
                RunManager.SpectrogramManager.spectrogram_registry.get(spectrogram_name)
            )
            parser = file_io.db_parser(connection=current_spectrogram.connection)
        else:
            parser = file_io.db_parser(file_name=spectrogram_name)

        new_name = Settings.Paths.spectrum_library / spectrum_name

        new_spectrum = file_io.db_writer.build_spectrum_from_db(
            parser, new_name, start_time, stop_time
        )

        new_spectrum.remark = spectrum_remark
        new_spectrum.name = spectrum_name

        self.io_manager.FileIndex.spectrum_index.save_file(new_spectrum)
        gui_logger.info(
            f"Spectrum Exported from spectrogram: "
            f"spectrogram={current_spectrogram.db_name}, "
            f"spectrum={new_spectrum.name}"
        )

    # --- ROIs ---
    def export_roi_dialog(self):
        "Export roi data to CSV"
        if len(SpectrumManager.ROIManager.roi_registry) == 0:
            QMessageBox.warning(None, "Error", "No ROIs to export")
            return
        file_path, _ = QFileDialog.getSaveFileName(
            None,
            "Export File",
            str(Settings.Paths.last_opened_dir),
            "Comma Separated Values (*.csv)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        if not file_path:
            return

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
            rois.extend(
                SpectrumManager.ROIManager.get_data_from_spectrum(spectrum).values()
            )

        file_io.csv_writer.export_rois(rois, file_path)

    def export_library(self):
        zip_file_path, _ = QFileDialog.getSaveFileName(
            None,
            "Import zipped data library",
            str(Path.home()),
            "Zip-File (*.zip)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        if not zip_file_path:
            return

        file_io.zip_library(Settings.Paths.appdata, Path(zip_file_path))
