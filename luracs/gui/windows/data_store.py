from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass

from PySide6.QtWidgets import (
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
    QMenu
)

from PySide6.QtCore import Qt, Signal

from ..misc.idx_table import StrIdxTable
from luracs.utils.file_io import io_dispatcher
from luracs.core import SpectrumManager, IOManager
from luracs.spectrogram import restart_spectrogram
from ..dialogs.data_store_edit_dialogs import InstrumentDialog, ROIDialog
from luracs.containers.instrument_classes import UniqueInstrument, GenericInstrument
from luracs.containers.spectrum_classes import Spectrum
from luracs.gui.dialogs.save_dialog import SaveNamingDialog

import shutil
from pathlib import Path
from datetime import timedelta
import luracs.utils.file_io as file_io


class DataLibrary(QWidget):
    sigRunIndex = Signal()
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setMinimumWidth(700)
        self.setMinimumHeight(500)
        
        self.sigRunIndex.connect(IOManager.FileIndex.run_index_all)

        # Main layout for the dialog
        main_layout = QVBoxLayout(self)

        # Tabs
        self.tabs = QTabWidget()
        self.tabs.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

        # Create tab widgets as class members
        self.spectrum_tab = SpectrumTab(self)
        self.roi_tab = ROIsTab(self)
        self.spectrogram_tab = SpectrogramTab(self)
        self.unique_instruments_tab = InstrumentsTab(self)
        self.generic_instruments_tab = GenericInstrumentsTab(self)

        # Add tabs
        self.tabs.addTab(self.spectrum_tab, "Spectrum Library")
        self.tabs.addTab(self.spectrogram_tab, "Spectrogram Library")
        self.tabs.addTab(self.roi_tab, "ROI Library")
        self.tabs.addTab(self.unique_instruments_tab, "Instruments")
        self.tabs.addTab(self.generic_instruments_tab, "Generic Instruments")

        main_layout.addWidget(self.tabs)
        # self.resize(self.tabs.sizeHint())
        self.adjustSize()

    def show(self):
        self.sigRunIndex.emit()
        self.spectrum_tab.set_table()
        self.spectrogram_tab.set_table()
        self.roi_tab.set_table()
        self.unique_instruments_tab.set_table()
        self.generic_instruments_tab.set_table()
        super().show()


class LibraryTab(QWidget):
    def __init__(self, parent, columns, widths=None, include_checks=False):
        super().__init__(parent=parent)

        self.path = ""
        self.delete_fn = None
        
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

    def _get_selection(self) -> list[str]:
        rows = [
            self.table.get_key_from_index(index.row())
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
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder", options=QFileDialog.Option.DontUseNativeDialog)
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                shutil.copy(file, str(folder / Path(file).name))

        else:
            default_name = (Path.home() / selection[0]).name
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrum", str(default_name), filter, options=QFileDialog.Option.DontUseNativeDialog
            )

            if not new_path:
                return
            new_path = Path(new_path)

            shutil.copy(selection[0], str(new_path.with_suffix(Path(selection[0]).suffix)))


    def show_info(self):
        raise NotImplementedError("Info button not implemented")
    
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
                self.delete_fn(file)
                self.table.delete_row(file)
                
        self.set_table()   
    


