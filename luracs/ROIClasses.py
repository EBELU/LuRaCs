from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from core.nuclide_library import NuclideLibrary

from NuclideClasses import Emission

from PySide6.QtCore import Signal, Qt
from pyqtgraph import LinearRegionItem
from dataclasses import dataclass
import numpy as np


@dataclass(frozen=True)
class Fit:
    "The results of a peak fitting in roi"

    # Full energy region if the roi is part of a group
    region_lower: float
    region_upper: float

    # Energy bounds of this specific roi. Redundant?
    lower: float
    upper: float

    # Fit parameters
    params: (
        np.ndarray
    )  # List of parameters so that it can be fed into the function with *params
    param_errs: np.ndarray  # Uncertainties from the optimization

    bkg_params: np.ndarray

    # --- Assumed Gaussian ---
    G: float
    B: float
    N: float
    peak_counts: float

    @property
    def A(self):
        return self.params[0]

    @property
    def mu(self):
        return self.params[1]

    @property
    def sigma(self):
        return np.sqrt(self.params[2])

    @property
    def fwhm(self):
        return 2.354820045 * np.sqrt(self.params[2])

    @property
    def A_err(self):
        return self.param_errs[0]

    @property
    def mu_err(self):
        return self.param_errs[1]

    @property
    def sigma_err(self):
        return self.param_errs[2] / (2 * self.sigma)

    @property
    def fwhm_err(self):
        return 2.354820045 * self.sigma_err


@dataclass
class ROI:
    "A dataclass representing a region of interest in spectrum. The region might contain a fitted peak."
    tag: str # Internal tag
    alias: str # Given name
    roi_bound: tuple # Bounds of this roi
    region_bound: tuple # Bounds of the roi group
    fit_type: str # Can be "None" or "Gaussain", possible more in the future
    bkg_type: str # None, Linear or Quadratic
    fit: Fit | None # Fitted peak
    roi_counts: float # Counts in the region, just summed
    live_time: float # Saved live time for cps conversion
    emission: Emission | None
    meta: dict # Metadata
    
    def get_count_data(self, field: str, cps = False):
        "Get data from the fit and the region counts, normalises to measurement time if requested"
        if not self.live_time and cps:
            return

        if field == "roi_counts":
            return self.roi_counts / self.live_time if cps else self.roi_counts

        elif field in ("A", "A_err", "G", "B", "N", "peak_counts"):
            return (
                getattr(self.fit, field) / self.live_time
                if cps
                else getattr(self.fit, field)
            )

        else:
            attr = getattr(self.fit, field, None)
            if attr is None:
                raise ValueError(f"Invalid field for roi data! {field}")
            else:
                return attr

from PySide6.QtWidgets import (QDialog, QVBoxLayout, QFormLayout, 
    QLineEdit, QDoubleSpinBox, QComboBox, QCheckBox, 
    QDialogButtonBox, QPushButton)

class ROIEditor(QDialog):
    DELETE = 2
    def __init__(
                self, roi_tag: str,
                roi_name:str,
                low: float, 
                high: float, 
                fit_type: str,
                bkg_type: str,
                merge: bool, 
                poisson_weights: bool, 
                movable: bool, 
                emission: Emission,
                nuclide_lib_ref: NuclideLibrary,
                title="", parent=None
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
        form.addRow("ROI Name:", self.roi_name)

        self.lower_bound = QDoubleSpinBox()
        self.lower_bound.setRange(0.0, 1e6)  # adjust as needed
        self.lower_bound.setDecimals(2)
        self.lower_bound.setSuffix(" keV")
        self.lower_bound.setValue(round(low))
        form.addRow("Lower Bound:", self.lower_bound)

        self.upper_bound = QDoubleSpinBox()
        self.upper_bound.setRange(0.0, 1e6)
        self.upper_bound.setDecimals(2)
        self.upper_bound.setSuffix(" keV")
        self.upper_bound.setValue(round(high))  # example default
        form.addRow("Higher Bound:", self.upper_bound)

        self.fit_type = QComboBox()
        self.fit_type.addItems(["None", "Gaussian"])
        self.fit_type.setCurrentText(fit_type)
        form.addRow("Peak Function:", self.fit_type)

        self.bkg_type = QComboBox()
        self.bkg_type.addItems(["None", "Linear", "Quadratic"])
        self.bkg_type.setCurrentText(bkg_type)
        form.addRow("Background:", self.bkg_type)
        
        self.nuclide = QComboBox()
        # self.nuclide.setView(QListView())  # forces Qt view
        self.nuclide.addItems(["None"] + self.nuclide_lib_ref.get_sorted_nuclide_names())
        self.nuclide.setMaxVisibleItems(10)
        self.nuclide.currentTextChanged.connect(self._update_emissions)
        form.addRow("Nuclide:", self.nuclide)
        
        self.photo_peak = QComboBox()
        self.photo_peak.addItems(["None"])
        
        form.addRow("Photopeak:",self.photo_peak)
        
        auto_match = QPushButton()
        auto_match.setText("Auto Match Nuclide")
        auto_match.clicked.connect(self._auto_match_nuclide)
        
        form.addRow("\t\t", auto_match)
        
        
        self.merge = QCheckBox("Allow merging")
        self.merge.setChecked(merge)
        form.addRow("\t\t", self.merge)

        self.movable = QCheckBox("Movable")
        self.movable.setChecked(movable)
        form.addRow("\t\t", self.movable)

        self.poisson_weights = QCheckBox("Use Poisson Weights")
        self.poisson_weights.setChecked(poisson_weights)
        form.addRow("\t\t", self.poisson_weights)

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
            "merge": self.merge.isChecked(),
            "movable": self.movable.isChecked(),
            "poisson_weights": self.poisson_weights.isChecked(),
            "emission": self.photo_peak.currentData() if self.nuclide.currentText() != "None" else Emission(parent_nuclide="None", 
                                                                       energy_keV=None, 
                                                                       energy_error_keV=None, 
                                                                       intensity_percent=None,
                                                                       intensity_error_percent=None,
                                                                       origin="",
                                                                       type="")
        }


