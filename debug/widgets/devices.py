from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QSizePolicy, QColorDialog, QFrame, QHBoxLayout, QDialog, QFormLayout, QTextEdit, QComboBox, QLineEdit, QDialogButtonBox,
    QPushButton, QCheckBox, QDoubleSpinBox, QTabWidget, QAbstractItemView, QMessageBox, QFileDialog, QLabel,
    QSpinBox, QDoubleSpinBox, QListWidget, QListWidgetItem
)
from PySide6.QtCore import Qt, Signal
import pyqtgraph as pg
pg.setConfigOptions(antialias=True)
import sys, os
from glob import glob

from pathlib import Path
import shutil

try:
    import pyqtgraph.opengl as gl
    HAS_OPENGL = True
except ImportError:
    HAS_OPENGL = False
import numpy as np

class StrIdxTable(QWidget):
    def __init__(self, title = "", parent = None, columns = None, has_menu_button=False):
        """Abstraction of QTable the that uses string keys for table indexing."""
        super().__init__(parent)
        
        self.table: QTableWidget = QTableWidget()
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)
        
        self.rowkeys: dict[str, int] = {}
        self.row_counter = 0
        self.has_been_set = False
        
        self.has_menu_button = has_menu_button
        
        if columns is not None:
            self.reset_table(columns)
    
    def get_key_from_index(self, index: int) -> str:
        for key, table_index in self.rowkeys.items():
            if table_index == index:
                return key
        
    def reset_table(self, titles: list, widths = None):
        "Clear the table and set new columns titles"
        assert isinstance(titles, list), f"Titles must be a list! Is {type(titles)}"
        if self.table is not None:
            self.table.clear()
            
        self.table.setRowCount(0)

        if self.has_menu_button:
            titles = [""] + titles
            self.table.setColumnCount(len(titles) + 1)
            self.table.setHorizontalHeaderLabels(titles)
        else:
            self.table.setColumnCount(len(titles))
            self.table.setHorizontalHeaderLabels(titles)
        self.table.setMinimumHeight(50)
        
        if widths is not None:
            if self.has_menu_button:
                assert len(widths) == len(titles) - 1, f"Length of widths does not match, titles {len(titles)}, widths {len(widths)}"
                for i in range(1, len(widths)):
                    self.table.setColumnWidth(i, widths[i])
            else:
                assert len(widths) == len(titles), f"Length of widths does not match, titles {len(titles)}, widths {len(widths)}"
                for i in range(len(widths)):
                    self.table.setColumnWidth(i, widths[i])
        
    def write_row(self, row_tag: str, values: list, menu_button: QWidget = None):
        "Write a row based on a str key, values must be list"
        assert isinstance(values, list), f"Titles must be a list! Is {type(values)}"
        if row_tag not in self.rowkeys:
            self.rowkeys[row_tag] = self.row_counter
            self.table.insertRow(self.row_counter)
            self.row_counter += 1

        shift = 1 if self.has_menu_button else 0

        
        row_index = self.rowkeys[row_tag]
        # If we have a menu button
        if self.has_menu_button and menu_button is not None:
            self.table.setCellWidget(row_index, 0, QPushButton("Hello"))
        
        # Normal content
        for col_index, value in enumerate(values):
            self.table.setItem(row_index, col_index + shift, QTableWidgetItem(str(value)))
            
    def delete_row(self, row_tag: str):
        row_index = self.rowkeys.pop(row_tag, None) # If a spectrum is hidden it might not be here
        if row_index is None:
            return
        self.table.removeRow(row_index)
        for key in self.rowkeys:
            if self.rowkeys[key] > row_index:
                self.rowkeys[key] -= 1
        self.row_counter -= 1

