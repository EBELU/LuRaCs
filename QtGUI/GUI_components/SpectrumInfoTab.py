from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QSizePolicy, QColorDialog, QFrame, QHBoxLayout, QToolButton, QMenu
)
from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor

from ..SpectrumClasses import ROI
from ..core import SpectrumManager, RunManager


def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))


def format_duration(seconds):
    if seconds is None:
        return
    seconds = int(seconds)
    days, seconds = divmod(seconds, 86400)
    hours, seconds = divmod(seconds, 3600)
    minutes, seconds = divmod(seconds, 60)
    if days > 0:
        return f"{days:02d}d {hours:02d}:{minutes:02d}:{seconds:02d}"
    else:
        return f"{hours:02d}:{minutes:02d}:{seconds:02d}"
    


from PySide6.QtWidgets import QWidget
from PySide6.QtGui import QColor, QPainter, QBrush, QAction
from PySide6.QtCore import Qt, Signal

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

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.setRenderHint(QPainter.Antialiasing)
        rect = self.rect().adjusted(1, 1, -1, -1)

        # Fill with color
        painter.setBrush(QBrush(self.color))
        painter.setPen(Qt.NoPen)  # no border
        painter.drawRoundedRect(rect, 3, 3)


class MenuButton(QWidget):
    # Optional: custom signals
    actionTriggered = Signal(str)

    def __init__(self, title="Menu", parent=None):
        super().__init__(parent)
        self.parent = parent

        self.button = QToolButton(self)
        self.button.setText(title)
        self.button.setPopupMode(QToolButton.InstantPopup)
        self.button.setToolButtonStyle(
            self.button.toolButtonStyle()
        )

        self.menu = QMenu(self)
        self.button.setMenu(self.menu)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.button)

    def add_action(self, text):
        action = QAction(text, self)
        self.menu.addAction(action)

        action.triggered.connect(
            lambda: self.actionTriggered.emit(text)
        )
        return action

    def add_separator(self):
        self.menu.addSeparator()
        
        

