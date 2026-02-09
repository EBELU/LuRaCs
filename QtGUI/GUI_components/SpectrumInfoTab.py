from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QSizePolicy, QColorDialog, QFrame, QHBoxLayout
)
from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QColor

from ..SpectrumClasses import ROI
from ..Globals import SpectrumManager


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
from PySide6.QtGui import QColor, QPainter, QBrush, QPen
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




class SpectrumInfoPane(QWidget):

    colorChanged = Signal(str, str, QColor)

    
    # spectrum_name, "foreground"/"background", color

    def __init__(self, title="", parent=None):
        super().__init__(parent)

        SpectrumManager.Signals.spectrumUpdated.connect(self.recieve_update)

        self.group_box = QGroupBox(title)

        titles = ["", "Spectrum", "Type", "Counts", "Live Time", "Real Time"]
        self.table = QTableWidget(0, len(titles))
        self.table.setHorizontalHeaderLabels(titles)
        self.table.setColumnWidth(0, 40)
        self.table.setColumnWidth(3, 100)
        self.table.setColumnWidth(4, 150)
        self.table.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.MinimumExpanding)

        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group_box)

        self.saved_rows = {}   # spectrum_name -> list of row indices
        self.row_counter = 0


    # ----------------- Color cell helpers -----------------

    def set_color_cell(self, row: int, spectrum_name: str, role: str, color: QColor):
        # Create the color swatch
        cell_widget = ColorCellWidget(color)

        # Wrap it in a QWidget with a layout to center it
        wrapper = QWidget()
        layout = QHBoxLayout(wrapper)
        layout.setContentsMargins(0, 0, 0, 0)  # no extra space
        layout.addStretch()
        layout.addWidget(cell_widget)
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
            SpectrumManager.set_color(spectrum_name, role, color)
            self.colorChanged.emit(spectrum_name, role, color)

    # ----------------- Table update -----------------
    def recieve_update(self, name):
        new_spect = SpectrumManager.get_spectrum(name)

        # Determine rows to insert
        if name not in self.saved_rows:
            rows = []

            # Foreground row
            self.table.insertRow(self.row_counter)
            rows.append(self.row_counter)
            self.row_counter += 1

            # Background row (if exists)
            if new_spect.get_background() is not None:
                self.table.insertRow(self.row_counter)
                rows.append(self.row_counter)
                self.row_counter += 1

            self.saved_rows[name] = rows

        indicies = self.saved_rows[name]

        # ----------------- Foreground -----------------
        fg = new_spect.foreground
        fg_row = indicies[0]
        self.set_color_cell(fg_row, name, "foreground", new_spect.color_foreground)
        write_row(self.table, fg_row, [
            "",
            new_spect.name,
            "Foreground",
            f"{fg.total_counts:,}".replace(",", " "),
            format_duration(fg.live_time),
            format_duration(fg.real_time),
        ])

        # ----------------- Background -----------------
        if new_spect.get_background() is not None:
            bkg = new_spect.background
            if len(indicies) == 1:
                # Insert row dynamically if it wasn't created yet
                self.table.insertRow(self.row_counter)
                bkg_row = self.row_counter
                self.row_counter += 1
                indicies.append(bkg_row)
                self.saved_rows[name] = indicies
            else:
                bkg_row = indicies[1]

            self.set_color_cell(bkg_row, name, "background", new_spect.color_background)
            write_row(self.table, bkg_row, [
                "",
                new_spect.name,
                "Background",
                f"{bkg.total_counts:,}".replace(",", " "),
                format_duration(bkg.live_time),
                format_duration(fg.real_time),
            ])