class ROIEditor(QDialog):
    DELETE = 2
    def __init__(self, roi_name, low, high, fit_type, bkg_type,
                 merge, poisson_weights,
                 title="", parent=None):
        super().__init__(parent=parent)
        
        self.setWindowTitle("ROI Editor")
        self.setMinimumWidth(150)
        self.setMinimumHeight(300)
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)
        
        self.layout = main_layout
        form = QFormLayout()
        form.setSpacing(9)


        
        self.roi_name = QLineEdit()
        self.roi_name.setText(roi_name)
        form.addRow("ROI Name:", self.roi_name)
        
        self.lower_bound = QDoubleSpinBox()
        self.lower_bound.setRange(0.0, 1e6)   # adjust as needed
        self.lower_bound.setDecimals(2)
        self.lower_bound.setSuffix(" keV")
        self.lower_bound.setValue(round(low))
        form.addRow("Lower Bound:", self.lower_bound)

        self.higher_bound = QDoubleSpinBox()
        self.higher_bound.setRange(0.0, 1e6)
        self.higher_bound.setDecimals(2)
        self.higher_bound.setSuffix(" keV")
        self.higher_bound.setValue(round(high))     # example default
        form.addRow("Higher Bound:", self.higher_bound)
        

        self.fit_type = QComboBox()
        self.fit_type.addItems(["None", "Gaussian"])
        self.fit_type.setCurrentText(fit_type)
        form.addRow("Peak Function:", self.fit_type)
        
        self.bkg_type = QComboBox()
        self.bkg_type.addItems(["None", "Linear", "Quadratic"])
        self.bkg_type.setCurrentText(bkg_type)
        form.addRow("Background:", self.bkg_type)
        
        self.merge = QCheckBox("Allow merging")
        self.merge.setChecked(merge)
        form.addRow("\t\t",self.merge)
        
        self.poisson_weights = QCheckBox("Use Poisson Weights")
        self.poisson_weights.setChecked(poisson_weights)
        form.addRow("\t\t",self.poisson_weights)
        
        def update_poisson_state():
            is_gaussian = self.fit_type.currentText() == "Gaussian"
            self.poisson_weights.setEnabled(is_gaussian)

        self.fit_type.currentTextChanged.connect(update_poisson_state)
            
        main_layout.addLayout(form)
        
        # --- Bottom Buttons
        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        delete_button = QPushButton("Delete")
        delete_button.clicked.connect(self.on_delete)
        buttons.addButton(delete_button, QDialogButtonBox.ActionRole)

        main_layout.addWidget(buttons)


    def on_delete(self):
        self.done(self.DELETE)
        
class GenericLibrary(QWidget):
    def __init__(self, parent = None):
        super().__init__(self, parent)
        

    
        
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
        self.spectrogram_tab = QWidget()
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

class LibraryTab(QWidget):
    def __init__(self, parent, columns, include_checks = False):
        super().__init__(parent=parent)

        self.file_index = {}
        self.path = ""
        
        main_layout = QVBoxLayout(self)
        
        self.table = StrIdxTable()
        self.table.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.table.setSelectionMode(QAbstractItemView.ExtendedSelection)

        self.table.reset_table(columns)    
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
                os.remove(file)
            # self.run_index()
    
    def show_info(self):
        raise NotImplementedError("Info button not implemented")
            
    
    def run_index(self):
        raise NotImplementedError("run_index not implemented")
        return
        for file in glob("**.xml", self.path):
            pass
        
class SpectrumTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "Date", "Live Time", "Background", "ROIs", "Instrument"], True)
        
    def run_index(self):
        for file in glob(Path("/home/eewa/**.xml"), self.path):
            pass

class ROIsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "ROIs", "Regions"])

class InstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Name", "Type", "Calibration", "Resolution", "Efficiency", "Response Matrix"])
        self.btn_load.setText("New")

class GenericInstrumentsTab(LibraryTab):
    def __init__(self, parent):
        super().__init__(parent, ["Type", "Resolution", "Efficiency", "Response Matrix"])
        self.btn_load.setText("New")

from PySide6.QtWidgets import (
    QDialog, QFormLayout, QLineEdit, QComboBox, QSpinBox,
    QLabel, QHBoxLayout, QVBoxLayout, QDialogButtonBox, QWidget
)

