from PySide6.QtWidgets import (
    QWidget,
    QHBoxLayout,
    QVBoxLayout,
    QListWidget,
    QListWidgetItem,
    QLineEdit,
    QTextEdit,
    QPushButton,
    QComboBox,
    QCheckBox,
    QLabel,
    QColorDialog,
)
from PySide6.QtCore import Signal, Qt
from PySide6.QtGui import QColor, QPainter, QBrush, QAction, QFont

from core import SpectrumManager


def format_emissions(emissions):
    lines = [
        f"{'Energy (keV)':>12} | {'Intensity (%)':>14} | {'Type':>6} | Origin",
        "-" * 60,
    ]

    lines += [
        f"{e.energy_keV:12.4f} | {e.intensity_percent:14.2f} | {e.type:6} | {e.origin}"
        for e in emissions
    ]

    return "\n".join(lines)


def format_duration(seconds: float) -> str:
    if seconds == 0:
        return "0 s"

    abs_s = abs(seconds)

    minute = 60
    hour = 3600
    day = 86400
    year = 365.25 * day

    # Very large → scientific notation in years
    if abs_s >= 1e12:
        return f"{seconds / year:.3e} yr"

    # Years
    if abs_s >= year:
        y = seconds / year
        if abs(y) >= 1e9:
            return f"{y / 1e9:.2f} Gyr"
        if abs(y) >= 1e6:
            return f"{y / 1e6:.2f} Myr"
        return f"{y:.2f} yr"

    # Days
    if abs_s >= day:
        return f"{seconds / day:.2f} days"

    # Hours
    if abs_s >= hour:
        return f"{seconds / hour:.2f} h"

    # Minutes
    if abs_s >= minute:
        return f"{seconds / minute:.2f} min"

    # Seconds / small values
    if abs_s < 1e-3:
        return f"{seconds * 1e6:.2f} µs"
    if abs_s < 1:
        return f"{seconds * 1e3:.2f} ms"

    return f"{seconds:.2f} s"


class ColorCellWidget(QWidget):
    """Clean color swatch for QTableWidget cells."""

    clicked = Signal()
    sigColorChanged = Signal(object)

    def __init__(self, color: QColor, parent=None):
        super().__init__(parent)
        self.color = color
        self.setFixedWidth(30)
        self.setFixedHeight(20)
        self.setToolTip("Click to change color")

    def set_color(self, color: QColor):
        self.color = color
        self.update()  # repaint
        self.sigColorChanged.emit(self.color)

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


class NuclideListItem(QWidget):
    sigViewCheckChanged = Signal(str, bool)  # (name, checked)
    sigColorChanged = Signal(str, object)  # (name, QColor)

    def __init__(self, text, color):
        super().__init__()

        self.name = text

        layout = QHBoxLayout(self)
        layout.setContentsMargins(6, 2, 6, 2)
        layout.setSpacing(8)

        self.checkbox = QCheckBox()

        #         self.checkbox.setStyleSheet("""
        # QCheckBox::indicator:unchecked {
        #     border: 2px solid #888;
        #     background-color: #222;
        # }
        # """)

        self.label = QLabel(text)

        self.color_widget = ColorCellWidget(color)

        layout.addWidget(self.checkbox)
        layout.addWidget(self.label)
        layout.addStretch()
        layout.addWidget(self.color_widget)

        # connect checkbox
        self.checkbox.stateChanged.connect(self._on_state_changed)

        # connect color picker
        self.color_widget.clicked.connect(self.color_widget.get_color)
        self.color_widget.sigColorChanged.connect(self._on_color_changed)

    def _on_state_changed(self, state):
        self.sigViewCheckChanged.emit(
            self.name, Qt.CheckState(state) == Qt.CheckState.Checked
        )

    def _on_color_changed(self, color):
        self.sigColorChanged.emit(self.name, color)


