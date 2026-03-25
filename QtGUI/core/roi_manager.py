from PySide6.QtCore import Signal, QObject
from PySide6.QtGui import QColor
from ROIClasses import DeletableROI, Fit, ROI
from core import Settings
import numpy as np
from utils.numerics import curve_fit, poisson_weights, multi_gaussian, multi_gaussian_jacobian
import pyqtgraph as pg
from SpectrumClasses import Spectrum
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from .spectrum_manager import SpectrumManagerBase
    from pyqtgraph import PlotWidget

pastel_colors = [
    "#FFB3BA",  # soft pink
    "#FFDFBA",  # peach
    "#FFFFBA",  # light yellow
    "#BAFFC9",  # mint green
    "#BAE1FF",  # baby blue
    "#D5BAFF",  # lavender
    "#FFBAED",  # pastel magenta
    "#C2F0FC",  # light cyan
    "#F3E5AB",  # pastel sand
    "#E6CCFF",  # pale violet
]

def fit_gaussians(x_axis, y_axis, bounds, spectrum_live_time, 
                  fit_type, use_poisson_weights, weigh_cov_chi2,
                  bkg_type, bkg_fit_extension,):
    region_min, region_max = np.min(bounds), np.max(bounds)
    
    region = (region_min <= x_axis) & (x_axis <= region_max)
    x_region = x_axis[region].copy().astype(float)
    y_region = y_axis[region].copy().astype(float)
    
    p0 = []
    for b in bounds:
        lower, upper = np.min(b), np.max(b)
    
        # mask for this peak window
        peak_mask = (lower <= x_region) & (x_region <= upper)
    
        x_peak = x_region[peak_mask]
        y_peak = y_region[peak_mask]
    
        if len(x_peak) == 0:
            continue  # skip empty regions
    
        # Initial guesses
        A0 = np.max(y_peak)                 
        mu0 = x_peak[np.argmax(y_peak)]      
        s0 = (upper - lower) / 6.0           
    
        p0.extend([A0, mu0, s0])
        
    p0s = np.array(p0)
        
    
    if bkg_type != "None":
        i_low = np.searchsorted(x_axis, region_min)
        i_high = np.searchsorted(x_axis, region_max)
        
        bkg_extention_lower = i_low - bkg_fit_extension
        if bkg_extention_lower < 0: bkg_extention_lower = 0
        
        lower_bkg_points_x = x_axis[bkg_extention_lower:i_low]
        lower_bkg_points_y = y_axis[bkg_extention_lower:i_low]
        
        bkg_extention_upper = i_high + bkg_fit_extension
        if bkg_extention_upper > len(x_axis): bkg_extention_lower = len(x_axis)
        
        upper_bkg_points_x = x_axis[i_high:bkg_extention_upper]
        upper_bkg_points_y = y_axis[i_high:bkg_extention_upper]
        
        if bkg_type == "Linear":
            poly_order = 1
        elif bkg_type == "Quadratic":
            poly_order = 2
        else:
            raise ValueError(f"Invald background type {bkg_type}")
        
        bkg_fit = np.polyfit(np.concatenate((lower_bkg_points_x, upper_bkg_points_x)), 
                             np.concatenate((lower_bkg_points_y, upper_bkg_points_y)), 
                             poly_order)
        
        y_region -= np.polyval(bkg_fit, x_region)
    
    else:
        bkg_fit = None
            
    p0 = p0s.flatten()
    
    weight = None
    if use_poisson_weights:
        weight = poisson_weights
    
    fits, cov, converged = curve_fit(multi_gaussian, x_region, y_region, p0, jac=multi_gaussian_jacobian, weight_fn=weight,
                                    weight_cov_chi2=weigh_cov_chi2)
    
    if np.any(fits > 1e12) or np.any(fits == np.nan) or np.any(np.diag(cov) < 0) or np.any(np.sqrt(np.diag(cov)) > 1e12):
        return None, False
    
    fits = fits.reshape(-1, 3)
    errs = np.sqrt(np.diag(cov)).reshape(-1, 3)
    
    results = []
    for b, fit, err in zip(bounds, fits, errs):
        lower, upper = np.min(b), np.max(b)
    
        peak_mask = (lower <= x_axis) & (x_axis<= upper)
    
        x_peak = x_axis[peak_mask]
        y_peak = y_axis[peak_mask]
        
        G = np.sum(y_peak)
        B = np.sum(np.polyval(bkg_fit, x_peak))
        N = G - B
        
        peak_counts = np.sum(multi_gaussian(x_region, fit))
        fit_data = Fit(
            region_min, region_max,
            lower, upper,
            fit_type, fit, err,
            bkg_type, bkg_fit,
            G, B, N, peak_counts,
            spectrum_live_time)
        results.append(fit_data)
    
    return results, converged


