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
    QPushButton,
    QDialog,
    QTextEdit
)
from PySide6.QtCore import Signal


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
        print(row_index)
        if row_index is None:
            return
        self.table.removeRow(row_index)
        for key in self.rowkeys:
            if self.rowkeys[key] > row_index:
                self.rowkeys[key] -= 1
        
        self.row_counter -= 1
        
class RoiInfoDialog(QDialog):
    def __init__(self, rois, parent=None):
        """
        rois: dict[str, ROI] or list[ROI]
        """
        super().__init__(parent)
        self.setWindowTitle("ROI Info Viewer")
        self.resize(500, 400)

        # Layout
        layout = QVBoxLayout(self)

        # Combo box for ROI selection
        self.combo = QComboBox()
        self.combo.addItems([r.alias for r in SpectrumManager.ROIManager.ROIs.values()])
        layout.addWidget(self.combo)

        # Text edit to display info
        self.text = QTextEdit()
        self.text.setReadOnly(True)
        layout.addWidget(self.text)

        # Connect combo selection
        self.combo.currentTextChanged.connect(self.update_text)

        # Initialize with first ROI
        if rois:
            self.update_text(self.combo.currentText())

    def update_text(self, tag):
        rois = SpectrumManager.ROIManager.get_data_from_roi(
            SpectrumManager.ROIManager.get_tag_from_alias(tag)
        )
        if rois is None:
            self.text.clear()
            return
        lines = []
        for spect, roi in rois.items():
            title = f"| {spect} |"
            lines.extend([
                "="*(len(title) - 2),
                title,
                "="*(len(title) - 2),
                f"Tag: {roi.tag}",
                f"Alias: {roi.alias}",
                f"ROI Bound: {roi.roi_bound}",
                f"Region Bound: {roi.region_bound}",
                f"Counts: {roi.counts}",
                f"Meta: {roi.meta}"
            ]
            )
            # Add fit info if present
            if roi.fit:
                f = roi.fit
                fit_lines = [
                    "\n-- Fit Info --",
                    f"Fit Type: {f.fit_type}",
                    f"Region: [{f.region_lower}, {f.region_upper}]",
                    f"Lower/Upper: [{f.lower}, {f.upper}]",
                    f"Params: {f.params}",
                    f"Param Errors: {f.param_errs}",
                    f"A = {f.A} ± {f.A_err}",
                    f"mu = {f.mu} ± {f.mu_err}",
                    f"sigma = {f.sigma} ± {f.sigma_err}",
                    f"FWHM = {f.fwhm} ± {f.fwhm_err}",
                    f"Background Type: {f.bkg_type}",
                    f"Background Params: {f.bkg_params}",
                    f"G = {f.G}\nB = {f.B}\nN = {f.N}\nPeak Counts = {f.peak_counts}",
                    "\n\n"
                ]
                lines.extend(fit_lines)
            else:
                lines.append("\n-- No Fit --")

        self.text.setPlainText("\n".join(map(str, lines)))

class ROIInfoTab(QWidget):
    clearROIs = Signal()
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        # --- Signals ---
        SpectrumManager.ROIManager.sigROIUpdated.connect(self.recieve_roi)
        SpectrumManager.ROIManager.sigROIDeleted.connect(self.delete_roi)
        
        # ---- Box ----
        self.group_box = QGroupBox(title)

        # ---- Table ----
        titles = ["ROI", "Low", "High", "Peak Center", "FWHM", "Max Height","G", "B", "N",]
        self.table = StrIdxTable()
        self.table.reset_table(titles)
        
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
        
        btn_clear = QPushButton("Clear ROIs")
        btn_clear.clicked.connect(self._clear_rois)
        
        btn_view_info = QPushButton("View Full Info")
        btn_view_info.clicked.connect(self._open_info)
        

        options_bar.addWidget(self.combo)
        options_bar.addWidget(btn_by_spect)
        options_bar.addWidget(btn_by_roi)
        options_bar.addWidget(btn_clear)
        options_bar.addWidget(btn_view_info)
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
        
    def _open_info(self):
        info_box = RoiInfoDialog(list(SpectrumManager.ROIManager.ROIs.keys()))
        info_box.exec()


    def update_combo(self):
        """Rebuild combo safely without recursive signals."""
        self.combo.blockSignals(True)
        self.combo.clear()

        if self.combo_show_spectrum:
            for key in SpectrumManager.get_spectra_dict().keys():
                self.combo.addItem(key)
        else:
            for roi in SpectrumManager.ROIManager.ROIs.values():
                self.combo.addItem(roi.alias)

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

        rois = SpectrumManager.ROIManager.get_data_from_spectrum(spectrum)
        

        titles = ["ROI", "Lower", "Upper", "Peak Center",
                "FWHM", "Peak Counts"]

        self.table.reset_table(titles)
        


        for tag, roi in rois.items():
            if not SpectrumManager.get_spectrum(spectrum).fit_rois:
                self.table.write_row(roi.alias, ["Spectrum"] +["Hidden"] + [""] * (len(titles) - 1))
                return
            else:
                self._put_roi(roi.alias, roi)


    def set_table_by_roi(self, roi_idx: int):
        roi_tag = SpectrumManager.ROIManager.get_tag_from_alias(self.combo.itemText(roi_idx))
        if not roi_tag:
            return

        rois = SpectrumManager.ROIManager.get_data_from_roi(roi_tag)

        titles = ["Spectrum", "Lower", "Upper", "Peak Center",
                "FWHM", "Peak Counts"]

        self.table.reset_table(titles)

        for tag, roi in rois.items():
            self._put_roi(tag, roi)


    def _put_roi(self, tag: str, roi, none_fallback: str = "Fit Failed"):
        if not roi:
            return

        if roi.fit is not None:
            row = [
                tag,
                round(roi.fit.lower),
                round(roi.fit.upper),
                round(roi.fit.mu, 4),
                round(roi.fit.fwhm, 4),
                round(roi.fit.peak_counts, 4)
            ]
        else:
            row = [tag, round(roi.roi_bound[0]), round(roi.roi_bound[1]), none_fallback] + [None] * 6

        self.table.write_row(tag, row)
        
    
    def recieve_roi(self, roi_tag): 
        rois = SpectrumManager.ROIManager.get_data_from_roi(roi_tag)
        
        for roi in rois.values():
            self._put_roi(roi.alias, roi)
        
        self.update_combo()

    def delete_roi(self, roi):
        if self.combo_show_spectrum:
            self.table.delete_row(roi.alias)
        else:
            # ROI view
            current_roi = self.combo.currentText()
            if roi.alias == current_roi:
                self.set_table_by_roi(self.combo.currentIndex())
        
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
        
        
    

    