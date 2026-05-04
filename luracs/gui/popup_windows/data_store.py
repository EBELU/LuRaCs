from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from utils.file_io.db_parser import db_parser

from PySide6.QtWidgets import (
    QDialog,
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QTabWidget,
    QSizePolicy,
    QGroupBox,
    QPushButton,
    QCheckBox,
    QMessageBox,
    QFileDialog,
    QAbstractItemView,
    QToolButton,
    QMenu
)

from PySide6.QtCore import Qt

from ..misc.idx_table import StrIdxTable
from utils.file_io import io_dispatcher
from core import Settings, SpectrumManager
from utils.DataLogging import restart_logger
from .data_store_edit_dialogs import InstrumentDialog
from InstrumentClasses import UniqueInstrument, GenericInstrument

import os
import shutil
from pathlib import Path
from glob import glob
from datetime import timedelta, datetime
import utils.file_io as file_io
from gui.import_export import save_instrument


class DataLibrary(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)

        # Main layout for the dialog
        main_layout = QVBoxLayout(self)

        # Tabs
        self.tabs = QTabWidget()
        self.tabs.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

        # Create tab widgets as class members
        self.spectrum_tab = SpectrumTab(self)
        self.roi_tab = ROIsTab(self)
        self.spectrogram_tab = SpectrogramTab(self)
        self.instruments_tab = InstrumentsTab(self)
        self.generic_instruments_tab = GenericInstrumentsTab(self)

        # Add tabs in the desired order
        self.tabs.addTab(self.spectrum_tab, "Spectrum Library")
        self.tabs.addTab(self.spectrogram_tab, "Spectrogram Library")
        self.tabs.addTab(self.roi_tab, "ROI Library")
        self.tabs.addTab(self.instruments_tab, "Instruments")
        self.tabs.addTab(self.generic_instruments_tab, "Generic Instruments")

        main_layout.addWidget(self.tabs)
        # self.resize(self.tabs.sizeHint())
        self.adjustSize()

    def show(self):
        self.spectrum_tab.run_index()
        self.spectrogram_tab.run_index()
        self.roi_tab.run_index()
        super().show()


class LibraryTab(QWidget):
    def __init__(self, parent, columns, widths=None, include_checks=False):
        super().__init__(parent=parent)

        self.file_index = {}
        self.path = ""

        main_layout = QVBoxLayout(self)

        self.table = StrIdxTable()
        self.table.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.table.setSelectionMode(QAbstractItemView.ExtendedSelection)

        self.table.reset_table(columns, widths)
        table_layout = QHBoxLayout()
        table_box = QGroupBox()

        table_layout.addWidget(self.table.table)
        table_box.setLayout(table_layout)
        main_layout.addWidget(table_box)

        btn_group = QGroupBox()
        self.btn_bar = QHBoxLayout()

        self.btn_close = QPushButton("Close")
        self.btn_close.clicked.connect(parent.close)

        self.btn_load = QPushButton("Load")

        self.export_menu = QMenu()

        self.btn_export = QPushButton()
        self.btn_export.setText("Export")
        self.btn_export.setMenu(self.export_menu)

        # self.btn_export.setStyleSheet("""
        #     QToolButton {
        #         padding: 1px 12px;
        #     }
        # """)

        self.btn_info = QPushButton("Edit")

        self.include_instrument_check = QCheckBox("Load Instrument")
        self.include_roi_check = QCheckBox("Load ROIs")

        self.btn_delete = QPushButton("Delete")
        self.btn_delete.clicked.connect(self.delete_selected)

        self.btn_bar.addWidget(self.btn_delete, alignment=Qt.AlignLeft)
        # After this buttons will be aligned to the right
        self.btn_bar.addStretch()
        if include_checks:
            self.btn_bar.addWidget(self.include_instrument_check)
            self.btn_bar.addWidget(self.include_roi_check)

        self.btn_bar.addWidget(self.btn_info)
        self.btn_bar.addWidget(self.btn_export)
        self.btn_bar.addWidget(self.btn_load)
        self.btn_bar.addWidget(self.btn_close)

        # Dont preselect delete
        self.btn_close.setDefault(True)
        self.btn_close.setAutoDefault(True)

        for btn in [self.btn_load, self.btn_export, self.btn_info, self.btn_delete]:
            btn.setAutoDefault(False)

        btn_group.setLayout(self.btn_bar)
        btn_group.setMaximumHeight(70)
        btn_group.setContentsMargins(1, 0, 1, 0)
        main_layout.addWidget(btn_group)

        # self.run_index()

    def _get_selection(self) -> list[Path]:
        rows = [
            Path(self.table.get_key_from_index(index.row()))
            for index in self.table.table.selectionModel().selectedRows()
        ]

        if len(rows) == 0:
            QMessageBox.warning(self, "Select a row", "Please select an item.")
            return

        return rows

    def export_same(self, filter: str):
        selection = self._get_selection()
        if selection is None:
            return

        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder")
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                shutil.copy(file, str(folder / file.name))

        else:
            default_name = Path.home() / selection[0].name
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrum", str(default_name), filter
            )

            if not new_path:
                return
            new_path = Path(new_path)

            shutil.copy(selection[0], new_path.with_suffix(selection[0].suffix))

    def delete_selected(self):
        selection = self._get_selection()
        if selection is None:
            return
        reply = QMessageBox.question(
            None,
            "Question",
            f"Do you want to delete {len(selection)} selected items?",
            QMessageBox.Yes | QMessageBox.No,
        )

        if reply == QMessageBox.Yes:
            for file in selection:
                self.file_index.pop(file)
                self.table.delete_row(file)
                os.remove(file)
            self.run_index()

    def show_info(self):
        raise NotImplementedError("Info button not implemented")


class SpectrumTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent,
            ["Name", "Date", "Live Time", "Background", "ROIs", "Instrument"],
            [150, 140, 75, 95, 50, 100],
            True,
        )
        self.run_index()
        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)

        export_xml = self.export_menu.addAction("LuRaCs XML (*.xml)")
        export_xml.triggered.connect(
            lambda: self.export_same("LuRaCs spectrum file, n42 compatible (*.xml)")
        )
        export_csv = self.export_menu.addAction("CSV (*.csv)")
        export_xlsx = self.export_menu.addAction("Exel Workbook (*.xlsx)")

    def run_index(self):
        for file in glob(str(Settings.Paths.spectrum_library / "*.xml")):
            if file not in self.file_index:
                self.file_index[str(file)] = io_dispatcher(file, True)

        self.set_table()

    def set_table(self):
        for key, parser in self.file_index.items():
            name = parser.data.get("name")
            fg = parser.data.get("foreground")
            if fg is not None:
                date = fg.start_date
                live_time = timedelta(seconds=round(fg.live_time))
            else:
                date = live_time = None

            has_bkg = True if parser.data.get("background") is not None else False
            peaks = (
                parser.data.get("peaks") if parser.data.get("peaks") is not None else []
            )
            rois = len(peaks)
            instrument = parser.data.get("instrument_model")
            self.table.write_row(
                key, [name, date, live_time, has_bkg, rois, instrument]
            )

    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            parser = io_dispatcher(file)
            if not self.include_roi_check.isChecked():
                parser.data.pop("peaks", None)

            SpectrumManager.import_spectrum(parser.data)


class SpectrogramTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent,
            ["Name", "Start Date", "End Date", "Duration", "Instrument"],
            (230, 150, 150, 65, 130),
            True,
        )
        self.file_index: dict[str, db_parser] = {}
        self.run_index()
        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)

        self.include_roi_check.setText("Export Spectrum Data")

        self.include_instrument_check.setVisible(False)

        export_db = self.export_menu.addAction("LuRaCs Sqlite Database (*.db)")
        export_db.triggered.connect(
            lambda: self.export_same("LuRaCs Sqlite Database (*.db)")
        )

        export_xlsx = self.export_menu.addAction("Exel Workbook (*.xlsx)")
        export_xlsx.triggered.connect(self.export_xlsx)

    def run_index(self):
        for file in glob(str(Settings.Paths.spectrogram_library / "**.db")):
            file = Path(file)
            if file not in self.file_index:
                self.file_index[file] = io_dispatcher(file, True)

        self.set_table()

    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            restart_logger(Path(file).name)

    def set_table(self):
        for key, parser in self.file_index.items():
            header, summary = parser.get_header(), parser.get_summary()

            start_date = header.get("created")
            end_date = summary.get("last_update")
            duration = timedelta(seconds=round(summary.get("total_duration")))
            instrument = header.get("device_id")

            self.table.write_row(
                key, [key.stem, start_date, end_date, duration, instrument]
            )

    def export_xlsx(self):
        selection = self._get_selection()
        if selection is None:
            return

        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder")
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                parser = file_io.db_parser(file)
                file_io.spectrogram_exporter(
                    parser, "xlsx", (folder / Path(file).stem).with_suffix(".xlsx")
                )

        else:
            default_name = (Path.home() / selection[0]).with_suffix(".xlsx")
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrogram", str(default_name), "Excel Workbook (*.xlsx)"
            )

            if not new_path:
                return

            parser = file_io.db_parser(selection[0])
            file_io.spectrogram_exporter(
                parser,
                "xlsx",
                Path(new_path).with_suffix(".xlsx"),
                include_spectrogram_data=self.include_roi_check.isChecked(),
            )


class ROIsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "ROIs", "Regions"], [150, 50, 400])
        self.run_index()
        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)

    def run_index(self):
        for file in glob(str(Settings.Paths.roi_library / "**.xml")):
            if file not in self.file_index:
                self.file_index[str(file)] = io_dispatcher(file, True)

        self.set_table()

    def set_table(self):
        for key, parser in self.file_index.items():
            rois = parser.get_rois()
            regions = ", ".join([str([round(rb) for rb in r.roi_bound]) for r in rois])
            self.table.write_row(key, [Path(key).name, len(rois), str(regions)])

    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            parser = io_dispatcher(file)
            rois = parser.get_rois()
            for peak in rois:
                extented_kwargs = {
                    "alias": peak.alias,
                    "fit_type": peak.fit_type,
                    "bkg_type": peak.bkg_type,
                    **peak.meta,
                }

                SpectrumManager.ROIManager.add_roi(*peak.roi_bound, **extented_kwargs)


class InstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent,
            [
                "Name",
                "Model",
                "Calibration",
                "Resolution",
                "Efficiency",
                "Response Matrix",
            ],
        )
        self.btn_load.setText("New")
        self.btn_load.clicked.connect(self.new)
        self.run_index()
        self.btn_info.clicked.connect(self.edit)
        
    def run_index(self):
        for file in glob(str(Settings.Paths.unique_instrument_library / "**.xml")):
            file = Path(file)
            if file not in self.file_index:
                self.file_index[file] = io_dispatcher(file, True)

        self.set_table()
        
    def set_table(self):
        for key, parser in self.file_index.items():
            instr_data = parser.get_instrument_data()
            
            calibration = "True" if instr_data.get("calibration") is not None else "False"
            resolution = "True" if instr_data.get("resolution") is not None else "False"
            efficiency = "True" if instr_data.get("efficiency") is not None else "False"
            response_matrix = "True" if instr_data.get("response_matrix") is not None else "False"
            
            self.table.write_row(key, [instr_data["name"], instr_data["model"], calibration, resolution, efficiency, response_matrix])
            
    def edit(self):
        selection = self._get_selection()
        if not selection:
            return
        
        if len(selection) > 1:
            QMessageBox.warning(self, "Selection error", "Only one item may be edited simultaneously")
            return
        
        edit_dialog = InstrumentDialog(**self.file_index[selection[0]].get_instrument_data())
        res = edit_dialog.exec()
        
        if res != InstrumentDialog.Accepted:
            return
        
        
        self.file_index.pop(selection[0])
        self.table.delete_row(selection[0])
        os.remove(selection[0])
        
        new_instrument = UniqueInstrument(**edit_dialog.get_data(), detector_type="Gamma Spectrometer")
        
        save_instrument(new_instrument, edit_dialog.get_data()["name"])
        self.run_index()
        
    def new(self):
        edit_dialog = InstrumentDialog()
        res = edit_dialog.exec()
        if res != InstrumentDialog.Accepted:
            return
        
        new_instrument = UniqueInstrument(**edit_dialog.get_data(), detector_type="Gamma Spectrometer")
        
        save_instrument(new_instrument, edit_dialog.get_data()["name"])
        self.run_index()
        

class GenericInstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent, ["Model", "Resolution", "Efficiency", "Response Matrix"]
        )
        self.btn_load.setText("New")
        self.btn_load.clicked.connect(self.new)

    def new(self):
        edit_dialog = InstrumentDialog()
        edit_dialog.name_input.setEnabled(False)

        res = edit_dialog.exec()
        
