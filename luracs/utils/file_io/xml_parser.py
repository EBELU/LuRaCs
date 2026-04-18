from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from pathlib import Path

from datetime import datetime
import numpy as np
from lxml import etree

from SpectrumClasses import SpectrumData
from NuclideClasses import Emission
from ROIClasses import ROI, Fit


# --- Helper Functions ---
def decompress_counted_zeroes(tokens):
    result = []
    i = 0

    while i < len(tokens):
        v = int(tokens[i])

        if v == 0 and i + 1 < len(tokens):
            # next token = number of zero channels
            run = int(tokens[i + 1])
            result.extend([0] * run)
            i += 2
        else:
            result.append(v)
            i += 1

    return np.array(result)

def _get_float(node, path, ns):
    res = node.xpath(path, namespaces=ns)
    return float(res[0].text) if res and res[0].text else 0.0

def _parse_pair(node, path, ns):
    res = node.xpath(path, namespaces=ns)
    if not res or not res[0].text:
        return None, None
    parts = res[0].text.split()
    return [float(p) for p in parts] if len(parts) > 1 else None

def _parse_iso8601_duration(duration: str) -> float:
    """Parses ISO 8601 duration like PT1234.56S into seconds."""
    if not duration:
        return None
    try:
        if duration.startswith('PT') and duration.endswith('S'):
            return float(duration[2:-1])
    except Exception:
        return None

def _parse_array(text: str):
    return np.array([float(x) for x in text.split() if x.strip()]) if text else np.array([])

def _safe_iso(value: str):
    return datetime.fromisoformat(value).replace(tzinfo=None) if value else None

def _build_roi(general_kwargs: dict, fit_kwargs: dict):
    if fit_kwargs is not None:
        fit = Fit(
                region_lower = None, region_upper = None,
                G = 0, B = 0, N = 0, 
                **fit_kwargs)
    else:
        fit = None
    
    return ROI(fit=fit, **general_kwargs)

def _build_spectrum(y_axis, live_time, real_time, **meta):
    total = int(y_axis.sum())
    return SpectrumData(
        y_axis=y_axis,
        channels=len(y_axis),
        total_counts=total,
        live_time=live_time,
        real_time=real_time,
        avg_cps=(total / live_time if live_time else None),
        **meta
    )
        
def str_to_bool(inpt: str):
    assert isinstance(inpt, str), f"Input was not a string! Was {type(inpt)}"
    
    if inpt.lower() == "true":
        return True
    elif inpt.lower() == "false":
        return False
    else:
        return