class SpectrumTab(LibraryTab):
    def __init__(self, parent):
        self.titles = ["Name", "Date", "Live Time", "Background", "ROIs", "Instrument"]
        super().__init__(
            parent,
            self.titles,
            [150, 140, 75, 95, 50, 100],
            True,
        )
        self.include_instrument_check.setChecked(True)
        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)

        export_xml = self.export_menu.addAction("LuRaCs XML (*.xml)")
        export_xml.triggered.connect(
            lambda: self.export_same("LuRaCs spectrum file, n42 compatible (*.xml)")
        )
        export_csv = self.export_menu.addAction("CSV (*.csv)")
        export_csv.triggered.connect(self.export_csv)
        export_xlsx = self.export_menu.addAction("Excel Workbook (*.xlsx)")
        export_xlsx.triggered.connect(self.export_xlsx)

        self.btn_info.clicked.connect(self.edit)

        self.delete_fn = IOManager.FileIndex.spectrum_index.delete_file
        
    def set_table(self):
        self.table.reset_table(self.titles)
        for key, parser in IOManager.FileIndex.spectrum_index.get_index().items():
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
            instrument = parser.data.get("instrument")
            
            self.table.write_row(
                key, [name, date, live_time, has_bkg, rois, instrument.name if instrument is not None else "None"]
            )

    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            parser = io_dispatcher(file)
            if not self.include_roi_check.isChecked():
                parser.data.pop("peaks", None)
            
            if not self.include_instrument_check.isChecked():
                parser.data.pop("instrument", None)
                
            SpectrumManager.import_spectrum(parser.data)
            
    def edit(self):
        selection = self._get_selection()
        if not selection:
            return
        
        if len(selection) > 1:
            QMessageBox.warning(self, "Selection error", "Only one item may be edited simultaneously")
            return
        
        parser = io_dispatcher(selection[0], meta_parsing = True)
        
        edit_dialog = SaveNamingDialog(name=parser.get_header().name)
        edit_dialog.remark_edit.clear()
        edit_dialog.remark_edit.setPlainText(parser.get_header().remark or "")

        res = edit_dialog.exec()
        
        if res != ROIDialog.Accepted:
            return



        header_dict = parser.get_header().__dict__
        del header_dict["name"]
        dummy_spectrum = Spectrum(parser.get_foreground_spectrum().channels, edit_dialog.get_name(), **header_dict)
        dummy_spectrum.set_foreground(parser.get_foreground_spectrum())
        dummy_spectrum.set_background(parser.get_background_spectrum())
        
        for roi in parser.get_rois():
            dummy_spectrum.set_roi(roi)
        
        if parser.get_instrument() is not None:
            dummy_spectrum.set_instrument(parser.get_instrument())
            
        dummy_spectrum.remark = edit_dialog.get_remark()
        
        IOManager.FileIndex.spectrum_index.delete_file(selection[0])
        IOManager.FileIndex.spectrum_index.save_file(dummy_spectrum)
        
        self.set_table()
        
        
            
            
    def export_csv(self):
        filter = "Comma Separated Values (*.csv)"
        selection = self._get_selection()
        if selection is None:
            return

        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder", options=QFileDialog.Option.DontUseNativeDialog)
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                parser = file_io.xml_parser(file)
                spectrum = IOManager.Importer.build_spectrum_from_parser_data(parser.data)
                
                file_io.csv_writer.export_spectrum(spectrum, folder / Path(file).stem)

        else:
            default_name = (Path.home() / selection[0]).stem
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrum", str(default_name), filter, options=QFileDialog.Option.DontUseNativeDialog
            )

            if not new_path:
                return
            
            new_path = Path(new_path)
            
            parser = file_io.xml_parser(selection[0])
            spectrum = IOManager.Importer.build_spectrum_from_parser_data(parser.data)
            
            file_io.csv_writer.export_spectrum(spectrum, new_path)
            
    def export_xlsx(self):
        filter = "Excel Workbook (*.xlsx)"
        selection = self._get_selection()
        if selection is None:
            return

        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder", options=QFileDialog.Option.DontUseNativeDialog)
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                parser = file_io.xml_parser(file)
                spectrum = IOManager.Importer.build_spectrum_from_parser_data(parser.data)
                
                file_io.xlsx_writer.export_spectrum(spectrum, folder / Path(file).stem)

        else:
            default_name = (Path.home() / selection[0]).stem
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrum", str(default_name), filter, options=QFileDialog.Option.DontUseNativeDialog
            )

            if not new_path:
                return
            
            new_path = Path(new_path)
            
            parser = file_io.xml_parser(selection[0])
            spectrum = IOManager.Importer.build_spectrum_from_parser_data(parser.data)
            
            file_io.xlsx_writer.export_spectrum(spectrum, new_path)
                       

class SpectrogramTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent,
            ["Name", "Start Date", "End Date", "Duration", "Instrument"],
            (230, 150, 150, 65, 130),
            True,
        )

        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)

        self.include_roi_check.setText("Export Spectrum Data")

        self.include_instrument_check.setVisible(False)

        export_db = self.export_menu.addAction("LuRaCs SQLite Database (*.db)")
        export_db.triggered.connect(
            lambda: self.export_same("LuRaCs SQLite Database (*.db)")
        )

        export_xlsx = self.export_menu.addAction("Exel Workbook (*.xlsx)")
        export_xlsx.triggered.connect(self.export_xlsx)
        
        self.delete_fn = IOManager.FileIndex.spectrogram_index.delete_file
        
    def set_table(self):
        for key, parser in IOManager.FileIndex.spectrogram_index.get_index().items():
            header, summary = parser.get_header(), parser.get_summary()

            start_date = header.created
            end_date = summary.last_update
            duration = timedelta(seconds=round(summary.total_duration))
            instrument = header.device_id

            self.table.write_row(
                key, [Path(key).stem, start_date, end_date, duration, instrument]
            )
            
    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            restart_spectrogram(Path(file).name)
            


    def export_xlsx(self):
        selection = self._get_selection()
        if selection is None:
            return

        if len(selection) > 1:
            folder = QFileDialog.getExistingDirectory(None, "Select or Create Folder", options=QFileDialog.Option.DontUseNativeDialog)
            if not folder:
                return

            folder = Path(folder)
            for file in selection:
                parser = file_io.db_parser(file)
                file_io.db_writer.export_full_xlsx(
                    parser, (folder / Path(file).stem).with_suffix(".xlsx")
                )

        else:
            default_name = (Path.home() / selection[0]).with_suffix(".xlsx")
            new_path, _ = QFileDialog.getSaveFileName(
                None, "Export Spectrogram", str(default_name), "Excel Workbook (*.xlsx)", options=QFileDialog.Option.DontUseNativeDialog
            )

            if not new_path:
                return

            parser = file_io.db_parser(selection[0])
            file_io.db_writer.export_full_xlsx(
                parser,
                Path(new_path).with_suffix(".xlsx"),
                include_spectrogram_data=self.include_roi_check.isChecked(),
            )


class ROIsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "ROIs", "Regions"], [150, 50, 400])
        self.btn_load.clicked.connect(self.load)
        self.table.table.cellDoubleClicked.connect(self.load)
        export_xml = self.export_menu.addAction("LuRaCs ROI File (*.xml)")
        export_xml.triggered.connect(
            lambda: self.export_same("LuRaCs ROI file (*.xml)")
        )
        
        self.btn_info.clicked.connect(self.edit)
        self.delete_fn = IOManager.FileIndex.roi_index.delete_file

    def set_table(self):
        self.table.reset_table(["Name", "ROIs", "Regions"], [150, 50, 400])
        for key, parser in IOManager.FileIndex.roi_index.get_index().items():
            rois = parser.get_rois()
            regions = ", ".join([str([round(rb) for rb in r.roi_bound]) for r in rois])
            self.table.write_row(key, [Path(key).stem, len(rois), str(regions)])

    def load(self):
        selection = self._get_selection()
        if selection is None:
            return

        for file in selection:
            parser = io_dispatcher(file)
            rois = parser.get_rois()
            for peak in rois:
                extented_kwargs = {
                    **peak.__dict__,
                    **peak.meta,
                }
                del extented_kwargs["tag"]
                SpectrumManager.ROIManager.add_roi(*peak.roi_bound, **extented_kwargs)
                
    def edit(self):
        selection = self._get_selection()
        if not selection:
            return
        
        if len(selection) > 1:
            QMessageBox.warning(self, "Selection error", "Only one item may be edited simultaneously")
            return
        

        edit_dialog = ROIDialog(io_dispatcher(selection[0]).get_rois(), Path(selection[0]).stem)

        res = edit_dialog.exec()
        
        if res != ROIDialog.Accepted:
            return

        IOManager.FileIndex.roi_index.delete_file(selection[0])

        dummy_spectrum = Spectrum(1, edit_dialog.get_name())
        
        for roi in edit_dialog.get_rois():
            dummy_spectrum.set_roi(roi)

        IOManager.FileIndex.roi_index.save_file(dummy_spectrum)
        
        self.set_table()
        
        print(IOManager.FileIndex.roi_index.index_registry.keys())


class InstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent,
            ["Name", "Model", "Calibration", "Resolution", "Efficiency"],
            [200, 200, 100, 100, 100]
        )
        self.btn_load.setText("New")
        self.btn_load.clicked.connect(self.new)
        self.btn_info.clicked.connect(self.edit)
        export_instrument = self.export_menu.addAction("LuRaCs Instrument File (*.xml)")
        export_instrument.triggered.connect(
            lambda: self.export_same("LuRaCs Instrument File (*.xml)")
        )
        SpectrumManager.Signals.newInstrumentLoaded.connect(self.new_instrument_from_spectrum)
        self.delete_fn = SpectrumManager.UniqueInstrumentLibrary.remove_instrument
        
    def set_table(self):
        for key, instr in SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items():
            calibration = str(instr.calibration_coefficients is not None)
            resolution = str(instr.resolution_fn is not None)
            efficiency = str(instr.int_efficiency_fn is not None)
            # response_matrix = str(instr.response_matrix is not None)

            name = getattr(instr, "name", "Generic")

            self.table.write_row(
                key,
                [name, instr.model, calibration, resolution, efficiency,],
            )
            
            
    def edit(self):
        selection = self._get_selection()
        if not selection:
            return
        
        if len(selection) > 1:
            QMessageBox.warning(self, "Selection error", "Only one item may be edited simultaneously")
            return
        
        edit_dialog = InstrumentDialog(**SpectrumManager.UniqueInstrumentLibrary.instrument_registry[selection[0]].__dict__)
        res = edit_dialog.exec()
        
        if res != InstrumentDialog.Accepted:
            return
        
        
        self.table.delete_row(selection[0])
        
        new_instrument_dict = {}
        
        dialog_data, base_instrument_key = edit_dialog.get_data()
        

        base_instrument_dict = SpectrumManager.GenericInstrumentLibrary.instrument_registry[selection[0]].__dict__
        new_instrument_dict.update(base_instrument_dict)

        new_instrument_dict.update(dialog_data)
        new_instrument_dict["detector_type"] = "Gamma Spectrometer"
        
        new_instrument = UniqueInstrument(**new_instrument_dict)
        
        existing_instrument_key = SpectrumManager.UniqueInstrumentLibrary.get_key_from_attr("name", new_instrument.name)
        if existing_instrument_key is None:
            SpectrumManager.UniqueInstrumentLibrary.add_instrument(new_instrument)
        else:
            SpectrumManager.UniqueInstrumentLibrary.rename_instrument(selection[0], new_instrument)
        self.set_table()
        
    def new(self):
        edit_dialog = InstrumentDialog()
        res = edit_dialog.exec()
        if res != InstrumentDialog.Accepted:
            return
        
        new_instrument_dict = {}
        
        dialog_data, base_instrument_key = edit_dialog.get_data()
        
        if base_instrument_key is not None:
            base_instrument_dict = SpectrumManager.GenericInstrumentLibrary.instrument_registry[base_instrument_key].__dict__
            new_instrument_dict.update(base_instrument_dict)

        new_instrument_dict.update(dialog_data)
        new_instrument_dict["detector_type"] = "Gamma Spectrometer"
        
        new_instrument = UniqueInstrument(**new_instrument_dict)
        
        existing_instrument_key = SpectrumManager.UniqueInstrumentLibrary.get_key_from_attr("name", new_instrument.name)
        if existing_instrument_key is None:
            SpectrumManager.UniqueInstrumentLibrary.add_instrument(new_instrument)
        else:
            SpectrumManager.UniqueInstrumentLibrary.rename_instrument(existing_instrument_key, new_instrument)
        self.set_table()
        
    def new_instrument_from_spectrum(self, instrument_file_path):
        QMessageBox.information(self, "New Instrument Loaded", f"A new instrument was found when loading a spectrum\nInstrument '{SpectrumManager.UniqueInstrumentLibrary.instrument_registry[instrument_file_path].name}' has been added")
        
        

class GenericInstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(
            parent, ["Model", "Resolution", "Efficiency"], [200, 100, 100]
        )
        self.btn_load.setText("New")
        self.btn_load.clicked.connect(self.new)
        self.btn_info.clicked.connect(self.edit)
        
        export_instrument = self.export_menu.addAction("LuRaCs Instrument File (*.xml)")
        export_instrument.triggered.connect(
            lambda: self.export_same("LuRaCs Instrument File (*.xml)")
        )
        
        self.delete_fn = SpectrumManager.GenericInstrumentLibrary.remove_instrument
        
    def set_table(self):
        for key, instr in SpectrumManager.GenericInstrumentLibrary.instrument_registry.items():

            resolution = str(instr.resolution_fn is not None)
            efficiency = str(instr.int_efficiency_fn is not None)
            response_matrix = str(instr.response_matrix is not None)

            self.table.write_row(
                key,
                [instr.model, resolution, efficiency,],
            )
            
    def edit(self):
        selection = self._get_selection()
        if not selection:
            return
        
        if len(selection) > 1:
            QMessageBox.warning(self, "Selection error", "Only one item may be edited simultaneously")
            return
        
        edit_dialog = InstrumentDialog(**SpectrumManager.GenericInstrumentLibrary.instrument_registry[selection[0]].__dict__)
        edit_dialog.name_input.setEnabled(False)
        res = edit_dialog.exec()
        
        if res != InstrumentDialog.Accepted:
            return
        
        self.table.delete_row(selection[0])
        
        new_instrument_dict = {}
        
        dialog_data, _ = edit_dialog.get_data()
        del dialog_data["name"]
        

        base_instrument_dict = SpectrumManager.GenericInstrumentLibrary.instrument_registry[selection[0]].__dict__
        new_instrument_dict.update(base_instrument_dict)

        new_instrument_dict.update(dialog_data)
        new_instrument_dict["detector_type"] = "Gamma Spectrometer"
        
        assert "name" not in new_instrument_dict
        new_instrument = GenericInstrument(**new_instrument_dict)

        existing_instrument_key = SpectrumManager.GenericInstrumentLibrary.get_key_from_attr("model", new_instrument.model)
        if existing_instrument_key is None:
            SpectrumManager.GenericInstrumentLibrary.add_instrument(new_instrument)
        else:
            print("Renaming instrument", selection[0])
            SpectrumManager.GenericInstrumentLibrary.rename_instrument(selection[0], new_instrument)
        self.set_table()
        
    def new(self):
        edit_dialog = InstrumentDialog()
        edit_dialog.name_input.setEnabled(False)
        res = edit_dialog.exec()
        if res != InstrumentDialog.Accepted:
            return
        
        new_instrument_dict = {}
        
        dialog_data, base_instrument_key = edit_dialog.get_data()
        del dialog_data["name"]
        
        if base_instrument_key is not None:
            base_instrument_dict = SpectrumManager.GenericInstrumentLibrary.instrument_registry[base_instrument_key].__dict__
            new_instrument_dict.update(base_instrument_dict)

        new_instrument_dict.update(dialog_data)
        new_instrument_dict["detector_type"] = "Gamma Spectrometer"
        
        assert "name" not in new_instrument_dict
        new_instrument = GenericInstrument(**new_instrument_dict)

        existing_instrument_key = SpectrumManager.GenericInstrumentLibrary.get_key_from_attr("model", new_instrument.model)
        if existing_instrument_key is None:
            SpectrumManager.GenericInstrumentLibrary.add_instrument(new_instrument)
        else:
            SpectrumManager.GenericInstrumentLibrary.rename_instrument(existing_instrument_key, new_instrument)
        self.set_table()
