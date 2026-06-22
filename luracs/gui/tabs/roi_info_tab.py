from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import ROI
from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QHBoxLayout,
    QSizePolicy,
    QComboBox,
    QMessageBox,
    QPushButton,
    QDialog,
    QTextEdit,
)
from PySide6.QtCore import Signal
from luracs.core import SpectrumManager, IOManager
from ..misc.idx_table import StrIdxTable
from ..save_to_internal_dialogs import save_roi_references


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
        self.combo.addItems(
            [r.alias for r in SpectrumManager.ROIManager.roi_registry.values()]
        )
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

            lines.extend(
                [
                    sep,
                    title,
                    sep,
                    f"Tag: {roi.tag}",
                    f"Spectrum: {roi.spectrum}",
                    f"Alias: {roi.alias}",
                    f"ROI Bound: {roi.roi_bound}",
                    f"Region Bound: {roi.region_bound}",
                    f"ROI Counts: {fmt(roi.roi_counts)}",
                    f"Live Time: {fmt(roi.live_time)}",
                    f"Background Type: {roi.bkg_type}",
                    f"Fit Type: {roi.fit_type}",
                    f"Meta: {roi.meta}",
                ]
            )

            # ---- Fit info ----
            if roi.fit is not None:
                f = roi.fit

                lines.extend(
                    [
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
                        "",
                    ]
                )
            else:
                lines.extend(["", "-- No Fit --", ""])

        self.text.setPlainText("\n".join(lines))


