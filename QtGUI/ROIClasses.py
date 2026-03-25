from PySide6.QtCore import Signal, Qt
from pyqtgraph import LinearRegionItem
from GUI_components.popup_windows.roi_editor import ROIEditor
from dataclasses import dataclass
import numpy as np
    
@dataclass
class Fit:
    region_lower: float
    region_upper: float
    
    lower: float
    upper: float

    fit_type: str
    params: np.array
    param_errs: np.array
    
    bkg_type: str
    bkg_params: str
    
    G: float
    B: float
    N: float
    peak_counts: float
    
    live_time: float = 0
    
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
        return self.param_errs[2] / (2* self.sigma)
    @property
    def fwhm_err(self):
        return 2.354820045 * self.sigma_err
    
    def get_as_cps(self, variable):
        if variable not in ("A", "A_err", "G", "B", "N", "peak_counts"):
            raise ValueError(f"Invalid variable for CPS conversion: {variable}")

        if not self.live_time:
            return
        else:
            return getattr(self, variable) / self.live_time
    
@dataclass
class ROI:
    tag: str
    alias: str
    roi_bound: tuple
    region_bound: tuple
    fit: Fit | None
    counts: float
    meta: dict
    

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

        self.setToolTip(f"ROI: {self.alias}\nRight-click to delete")
        

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