from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.core.nuclide_library import NuclideLibrary

from luracs.containers.nuclide_classes import Emission


from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QFormLayout,
    QLineEdit,
    QDoubleSpinBox,
    QComboBox,
    QCheckBox,
    QDialogButtonBox,
    QPushButton,
    QSpinBox,
)


class ROIEditor(QDialog):
    DELETE = 2

    def __init__(
        self,
        roi_tag: str,
        roi_name: str,
        low: float,
        high: float,
        fit_type: str,
        bkg_type: str,
        bkg_est_channels: int,
        merge: bool,
        poisson_weights: bool,
        movable: bool,
        emission: Emission,
        nuclide_lib_ref: NuclideLibrary,
        title="",
        parent=None,
    ):
        super().__init__(parent=parent)

        self.setWindowTitle("ROI Editor")
        self.setMinimumWidth(150)
        self.setMinimumHeight(300)
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)
        self.nuclide_lib_ref = nuclide_lib_ref
        self.roi_tag = roi_tag

        self.layout = main_layout
        form = QFormLayout()
        form.setSpacing(9)

        self.roi_name = QLineEdit()
        self.roi_name.setText(roi_name)
        form.addRow("ROI Name", self.roi_name)

        self.lower_bound = QDoubleSpinBox()
        self.lower_bound.setRange(0.0, 1e6)
        self.lower_bound.setDecimals(2)
        self.lower_bound.setSuffix(" keV")
        self.lower_bound.setValue(round(low))
        form.addRow("Lower Bound", self.lower_bound)

        self.upper_bound = QDoubleSpinBox()
        self.upper_bound.setRange(0.0, 1e6)
        self.upper_bound.setDecimals(2)
        self.upper_bound.setSuffix(" keV")
        self.upper_bound.setValue(round(high))  # example default
        form.addRow("Higher Bound", self.upper_bound)

        self.fit_type = QComboBox()
        self.fit_type.addItems(["None", "Gaussian"])
        self.fit_type.setCurrentText(fit_type)
        form.addRow("Peak Function", self.fit_type)

        self.bkg_type = QComboBox()
        self.bkg_type.addItems(["None", "Linear", "Quadratic"])
        self.bkg_type.setCurrentText(bkg_type)
        form.addRow("Background", self.bkg_type)
        
        self.bkg_est_channels = QSpinBox()
        self.bkg_est_channels.setRange(1, 32)
        self.bkg_est_channels.setValue(bkg_est_channels)
        form.addRow("Edge Channels", self.bkg_est_channels)

        self.nuclide = QComboBox()
        # self.nuclide.setView(QListView())  # forces Qt view
        self.nuclide.addItems(
            ["None"]
            + self.nuclide_lib_ref.get_sorted_nuclide_names(
                require_photon_emissions=True
            )
        )
        self.nuclide.setMaxVisibleItems(10)
        self.nuclide.currentTextChanged.connect(self._update_emissions)
        form.addRow("Nuclide", self.nuclide)

        self.photo_peak = QComboBox()
        self.photo_peak.addItems(["None"])

        form.addRow("Photopeak", self.photo_peak)

        auto_match = QPushButton()
        auto_match.setText("Auto Match Nuclide")
        auto_match.clicked.connect(self._auto_match_nuclide)

        form.addRow("", auto_match)

        self.merge = QCheckBox("Allow merging")
        self.merge.setChecked(merge)
        form.addRow("", self.merge)

        self.movable = QCheckBox("Movable")
        self.movable.setChecked(movable)
        form.addRow("", self.movable)

        self.poisson_weights = QCheckBox("Use Poisson Weights")
        self.poisson_weights.setChecked(poisson_weights)
        form.addRow("", self.poisson_weights)

        def update_button_state():
            is_gaussian = self.fit_type.currentText() == "Gaussian"
            self.poisson_weights.setEnabled(is_gaussian)
            if not is_gaussian:
                self.merge.setChecked(False)
            self.merge.setEnabled(is_gaussian)
            self.bkg_type.setEnabled(is_gaussian)

        self.fit_type.currentTextChanged.connect(update_button_state)

        main_layout.addLayout(form)

        update_button_state()

        # --- Bottom Buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        delete_button = QPushButton("Delete")
        delete_button.clicked.connect(self.on_delete)
        buttons.addButton(delete_button, QDialogButtonBox.ActionRole)

        main_layout.addWidget(buttons)

        if emission is not None:
            self.nuclide.setCurrentText(emission.parent_nuclide)

            for i in range(self.photo_peak.count()):
                data = self.photo_peak.itemData(i)
                if data == emission:
                    self.photo_peak.setCurrentIndex(i)
                    return

    def _update_emissions(self, nuclide: str):
        emissions = self.nuclide_lib_ref.get_nuclide(nuclide)

        self.photo_peak.clear()

        if emissions is None:
            return

        emissions = emissions.emissions

        for e in emissions:
            text = f"{e.energy_keV} keV   {e.intensity_percent}%"
            self.photo_peak.addItem(text, e)

    def _auto_match_nuclide(self, match: Emission = None):
        match = self.nuclide_lib_ref.match_roi_to_nuclide(self.roi_tag)
        if match is None:
            return

        self.nuclide.setCurrentText(match.parent_nuclide)

        for i in range(self.photo_peak.count()):
            data = self.photo_peak.itemData(i)
            if data == match:
                self.photo_peak.setCurrentIndex(i)
                return

    def on_delete(self):
        self.done(self.DELETE)

    def get_values(self):
        return {
            "roi_name": self.roi_name.text(),
            "lower_bound": self.lower_bound.value(),
            "upper_bound": self.upper_bound.value(),
            "fit_type": self.fit_type.currentText(),
            "bkg_type": self.bkg_type.currentText(),
            "bkg_est_channels": self.bkg_est_channels.value(),
            "merge": self.merge.isChecked(),
            "movable": self.movable.isChecked(),
            "poisson_weights": self.poisson_weights.isChecked(),
            "emission": self.photo_peak.currentData()
            if self.nuclide.currentText() != "None"
            else Emission(
                parent_nuclide="None",
                energy_keV=None,
                energy_error_keV=None,
                intensity_percent=None,
                intensity_error_percent=None,
                origin="",
                type="",
            ),
        }
