from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum, SpectrumData
    from containers.spectrum_classes import Spectrum
    from gui.popup_windows.data_store import DataLibrary

from datetime import timedelta
from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QTableWidget,
    QTableWidgetItem,
    QSizePolicy,
    QColorDialog,
    QFrame,
    QHBoxLayout,
    QToolButton,
    QMenu,
)
from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor


from core import SpectrumManager, RunManager
from gui.misc.idx_table import StrIdxTable
from utils.file_io import io_dispatcher


from PySide6.QtWidgets import QWidget
from PySide6.QtGui import QColor, QPainter, QBrush, QAction
from PySide6.QtCore import Qt, Signal

from gui.import_export import save_spectrum_to_library
from gui.popup_windows.data_store_edit_dialogs import SpectrumEditDialog


class ColorCellWidget(QWidget):
    """Clean color swatch for QTableWidget cells."""

    clicked = Signal()

    def __init__(self, color: QColor, parent=None):
        super().__init__(parent)
        self.color = color
        self.setFixedWidth(30)
        self.setFixedHeight(20)
        self.setToolTip("Click to change color")

    def set_color(self, color: QColor):
        self.color = color
        self.update()  # repaint

    def mousePressEvent(self, event):
        if event.button() == Qt.LeftButton:
            self.clicked.emit()
        super().mousePressEvent(event)

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect().adjusted(1, 1, -1, -1)

        # Fill with color
        painter.setBrush(QBrush(self.color))
        painter.setPen(Qt.NoPen)  # no border
        painter.drawRoundedRect(rect, 3, 3)

    def get_color(self):
        color = QColorDialog.getColor(self.color, self, "Select color")
        if color.isValid():
            self.set_color(color)
            return color


class MenuButton(QWidget):
    def __init__(self, title="Menu", parent=None):
        super().__init__(parent)
        self.parent = parent

        self.button = QToolButton(self)
        self.button.setText(title)
        self.button.setPopupMode(QToolButton.InstantPopup)
        self.button.setToolButtonStyle(self.button.toolButtonStyle())

        self.menu = QMenu(self)
        self.button.setMenu(self.menu)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.button)

    def add_action(self, text: str) -> QAction:
        action = QAction(text, self)
        self.menu.addAction(action)
        return action


