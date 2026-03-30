from PySide6.QtCore import Signal, Qt
from pyqtgraph import LinearRegionItem
from GUI_components.popup_windows.roi_editor import ROIEditor
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
    fit_type: str # Can be "None" or "Gaussain", possible more in the future
    params: np.array # List of parameters so that it can be fed into the function with *params
    param_errs: np.array # Uncertainties from the optimization
    
    bkg_type: str # None, Linear or Quadratic
    bkg_params: np.array
    
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
    fit: Fit | None # Fitted peak
    roi_counts: float # Counts in the region, just summed
    live_time: float # Saved live time for cps conversion
    meta: dict # Metadata
    
    def get_count_data(self, field: str, cps = False):
        "Get data from the fit and the region counts, normalises to measurement time if requested"
        if not self.live_time and cps:
            return
        
        if field == "roi_counts":
            return self.roi_counts / self.live_time if cps else self.roi_counts
        
        elif field in ("A", "A_err", "G", "B", "N", "peak_counts"):
            return getattr(self.fit, field) / self.live_time if cps else getattr(self.fit, field)
        
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
    def __init__(
        self,
        tag: str,
        region,
        alias = None,
        fit_type = "Gaussian",
        bkg_type = "Linear",
        merge = True,
        poisson_weights = False,
        movable = True,
        
    ):
        super().__init__(
            values=region,
            orientation="vertical",
            movable=movable
        )
        self.setZValue(25)
        self.tag = tag
        self.merge = merge
        self.perform_fit = True
        self.alias = alias if alias else tag
        self.fit_type = fit_type
        self.bkg_type = bkg_type
        self.poisson_weights = poisson_weights

        self.setToolTip(f"ROI: {self.alias}\nRight-click to edit")
        

    def mouseClickEvent(self, ev):
        if ev.button() == Qt.RightButton:
            ev.accept()
            editor = ROIEditor(self.alias, 
                          *self.getRegion(), 
                          self.fit_type, 
                          self.bkg_type, 
                          self.merge, 
                          self.poisson_weights, 
                          self.movable)
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
        self, roi_name=None, lower_bound=None, upper_bound=None, fit_type=None, bkg_type=None, merge=None, poisson_weights=None, movable=None, signal_update=True):
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

        if movable is not None:
            self.setMovable(movable)

        if signal_update:
            self.sigSettingsUpdated.emit(self)