class DeletableROI(LinearRegionItem):
    """Visual ROI selector modified to have a 'tag' and can be deleted by right clicking"""

    sigDeleteRequested = Signal(str)
    sigSelected = Signal(str)
    sigSettingsUpdated = Signal(object)

    def __init__(
        self,
        tag: str,
        region: tuple,
        nuclide_lib_ref: NuclideLibrary, # keep a reference to avoid circular imports
        alias: str | None = None,
        fit_type: str = "Gaussian", # Gaussian, None
        bkg_type: str = "Linear",
        merge: bool = True,
        poisson_weights: bool = False,
        movable: bool = True,
        emission: Emission = None,
        owner_spectrum: str = None,
        **kwargs
        
    ):
        super().__init__(values=region, orientation="vertical", movable=movable)
        self.setZValue(25)
        self.tag: str = tag
        self.merge: bool = merge
        self.perform_fit: bool = True
        self.alias: str = alias if alias else tag
        self.fit_type: str = fit_type
        self.bkg_type: str = bkg_type
        self.poisson_weights: bool = poisson_weights
        self.emission: Emission = emission
        self.nuclide_lib_ref = nuclide_lib_ref
        self.owner_spectrum: str = owner_spectrum
        
        self.setToolTip(f"ROI: {self.alias}\nRight-click to edit")

    def mouseClickEvent(self, ev):
        if ev.button() == Qt.RightButton:
            ev.accept()
            editor = ROIEditor(self.tag, self.alias, 
                          *self.getRegion(), 
                          fit_type=self.fit_type, 
                          bkg_type=self.bkg_type, 
                          merge=self.merge, 
                          poisson_weights=self.poisson_weights, 
                          movable=self.movable,
                          emission=self.emission,
                          nuclide_lib_ref=self.nuclide_lib_ref)
            res = editor.exec()
            if res == ROIEditor.DELETE:
                self.sigDeleteRequested.emit(self.tag)
            elif res:
                self.update_self(**editor.get_values())

        elif ev.button() == Qt.LeftButton:
            ev.accept()
            self.sigSelected.emit(self.tag)
        else:
            super().mouseClickEvent(ev)

    def update_self(
        self, roi_name=None, lower_bound=None, upper_bound=None, fit_type=None, bkg_type=None, merge=None, poisson_weights=None, movable=None, emission=None, signal_update=True):
        if roi_name is not None:
            self.alias = roi_name

        if lower_bound is not None and upper_bound is not None:
            self.setRegion([lower_bound, upper_bound])

        if fit_type is not None:
            self.fit_type = fit_type

        if bkg_type is not None:
            self.bkg_type = bkg_type

        if merge is not None:
            self.merge = merge

        if poisson_weights is not None:
            self.poisson_weights = poisson_weights

        if emission is not None:
            self.emission = emission

        if movable is not None:
            self.setMovable(movable)

        if signal_update:
            self.sigSettingsUpdated.emit(self)
            
        self.setToolTip(f"ROI: {self.alias}\nRight-click to edit")
        
