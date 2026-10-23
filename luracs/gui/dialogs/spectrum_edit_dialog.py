
from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum


from datetime import timedelta

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QAbstractItemView,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QHBoxLayout,
    QHeaderView,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMessageBox,
    QPushButton,
    QScrollArea,
    QTableWidget,
    QTableWidgetItem,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from luracs.core import IOManager, Settings, SpectrumManager
from luracs.utils.color_rotator import ColorRotator
from luracs.utils.file_io import io_dispatcher, xml_parser


class _TableWidget(QTableWidget):
    def wheelEvent(self, event):
        super().wheelEvent(event)
        event.accept()


class SpectrumEditDialog(QDialog):
    def __init__(
        self,
        parent=None,
        spectrum: Spectrum = None,
        spectrum_is_connected: bool = False,
    ):
        super().__init__(parent)
        self.setWindowTitle("Spectrum Edit Dialog")
        self.resize(600, 600)

        main_layout = QVBoxLayout(self)

        # Scroll area
        scroll_area = QScrollArea()
        scroll_area.setWidgetResizable(True)

        scroll_content = QWidget()
        form_layout = QFormLayout(scroll_content)

        scroll_area.setWidget(scroll_content)

        main_layout.addWidget(scroll_area)

        # Name
        self.name_input = QLineEdit()
        self.name_input.setEnabled(not spectrum_is_connected)
        self.name_input.setText(spectrum.name if spectrum else "")
        form_layout.addRow("Spectrum Name", self.name_input)

        # Remark
        self.remark_input = QTextEdit()
        self.remark_input.setMinimumHeight(100)
        self.remark_input.setPlainText(spectrum.remark if spectrum else "")
        form_layout.addRow("Remark", self.remark_input)

        self.calibration_text = QTextEdit()
        self.calibration_text.setMinimumHeight(75)
        self.calibration_text.setReadOnly(True)
        if spectrum is not None and spectrum.calibration_coefficients is not None:
            calib_str = "\n".join(
                [
                    f"a{i} = {a}"
                    for i, a in enumerate(reversed(spectrum.calibration_coefficients))
                ]
            )
        else:
            calib_str = ""
        self.calibration_text.setText(calib_str)
        form_layout.addRow("Calibration", self.calibration_text)

        # --- Background ---
        # Background name
        self.background_line = QLineEdit()
        self.background_line.setText(
            spectrum.background.spectrum_name
            if spectrum and spectrum.background is not None
            else ""
        )
        self.background_line.setReadOnly(True)
        form_layout.addRow("Background", self.background_line)
        
        
        # --- Preview plot ---
        self.spectrum_plot_widget = pg.PlotWidget()
        self.spectrum_plot_widget.setMinimumHeight(150)
        self.spectrum_plot_widget.setMaximumHeight(200)
        # self.spectrum_plot_widget.getPlotItem().hideAxis("left")
        self.spectrum_plot_widget.showGrid(x=True, y=True, alpha=0.2)
        self.spectrum_plot_widget.enableAutoRange(axis=pg.ViewBox.XYAxes, enable=True)
        self.spectrum_plot_widget.setXRange(0, 1000, padding=0)
        
        color_rotator = ColorRotator(Settings.Appearance.color_rotator_scheme)
        fg_color = color_rotator.next_color()
        self.fg_lines = self.spectrum_plot_widget.plot(
                        [],
                        [],
                        name=spectrum.name,
                        pen=pg.mkPen(fg_color),
                        brush=pg.mkBrush(fg_color),
                        fillLevel=0,
                        stepMode="right",
                        antialias=False,
                    )
        
        bg_color = color_rotator.next_color()
        self.bg_lines = self.spectrum_plot_widget.plot(
                        [],
                        [],
                        name=spectrum.name,
                        pen=pg.mkPen(bg_color),
                        brush=pg.mkBrush(bg_color),
                        fillLevel=0,
                        stepMode="right",
                        antialias=False,
                    )
        
        self.x_axis = spectrum.x_axis
        self.fg_lines.setData(spectrum.x_axis, spectrum.get_foreground(cps=True))
        
        if spectrum.background is not None:
            self.bg_lines.setData(spectrum.x_axis, spectrum.get_background(cps=True))


        self.spectrum_plot_widget.setLimits(
            xMin=-1,
            xMax=Settings.Appearance.spectrum_plot_E_max,
            yMin=1e-9,
            yMax=np.max(spectrum.get_foreground(cps=True)*1.15),
            minXRange=5,
            maxXRange=1e4,
            minYRange=1e-4,
            maxYRange=1e16,
        )
        self.spectrum_plot_widget.getPlotItem().layout.setContentsMargins(2, 13, 13, 5)
        
        form_layout.addRow("", self.spectrum_plot_widget)

        # Buttons
        bkg_btns = QHBoxLayout()

        bkg_btn_select = QPushButton("Select")
        bkg_btn_select.clicked.connect(self.select_background)
        bkg_btn_import = QPushButton("Import")
        bkg_btn_import.clicked.connect(self.import_background)
        bkg_btn_clear = QPushButton("Clear Bkg")
        bkg_btn_clear.clicked.connect(self.clear_bkg)

        bkg_btns.addWidget(bkg_btn_select)
        bkg_btns.addWidget(bkg_btn_import)
        bkg_btns.addWidget(bkg_btn_clear)

        form_layout.addRow("", bkg_btns)

        # Internal spectra list
        self.spectrum_list = _TableWidget(0, 3)
        self.spectrum_list.setMinimumHeight(150)
        
        # Columns titles
        self.spectrum_list.setHorizontalHeaderLabels(
            ["Spectrum", "Live Time", "Detector"]
        )
        self.spectrum_list.horizontalHeader().setSectionResizeMode(
            QHeaderView.ResizeMode.ResizeToContents
        )
        self.spectrum_list.horizontalHeader().setStretchLastSection(True)
        
        # Row vertical size
        self.spectrum_list.verticalHeader().setSectionResizeMode(
            QHeaderView.ResizeMode.ResizeToContents
        )
        
        # Selection options
        self.spectrum_list.setSelectionBehavior(
            QAbstractItemView.SelectionBehavior.SelectRows
        )

        self.spectrum_list.setSelectionMode(
            QAbstractItemView.SelectionMode.SingleSelection
        )
        
        # Disable editing
        self.spectrum_list.setEditTriggers(
            QAbstractItemView.EditTrigger.NoEditTriggers
        )
        
        self.spectrum_list.cellDoubleClicked.connect(
            self.select_background
        )


        form_layout.addRow("", self.spectrum_list)

        for path, parser in sorted(
            IOManager.FileIndex.spectrum_index.get_index().items(),
            key=lambda item: item[0],
        ):
            row = self.spectrum_list.rowCount()
            self.spectrum_list.insertRow(row)

            # Spectrum name
            name_item = QTableWidgetItem(str(parser.data["name"]))
            name_item.setData(Qt.ItemDataRole.UserRole, path)
            self.spectrum_list.setItem(row, 0, name_item)

            # Live Time
            live_time = parser.get_foreground_spectrum().live_time
            if not live_time:
                live_time = parser.get_foreground_spectrum().real_time
                
            self.spectrum_list.setItem(
                row, 1, QTableWidgetItem(str(timedelta(seconds=round(live_time))))
            )

            # Instrument
            instrument_name = parser.get_header().instrument_id
            if not instrument_name or instrument_name == "Unknown":
                instrument = parser.get_instrument()
                instrument_name = instrument.name if instrument is not None else ""
            self.spectrum_list.setItem(
                row, 2, QTableWidgetItem(str(instrument_name))
            )

        # --- Instrument ---
        # Name
        self.instrument_line = QLineEdit()
        self.instrument_line.setReadOnly(True)
        form_layout.addRow("Instrument", self.instrument_line)

        # Buttons
        instrument_btns = QHBoxLayout()
        instrument_btn_select = QPushButton("Select")
        instrument_btn_select.clicked.connect(self.select_instrument)
        instrument_btn_clear = QPushButton("Clear Instrument")
        instrument_btn_clear.clicked.connect(self.clear_instrument)
        instrument_btns.addWidget(instrument_btn_select)
        instrument_btns.addWidget(instrument_btn_clear)
        form_layout.addRow("", instrument_btns)

        # Instrument list from instrument libraries
        self.instrument_list = QListWidget()
        self.instrument_list.setMinimumHeight(150)
        self.instrument_line.setText(
            spectrum.instrument.name if spectrum.instrument is not None else ""
        )
        self.instrument_list.itemDoubleClicked.connect(
            self.select_instrument
        )
        
        form_layout.addRow("", self.instrument_list)

        for path, instr in sorted(
            SpectrumManager.UniqueInstrumentLibrary.instrument_registry.items(),
            key=lambda item: item[0],
        ):
            item = QListWidgetItem(instr.name)
            item.setData(Qt.ItemDataRole.UserRole, instr)
            self.instrument_list.addItem(item)

        # --- Bottom buttons ---
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

        # --- Flags ---
        # These set the response of things handled outside of this dialog
        self.flag_clear_bkg = False
        self.flag_clear_instrument = False
        self.flag_change_bkg = False
        self.flag_change_instrument = False
        self.flag_can_change_name = not spectrum_is_connected  # A spectrum with a connection is not allowed to change its name, it would mess with receiving now data from the the connected device

        self.selected_background = None
        self.selected_instrument = None

    def get_data(self):
        """Collect data from the dialog and return as a dictionary."""
        # Flags denote bool values that prompts another part of the application to take action
        data = {
            "name": self.name_input.text(),
            "remark": self.remark_input.toPlainText(),
            "background_pth": self.selected_background,
            "instrument": self.selected_instrument,
            "flag_clear_bkg": self.flag_clear_bkg,
            "flag_clear_instrument": self.flag_clear_instrument,
            "flag_can_change_name": self.flag_can_change_name,
            "flag_change_bkg": self.flag_change_bkg,
            "flag_change_instrument": self.flag_change_instrument,
        }
        return data

    def select_background(self):
        "Select a background spectrum from the Data Library"
        row = self.spectrum_list.currentRow()
        item = self.spectrum_list.item(row, 0)
        if item is None:
            return
        
        name = item.text()
        path = item.data(Qt.ItemDataRole.UserRole)
        
        # Check the chosen spectrum
        spectrum = xml_parser(path).get_spectrum()
        if spectrum.channels != len(self.x_axis):
            QMessageBox.warning(self, "Error", f"Channels of foreground ({len(self.x_axis)}) does not match channels of selected background ({spectrum.channels})")
            return
        
        # Update shown data
        self.bg_lines.setData(self.x_axis, spectrum.get_foreground(cps=True))
        
        self.background_line.setText(name)
        self.selected_background = path
        self.flag_clear_bkg = False
        self.flag_change_bkg = True
        


    def import_background(self):
        "Import a background spectrum from an external file"
        path, _ = IOManager.Importer.import_file(
            IOManager.Importer.ImportFilters.SPECTRUM.value
        )

        # If the selection is cancelled
        if path is None:
            return
        
        # Load the spectrum for testing
        parser = io_dispatcher(path)
        
        if parser is None:
            return
        
        # Check the spectrum
        spectrum = parser.get_spectrum()
        
        if spectrum.channels != len(self.x_axis):
            QMessageBox.warning(self, "Error", f"Channels of foreground ({len(self.x_axis)}) does not match channels of imported background ({spectrum.channels})")
            return
        
        # Update info
        self.bg_lines.setData(self.x_axis, spectrum.get_foreground(cps=True))
        
        self.background_line.setText(path.name)
        self.selected_background = path
        self.flag_clear_bkg = False
        self.flag_change_bkg = True
        
    def clear_bkg(self):
        self.flag_clear_bkg = True
        self.background_line.setText("")
        self.bg_lines.setData([], [])

    def select_instrument(self):
        item = self.instrument_list.selectedItems()
        if len(item) > 0:
            item = item[0]
        else:
            # If none is selected then sod off
            return
        name = item.text()
        instr = item.data(Qt.ItemDataRole.UserRole)

        self.instrument_line.setText(name)
        self.selected_instrument = instr
        self.flag_clear_instrument = False
        self.flag_change_instrument = True

    def clear_instrument(self):
        self.flag_clear_instrument = True
        self.instrument_line.setText("")
        
if __name__ == "__main__":
    import sys

    from PySide6.QtWidgets import QApplication

    app = QApplication(sys.argv)
    dialog = SpectrumEditDialog()
    dialog.show()
    sys.exit(app.exec())