class InstrumentDialog(QDialog):
    def __init__(self, parent=None, **kwargs):
        super().__init__(parent)
        self.setWindowTitle("Instrument Dialog")
        self.resize(500, 500)

        # Main layout
        main_layout = QVBoxLayout(self)

        # Form layout
        form_layout = QFormLayout()
        main_layout.addLayout(form_layout)
        
        # Generic instruments as a base
        self.generic_list = QComboBox()
        form_layout.addRow("Generic Instruments:", self.generic_list)

        # Name and model
        model_name_row = QHBoxLayout()
        
        self.name_input = QLineEdit()
        self.name_input.setText(kwargs.get("name"))
        
        model_name_row.addWidget(self.name_input)
        model_name_row.addWidget(QLabel("Instrument Model:"))

        self.model_input = QLineEdit()
        self.model_input.setText(kwargs.get("model"))
        model_name_row.addWidget(self.model_input)
        
        form_layout.addRow("Instrument Name:", model_name_row)
        
        # Manufacturer and material 
        manufacturer_material_row = QHBoxLayout()

        self.manufacturer = QLineEdit()
        self.manufacturer.setText(kwargs.get("manufacturer"))
        manufacturer_material_row.addWidget(self.manufacturer)

        manufacturer_material_row.addWidget(QLabel("Detector Material:"))

        self.detector_material = QLineEdit()
        self.detector_material.setText(kwargs.get("detector_material"))
        manufacturer_material_row.addWidget(self.detector_material)

        form_layout.addRow("Manufacturer:", manufacturer_material_row)

        # Shape combo
        self.shape = QComboBox()
        self.shape.addItems(["Cuboid", "Cylinder", "Other"])
        self.shape.currentTextChanged.connect(self.update_spinboxes)
        form_layout.addRow("Shape:", self.shape)

        # Dimensions spin boxes
        self.height_spin = QDoubleSpinBox()
        self.height_spin.setRange(0, 500)
        self.height_spin.setDecimals(3)
        self.height_spin.setValue(kwargs.get("detector_dimensions")[0])

        self.width_spin = QDoubleSpinBox()
        self.width_spin.setRange(0, 500)
        self.width_spin.setDecimals(3)
        self.width_spin.setValue(kwargs.get("detector_dimensions")[1])

        self.depth_spin = QDoubleSpinBox()
        self.depth_spin.setRange(0, 500)
        self.depth_spin.setDecimals(3)
        if self.shape.currentText != "Cylinder":
            self.height_spin.setValue(kwargs.get("detector_dimensions")[2])

        # Layout for dimensions
        dim_layout = QHBoxLayout()

        # Height
        h_widget = QWidget()
        h_layout = QHBoxLayout(h_widget)
        h_layout.setContentsMargins(0, 0, 0, 0)
        h_layout.addWidget(QLabel("Height:"))
        h_layout.addWidget(self.height_spin)
        h_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(h_widget)

        # Width
        w_widget = QWidget()
        w_layout = QHBoxLayout(w_widget)
        w_layout.setContentsMargins(0, 0, 0, 0)
        self.W_label = QLabel("Width:")
        w_layout.addWidget(self.W_label)
        w_layout.addWidget(self.width_spin)
        w_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(w_widget)

        # Depth
        d_widget = QWidget()
        d_layout = QHBoxLayout(d_widget)
        d_layout.setContentsMargins(0, 0, 0, 0)
        d_layout.addWidget(QLabel("Length:"))
        d_layout.addWidget(self.depth_spin)
        d_layout.addWidget(QLabel("cm  "))
        dim_layout.addWidget(d_widget)

        form_layout.addRow("Dimensions:", dim_layout)
        
        # Resolution
        self.resolution = QLineEdit()
        self.resolution.setEnabled(False)
        form_layout.addRow("Resolution: ",self.resolution)
        
        # Resolution Plot
        self.res_plot_widget = pg.PlotWidget()
        self.res_plot_widget.setMouseEnabled(x=False, y=False)
        form_layout.addRow("", self.res_plot_widget)

        # Example data
        self.res_plot_widget.plot([1, 2, 3, 4], [10, 20, 15, 30])
        
        # Efficiency
        self.efficiency = QLineEdit()
        self.efficiency.setEnabled(False)
        form_layout.addRow("Efficiency: ",self.efficiency)
        
        # Efficiency plot
        self.eff_plot_widget = pg.PlotWidget()
        self.eff_plot_widget.setMouseEnabled(x=False, y=False)
        form_layout.addRow("", self.eff_plot_widget)

        # Example data
        self.eff_plot_widget.plot([1, 2, 3, 4], [10, 20, 15, 30])
        
        # Response Matrix
        response_matrix_row = QHBoxLayout()
        self.response_matrix = QLineEdit()
        self.response_matrix.setEnabled(False)
        response_matrix_row.addWidget(self.response_matrix)
        
        self.load_matrix = QPushButton()
        self.load_matrix.setText("Import")
        response_matrix_row.addWidget(self.load_matrix)
        form_layout.addRow("Response Matrix: ", response_matrix_row)
    
        # Buttons
        bottom_box = QGroupBox()
        
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        main_layout.addWidget(buttons)

        # Initialize spinboxes
        self.update_spinboxes(self.shape.currentText())
        
        

    def update_spinboxes(self, shape_text):
        """Disable depth spinbox for Cylinder, enable otherwise."""
        if shape_text == "Cylinder":
            self.depth_spin.setDisabled(True)
            self.W_label.setText("Diameter: ")
        else:
            self.depth_spin.setEnabled(True)
            self.W_label.setText("Width: ")
     