class ROIInfoTab(QWidget):
    clearROIs = Signal()

    def __init__(self, title="", parent=None):
        super().__init__(parent)

        # --- Signals ---
        SpectrumManager.ROIManager.sigROIUpdated.connect(self.recieve_roi)
        SpectrumManager.ROIManager.sigROIDeleted.connect(self.delete_roi)
        SpectrumManager.Signals.spectrumRemoved.connect(self.spectrum_deleted)

        # ---- UI ----
        self.group_box = QGroupBox(title)

        self.table = StrIdxTable()

        self.titles = [
            "ROI",
            "Spectrum",
            "Lower",
            "Upper",
            "Peak Center",
            "FWHM",
            "Peak Counts",
            "ROI Counts",
            "Nuclide",
            "Photo Peak",
            "Peak Difference",
        ]

        self.widths = [120, 120, 75, 75, 100, 100, 150, 150, 100, 100, 120]
        self.table.reset_table(self.titles, self.widths)

        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table.table, 1)


        # --- Top bar ---
        options_widget = QWidget()
        options_bar = QHBoxLayout(options_widget)
        options_bar.setContentsMargins(1, 0, 1, 0)

        btn_clear = QPushButton("Clear ROIs")
        btn_clear.clicked.connect(self._clear_rois)

        btn_view_info = QPushButton("View Full Info")
        btn_view_info.clicked.connect(self._open_info)

        btn_roi_cps = QPushButton("CPS")
        btn_roi_cps.setCheckable(True)
        btn_roi_cps.toggled.connect(SpectrumManager.ROIManager.set_cps)
        SpectrumManager.ROIManager.sigCpsChanged.connect(self.rebuild_table)

        btn_export_to_csv = QPushButton("Export to CSV")
        btn_export_to_csv.clicked.connect(IOManager.Exporter.export_roi_dialog)
        
        btn_export_roi_references = QPushButton("Export Reference ROIs")
        btn_export_roi_references.clicked.connect(save_roi_references)

        options_bar.addWidget(btn_clear)
        options_bar.addWidget(btn_view_info)
        options_bar.addWidget(btn_roi_cps)
        options_bar.addWidget(btn_export_to_csv)
        options_bar.addWidget(btn_export_roi_references)
        options_bar.addStretch()
        options_widget.setSizePolicy(QSizePolicy.Minimum, QSizePolicy.Preferred)

        # --- Main layout ---
        main_layout = QVBoxLayout(self)
        main_layout.addWidget(options_widget)
        main_layout.addWidget(self.group_box, 1)


        # Initial build
        self.rebuild_table()
        SpectrumManager.Signals.spectrumRenamed.connect(lambda: self.rebuild_table())

    # --------------------------------------------------
    # Core logic
    # --------------------------------------------------

    def rebuild_table(self):
        """Rebuild entire table from current ROI data."""
        self.table.reset_table(self.titles, self.widths)

        for spectrum_name, spectrum in SpectrumManager.get_spectra_dict().items():
            rois = SpectrumManager.ROIManager.get_data_from_spectrum(spectrum_name)

            for tag, roi in rois.items():
                self._put_roi(tag, spectrum_name, roi)

    def _put_roi(
        self, tag: str, spectrum_name: str, roi: ROI, none_fallback="Fit Failed"
    ):
        if not roi:
            return

        cps = SpectrumManager.ROIManager.spectrum_is_cps

        # ROI counts
        if cps:
            roi_counts = f"{round(roi.get_count_data('roi_counts', True), 4)} CPS"
        else:
            roi_counts = f"{int(roi.get_count_data('roi_counts', False)):,}".replace(
                ",", " "
            )

        if roi.fit is not None:
            peak_counts = (
                f"{int(roi.get_count_data('peak_counts')):,}".replace(",", " ")
                if not cps
                else f"{round(roi.get_count_data('peak_counts', True), 4)} CPS"
            )

            row = [
                roi.alias,
                spectrum_name,
                f"{round(roi.fit.lower)} keV",
                f"{round(roi.fit.upper)} keV",
                f"{round(roi.fit.mu, 2)} keV",
                f"{round(roi.fit.fwhm, 2)} keV",
                peak_counts,
                roi_counts,
                roi.emission.parent_nuclide if roi.emission else "None",
                f"{roi.emission.energy_keV} keV"
                if roi.emission and roi.emission.energy_keV is not None
                else "None",
                f"{round(roi.fit.mu - roi.emission.energy_keV, 3)} keV"
                if roi.emission and roi.emission.energy_keV is not None
                else "None",
            ]
        else:
            row = [
                roi.alias,
                spectrum_name,
                round(roi.roi_bound[0]),
                round(roi.roi_bound[1]),
                none_fallback,
                None,
                None,
                roi_counts,
                "None",
                "None",
                "None",
            ]

        # Unique row key
        row_key = f"{tag}_{spectrum_name}"
        self.table.write_row(row_key, row)

    # --------------------------------------------------
    # Signal handlers
    # --------------------------------------------------

    def recieve_roi(self, roi_tag, spectrum_name, roi):
        """Update or insert a single ROI row."""
        self._put_roi(roi_tag, spectrum_name, roi)

    def delete_roi(self, roi):
        """Remove all rows belonging to a deleted ROI."""
        keys_to_remove = [
            key for key in self.table.current_keys if key.startswith(f"{roi.tag}_")
        ]

        for key in keys_to_remove:
            self.table.delete_row(key)
            
    def spectrum_deleted(self, spectrum_name: str):
        keys_to_remove = [
            key for key in self.table.current_keys if key.endswith(f"{spectrum_name}")
        ]

        for key in keys_to_remove:
            self.table.delete_row(key)

    # --------------------------------------------------
    # UI actions
    # --------------------------------------------------

    def _clear_rois(self):
        reply = QMessageBox.question(
            self,
            "Clear ROI",
            "Clear all ROIs?",
            QMessageBox.StandardButton.Yes | QMessageBox.StandardButton.No,
            QMessageBox.StandardButton.No,
        )

        if reply == QMessageBox.StandardButton.Yes:
            self.clearROIs.emit()

    def _open_info(self):

        dialog = RoiInfoDialog(list(SpectrumManager.ROIManager.roi_registry.keys()))
        dialog.exec()
