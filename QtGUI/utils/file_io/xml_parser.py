from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from pathlib import Path

from datetime import datetime
import numpy as np
from lxml import etree

from SpectrumClasses import SpectrumData


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
    return float(parts[0]), float(parts[1]) if len(parts) > 1 else None


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
    return datetime.fromisoformat(value) if value else None
    


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
        self.kwargs = self._dispatch_parser()

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



    def _build_spectrum(self, y_axis, live_time, real_time, **meta):
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

    def _parse_raysid(self):
        # Raysid uses almost n42 but has an issues with a namespace
        ns, kwargs = self.DHS_NS, {"name": self.file_name}
        node = self.root.find(".//n42:EnergyCalibration/n42:EnergyBoundaryValues", namespaces=ns)
        if node is not None and node.text:
            b = _parse_array(node.text)
            # Convert to polynomial
            kwargs["calibration"] = np.polyfit(np.arange(len(b)), b, 4)

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

            kwargs[kind] = self._build_spectrum(
                y, live, real,
                avg_dose_rate=dose,
                start_date=_safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=_safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=spec.get("id"),
                instrument=spec.get("radDetectorInformationReference")
            )
        return kwargs

    def _parse_radiacode(self):
        kwargs = {"name": self.file_name}
        coeffs = self.root.findall(".//EnergyCalibration/Coefficients/Coefficient")
        if coeffs is not None:
            kwargs["calibration"] = [float(c.text) for c in coeffs][::-1][:3]

        for result in self.root.findall(".//ResultData"):
            fg = result.find(".//EnergySpectrum/Spectrum")
            if fg is not None:
                y = np.array([float(x.text) for x in fg.findall("DataPoint")])
                kwargs["foreground"] = self._build_spectrum(
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
                kwargs["background"] = self._build_spectrum(
                    y,
                    float(result.findtext(".//BackgroundEnergySpectrum/LiveTime", "0")),
                    float(result.findtext(".//BackgroundEnergySpectrum/MeasurementTime", "0")),
                    start_date=_safe_iso(result.findtext(".//StartTime")),
                    end_date=_safe_iso(result.findtext(".//EndTime")),
                    spectrum_name=bg.findtext("../SpectrumName"),
                    instrument=result.findtext(".//DeviceConfigReference/Name")
                )
        return kwargs

    def _parse_n42(self):
        ns, kwargs = self.N42_NS, {"name": self.file_name}
        instrument = self.root.findtext(".//n42:RadInstrumentIdentifier", namespaces=ns)
        
        kwargs["instrument_model"] = self.root.findtext(".//n42:RadInstrumentModelName", namespaces=ns) 
        kwargs["instrument_class_code"] = self.root.findtext(".//n42:RadInstrumentClassCode", namespaces=ns)    

        coeff = self.root.findtext(".//n42:EnergyCalibration/n42:CoefficientValues", namespaces=ns)
        if coeff:
            kwargs["calibration"] = [float(x) for x in coeff.split()][::-1]

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
                data = chan.text.strip() if chan is not None else ""
                y = _parse_array(data)

                if chan is not None and chan.get("compressionCode") == "CountedZeroes":
                    y = decompress_counted_zeroes(y)

            kwargs[kind] = self._build_spectrum(
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
            kwargs["peaks"] = self.parse_roisV1(lrc)

        return kwargs

    def parse_roisV1(self, ext_root):
        "Parse ROI data saved by LuRaCs"
        ns = self.LRC_NS
        peaks = []
    
        for roi in ext_root.xpath(".//n42:Roi", namespaces=self.N42_NS):
            roi_id, alias, spectrum = roi.get("id"), roi.get("alias"), roi.get("spectrum_ref")
            # --- Continuum ---
            cont = roi.xpath(".//n42:PeakContinuum", namespaces=ns)
            cont = cont[0] if cont else None
            if cont is not None:
                region_counts = _get_float(cont, ".//n42:RegionCounts", ns)
                live_time = _get_float(cont, ".//n42:LiveTime", ns)
                e_low = _get_float(cont, ".//n42:LowerEnergy", ns)
                e_high = _get_float(cont, ".//n42:UpperEnergy", ns)
            else:
                region_counts = live_time = e_low = e_high = None

            # --- Peak ---
            peak_nodes = roi.xpath("./n42:Peak", namespaces=ns)
            if peak_nodes is not None:
                peak = peak_nodes[0]
                
                centroid, centroid_err = _parse_pair(peak, ".//n42:Centroid", ns)
                sigma, sigma_err = _parse_pair(peak, ".//n42:StandardDeviation", ns)
                amp, amp_err = _parse_pair(peak, ".//n42:Amplitude", ns)

                counts_res = peak.xpath(".//n42:PeakCounts", namespaces=ns)
                counts = float(counts_res[0].text) if counts_res and counts_res[0].text else 0.0
                peaks.append({
                    "roi": roi_id,
                    "alias": alias,
                    "spectrum": spectrum,
                    "centroid": centroid,
                    "centroid_err": centroid_err,
                    "sigma": sigma,
                    "sigma_err": sigma_err,
                    "amplitude": amp,
                    "amplitude_err": amp_err,
                    "peak_counts": counts,
                    "continuum_counts": region_counts,
                    "energy_range": (e_low, e_high),
                    "live_time": live_time
                })
            else:
                peaks.append({
                    "roi": roi_id,
                    "alias": alias,
                    "spectrum": spectrum,
                    "continuum_counts": region_counts,
                    "energy_range": (e_low, e_high),
                    "live_time": live_time
                })
                
        return peaks