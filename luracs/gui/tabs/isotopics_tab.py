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
    QSpinBox
)
from PySide6.QtCore import Signal, Qt
from PySide6.QtGui import QColor, QPainter, QBrush, QAction, QFont
import pyqtgraph as pg
import numpy as np

from core import SpectrumManager, Settings
from utils.numerics import find_peaks

from textwrap import dedent

# --- Helpers ---
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
    sigViewCheckChanged = Signal(str, bool, object)  # (name, checked, QColor)
    sigColorChanged = Signal(str, object)  # (name, QColor)

    def __init__(self, text, color, chain=False):
        super().__init__()

        self.name = text

        layout = QHBoxLayout(self)
        layout.setContentsMargins(6, 2, 6, 2)
        layout.setSpacing(8)

        self.checkbox = QCheckBox()

        self.label = QLabel(text)


        self.color_widget = ColorCellWidget(color)

        layout.addWidget(self.checkbox)
        layout.addWidget(self.label)
        layout.addStretch()
        if not chain:
            layout.addWidget(self.color_widget)

        # connect checkbox
        self.checkbox.stateChanged.connect(self._on_state_changed)

        # connect color picker
        self.color_widget.clicked.connect(self.color_widget.get_color)
        self.color_widget.sigColorChanged.connect(self._on_color_changed)
        
    def mouseDoubleClickEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.checkbox.toggle()

        super().mouseDoubleClickEvent(event)

    def _on_state_changed(self, state):
        self.sigViewCheckChanged.emit(
            self.name, Qt.CheckState(state) == Qt.CheckState.Checked,
            self.color_widget.color
        )
        
    def emit_info(self):
        self.sigViewCheckChanged.emit(
            self.name, True,
            self.color_widget.color
        )
        

    def _on_color_changed(self, color):
        self.sigColorChanged.emit(self.name, color)

class SpectrumDebugViewWidget(QWidget):
    def __init__(self, parent = None):
        super().__init__(parent=parent)
        self.spectrum_y = None
        self.d1 = None
        self.d2 = None
        self.peaks = None

        self.spectrum_plot = pg.PlotWidget()
        self.d1_plot = pg.PlotWidget()
        self.d2_plot = pg.PlotWidget()
        
        self.d1_plot.getPlotItem().setXLink(self.spectrum_plot)
        self.d2_plot.getPlotItem().setXLink(self.spectrum_plot) 
        
        self.plots_L = [self.spectrum_plot, self.d1_plot, self.d2_plot]
        
        main_layout = QVBoxLayout(self)
        
        main_layout.addWidget(self.spectrum_plot)
        main_layout.addWidget(self.d1_plot)
        main_layout.addWidget(self.d2_plot)
        

    def show(self):
        for plot in self.plots_L:
            plot.getPlotItem().clear()

        # Split peak positions by type
        left_positions = []
        centre_positions = []
        right_positions = []

        if self.peaks is not None:
            for left, centre, right in self.peaks:
                left_positions.append(left)
                centre_positions.append(centre)
                right_positions.append(right)

        def add_peak_markers(plot_item, data):
            # Left = red
            if left_positions:
                x = np.array(left_positions)
                y = data[x]

                plot_item.plot(
                    x, y,
                    pen=None,
                    symbol='o',
                    symbolSize=8,
                    symbolBrush='r'
                )

            # Centre = blue
            if centre_positions:
                x = np.array(centre_positions)
                y = data[x]

                plot_item.plot(
                    x, y,
                    pen=None,
                    symbol='o',
                    symbolSize=8,
                    symbolBrush='b'
                )

            # Right = green
            if right_positions:
                x = np.array(right_positions)
                y = data[x]

                plot_item.plot(
                    x, y,
                    pen=None,
                    symbol='o',
                    symbolSize=8,
                    symbolBrush='g'
                )

        # ---------------- Spectrum ----------------
        if self.spectrum_y is not None:
            x = np.arange(len(self.spectrum_y))

            plot_item = self.spectrum_plot.getPlotItem()
            plot_item.plot(x, self.spectrum_y)

            add_peak_markers(plot_item, self.spectrum_y)

        # ---------------- First derivative ----------------
        if self.d1 is not None:
            x = np.arange(len(self.d1))

            plot_item = self.d1_plot.getPlotItem()
            plot_item.plot(x, self.d1)

            add_peak_markers(plot_item, self.d1)

        # ---------------- Second derivative ----------------
        if self.d2 is not None:
            x = np.arange(len(self.d2))

            plot_item = self.d2_plot.getPlotItem()
            plot_item.plot(x, self.d2)

            add_peak_markers(plot_item, self.d2)

        super().show()