class IsotopicsTab(QWidget):
    sigViewCheckChanged = Signal(str, bool)  # (name, checked)
    sigListItemClicked = Signal(str)
    sigColorChanged = Signal(str, object)  # (name, QColor)

    def __init__(self, all_nuclides: list, parent=None, title=""):
        super().__init__(parent=parent)

        self.sigListItemClicked.connect(self.change_nuclide_info)

        main_layout = QHBoxLayout(self)

        nuclides_list_layout = QVBoxLayout()

        self.nuclide_search_bar = QLineEdit()
        self.nuclide_search_bar.setPlaceholderText("Search nuclide...")
        self.nuclide_search_bar.textChanged.connect(self.filter_nuclides)

        self.nuclide_list_widget = QListWidget()

        self.nuclide_list_widget.currentItemChanged.connect(self._on_item_selected)

        nuclides_list_layout.addWidget(self.nuclide_search_bar)
        nuclides_list_layout.addWidget(self.nuclide_list_widget)
        
        self.all_nuclides = all_nuclides
        
        for nuclide in self.all_nuclides:
            self.add_nuclide(nuclide, "blue")

        self.nuclides_info_textbox = QTextEdit()
        self.nuclides_info_textbox.setReadOnly(True)

        font = QFont()
        font.setFamily("Consolas")  # Windows-friendly monospace
        font.setStyleHint(QFont.Monospace)
        self.nuclides_info_textbox.setFont(font)

        peak_search_layout = QVBoxLayout()

        btn_search_peaks = QPushButton("Search peaks")

        self.search_spect_combo = QComboBox()

        peak_search_layout.addWidget(btn_search_peaks)
        peak_search_layout.addWidget(self.search_spect_combo)

        nuclides_list_layout.setContentsMargins(0, 0, 0, 0)
        nuclides_list_layout.setSpacing(6)

        peak_search_layout.setContentsMargins(0, 0, 0, 0)
        peak_search_layout.setSpacing(6)

        main_layout.addLayout(nuclides_list_layout, 2)
        main_layout.addWidget(self.nuclides_info_textbox, 4)
        main_layout.addLayout(peak_search_layout, 2)

    def add_nuclide(self, name, color):
        item = QListWidgetItem()
        widget = NuclideListItem(name, color)

        item.setSizeHint(widget.sizeHint())

        self.nuclide_list_widget.addItem(item)
        self.nuclide_list_widget.setItemWidget(item, widget)

        # Propagate signals from lines
        widget.sigViewCheckChanged.connect(self.sigViewCheckChanged)
        widget.sigColorChanged.connect(self.sigColorChanged)

    def filter_nuclides(self, text):
        text = text.lower()

        for i in range(self.nuclide_list_widget.count()):
            item = self.nuclide_list_widget.item(i)
            widget = self.nuclide_list_widget.itemWidget(item)

            if widget is None:
                continue

            name = widget.name.lower()

            item.setHidden(text not in name)

    def _on_item_selected(self, current, previous):
        if current is None:
            return

        widget = self.nuclide_list_widget.itemWidget(current)
        if widget is None:
            return

        name = widget.name

        # Example: reuse your existing signal
        self.sigListItemClicked.emit(name)

    def change_nuclide_info(self, name):
        nuc = SpectrumManager.NuclideLibrary.get_nuclide(name)
        
        title_str = f"| {nuc.element} | {nuc.nuclide} |"
        separator = "="*len(title_str)
        
        daughters = "\n".join([f"{n} {p}%" for n, p in nuc.daughters])

        self.nuclides_info_textbox.setText(
            f"""{separator}
{title_str}
{separator}
Half-Life = {format_duration(nuc.half_life_s[0])}
Z = {nuc.Z}


Emissions:
{format_emissions(nuc.emissions)}

Daughter Products:
{daughters}

LNHB citation volume: {nuc.lnhb_volume}
(see bibliography)
        """
        )
