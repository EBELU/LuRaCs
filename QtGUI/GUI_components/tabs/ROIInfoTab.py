from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QHBoxLayout,
    QTableWidget,
    QTableWidgetItem,
    QSizePolicy,
    QComboBox,
    QRadioButton,
    QButtonGroup,
    QMessageBox,
    QPushButton
)
from PySide6.QtCore import Signal

from SpectrumClasses import ROI


from core import SpectrumManager

def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))
        
class StrIdxTable(QWidget):
    def __init__(self, title = "", parent = None):
        super().__init__(parent)
        
        self.table: QTableWidget = QTableWidget()
        
        self.rowkeys: dict[str, int] = {}
        self.row_counter = 0
        
    def reset_table(self, titles):
        if self.table is not None:
            self.table.clear()
            
        self.table.setRowCount(0)
        self.table.setColumnCount(len(titles))
        self.table.setHorizontalHeaderLabels(titles)
        self.table.setMinimumHeight(50)
        
        self.table.setSizePolicy(
            QSizePolicy.Expanding,      # vertical
            QSizePolicy.MinimumExpanding,  # horizontal
        )
        
        self.row_counter = 0
        self.rowkeys.clear()
    
    def write_row(self, row_tag, values):
        if row_tag not in self.rowkeys:
            self.rowkeys[row_tag] = self.row_counter
            self.table.insertRow(self.row_counter)
            self.row_counter += 1


        row_index = self.rowkeys[row_tag]
        for col_index, value in enumerate(values):
            self.table.setItem(row_index, col_index, QTableWidgetItem(str(value)))
            
    def delete_row(self, row_tag):
        row_index = self.rowkeys.pop(row_tag, None) # If a spectrum is hidden it might not be here
        if not row_index:
            return
        self.table.removeRow(row_index)
        for key in self.rowkeys:
            if self.rowkeys[key] > row_index:
                self.rowkeys[key] -= 1
        
        self.row_counter -= 1
        
    