class SpectrumInfoTab(QWidget):
    sigColorChanged = Signal(str, str, QColor)
    sigDisconnectAndRemove = Signal(str, bool)
    sigRemoveSpectrum = Signal(str)
    sigToggleVisibility = Signal(str)
    sigUpdateSpectrumInstrument = Signal(str, object)

    # spectrum_name, "foreground"/"background", color

    def __init__(self, main_window, title="", parent=None, ):
        super().__init__(parent)
        self.parent = parent
        self.main_window = main_window

        # Incoming signals from SpectrumManager
        SpectrumManager.Signals.spectrumUpdated.connect(self.recieve_update)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_spectrum)
        SpectrumManager.Signals.backgroundRemoved.connect(self.remove_spectrum)

        # Outgoing signals to SpectrumManager and RunManager
        self.sigRemoveSpectrum.connect(SpectrumManager.remove_spectrum)
        self.sigToggleVisibility.connect(SpectrumManager.update_visibility)
        self.sigColorChanged.connect(SpectrumManager.set_color)
        self.sigUpdateSpectrumInstrument.connect(SpectrumManager.set_spectrum_instrument)
        self.sigDisconnectAndRemove.connect(RunManager.remove_device)

        self.group_box = QGroupBox(title)

        titles = [
            "Spectrum",
            "Type",
            "Counts",
            "Live Time",
            "Real Time",
            "Calibrated",
            "Instrument",
        ]
        self.table = StrIdxTable(
            columns=titles,
            column_widths=[65, 150, 150, 150, 100, 100, 100, 200],
            has_menu_button=True,
        )

        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table.table)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group_box)

        self.hide_show_btn = {}
        self.hide_show_states = {}
        
    def edit_spectrum(self, spectrum: Spectrum):
        spectrum_index = self.main_window.data_store.spectrum_tab.file_index
        connected = True if spectrum.connection is not None else False
        dialog = SpectrumEditDialog(spectrum=spectrum, spectrum_is_connected=connected, spectrum_index=spectrum_index)
        res = dialog.exec()
        
        if res != SpectrumEditDialog.Accepted:
            return
        
        data = dialog.get_data()
        if data["flag_can_change_name"] and data["name"] not in SpectrumManager.spectra:
            SpectrumManager.rename_spectrum(spectrum.name, data["name"])
        
        if data["background_pth"] and not data["flag_clear_bkg"] and data["flag_change_bkg"]:
            SpectrumManager.import_spectrum_as_background(data["name"], io_dispatcher(data["background_pth"]).data)
            
        elif data["flag_clear_bkg"]:
            SpectrumManager.clear_background(data["name"])
            
        if data["remark"]:
            SpectrumManager.spectra[data["name"]].remark = data["remark"]
            
        if data["instrument"] is not None:
            self.sigUpdateSpectrumInstrument.emit(data["name"], data["instrument"])

    def build_menu_button(
        self, spectrum: Spectrum, role: str, color: QColor, parent=None
    ) -> MenuButton:
        menu_button = MenuButton(parent=parent, title="...")
        color_widget = ColorCellWidget(color)
        if (
            role == "foreground" and spectrum.connection is not None
        ):  # Has connected device
            disconnect = menu_button.add_action("Remove and Disconnect")
            disconnect.triggered.connect(
                lambda x: RunManager.remove_device(spectrum.name, True)
            )
            
            save = menu_button.add_action("Save")
            save.triggered.connect(lambda : save_spectrum_to_library(spectrum))

            # add_bkg = menu_button.add_action("Add Background")
            # add_bkg.triggered.connect(
            #     lambda : parent.file_import_export.load_spectrum_as_background(
            #         spectrum.name
            #     )
            # )

            self.hide_show_btn[spectrum.name] = menu_button.add_action("Hide")
            self.hide_show_btn[spectrum.name].triggered.connect(
                lambda : self._show_hide_action(spectrum.name)
            )
            
            edit_action = menu_button.add_action("Edit")
            edit_action.triggered.connect(lambda : self.edit_spectrum(spectrum))

        elif role == "foreground":
            remove = menu_button.add_action("Remove Spectrum")
            remove.triggered.connect(
                lambda x: SpectrumManager.remove_spectrum(spectrum.name)
            )
            
            save = menu_button.add_action("Save")
            save.triggered.connect(lambda : save_spectrum_to_library(spectrum))
                        
            # add_bkg = menu_button.add_action("Add Background")
            # add_bkg.triggered.connect(
            #     lambda : parent.file_import_export.load_spectrum_as_background(
            #         spectrum.name
            #     )
            # )

            self.hide_show_btn[spectrum.name] = menu_button.add_action("Hide")
            self.hide_show_btn[spectrum.name].triggered.connect(
                lambda : self._show_hide_action(spectrum.name)
            )
            
            edit_action = menu_button.add_action("Edit")
            edit_action.triggered.connect(lambda : self.edit_spectrum(spectrum))

        else:  # Background
            rm_bkg = menu_button.add_action("Remove Background")
            rm_bkg.triggered.connect(
                lambda : SpectrumManager.clear_background(spectrum.name)
            )

        # export_btn = menu_button.add_action("Export")
        # export_btn.triggered.connect(lambda _:parent.file_import_export.export_spectrum(spectrum.name))

        
        

        wrapper = QWidget()
        layout = QHBoxLayout(wrapper)
        layout.setContentsMargins(0, 0, 0, 0)  # no extra space
        layout.addWidget(color_widget)
        layout.addWidget(menu_button)
        layout.addStretch()
        wrapper.setLayout(layout)

        # Connect click
        color_widget.clicked.connect(
            lambda cw=color_widget: self.open_color_dialog(cw, spectrum.name, role)
        )

        return wrapper

    # ----------------- Options cell helpers -----------------
    def _show_hide_action(self, name):
        spect = SpectrumManager.get_spectrum(name)
        if spect.show_in_plot:
            self.hide_show_btn[name].setText("Show")
            self.sigToggleVisibility.emit(name)
        else:
            self.hide_show_btn[name].setText("Hide")
            self.sigToggleVisibility.emit(name)

    def open_color_dialog(
        self, cell_widget: ColorCellWidget, spectrum_name: str, role: str
    ):
        color = QColorDialog.getColor(cell_widget.color, self, "Select color")
        if color.isValid():
            cell_widget.set_color(color)
            self.sigColorChanged.emit(spectrum_name, role, color)

    # ----------------- Table update -----------------
    def format_large_int(self, value: int) -> str:
        return f"{int(value):,}".replace(",", " ")

    def recieve_update(self, name):
        new_spect = SpectrumManager.get_spectrum(name)
        
        # Get instrument
        instr = new_spect.instrument.name if new_spect.instrument is not None else "None"

        # --- Foreground ---
        if name + "f" not in self.table.get_all_keys():
            foreground_menu_button = self.build_menu_button(
                new_spect, "foreground", new_spect.color_foreground, parent=self.parent
            )
        else:
            foreground_menu_button = None
            
        live_time = (
            timedelta(seconds=round(new_spect.foreground.live_time))
            if new_spect.foreground.live_time is not None
            else "None"
        )
        
        real_time = (
            timedelta(seconds=round(new_spect.foreground.real_time))
            if new_spect.foreground.real_time is not None
            else "None"
        )
        self.table.write_row(
            name + "f",
            [
                name,
                "Foreground",
                self.format_large_int(new_spect.foreground.total_counts),
                live_time,
                real_time,
                str(new_spect.calibrated),
                instr
            ],
            menu_button=foreground_menu_button,
        )

        # --- Background ---
        if name + "b" not in self.table.get_all_keys():
            background_menu_button = self.build_menu_button(
                new_spect, "background", new_spect.color_background, parent=self.parent
            )
        else:
            background_menu_button = None
        if new_spect.background is not None:
            live_time = (
                timedelta(seconds=round(new_spect.background.live_time))
                if new_spect.background.live_time is not None
                else "None"
            )
            real_time = (
                timedelta(seconds=round(new_spect.background.real_time))
                if new_spect.background.real_time is not None
                else "None"
            )

            self.table.write_row(
                name + "b",
                [
                    name,
                    "Background",
                    self.format_large_int(new_spect.background.total_counts),
                    live_time,
                    real_time,
                    str(new_spect.calibrated),
                    instr,
                ],
                menu_button=background_menu_button,
            )

    def remove_spectrum(self, name, background=False):
        if background:
            self.table.delete_row(name + "b")
            self.hide_show_btn.pop(name + "b", None)
        else:
            self.table.delete_row(name + "f")
            self.hide_show_btn.pop(name + "f", None)
            if self.table.get_index_from_key(name + "b") is not None:
                self.table.delete_row(name + "b")
                self.hide_show_btn.pop(name + "b", None)
