from lxml import etree
from uuid import uuid4
from datetime import datetime, timezone
from SpectrumClasses import Spectrum, SpectrumData

NS = "http://physics.nist.gov/N42/2011/N42"
MY = "https://example.com/n42/extensions"
NSMAP = {None: NS, "LRC": MY}
samples = 1

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

def write_SpectrumData(data: SpectrumData, root, kind: str):
    assert kind in ("foreground", "background")
    global samples
    rad_measurement = etree.SubElement(root, n42("RadMeasurement"), id=f"Sample{samples}")
    samples += 1

    measurement_class = etree.SubElement(rad_measurement, n42("MeasurementClassCode"))
    measurement_class.text = kind

    start_date = etree.SubElement(rad_measurement, n42("StartDateTime"))
    if data.start_date is None:
        data.start_date
    else:
        start_date.text = data.start_date.strftime("%Y-%m-%dT%H:%M:%SZ")

    if data.real_time:
        real_time_duration = etree.SubElement(rad_measurement, n42("RealTimeDuration"))
        real_time_duration.text = f"PT{round(data.real_time, 2)}S"

    spectrum = etree.SubElement(rad_measurement, n42("Spectrum"), energyCalibrationReference="EnergyCal0")

    live_time_duration = etree.SubElement(spectrum, n42("LiveTimeDuration"))
    live_time_duration.text = f"PT{round(data.live_time, 2)}S"

    channel_data = etree.SubElement(spectrum, n42("ChannelData"), compressionCode="None")
    channel_data.text = " ".join(data.y_axis.astype(str))
    
    
def write_ROI_data(spectrum: Spectrum, peaks_section):
    num_peaks = etree.SubElement(peaks_section, "NumberOfPeaks")
    num_peaks.text = str(len(spectrum.ROIs))
    
    for tag, roi in spectrum.ROIs.items():
        new_peak = etree.SubElement(peaks_section, "Roi", id = tag, alias = roi.alias, spectrum_ref = spectrum.name, version = "1")       
        continuum = etree.SubElement(new_peak, "PeakContinuum")
        write_text_to_SubElement(continuum, "RegionCounts", roi.counts)
        write_text_to_SubElement(continuum, "LiveTime", roi.live_time)
        
        # Energy bounds
        write_text_to_SubElement(continuum, "LowerEnergy", roi.roi_bound[0])
        write_text_to_SubElement(continuum, "UpperEnergy", roi.roi_bound[1])

        # Create the peak element
        peak = etree.SubElement(new_peak, "Peak")
        write_text_to_SubElement(peak, "Fit", "Gaussian")
        if roi.fit is not None:
            write_text_to_SubElement(peak, "BackgroundType", roi.fit.bkg_type)
            write_text_to_SubElement(peak, "BackgroundParameters", list(roi.fit.bkg_params))
            
            write_text_to_SubElement(peak, "Centroid", [roi.fit.mu, roi.fit.mu_err])
            write_text_to_SubElement(peak, "StandardDeviation", [roi.fit.sigma, roi.fit.sigma_err])
            write_text_to_SubElement(peak, "Amplitude", [roi.fit.A, roi.fit.A_err])
            
            write_text_to_SubElement(peak, "PeakCounts", roi.fit.peak_counts)
            
def write_instrument_data(instument, root):
    instrment_section = etree.SubElement(root, "Instrument")


def write_extension_data(spectrum: Spectrum, root):
    extension_section = etree.SubElement(root, LRC("LuRaCs"), version = "1")
    if len(spectrum.ROIs) > 0:
        peaks = etree.SubElement(extension_section, "Peaks")
        write_ROI_data(spectrum, peaks)



def write_xml_spectrum(spectrum: Spectrum, file_name: str):    
    root = etree.Element(
        n42("RadInstrumentData"),
        nsmap=NSMAP,
        n42DocUUID=str(uuid4()),
        n42DocDateTime=datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    )
    
    creator = etree.SubElement(root, n42("RadInstrumentDataCreatorName"))
    creator.text = "LuRaCs"

    if spectrum.calibrated:
        energy_cal = etree.SubElement(root, "EnergyCalibration", id="EnergyCal0")
        write_text_to_SubElement(energy_cal, "CoefficientValues", spectrum.calibration_coefficients[::-1])

    write_SpectrumData(spectrum.foreground, root, "foreground")

    if spectrum.background is not None:
        write_SpectrumData(spectrum.background, root, "background")
        
    write_extension_data(spectrum, root)
    
    tree = etree.ElementTree(root)
    tree.write(
        f"{file_name}.xml",
        pretty_print=True,
        xml_declaration=True,
        encoding="utf-8"
    )
    
if __name__ == "__main__":
    write_xml_spectrum()