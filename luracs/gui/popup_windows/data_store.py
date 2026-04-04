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
    QAbstractItemView
)

from PySide6.QtCore import Qt

from ..misc.idx_table import StrIdxTable
from utils.file_io import io_dispatcher
from core import Settings, SpectrumManager

import os
import shutil
from pathlib import Path
from glob import glob
from datetime import timedelta, datetime

class DataLibrary(QDialog):
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
    def __init__(self, parent, columns, widths = None, include_checks = False):
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

        self.btn_export = QPushButton("Export")
        self.btn_export.clicked.connect(self.export_selected)


        self.btn_info = QPushButton("Info")


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
        
    def _get_selection(self) -> list:
        rows = [self.table.get_key_from_index(index.row()) for index in self.table.table.selectionModel().selectedRows()]

        if len(rows) == 0:
            QMessageBox.warning(self, "Select a row", "Please select an item.")
            return
        
        return rows
    
    def export_selected(self):
        selection = self._get_selection()
        if selection is None:
            return
        
        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(
                None,
                "Select or Create Folder"
            )
            if folder is None:
                return
            
            for file in selection:
                shutil.move(file, folder)

        else:
            new_path = QFileDialog.getOpenFileName(
                None,
                "Export Spectrum",
                str(Path.home()),
                
                
            )
            if not new_path:
                return
            
            shutil.move(selection[0], new_path)

    def delete_selected(self):
        selection = self._get_selection()
        if selection is None:
            return
        reply = QMessageBox.question(
            None,
            "Question",
            f"Do you want to delete {len(selection)} selected items?",
            QMessageBox.Yes | QMessageBox.No
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
        super().__init__(parent, ["Name", "Date", "Live Time", "Background", "ROIs", "Instrument"], 
                         [150, 140, 75, 95, 50, 100],True)
        self.run_index()
        self.btn_load.clicked.connect(self.load)
        
    def run_index(self):
        for file in glob(str(Path("/home/eewa/**.xml"))):
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
            peaks = parser.data.get("peaks") if parser.data.get("peaks") is not None else []
            rois = len(peaks)
            instrument = parser.data.get("instrument_model")
            self.table.write_row(key, [name, date, live_time, has_bkg, rois, instrument])
            
    def load(self):
        selection = self._get_selection()
        if selection is None:
            return
        
        for file in selection:
            parser = io_dispatcher(file)
            if not self.include_roi_check.isChecked():
                parser.data.pop("peaks")
            
            SpectrumManager.import_spectrum(parser.data)
            
class SpectrogramTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "Start Date", "End Date", "Duration", "Instrument"])
        self.file_index: dict[str, db_parser] = {}
        self.run_index()

        
    def run_index(self):
        for file in glob(str(Settings.Paths.spectrogram_library / "**.db")):
            file = Path(file)
            if file not in self.file_index:
                self.file_index[file] = io_dispatcher(file, True)

        
        self.set_table()
        
    def set_table(self):
        for key, parser in self.file_index.items():
            header, summary = parser.get_header(), parser.get_summary()
            
            start_date = datetime.fromtimestamp(round(header.get("created")))
            end_date = datetime.fromtimestamp(round(summary.get("last_update")))
            duration = timedelta(seconds=round(summary.get("total_duration")))
            instrument = header.get("device_id")
            
            self.table.write_row(key, [key.name, start_date, end_date, duration, instrument])
            

            

class ROIsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "ROIs", "Regions"], [150, 50, 400])
        self.run_index()
        self.btn_load.clicked.connect(self.load)
        
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
                extented_kwargs = {"alias": peak.alias,
                    "fit_type": peak.fit_type,
                    "bkg_type": peak.bkg_type,
                    **peak.meta}
                
                SpectrumManager.ROIManager.add_roi(*peak.roi_bound, **extented_kwargs)
        

class InstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "Type", "Calibration", "Resolution", "Efficiency", "Response Matrix"])
        self.btn_load.setText("New")

class GenericInstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Type", "Resolution", "Efficiency", "Response Matrix"])
        self.btn_load.setText("New")