class ROIInfoTab(QWidget):
    clearROIs = Signal()
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        # --- Signals ---
        SpectrumManager.Signals.roiUpdated.connect(self.recieve_roi)
        SpectrumManager.Signals.roiRemoved.connect(self.delete_roi)
        SpectrumManager.Signals.spectrumRemoved.connect(lambda x: self.update_combo())
        
        # ---- Box ----
        self.group_box = QGroupBox(title)

        # ---- Table ----
        titles = ["ROI", "Low", "High", "Peak Center", "FWHM", "Max Height","G", "B", "N",]
        self.table = StrIdxTable()
        self.table.reset_table(titles)

        # self.table.setMaximumWidth(
        #     self.table.verticalHeader().width()
        #     + self.table.horizontalHeader().length()
        #     + self.table.frameWidth() * 2
        # )



        
        # Layout inside the box
        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table.table)
        box_layout.addStretch()


        main_layout = QVBoxLayout(self)

        # Create a container widget for the horizontal layout
        options_widget = QWidget()
        options_bar = QHBoxLayout(options_widget)

        self.combo = QComboBox()
        self.combo.setMinimumWidth(250)
        btn_by_roi = QRadioButton("View by ROI")
        btn_by_roi.clicked.connect(self._update_show)

        btn_by_spect = QRadioButton("View by spectrum")
        btn_by_spect.setChecked(True)
        btn_by_spect.clicked.connect(self._update_show)

        btns = QButtonGroup(self)
        btns.addButton(btn_by_spect)
        btns.addButton(btn_by_roi)
        
        btn_clear = QPushButton()
        btn_clear.setText("Clear ROIs")
        btn_clear.clicked.connect(self._clear_rois)

        options_bar.addWidget(self.combo)
        options_bar.addWidget(btn_by_spect)
        options_bar.addWidget(btn_by_roi)
        options_bar.addWidget(btn_clear)
        options_bar.setSpacing(5)
        options_bar.setContentsMargins(0, 0, 0, 3)  
        options_bar.addStretch()
        options_widget.setSizePolicy(
            QSizePolicy.Minimum,
            QSizePolicy.Preferred
        )
        

        main_layout.addWidget(options_widget)
        main_layout.addWidget(self.group_box)
        main_layout.addStretch() 
        
        self.ROIs = {}
        self.combo.currentIndexChanged.connect(self._on_combo_changed)
        self.combo_show_spectrum = True

    def _update_show(self, _=None):
        """Toggle between spectrum and ROI mode."""
        self.combo_show_spectrum = not self.combo_show_spectrum
        self.update_combo()


    def update_combo(self):
        """Rebuild combo safely without recursive signals."""
        self.combo.blockSignals(True)
        self.combo.clear()

        if self.combo_show_spectrum:
            for key in SpectrumManager.get_spectra_dict().keys():
                self.combo.addItem(key)
        else:
            for key in SpectrumManager.existing_rois:
                self.combo.addItem(key)

        self.combo.blockSignals(False)

        # Trigger table update manually if items exist
        if self.combo.count() > 0:
            self._on_combo_changed(self.combo.currentIndex())


    def _on_combo_changed(self, index: int):
        """Single routing point for combo changes."""
        if index < 0:
            return

        if self.combo_show_spectrum:
            self.set_table_by_spectrum(index)
        else:
            self.set_table_by_roi(index)


    def set_table_by_spectrum(self, spectrum_idx: int):
        spectrum = self.combo.itemText(spectrum_idx)
        if not spectrum:
            return

        rois = SpectrumManager.get_ROIs(spectrum=spectrum)

        titles = ["ROI", "Low", "High", "Peak Center",
                "FWHM", "Max Height", "G", "B", "N", "A"]

        self.table.reset_table(titles)
        


        for tag, roi in rois.items():
            if not SpectrumManager.get_spectrum(spectrum).fit_rois:
                self.table.write_row(tag, ["Spectrum"] +["Hidden"] + [""] * (len(titles) - 1))
                return
            else:
                self._put_roi(tag, roi)


    def set_table_by_roi(self, roi_idx: int):
        roi_tag = self.combo.itemText(roi_idx)
        if not roi_tag:
            return

        rois = SpectrumManager.get_ROIs(roi_tag=roi_tag)

        titles = ["Spectrum", "Low", "High", "Peak Center",
                "FWHM", "Max Height", "G", "B", "N", "A"]

        self.table.reset_table(titles)

        for tag, roi in rois.items():
            self._put_roi(tag, roi)


    def _put_roi(self, tag: str, roi, none_fallback: str = "Fit Failed"):
        if not roi:
            return

        if roi.gaussian is not None:
            row = [
                tag,
                round(roi.low),
                round(roi.high),
                round(roi.gaussian.mu, 4),
                round(roi.gaussian.FWHM(), 4),
                round(roi.gaussian.max_height(), 4),
                round(roi.gaussian.G, 4),
                round(roi.gaussian.B, 4),
                round(roi.gaussian.N, 4),
                round(roi.gaussian.A, 4),
            ]
        else:
            row = [tag, round(roi.low), round(roi.high), none_fallback] + [None] * 6

        self.table.write_row(tag, row)
        
    
    def recieve_roi(self, roi_tag): 
        rois = SpectrumManager.get_ROIs(roi_tag)
        
        for roi in rois.values():
            self._put_roi(roi.tag, roi)
        
        # roi = list(SpectrumManager.get_spectra_dict().values())[0].ROIs[roi_tag]
        
        # if roi.gaussian is not None:
        #     row = [roi.tag, round(roi.low), round(roi.high), 
        #             round(roi.gaussian.mu, 4), 
        #             round(roi.gaussian.FWHM(), 4), 
        #             round(roi.gaussian.max_height(), 4),
        #             round(roi.gaussian.G, 4), 
        #             round(roi.gaussian.B, 4), 
        #             round(roi.gaussian.N, 4), 
        #             round(roi.gaussian.A, 4), 
        #         ]
        # else:
        #     row = [roi.tag, round(roi.low), round(roi.high), "Fit failed"] + [None] * 6
        # self.table.write_row(roi.tag, row)
        
        self.update_combo()

    def delete_roi(self, roi_tag: str):
        if self.combo_show_spectrum:
            self.table.delete_row(roi_tag)
            self.update_combo()
        
        else:
            self.table.reset_table([""] * 10)
            self.update_combo()
        
    def _clear_rois(self, _):
        reply = QMessageBox.question(
            self,
            "Clear ROI",
            "Clear all ROIs?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,  # default button
        )

        if reply == QMessageBox.StandardButton.Yes:
                self.clearROIs.emit()
        
        
    

    