class IsotopicsTab(QWidget):
    sigViewCheckChanged = Signal(str, bool, object)  # (name, checked, QColor)
    sigListItemClicked = Signal(str)
    sigColorChanged = Signal(str, object)  # (name, QColor)

    def __init__(self, all_nuclides: list, parent=None, title=""):
        super().__init__(parent=parent)

        self.sigListItemClicked.connect(self.change_nuclide_info)
        self.sigViewCheckChanged.connect(SpectrumManager.NuclideLibrary._track_selected_nuclies)
        
        SpectrumManager.Signals.spectrumCreated.connect(self.set_search_combo)
        SpectrumManager.Signals.spectrumRemoved.connect(self.set_search_combo)
        

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
        
        for nuclide in sorted([*self.all_nuclides, *SpectrumManager.NuclideLibrary.decay_chains.keys()], key=lambda x: int(x.removesuffix("-- Chain").split("-")[1].removesuffix("m"))):
            self.add_nuclide(
                            nuclide, "blue", 
                            is_chain = True if "Chain" in nuclide else False
                            )
            


        self.nuclides_info_textbox = QTextEdit()
        self.nuclides_info_textbox.setReadOnly(True)

        font = QFont()
        font.setFamily("Consolas")  # Windows-friendly monospace
        font.setStyleHint(QFont.Monospace)
        self.nuclides_info_textbox.setFont(font)

        peak_search_layout = QVBoxLayout()
        
        self.btn_assign_emissions = QPushButton("Auto Set Emissions")
        peak_search_layout.addWidget(self.btn_assign_emissions)


        self.btn_search_peaks = QPushButton("Search peaks")
        self.btn_search_peaks.clicked.connect(self.peak_finder)

        self.search_spect_combo = QComboBox()
        
        self.search_window_length = QSpinBox()
        self.search_window_length.setRange(3, 1001)
        self.search_window_length.setSingleStep(2)
        self.search_window_length.setValue(31)
        
        self.debug_view = SpectrumDebugViewWidget()
        self.btn_view_calculation = QPushButton("View Derivatives")
        self.btn_view_calculation.clicked.connect(self.debug_view.show)

        peak_search_layout.addWidget(self.btn_search_peaks)
        peak_search_layout.addWidget(self.search_spect_combo)
        peak_search_layout.addWidget(QLabel("Channels in Search Window"))
        peak_search_layout.addWidget(self.search_window_length)
        peak_search_layout.addWidget(self.btn_view_calculation)
        peak_search_layout.addStretch()

        nuclides_list_layout.setContentsMargins(0, 0, 0, 0)
        nuclides_list_layout.setSpacing(6)

        peak_search_layout.setContentsMargins(0, 0, 0, 0)
        peak_search_layout.setSpacing(6)

        main_layout.addLayout(nuclides_list_layout, 2)
        main_layout.addWidget(self.nuclides_info_textbox, 4)
        main_layout.addLayout(peak_search_layout, 2)
        

    def add_nuclide(self, name, color, is_chain=False):
        "Used during startup"
        item = QListWidgetItem()
        widget = NuclideListItem(name, color, is_chain)

        item.setSizeHint(widget.sizeHint())

        self.nuclide_list_widget.addItem(item)
        self.nuclide_list_widget.setItemWidget(item, widget)

        # Propagate signals from lines if its not a decay chain
        if not is_chain:
            widget.sigViewCheckChanged.connect(self.sigViewCheckChanged)
            widget.sigColorChanged.connect(self.sigColorChanged)
        else:
            widget.sigViewCheckChanged.connect(self.expand_chain)

    def filter_nuclides(self, text):
        "Search functionality"
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

        self.sigListItemClicked.emit(name)
        
    def expand_chain(self, chain: str, state: bool, _):
        chain_members = SpectrumManager.NuclideLibrary.decay_chains[chain]
        
        for nuclide in chain_members:
            self.set_nuclide_check(nuclide, state)
            
        
    def request_line_data(self):
        "Get data from currently selected nuclides"
        for i in range(self.nuclide_list_widget.count()):
            item = self.nuclide_list_widget.item(i)
            item_widget = self.nuclide_list_widget.itemWidget(item)

            if item_widget.checkbox.isChecked():
                item_widget.emit_info()
                
    def set_nuclide_check(self, nuclide: str, state: bool):
        for i in range(self.nuclide_list_widget.count()):
            item = self.nuclide_list_widget.item(i)
            item_widget = self.nuclide_list_widget.itemWidget(item)

            if item_widget.name.lower() == nuclide.lower():
                item_widget.checkbox.setChecked(state)

    def change_nuclide_info(self, name):
        "Display nuclide data in the GUI"
        if "Chain" in name:
            # TODO Display decay chain in GUI
            return
        nuc = SpectrumManager.NuclideLibrary.get_nuclide(name)
        
        title_str = f"| {nuc.element} | {nuc.nuclide} |"
        separator = "="*len(title_str)
        
        daughters = "\n".join([f"{d.replace("alpha", "α").replace("B", "β")} {n} {p}%" for d, n, p in nuc.daughters])

        self.nuclides_info_textbox.setText(
            f"{separator}\n"
            f"{title_str}\n"
            f"{separator}\n"
            f"Half-Life = {format_duration(nuc.half_life_s[0])}\n"
            f"Z = {nuc.Z}\n\n"
            "Emissions:\n"
            f"{format_emissions(nuc.emissions)}\n\n"
            "Daughter Products:\n"
            f"{daughters}\n\n"
            f"Source: {nuc.citation_ref}\n"
            "(see bibliography)"
        )

    def peak_finder(self):
        spectrum_key = self.search_spect_combo.currentData()
        
        spectrum = SpectrumManager.get_spectrum(spectrum_key)
        if spectrum is None:
            return
        
        if SpectrumManager.ROIManager.spectrum_is_bkg_sub:
            y_data = spectrum.get_bkg_sub()
        else:
            y_data = spectrum.get_foreground()
        
        peaks, d1, d2 = find_peaks(y_data)
        
        self.debug_view.spectrum_y = y_data
        self.debug_view.d1 = d1
        self.debug_view.d2 = d2
        self.debug_view.peaks = peaks
        
        for le, p, re in peaks:
            le_e, re_e = spectrum.x_axis[le], spectrum.x_axis[re]
            
            width_threshold = 75 if re_e < 250 else 250
            
            if abs(le_e - re_e) < width_threshold:            
                SpectrumManager.ROIManager.add_roi(le_e, re_e)
            
    def set_search_combo(self):
        self.search_spect_combo.clear()
        if Settings.Appearance.tabbed_spectrum_view:
            self.search_spect_combo.setEnabled(False)
            for key, spectrum in SpectrumManager.get_spectra_dict().items():
                self.search_spect_combo.addItem(spectrum.name, key)
        else:
            self.search_spect_combo.setEnabled(True)
            for key, spectrum in SpectrumManager.get_spectra_dict().items():
                self.search_spect_combo.addItem(spectrum.name, key)
    

        
        
        