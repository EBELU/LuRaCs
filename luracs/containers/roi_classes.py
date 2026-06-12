from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from core.nuclide_library import NuclideLibrary

from luracs.containers.nuclide_classes import Emission

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

    tag: str  # Internal tag
    alias: str  # Given name
    spectrum: str  # Name of the spectrum the roi was fitted to
    roi_bound: tuple  # Bounds of this roi
    region_bound: tuple  # Bounds of the roi group
    fit_type: str  # Can be "None" or "Gaussain", possibly more in the future
    bkg_type: str  # None, Linear or Quadratic
    fit: Fit | None  # Fitted peak
    roi_counts: float  # Counts in the region, just summed
    live_time: float  # Saved live time for cps conversion
    emission: Emission | None
    meta: dict  # Metadata

    def get_count_data(self, field: str, cps=False):
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





class DeletableROI(LinearRegionItem):
    """Visual ROI selector modified to have a 'tag' and can be deleted by right clicking"""

    sigDeleteRequested = Signal(str)
    sigSelected = Signal(str)
    sigSettingsUpdated = Signal(object)
    
    # Container components can not depend on gui components!
    # Dialog is set during initialisation of main
    roi_editor_dialog = None

    def __init__(
        self,
        tag: str,
        region: tuple,
        nuclide_lib_ref: NuclideLibrary,  # keep a reference to avoid circular imports
        alias: str | None = None,
        fit_type: str = "Gaussian",  # Gaussian, None
        bkg_type: str = "Linear",
        merge: bool = True,
        poisson_weights: bool = False,
        movable: bool = True,
        emission: Emission = None,
        owner_spectrum: str = None,
        **kwargs,
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
        
        # Self check
        if self.roi_editor_dialog is None:
            raise NotImplementedError("ROI Edit dialog has not been set properly")

    def mouseClickEvent(self, ev):
        if ev.button() == Qt.RightButton:
            ev.accept()
            editor = self.roi_editor_dialog(
                self.tag,
                self.alias,
                *self.getRegion(),
                fit_type=self.fit_type,
                bkg_type=self.bkg_type,
                merge=self.merge,
                poisson_weights=self.poisson_weights,
                movable=self.movable,
                emission=self.emission,
                nuclide_lib_ref=self.nuclide_lib_ref,
            )
            res = editor.exec()
            if res == self.roi_editor_dialog.DELETE:
                self.sigDeleteRequested.emit(self.tag)
            elif res:
                self.update_self(**editor.get_values())

        elif ev.button() == Qt.LeftButton:
            ev.accept()
            self.sigSelected.emit(self.tag)
        else:
            super().mouseClickEvent(ev)

    def update_self(
        self,
        roi_name=None,
        lower_bound=None,
        upper_bound=None,
        fit_type=None,
        bkg_type=None,
        merge=None,
        poisson_weights=None,
        movable=None,
        emission=None,
        signal_update=True,
    ):
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

        self.setToolTip(f"ROI: {self.alias}\nRight-click to edit or delete")
