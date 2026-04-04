from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from SpectrumClasses import Spectrum, SpectrumData


from lxml import etree
from uuid import uuid4
from datetime import datetime, timezone

NS = "http://physics.nist.gov/N42/2011/N42"
MY = "https://example.com/n42/extensions"
NSMAP = {None: NS, "LRC": MY}

def n42(tag):
    return f"{{{NS}}}{tag}"

def LRC(tag):
    return f"{{{MY}}}{tag}"

def write_text_to_SubElement(branch, sub_element: str,  data: str | int | float | list, **kwargs):
    if isinstance(data, list):
        data = " ".join([str(i) for i in data])
    else:
        data = str(data)
        
    sub_branch = etree.SubElement(branch, sub_element, **kwargs)
    sub_branch.text = data

class xml_writer:
    def __init__(self, spectrum: Spectrum, file_name: str, export_spectrum = True, export_rois = True, export_instrument = True):
        self.spectrum = spectrum
        self.file_name = file_name

        self.extension_section = None
        self.root = etree.Element(
            n42("RadInstrumentData"),
            nsmap=NSMAP,
            n42DocUUID=str(uuid4()),
            n42DocDateTime=datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
        )
        write_text_to_SubElement(self.root, n42("RadInstrumentDataCreatorName"), "LuRaCs")


        # --- Spectrum data ---
        if export_spectrum:
            foreground_id = spectrum.foreground.spectrum_name if spectrum.foreground.spectrum_name else spectrum.name
            
            background_id = spectrum.background.spectrum_name if spectrum.background and spectrum.background.spectrum_name else "BkgSample0"

            if spectrum.calibrated:
                energy_cal = etree.SubElement(self.root, "EnergyCalibration", id="EnergyCal0")
                write_text_to_SubElement(energy_cal, "CoefficientValues", spectrum.calibration_coefficients[::-1])

            write_SpectrumData(spectrum.foreground, self.root, "foreground", foreground_id)

            if spectrum.background is not None:
                write_SpectrumData(spectrum.background, self.root, "background", background_id)

        # --- Extensions ---
        if export_rois and len(spectrum.ROIs) > 0:
            self.extension_section = etree.SubElement(self.root, LRC("LuRaCs"), version = "1") if self.extension_section is None else self.extension_section
            peaks = etree.SubElement(self.extension_section, "Peaks")
            write_ROI_data(spectrum.ROIs, peaks)


        if export_instrument and spectrum.instrument is not None:
            self.extension_section = etree.SubElement(self.root, LRC("LuRaCs"), version = "1") if self.extension_section is None else self.extension_section
            peaks = etree.SubElement(self.extension_section, "Instrument")
            pass


        # --- Write out ---
        tree = etree.ElementTree(self.root)
        tree.write(
            f"{file_name}.xml",
            pretty_print=True,
            xml_declaration=True,
            encoding="utf-8"
        )
    
        
def write_SpectrumData(data: SpectrumData, root, kind: str, spectrum_id: str):
    assert kind in ("foreground", "background")
    rad_measurement = etree.SubElement(root, n42("RadMeasurement"), id=spectrum_id)

    # MeasurementClassCode
    write_text_to_SubElement(rad_measurement, n42("MeasurementClassCode"), kind)

    start_date = etree.SubElement(rad_measurement, n42("StartDateTime"))
    if data.start_date is None:
        pass
    else:
        start_date.text = data.start_date.strftime("%Y-%m-%dT%H:%M:%SZ")

    if data.real_time:
        write_text_to_SubElement(rad_measurement, n42("RealTimeDuration"), f"PT{round(data.real_time, 2)}S")

    spectrum = etree.SubElement(rad_measurement, n42("Spectrum"), energyCalibrationReference="EnergyCal0")

    write_text_to_SubElement(spectrum, n42("LiveTimeDuration"), f"PT{round(data.live_time, 2)}S")

    channel_data = etree.SubElement(spectrum, n42("ChannelData"), compressionCode="None")
    channel_data.text = " ".join(data.y_axis.astype(str))
    
    
def write_ROI_data(ROIs: dict, peaks_section):
    num_peaks = etree.SubElement(peaks_section, "NumberOfPeaks")
    num_peaks.text = str(len(ROIs))
    
    for tag, roi in ROIs.items():
        new_peak = etree.SubElement(peaks_section, "Roi", id = tag, alias = roi.alias, spectrum_ref = str(roi.meta.get("spectrum_name")), version = "1")       
        continuum = etree.SubElement(new_peak, "PeakContinuum", merge = str(roi.meta.get("merge", True)), movable = str(roi.meta.get("movable", True)), background_subtracted = str(roi.meta.get("background_subtracted", False)))
        write_text_to_SubElement(continuum, "RegionCounts", roi.roi_counts)
        write_text_to_SubElement(continuum, "LiveTime", roi.live_time)
        
        # Energy bounds
        write_text_to_SubElement(continuum, "LowerEnergy", roi.roi_bound[0])
        write_text_to_SubElement(continuum, "UpperEnergy", roi.roi_bound[1])

        # Create the peak element
        
        write_text_to_SubElement(new_peak, "Fit", roi.fit_type, chi2_weighted_err = str(roi.meta.get("chi2_weighted_err", True)))
        write_text_to_SubElement(new_peak, "BackgroundType", roi.bkg_type)
        if roi.fit is not None:
            peak = etree.SubElement(new_peak, "Peak")
            write_text_to_SubElement(peak, "BackgroundParameters", list(roi.fit.bkg_params))
            
            write_text_to_SubElement(peak, "FitParameters", list(roi.fit.params))
            write_text_to_SubElement(peak, "FitParametersErr", list(roi.fit.param_errs))
            
            write_text_to_SubElement(peak, "Centroid", [roi.fit.mu, roi.fit.mu_err])
            write_text_to_SubElement(peak, "StandardDeviation", [roi.fit.sigma, roi.fit.sigma_err])
            write_text_to_SubElement(peak, "Amplitude", [roi.fit.A, roi.fit.A_err])
            
            write_text_to_SubElement(peak, "PeakCounts", roi.fit.peak_counts)
            
def write_instrument_data(instument, root):
    instrment_section = etree.SubElement(root, "Instrument")

    
if __name__ == "__main__":
    pass