from PySide6.QtGui import QColor, QPainter, QBrush, QAction
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
    sigColorChanged = Signal(str, object) # (name, QColor)
    def __init__(self, text, color):
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
        layout.addWidget(self.color_widget)
        
        # connect checkbox
        self.checkbox.stateChanged.connect(self._on_state_changed)

        # connect color picker
        self.color_widget.clicked.connect(self.color_widget.get_color) 
        self.color_widget.sigColorChanged.connect(self._on_color_changed)
        
    def _on_state_changed(self, state):
        self.sigViewCheckChanged.emit(self.name, Qt.CheckState(state) == Qt.CheckState.Checked)
        
    def _on_color_changed(self, color):
        self.sigColorChanged.emit(self.name, color)
        

            
class IsotopicsTab(QWidget):
    sigViewCheckChanged = Signal(str, bool)  # (name, checked)
    sigListItemClicked = Signal(str)
    sigColorChanged = Signal(str, object) # (name, QColor)
    
    def __init__(self, all_nuclides: list, parent=None, title=""):
        super().__init__(parent=parent)

        main_layout = QHBoxLayout(self)

        nuclides_list_layout = QVBoxLayout()

        self.nuclide_search_bar = QLineEdit()
        self.nuclide_search_bar.setPlaceholderText("Search nuclide...")
        self.nuclide_search_bar.textChanged.connect(self.filter_nuclides)

        self.nuclide_list_widget = QListWidget()
        
        self.nuclide_list_widget.currentItemChanged.connect(self._on_item_selected)

        nuclides_list_layout.addWidget(self.nuclide_search_bar)
        nuclides_list_layout.addWidget(self.nuclide_list_widget)
        
        self.all_nuclides = sorted(all_nuclides, key = lambda n: int(n.split("-")[-1]))
        
        for nuclide in self.all_nuclides:
            self.add_nuclide(nuclide, "blue")

        self.nuclides_info_textbox = QTextEdit()
        peak_search_layout = QVBoxLayout()
        
        btn_search_peaks = QPushButton("Search peaks")
        
        self.search_spect_combo = QComboBox()
        
        peak_search_layout.addWidget(btn_search_peaks)
        peak_search_layout.addWidget(self.search_spect_combo)

        nuclides_list_layout.setContentsMargins(0, 0, 0, 0)
        nuclides_list_layout.setSpacing(6)

        peak_search_layout.setContentsMargins(0, 0, 0, 0)
        peak_search_layout.setSpacing(6)

        main_layout.addLayout(nuclides_list_layout)
        main_layout.addWidget(self.nuclides_info_textbox)
        main_layout.addLayout(peak_search_layout)

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

        # Emit whatever you want here
        print("Selected:", name)

        # Example: reuse your existing signal
        self.sigListItemClicked.emit(name)
        


app = QApplication.instance() or QApplication(sys.argv)


w = ROIEditor("ROI_0", 124, 452.425, "Gaussian", "Linear", True, False)


# l = InstrumentDialog(model = "Rc-103G", detector_dimensions = [7.31, 7.31, 1])
# res = l.exec()

# print(res)



window = IsotopicsTab(["Cs-137", "Co-60", "Ba-133"])
window.sigColorChanged.connect(print)
window.sigViewCheckChanged.connect(print)
window.resize(800, 500)
window.show()



sys.exit(app.exec())