class ROIManager(QObject):
    sigROICreated = Signal(object)
    sigROIUpdated = Signal(str, str, object)
    sigROIUpdatedNames = Signal(str)
    sigROIDeleted = Signal(object)
    def __init__(self, spectrum_manager, title = "", parent = None):
        super().__init__(parent = parent)
        self.spectrum_manager: SpectrumManagerBase = spectrum_manager
        
        self.ROIs: dict[str, DeletableROI] = {}
        self.roi_counter: int = 0
        self.plot_widget: PlotWidget = None
        
        self.spectrum_is_log = False
        self.spectrum_is_cps = False
        self.spectrum_is_bkg_sub = False
        
        self.roi_groupings: list[set[str]] = []
        
        
    def set_plot(self, plot_widget):
        self.plot_widget = plot_widget
        
    def set_log(self, log_bool):
        self.spectrum_is_log = log_bool
    
    def set_bkg_sub(self, bkg_sub_bool):
        self.spectrum_is_bkg_sub = bkg_sub_bool
        
    def set_cps(self, cps_bool):
        self.spectrum_is_cps = cps_bool
        
    def add_roi(self, x_low = None, x_high = None):
        assert self.plot_widget is not None, "Plot item has not been set"
        
        roi_tag = f"ROI_{self.roi_counter}"
        self.roi_counter += 1
        
        # Pick a good position in the plit to spawn the new roi
        x_min, x_max = self.plot_widget.viewRange()[0]


        diff = float(x_max) - float(x_min)
        if diff > 400: diff = 400
        if x_low is None or x_low == 0: 
            x_low = float(x_min) + diff * 0.15 
        if x_high is None: 
            x_high = float(x_min) + diff * 0.45
            

        new_roi = DeletableROI(roi_tag,[x_low, x_high])
        self.plot_widget.addItem(new_roi)
        self.ROIs[roi_tag] = new_roi
        
        new_roi.sigDeleteRequested.connect(self.remove_roi)
        new_roi.sigRegionChangeFinished.connect(
            lambda: self.on_roi_change(roi_tag = new_roi.tag)
        )
        
        new_roi.sigSelected.connect(self.select_roi)
        new_roi.sigSettingsUpdated.connect(self.propagrade_roi_settings_change)
        
        self.on_roi_change(roi_tag = new_roi.tag)
        
    def remove_roi(self, roi_tag: str) -> None:
        popped_roi = self.ROIs.pop(roi_tag, None)
        if not popped_roi:
            return
        for spect in self.spectrum_manager.spectra.values():
            spect.remove_roi(roi_tag)
        self.sigROIDeleted.emit(popped_roi)
        self.roi_groupings = self.calculate_roi_groups()
        self.update_roi()
            
    
    def clear_all(self) -> None:
        for tag in self.ROIs.copy().keys():
            self.remove_roi(tag)
            
    def get_tag_from_alias(self, roi_alias: str) -> str:
        """Returns the internal ROI tag from a ROI alias"""
        for key, roi in self.ROIs.items():
            if roi.alias == roi_alias:
                return key
            
    def on_roi_change(self, roi_tag):
        """Callback for when a roi is moved, recalculates everything"""
        self.roi_groupings = self.calculate_roi_groups()       
        self.set_brushes() # Set brush color             
        self.update_roi(roi_tag=roi_tag)
        
    def select_roi(self, selected_roi_tag):
        """Callback for when a roi is clicked with M1"""
        for tag, roi in self.ROIs.items():
            # Get the current color of the ROI
            color = roi.brush.color()
            
            if tag == selected_roi_tag:
                # Increase alpha but keep RGB
                new_color = QColor(color.red(), color.green(), color.blue(), 50)
                roi.setBrush(pg.mkBrush(new_color))
                roi.setZValue(25)  # Bring selected ROI to front
            else:
                # Reset alpha for non-selected ROI (e.g., 50) but keep RGB
                new_color = QColor(color.red(), color.green(), color.blue(), 20)
                roi.setBrush(pg.mkBrush(new_color))
                # Optionally lower Z if it was above threshold
                if roi.zValue() >= 25:
                    roi.setZValue(20)
                    
    def propagrade_roi_settings_change(self, roi):
        for g in self.roi_groupings:
            if roi.tag in g:
                break
        
        for roi_tag in g:
            if roi_tag == roi.tag:
                continue
            
            self.ROIs[roi_tag].update_self(bkg_type=roi.bkg_type,
                                           poisson_weights=roi.poisson_weights,
                                           signal_update=False)
            
        for roi_tag in g:
            self.on_roi_change(roi_tag)
    
    def calculate_roi_groups(self) -> list[set[str]]:
        """Calculate overlapping rois and return groups of overlap"""
        mergeable = []
        singles = []

        for r in self.ROIs.values():
            tag = r.tag
            low, high = r.getRegion()

            if r.merge and r.fit_type != "None":
                mergeable.append((tag, low, high))
            else:
                singles.append({tag})  # singleton group


        mergeable.sort(key=lambda x: x[1])  # by low

        groups: list[set[str]] = []
        current_group: set[str] = set()
        current_end = None

        for tag, low, high in mergeable:
            if not current_group:
                current_group = {tag}
                current_end = high
                continue

            if low < current_end:  # overlap condition
                current_group.add(tag)
                current_end = max(current_end, high)
            else:
                groups.append(current_group)
                current_group = {tag}
                current_end = high

        if current_group:
            groups.append(current_group)

        groups.extend(singles)
           
        return groups
    
    def set_brushes(self):
        for g in self.roi_groupings:
            color = (0, 0, 255, 20) if len(g) == 1 else (0, 255, 0, 20)

            for roi_tag in g:
                roi = self.ROIs[roi_tag]

                # Only update if changed
                if getattr(roi, "_current_brush", None) != color:
                    roi.setBrush(color)
                    roi._current_brush = color
                    roi.setHoverBrush(color)      # Ensure hover uses same color
                    roi.update()  
                    
                    

    def update_roi(self, spectrum_name: str = None, roi_tag: str = None) -> None:
        """Update fits for a ROI, if None all rois for the given spectrum is updated.
            If The spectrum is None all spectra is updated for the given ROI.
            If both are None everything is updated"""
        # --- Get spectra ---
        spectra_dict = self.spectrum_manager.get_spectra_dict()

        if spectrum_name is None:
            spectra = spectra_dict.values()
        else:
            spectra = [spectra_dict[spectrum_name]]

        # --- Get ROIs ---
        if roi_tag is None:
            rois = self.ROIs.keys()
        else:
            rois = [roi_tag]



        # --- Update each ROI in its group(s) ---
        for spect in spectra:
            updated_rois = set()
            for roi in rois:
                if roi in updated_rois:
                    continue

                # Find the group(s) this ROI belongs to
                for group in self.roi_groupings:
                    if roi in group:
                        self.eval_roi(spect, group)
                        updated_rois.add(roi)
                        break

                
                
                
                
    def eval_roi(self, spectrum: Spectrum , roi_group):
        roi_group = [self.ROIs[k] for k in roi_group]
        bounds = [list(r.getRegion()) for r in roi_group]
        
        fit_type = roi_group[0].fit_type
        bkg_type = roi_group[0].bkg_type
        poission_weights = roi_group[0].poisson_weights

        y_axis = spectrum.get_foreground(cps=self.spectrum_is_cps) if not self.spectrum_is_bkg_sub else spectrum.get_bkg_sub()
        fits, converged = fit_gaussians(spectrum.x_axis, y_axis, bounds, spectrum.foreground.live_time, fit_type,
                                        poission_weights, Settings.Advanced.optimizer_use_chi2_weight, bkg_type, 5)
        
        meta_data = {"background_subtracted": self.spectrum_is_bkg_sub, 
                     "spectrum_name": spectrum.name
                     }
        if converged and not fit_type == "None":
            results = [ROI(r.tag, r.alias, tuple(r.getRegion()), (np.min(bounds), np.max(bounds)),
                           f, f.G, meta_data) for r, f in zip(roi_group, fits)]
        else:
            
            results = [ROI(r.tag, r.alias, tuple(r.getRegion()), (np.min(bounds), np.max(bounds)),
                None, np.sum(y_axis[(np.min(bounds) < spectrum.x_axis) & (spectrum.x_axis < np.max(bounds))]), meta_data) for r in roi_group]
            
        for roi in results:
            spectrum.set_roi(roi)
            self.sigROIUpdated.emit(roi.tag, spectrum.name, roi)
    
    def get_data_from_roi(self, roi_tag: str) -> dict:
        rois = {}
        for key, spectrum in self.spectrum_manager.get_spectra_dict().items():
            spect_roi = spectrum.ROIs.get(roi_tag, None)
            if spect_roi is not None:
                rois[key] = spect_roi
        
        return rois
    
    def get_data_from_spectrum(self, spectrum_name: str) -> dict:
        return self.spectrum_manager.get_spectrum(spectrum_name).ROIs.copy()