class SpectrumInfoPane(QWidget):

    colorChanged = Signal(str, str, QColor)
    disconnectAndRemove = Signal(str, bool)
    removeSpectrum = Signal(str)
    toggleVisibility = Signal(str)
    
    # spectrum_name, "foreground"/"background", color

    def __init__(self, title="", parent=None):
        super().__init__(parent)
        self.parent = parent

        SpectrumManager.Signals.spectrumUpdated.connect(self.recieve_update)
        SpectrumManager.Signals.spectrumRemoved.connect(self.remove_spectrum)
        SpectrumManager.Signals.backgroundRemoved.connect(self.remove_spectrum)

        self.removeSpectrum.connect(SpectrumManager.remove_spectrum)
        self.toggleVisibility.connect(SpectrumManager.update_visibility)
        
        self.colorChanged.connect(SpectrumManager.set_color)
        self.disconnectAndRemove.connect(RunManager.remove_device)

        self.group_box = QGroupBox(title)

        titles = ["", "Spectrum", "Type", "Counts", "Live Time", "Real Time", "Calibrated"]
        self.table = QTableWidget(0, len(titles))
        self.table.setMinimumHeight(50)
        self.table.setHorizontalHeaderLabels(titles)
        self.table.setColumnWidth(0, 60)
        self.table.setColumnWidth(3, 100)
        self.table.setColumnWidth(4, 150)
        self.table.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.MinimumExpanding)

        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group_box)

        self.saved_rows = {}   # spectrum_name -> list of row indices
        self.row_counter = 0
        
        self.hide_show_btn = {}
        self.hide_show_states = {}



    # ----------------- Options cell helpers -----------------
    def _show_hide_action(self, name):
        spect = SpectrumManager.get_spectrum(name)
        if spect.show_in_plot:
            self.hide_show_btn[name].setText("Show")
            self.toggleVisibility.emit(name)
        else:
            self.hide_show_btn[name].setText("Hide")
            self.toggleVisibility.emit(name)

    def set_options_cell(self, row: int, spectrum_name: str, role: str, color: QColor):
        # Create the color swatch
        cell_widget = ColorCellWidget(color)
        menu_button = MenuButton()
        if role == "foreground" and SpectrumManager.get_spectrum(spectrum_name).connected_device is not None:
            disconnect = menu_button.add_action("Remove and Disconnect")
            disconnect.triggered.connect(lambda x: self.disconnectAndRemove.emit(spectrum_name, True))
            
            add_bkg = menu_button.add_action("Add Background")
            
            self.hide_show_btn[spectrum_name] = menu_button.add_action("Hide")
            self.hide_show_btn[spectrum_name].triggered.connect(lambda x: self._show_hide_action(spectrum_name))
                
            
        elif role == "foreground":
            remove = menu_button.add_action("Remove Spectrum")
            remove.triggered.connect(lambda x: self.removeSpectrum.emit(spectrum_name))
            add_bkg = menu_button.add_action("Add Background")
            
            self.hide_show_btn[spectrum_name] = menu_button.add_action("Hide")
            self.hide_show_btn[spectrum_name].triggered.connect(lambda x: self._show_hide_action(spectrum_name))
            
        else:
            rm_bkg = menu_button.add_action("Remove Background")
            rm_bkg.triggered.connect(lambda x: SpectrumManager.clear_background(spectrum_name))
        

        export_btn = menu_button.add_action("Export")
        export_btn.triggered.connect(self.parent.file_import_export.export_file)
        
        menu_button.add_action("Info")

        # Wrap it in a QWidget with a layout to center it
        wrapper = QWidget()
        layout = QHBoxLayout(wrapper)
        layout.setContentsMargins(0, 0, 0, 0)  # no extra space
        layout.addStretch()
        layout.addWidget(cell_widget)
        layout.addWidget(menu_button)
        layout.addStretch()
        wrapper.setLayout(layout)
        

        # Connect click
        cell_widget.clicked.connect(
            lambda cw=cell_widget: self.open_color_dialog(cw, spectrum_name, role)
        )

        self.table.setCellWidget(row, 0, wrapper)

    def open_color_dialog(self, cell_widget: ColorCellWidget, spectrum_name: str, role: str):
        color = QColorDialog.getColor(cell_widget.color, self, "Select color")
        if color.isValid():
            cell_widget.set_color(color)
            self.colorChanged.emit(spectrum_name, role, color)

    # ----------------- Table update -----------------
    def recieve_update(self, name):
        new_spect = SpectrumManager.get_spectrum(name)
        # Determine rows to insert
        if name not in self.saved_rows:
            rows = []

            # Foreground row
            self.table.insertRow(self.row_counter)
            fg_row = self.row_counter
            rows.append(fg_row)
            self.row_counter += 1

            # Background row (if exists)
            if new_spect.get_background() is not None:
                self.table.insertRow(self.row_counter)
                bkg_row = self.row_counter
                rows.append(bkg_row)
                self.row_counter += 1

            self.saved_rows[name] = rows

        indicies = self.saved_rows[name]

        # ----------------- Foreground -----------------
        fg_row = indicies[0]
        fg = new_spect.foreground

        # Update color widget if it already exists
        cell_widget_wrapper = self.table.cellWidget(fg_row, 0)
        if cell_widget_wrapper:
            cell_widget = cell_widget_wrapper.layout().itemAt(1).widget()  # center widget
            cell_widget.set_color(new_spect.color_foreground)
        else:
            self.set_options_cell(fg_row, name, "foreground", new_spect.color_foreground)
        
        write_row(self.table, fg_row, [
            "",
            new_spect.name,
            "Foreground",
            f"{int(fg.total_counts):,}".replace(",", " "),
            format_duration(fg.live_time),
            format_duration(fg.real_time),
            new_spect.calibrated,
            #None if not new_spect.calibrated else ",".join([f"k{i}={coeff}" for i, coeff in enumerate(reversed(new_spect.calibration_coefficients))])
        ])

        # ----------------- Background -----------------
        if new_spect.get_background() is not None:
            bkg_row = indicies[1] if len(indicies) > 1 else None
            bkg = new_spect.background

            if bkg_row is None:
                # Insert row dynamically if not yet created
                self.table.insertRow(self.row_counter)
                bkg_row = self.row_counter
                self.row_counter += 1
                indicies.append(bkg_row)
                self.saved_rows[name] = indicies

            # Update color widget if exists
            cell_widget_wrapper = self.table.cellWidget(bkg_row, 0)
            if cell_widget_wrapper:
                cell_widget = cell_widget_wrapper.layout().itemAt(1).widget()
                cell_widget.set_color(new_spect.color_background)
            else:
                self.set_options_cell(bkg_row, name, "background", new_spect.color_background)

            write_row(self.table, bkg_row, [
                "",
                new_spect.name,
                "Background",
                f"{int(bkg.total_counts):,}".replace(",", " "),
                format_duration(bkg.live_time),
                format_duration(bkg.real_time),
            ])
    
    def remove_spectrum(self, name: str, which:str = "both"):
        """Remove an entire spectrum or just the background from the info tab."""
        rows = self.saved_rows[name]

        if which == "both":
            shift = len(rows)
            for i in reversed(rows): # Remove in reverse order, otherwise the background gets shifted to the foregrounds index
                self.table.removeRow(i)
            
            for key, rows_pair in self.saved_rows.items(): # Shift down the indicies of other rows
                new_pair = [i - shift for i in rows_pair if i > rows[-1]]
                if len(new_pair):
                    self.saved_rows[key] = new_pair
            
            self.row_counter -= shift # update the counter on where the top is
            self.saved_rows.pop(name)
            
        elif which == "bkg":
            shift = 1
            self.table.removeRow(rows[-1])
            self.saved_rows[name] = [rows[0]]
            
            for key, rows_pair in self.saved_rows.items(): # Shift down the indicies of other rows
                new_pair = [i - shift for i in rows_pair if i > rows[-1]]
                if len(new_pair):
                    self.saved_rows[key] = new_pair
            
            self.row_counter -= shift
            
                        