class xml_parser:
    N42_NS = {'n42': 'http://physics.nist.gov/N42/2011/N42'}
    DHS_NS = {'dhs': 'DHS', **N42_NS}
    LRC_NS = {"lrc": "https://example.com/n42/extensions", **N42_NS}
    

    def __init__(self, path: Path, meta_only: bool = False):
        self.path = path
        self.meta_only = meta_only
        self.file_name = path.stem
        parser = etree.XMLParser(recover=True, remove_comments=True)
        self.tree = etree.parse(path, parser)
        self.root = self.tree.getroot()
        self.data = self._dispatch_parser()
        
    def get_foreground_spectrum(self) -> SpectrumData:
        return self.data.get("foreground")
    
    def get_background_spectrum(self) -> SpectrumData:
        return self.data.get("foreground")
    
    def get_rois(self) -> list[ROI]:
        return self.data.get("peaks", [])
    
    def get_roi_data(self) -> list[dict]:
        return self.data.get("peak_data", [])
    
    def get_header(self) -> dict:
        return {self.data.get("name"),
                self.data.get("instrument_model"),
                self.data.get("instument_id"),
                self.data.get("instrument_class_code"),
                self.data.get("calibration")}
    
    def _dispatch_parser(self):
        root_tag = etree.QName(self.root).localname.lower()
        if root_tag == "radinstrumentdata":
            node = self.root.find(".//n42:RadInstrumentInformation", namespaces=self.N42_NS)
            if node is not None and node.get("id") == "Raysid":
                return  self._parse_raysid() 
            else:
                # If nothing else matches assume it is n42
                return self._parse_n42()
            
        if root_tag == "resultdatafile":
            return self._parse_radiacode()
        raise ValueError(f"Unknown XML format: {root_tag}")





    def _parse_raysid(self):
        # Raysid uses almost n42 but has an issues with a namespace
        ns, data = self.DHS_NS, {"name": self.file_name}
        node = self.root.find(".//n42:EnergyCalibration/n42:EnergyBoundaryValues", namespaces=ns)
        if node is not None and node.text:
            b = _parse_array(node.text)
            # Convert to polynomial
            data["calibration"] = np.polyfit(np.arange(len(b)), b, 4)

        for meas in self.root.xpath(".//n42:RadMeasurement", namespaces=ns):
            code = meas.findtext("n42:MeasurementClassCode", default="", namespaces=ns).lower()
            kind = "foreground" if "foreground" in code else "background"
            spec = meas.find("n42:Spectrum", namespaces=ns)
            if spec is None: continue

            y = _parse_array(spec.findtext("n42:ChannelData", default="", namespaces=ns))
            live = _parse_iso8601_duration(spec.findtext("n42:LiveTimeDuration", namespaces=ns))
            real = _parse_iso8601_duration(meas.findtext("n42:RealTimeDuration", namespaces=ns))

            dhs = meas.find("dhs:Raysid", namespaces=ns)
            dose = float(dhs.findtext("dhs:AverageDoseRate", "0", namespaces=ns)) if dhs is not None else None

            data[kind] = _build_spectrum(
                y, live, real,
                avg_dose_rate=dose,
                start_date=_safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=_safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=spec.get("id"),
                instrument=spec.get("radDetectorInformationReference")
            )
        return data

    def _parse_radiacode(self):
        data = {"name": self.file_name}
        coeffs = self.root.findall(".//EnergyCalibration/Coefficients/Coefficient")
        
        if coeffs is not None:
            data["calibration"] = [float(c.text) for c in coeffs][::-1][:3] # Flip list and remove background calibration

        for result in self.root.findall(".//ResultData"):
            fg = result.find(".//EnergySpectrum/Spectrum")
            if fg is not None:
                y = np.array([float(x.text) for x in fg.findall("DataPoint")])
                data["foreground"] = _build_spectrum(
                    y,
                    float(result.findtext(".//EnergySpectrum/LiveTime", "0")),
                    float(result.findtext(".//EnergySpectrum/MeasurementTime", "0")),
                    start_date=_safe_iso(result.findtext(".//StartTime")),
                    end_date=_safe_iso(result.findtext(".//EndTime")),
                    spectrum_name=fg.findtext("../SpectrumName"),
                    instrument=result.findtext(".//DeviceConfigReference/Name")
                )

            bg = result.find(".//BackgroundEnergySpectrum/Spectrum")
            if bg is not None:
                y = np.array([float(x.text) for x in bg.findall("DataPoint")])
                data["background"] = _build_spectrum(
                    y,
                    float(result.findtext(".//BackgroundEnergySpectrum/LiveTime", "0")),
                    float(result.findtext(".//BackgroundEnergySpectrum/MeasurementTime", "0")),
                    start_date=_safe_iso(result.findtext(".//StartTime")),
                    end_date=_safe_iso(result.findtext(".//EndTime")),
                    spectrum_name=bg.findtext("../SpectrumName"),
                    instrument=result.findtext(".//DeviceConfigReference/Name")
                )
        return data

    def _parse_n42(self):
        ns, data = self.N42_NS, {"name": self.file_name}
        instrument = self.root.findtext(".//n42:RadInstrumentIdentifier", namespaces=ns)
        
        data["instrument_model"] = self.root.findtext(".//n42:RadInstrumentModelName", namespaces=ns) 
        data["instrument_class_code"] = self.root.findtext(".//n42:RadInstrumentClassCode", namespaces=ns)    

        coeff = self.root.findtext(".//n42:EnergyCalibration/n42:CoefficientValues", namespaces=ns)
        if coeff:
            data["calibration"] = [float(x) for x in coeff.split()][::-1]

        for meas in self.root.xpath(".//n42:RadMeasurement", namespaces=ns):
            code = meas.findtext("n42:MeasurementClassCode", default="", namespaces=ns).lower()
            kind = "foreground" if "foreground" in code else "background"
            spec = meas.find("n42:Spectrum", namespaces=ns)
            if spec is None: continue
            # For library indexing
            if self.meta_only:
                y = np.array([])
            else:
                chan = spec.find("n42:ChannelData", namespaces=ns)
                chan_data = chan.text.strip() if chan is not None else ""
                y = _parse_array(chan_data)

                if chan is not None and chan.get("compressionCode") == "CountedZeroes":
                    y = decompress_counted_zeroes(y)

            data[kind] = _build_spectrum(
                y,
                _parse_iso8601_duration(spec.findtext("n42:LiveTimeDuration", namespaces=ns)),
                _parse_iso8601_duration(meas.findtext("n42:RealTimeDuration", namespaces=ns)),
                start_date=_safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=_safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=self.file_name,
                instrument=instrument
            )
            
        lrc = self.root.find("./lrc:LuRaCs", namespaces=self.LRC_NS)
        if lrc is not None:
            data["peaks"], data["peak_data"] = self.parse_roisV1(lrc)

        return data

    def parse_roisV1(self, ext_root):
        "Parse ROI data saved by LuRaCs"
        ns = self.LRC_NS
        peaks = []
        peak_data = []
    
        for roi in ext_root.xpath(".//n42:Roi", namespaces=self.N42_NS):
            roi_id, alias, spectrum = roi.get("id"), roi.get("alias"), roi.get("spectrum_ref")
            cont = roi.xpath(".//n42:PeakContinuum", namespaces=ns)
            # --- Continuum ---
            cont = cont[0] if cont else None
            if cont is not None:
                meta_data = {"merge": str_to_bool(cont.get("merge")), "movable": str_to_bool(cont.get("movable"))}
                region_counts = _get_float(cont, ".//n42:RegionCounts", ns)
                live_time = _get_float(cont, ".//n42:LiveTime", ns)
                e_low = _get_float(cont, ".//n42:LowerEnergy", ns)
                e_high = _get_float(cont, ".//n42:UpperEnergy", ns)
            else:
                region_counts = live_time = e_low = e_high = None
            
            # Fit type and bkg type
            fit = roi.xpath("./n42:Fit", namespaces=ns)[0]
            fit_type = fit.text
            bkg_type = roi.xpath(".//n42:BackgroundType", namespaces=ns)[0].text
            meta_data["chi2_weighted_err"] = fit.get("chi2_weighted_err")
            
            # --- Peak fit ---
            peak_nodes = roi.xpath("./n42:Peak", namespaces=ns)
            if peak_nodes is not None and len(peak_nodes) != 0:
                peak = peak_nodes[0]
                fit_params = _parse_array(peak.xpath(".//n42:FitParameters", namespaces=ns)[0].text)
                fit_params_err = _parse_array(peak.xpath(".//n42:FitParametersErr", namespaces=ns)[0].text)
                bkg_params = _parse_array(peak.xpath(".//n42:BackgroundParameters", namespaces=ns)[0].text)
                centroid, centroid_err = _parse_pair(peak, ".//n42:Centroid", ns)
                sigma, sigma_err = _parse_pair(peak, ".//n42:StandardDeviation", ns)
                amp, amp_err = _parse_pair(peak, ".//n42:Amplitude", ns)

                counts_res = peak.xpath(".//n42:PeakCounts", namespaces=ns)
                counts = float(counts_res[0].text) if counts_res and counts_res[0].text else 0.0
                peak_kwargs = { "lower": e_low,
                                "upper": e_high,
                                "params": fit_params,
                                "param_errs": fit_params_err,
                                "peak_counts": counts,
                                "bkg_params": bkg_params,
                }
                misc_peak_kwargs = {
                    "centroid": centroid,
                    "centroid_err": centroid_err,
                    "sigma": sigma,
                    "sigma_err": sigma_err,
                    "amplitude": amp,
                    "amplitude_err": amp_err,
                    "peak_counts": counts,
                }
            else:
                peak_kwargs = misc_peak_kwargs = None
                
            # --- Nuclide ---
            nuclide = roi.xpath("./n42:Nuclide", namespaces=ns)
            if nuclide:
                nuclide_name = nuclide.xpath("./Name", "None")
                emission_energy = nuclide.xpath("./Energy", "None")
            else:
                nuclide_name = emission_energy = None
                
            meta_data["nuclide"] = nuclide_name
            meta_data["emission_energy"] = emission_energy

            general_kwargs = {
                "tag": roi_id,
                "alias": alias,
                "roi_bound": (e_low, e_high),
                "region_bound": (None, None),
                "fit_type": fit_type,
                "bkg_type": bkg_type,
                "roi_counts": region_counts,
                "live_time": live_time,
                "meta": meta_data,
                "emission": None,
            }
            
            peaks.append(_build_roi(general_kwargs, peak_kwargs))
            peak_data.append(misc_peak_kwargs)
                
        return peaks, peak_data