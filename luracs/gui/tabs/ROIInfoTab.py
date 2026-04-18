from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from ROIClasses import ROI, Fit
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
    QTextEdit,
    QAbstractItemView
)
from PySide6.QtCore import Signal
from core import SpectrumManager
from ..misc.idx_table import StrIdxTable
        
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

        if not rois:
            self.text.clear()
            return

        def fmt(val):
            """Format numbers safely."""
            if isinstance(val, float):
                return f"{val:.5g}"
            return str(val)

        lines = []

        for spect, roi in rois.items():
            title = f"| {spect} |"
            sep = "=" * len(title)

            lines.extend([
                sep,
                title,
                sep,
                f"Tag: {roi.tag}",
                f"Alias: {roi.alias}",
                f"ROI Bound: {roi.roi_bound}",
                f"Region Bound: {roi.region_bound}",
                f"ROI Counts: {fmt(roi.roi_counts)}",
                f"Live Time: {fmt(roi.live_time)}",
                f"Background Type: {roi.bkg_type}",
                f"Fit Type: {roi.fit_type}",
                f"Meta: {roi.meta}",
            ])

            # ---- Fit info ----
            if roi.fit is not None:
                f = roi.fit

                lines.extend([
                    "",
                    "-- Fit Info --",
                    f"Region: [{fmt(f.region_lower)}, {fmt(f.region_upper)}]",
                    f"Bounds: [{fmt(f.lower)}, {fmt(f.upper)}]",
                    f"Params: {f.params}",
                    f"Param Errors: {f.param_errs}",
                    "",
                    f"A = {fmt(f.A)} ± {fmt(f.A_err)}",
                    f"mu = {fmt(f.mu)} ± {fmt(f.mu_err)}",
                    f"sigma = {fmt(f.sigma)} ± {fmt(f.sigma_err)}",
                    f"FWHM = {fmt(f.fwhm)} ± {fmt(f.fwhm_err)}",
                    "",
                    f"Background Params: {f.bkg_params}",
                    "",
                    f"G = {fmt(f.G)}",
                    f"B = {fmt(f.B)}",
                    f"N = {fmt(f.N)}",
                    f"Peak Counts = {fmt(f.peak_counts)}",
                    ""
                ])
            else:
                lines.extend([
                    "",
                    "-- No Fit --",
                    ""
                ])

        self.text.setPlainText("\n".join(lines))

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
        self.table = StrIdxTable()
        setattr(self.table, "mode", None) # Ugly trick <-
        
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
        """Rebuild combo safely without recursive signals and preserve selection."""
        self.combo.blockSignals(True)

        # Remember current text
        current_text = self.combo.currentText()

        self.combo.clear()
        
        items = []
        if self.combo_show_spectrum:
            items = list(SpectrumManager.get_spectra_dict().keys())
        else:
            items = [roi.alias for roi in SpectrumManager.ROIManager.ROIs.values()]

        self.combo.addItems(items)

        # Restore selection if it still exists
        if current_text in items:
            index = items.index(current_text)
            self.combo.setCurrentIndex(index)
        elif items:
            self.combo.setCurrentIndex(0)  # default to first if previous missing

        self.combo.blockSignals(False)

        # Trigger table update manually
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
        

        titles = ["ROI", "Lower", "Upper", "Peak Center", "FWHM", "Peak Counts", "ROI Counts", "Nuclide", "Photo Peak", "Peak Difference"]
        widths = [150, 75, 75, 100, 100, 150, 150, 75, 100, 100]

        if self.table.mode != 1:
            self.table.mode = 1
            self.table.reset_table(titles, widths)


        for tag, roi in rois.items():
            if not SpectrumManager.get_spectrum(spectrum).fit_rois:
                self.table.write_row(tag, ["Spectrum"] + ["Hidden"] + [""] * (len(titles) - 1))
                return
            else:
                self._put_roi(tag, roi)


    def set_table_by_roi(self, roi_idx: int):
        roi_tag = SpectrumManager.ROIManager.get_tag_from_alias(self.combo.itemText(roi_idx))
        if not roi_tag:
            return

        rois = SpectrumManager.ROIManager.get_data_from_roi(roi_tag)

        titles = ["Spectrum", "Lower", "Upper", "Peak Center", "FWHM", "Peak Counts", "ROI Counts", "Nuclide", "Photo Peak", "Peak Difference"]
        widths = [150, 75, 75, 100, 100, 150, 150, 75, 100, 100]
        
        if self.table.mode != 0:
            self.table.mode = 0
            self.table.reset_table(titles, widths)

        for tag, roi in rois.items():
            self._put_roi(tag, roi)


    def _put_roi(self, tag: str, roi: ROI,  none_fallback: str = "Fit Failed"):
        if not roi:
            return
        if self.combo_show_spectrum:
            first_cell = roi.alias
        else:
            first_cell = roi.meta["spectrum_name"]
        
        cps = SpectrumManager.ROIManager.spectrum_is_cps # alias
        if cps:
            roi_counts = f"{round(roi.get_count_data("roi_counts", True), 4)} CPS"
        else:
            roi_counts = f"{int(roi.get_count_data("roi_counts", False)):,}".replace(",", " ")        

        
        if roi.fit is not None:
            peak_counts = f"{int(roi.get_count_data("peak_counts")):,}".replace(",", " ") if not cps else f"{round(roi.get_count_data("peak_counts", True), 4)} CPS"
            row = [
                first_cell,
                f"{round(roi.fit.lower)} keV",
                f"{round(roi.fit.upper)} keV",
                f"{round(roi.fit.mu, 2)} keV",
                f"{round(roi.fit.fwhm, 2)} keV",
                peak_counts,
                roi_counts,
                roi.emission.parent_nuclide if roi.emission is not None else "None",
                f"{roi.emission.energy_keV} keV" if roi.emission is not None else "None",
                f"{round(roi.fit.mu - roi.emission.energy_keV,3)} keV" if roi.emission is not None and roi.fit is not None else "None",
            ]
        else:
            row = [first_cell, round(roi.roi_bound[0]), round(roi.roi_bound[1]), none_fallback, None, None, roi_counts]

        self.table.write_row(tag, row)
        
    
    def recieve_roi(self, roi_tag, spectrum_name, roi):
        if self.combo_show_spectrum:
            if spectrum_name not in [self.combo.itemText(i) for i in range(self.combo.count())]:
                self.update_combo()
                
        if spectrum_name != self.combo.currentText() and self.combo.currentText() != "":
            return
        self._put_roi(roi_tag, roi)
                
        if not self.table.has_been_set:
            self.update_combo()
        

    def delete_roi(self, roi):
        if self.combo_show_spectrum:
            self.table.delete_row(roi.tag)
        else:
            # ROI view
            current_roi = self.combo.currentText()
            if roi.tag == current_roi:
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
        
        
    

    