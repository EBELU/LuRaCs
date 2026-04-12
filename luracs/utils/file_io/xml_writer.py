from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from SpectrumClasses import Spectrum, SpectrumData

from InstrumentClasses import GenericInstrument, UniqueInstrument
from lxml import etree
from uuid import uuid4
from datetime import datetime, timezone
from utils.numerics.compression import compress_spectrum, encode_base64

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
            write_instrument_data(spectrum.instrument, self.extension_section)


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
    channel_data.text = " ".join(data.y_axis.round().astype(str))
    
    
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
            
def write_instrument_data(instrument: GenericInstrument | UniqueInstrument, root):
    instrument_section = etree.SubElement(
        root,
        "Instrument",
        generic_instrument=str(not isinstance(instrument, UniqueInstrument)).lower()
    )

    print("got here")
    # --- Basic info ---
    write_text_to_SubElement(instrument_section, "Name", getattr(instrument, "name", "Generic"))
    write_text_to_SubElement(instrument_section, "Model", instrument.model)
    write_text_to_SubElement(instrument_section, "Manufacturer", instrument.manufacturer)
    write_text_to_SubElement(instrument_section, "DetectorMaterial", instrument.detector_material)
    write_text_to_SubElement(instrument_section, "DetectorShape", instrument.detector_shape)

    if instrument.detector_dimensions_cm:
        write_text_to_SubElement(
            instrument_section,
            "DetectorDimensions",
            instrument.detector_dimensions_cm,
            unit="cm"
        )

    # --- Resolution ---
    if instrument.resolution_fn:
        resolution_section = etree.SubElement(instrument_section, "Resolution", type="energy")

        write_text_to_SubElement(resolution_section, "Function", instrument.resolution_fn)
        write_text_to_SubElement(resolution_section, "Parameters", instrument.resolution_param)

        points_section = etree.SubElement(resolution_section, "DataPoints")
        for E, fwhm in zip(instrument.resolution_E_points, instrument.resolution_FWHM_points):
            pt = etree.SubElement(points_section, "Point")
            write_text_to_SubElement(pt, "Energy", E, unit="keV")
            write_text_to_SubElement(pt, "FWHM", fwhm, unit="keV")

        
    # --- Efficiency ---
    if instrument.int_efficiency_fn:
        efficiency_section = etree.SubElement(instrument_section, "Efficiency", type="intrinsic")

        write_text_to_SubElement(efficiency_section, "Function", instrument.int_efficiency_fn)
        write_text_to_SubElement(efficiency_section, "Parameters", instrument.int_efficiency_params)
        write_text_to_SubElement(efficiency_section, "Created", instrument.int_efficiency_created.isoformat())
        write_text_to_SubElement(efficiency_section, "Description", instrument.int_efficiency_description)

    # --- Response Matrix ---
    if instrument.response_matrix is not None:
        comp_matrix = encode_base64(compress_spectrum(instrument.response_matrix))
        shape_as_str = " ".join([str(i) for i in instrument.response_matrix_shape])
        write_text_to_SubElement(instrument_section, "ResponseMatix", str(comp_matrix), matrix_shape=shape_as_str, compression="Base64-zLib")

    # --- Calibration (UniqueInstrument only) ---
    if isinstance(instrument, UniqueInstrument) and instrument.calibration_coefficients:
        calibration = etree.SubElement(instrument_section, "Calibration")

        write_text_to_SubElement(calibration, "PolynomialOrder", instrument.calibration_poly_order)
        write_text_to_SubElement(calibration, "Coefficients", instrument.calibration_coefficients)

        points_section = etree.SubElement(calibration, "CalibrationPoints")
        for E, ch in zip(instrument.calibration_energy_points, instrument.calibration_channel_points):
            pt = etree.SubElement(points_section, "Point")
            write_text_to_SubElement(pt, "Energy", E, unit="keV")
            write_text_to_SubElement(pt, "Channel", ch)

        if instrument.calibration_date:
            write_text_to_SubElement(calibration, "Date", instrument.calibration_date.isoformat())

        if instrument.remark:
            write_text_to_SubElement(calibration, "Remark", instrument.remark)


if __name__ == "__main__":
    pass