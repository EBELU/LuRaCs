from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    pass

from pathlib import Path

from datetime import datetime
import numpy as np
from lxml import etree

from containers.spectrum_classes import SpectrumData
from containers.nuclide_classes import Emission
from containers.roi_classes import ROI, Fit
from utils.numerics.compression import decode_base64, decompress_spectrum


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
        if duration.startswith("PT") and duration.endswith("S"):
            return float(duration[2:-1])
    except Exception:
        return None


def _parse_array(text: str):
    return (
        np.array([float(x) for x in text.split() if x.strip()])
        if text
        else np.array([])
    )


def _safe_iso(value: str):
    return datetime.fromisoformat(value).replace(tzinfo=None) if value else None


def _build_roi(general_kwargs: dict, fit_kwargs: dict):
    if fit_kwargs is not None:
        fit = Fit(region_lower=None, region_upper=None, G=0, B=0, N=0, **fit_kwargs)
    else:
        fit = None
    
    emission_data = general_kwargs.pop("emission_data")
    if len(emission_data):
        emission = Emission(**emission_data)
    else:
        emission = None

    return ROI(fit=fit, emission=emission, **general_kwargs)


def _build_spectrum(y_axis, live_time, real_time, **meta):
    total = int(y_axis.sum())
    return SpectrumData(
        y_axis=y_axis,
        channels=len(y_axis),
        total_counts=total,
        live_time=live_time,
        real_time=real_time,
        avg_cps=(total / live_time if live_time else None),
        **meta,
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
    N42_NS = {"n42": "http://physics.nist.gov/N42/2011/N42"}
    DHS_NS = {"dhs": "DHS", **N42_NS}
    LRC_NS = {"lrc": "https://example.com/n42/extensions", **N42_NS}

    def __init__(self, path: Path | str, meta_only: bool = False):
        self.path = path if isinstance(path, Path) else Path(path)
        self.meta_only = meta_only
        self.file_name = self.path.stem
        parser = etree.XMLParser(recover=True, remove_comments=True)
        self.tree = etree.parse(str(self.path), parser)
        self.root = self.tree.getroot()
        self.data = self._dispatch_parser()

    def get_foreground_spectrum(self) -> SpectrumData:
        """
        Return the foreground spectrum data.

        Returns:
            SpectrumData | None:
                Parsed foreground spectrum object containing spectral counts,
                timing information, calibration references, and metadata.
                Returns ``None`` if no foreground spectrum is available.
        """
        return self.data.get("foreground")


    def get_background_spectrum(self) -> SpectrumData:
        """
        Return the background spectrum data.

        Returns:
            SpectrumData | None:
                Parsed background spectrum object containing spectral counts,
                timing information, calibration references, and metadata.
                Returns ``None`` if no background spectrum is available.

        Note:
            The current implementation returns the ``foreground`` entry
            instead of ``background``. This may be unintended behavior.
        """
        return self.data.get("foreground")


    def get_rois(self) -> list[ROI]:
        """
        Return all parsed regions of interest (ROIs).

        Returns:
            list[ROI]:
                List of ROI objects extracted from the XML file. Each ROI
                may contain peak fitting information, energy bounds,
                nuclide assignments, and associated metadata.

                Returns an empty list if no ROI data is present.
        """
        return self.data.get("peaks", [])


    def get_roi_data(self) -> list[dict]:
        """
        Return auxiliary ROI peak-fit data.

        Returns:
            list[dict]:
                List of dictionaries containing additional peak-fit
                parameters and derived quantities for each ROI, such as:

                - centroid and centroid uncertainty
                - sigma and sigma uncertainty
                - amplitude and amplitude uncertainty
                - peak counts

                Returns an empty list if no ROI fit data is available.
        """
        return self.data.get("peak_data", [])


    def get_instrument_data(self) -> dict:
        """
        Return parsed instrument metadata and characterization data.

        Returns:
            dict | None:
                Dictionary containing instrument-specific information such as:

                - instrument name and model
                - manufacturer
                - detector material and geometry
                - detector dimensions
                - resolution model and calibration points
                - efficiency calibration
                - response matrix
                - energy calibration data

                Returns ``None`` if no instrument extension data exists.
        """
        return self.data.get("instrument")


    def get_header(self) -> tuple:
        """
        Return a compact summary of parsed file metadata.

        Returns:
            dict:
                Dictionary containing high-level metadata extracted from
                the XML document, including:

                - name
                - instrument_model
                - instrument_id
                - instrument_class_code
                - calibration

        Note:
            The current implementation constructs a ``set`` instead of a
            dictionary due to the use of curly braces without key-value
            pairs. Consider replacing with an explicit dictionary.
        """
        return (
            self.data.get("name"),
            self.data.get("instrument_model"),
            self.data.get("instument_id"),
            self.data.get("instrument_class_code"),
            self.data.get("calibration"),
        )
        
    def _dispatch_parser(self):
        root_tag = etree.QName(self.root).localname.lower()
        if root_tag == "radinstrumentdata":
            node = self.root.find(
                ".//n42:RadInstrumentInformation", namespaces=self.N42_NS
            )
            if node is not None and node.get("id") == "Raysid":
                return self._parse_raysid()
            else:
                # If nothing else matches assume it is n42
                return self._parse_n42()

        if root_tag == "resultdatafile":
            return self._parse_radiacode()
        raise ValueError(f"Unknown XML format: {root_tag}")

    def _parse_raysid(self):
        # Raysid uses almost n42 but has an issues with a namespace
        ns, data = self.DHS_NS, {"name": self.file_name}
        node = self.root.find(
            ".//n42:EnergyCalibration/n42:EnergyBoundaryValues", namespaces=ns
        )
        if node is not None and node.text:
            b = _parse_array(node.text)
            # Convert to polynomial
            data["calibration"] = np.polyfit(np.arange(len(b)), b, 4)

        for meas in self.root.xpath(".//n42:RadMeasurement", namespaces=ns):
            code = meas.findtext(
                "n42:MeasurementClassCode", default="", namespaces=ns
            ).lower()
            kind = "foreground" if "foreground" in code else "background"
            spec = meas.find("n42:Spectrum", namespaces=ns)
            if spec is None:
                continue

            y = _parse_array(
                spec.findtext("n42:ChannelData", default="", namespaces=ns)
            )
            live = _parse_iso8601_duration(
                spec.findtext("n42:LiveTimeDuration", namespaces=ns)
            )
            real = _parse_iso8601_duration(
                meas.findtext("n42:RealTimeDuration", namespaces=ns)
            )

            dhs = meas.find("dhs:Raysid", namespaces=ns)
            dose = (
                float(dhs.findtext("dhs:AverageDoseRate", "0", namespaces=ns))
                if dhs is not None
                else None
            )

            data[kind] = _build_spectrum(
                y,
                live,
                real,
                avg_dose_rate=dose,
                start_date=_safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=_safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=spec.get("id"),
                instrument=spec.get("radDetectorInformationReference"),
            )
        return data

    def _parse_radiacode(self):
        data = {"name": self.file_name}
        coeffs = self.root.findall(".//EnergyCalibration/Coefficients/Coefficient")

        if coeffs is not None:
            data["calibration"] = [float(c.text) for c in coeffs][::-1][
                :3
            ]  # Flip list and remove background calibration

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
                    instrument=result.findtext(".//DeviceConfigReference/Name"),
                )

            bg = result.find(".//BackgroundEnergySpectrum/Spectrum")
            if bg is not None:
                y = np.array([float(x.text) for x in bg.findall("DataPoint")])
                data["background"] = _build_spectrum(
                    y,
                    float(result.findtext(".//BackgroundEnergySpectrum/LiveTime", "0")),
                    float(
                        result.findtext(
                            ".//BackgroundEnergySpectrum/MeasurementTime", "0"
                        )
                    ),
                    start_date=_safe_iso(result.findtext(".//StartTime")),
                    end_date=_safe_iso(result.findtext(".//EndTime")),
                    spectrum_name=bg.findtext("../SpectrumName"),
                    instrument=result.findtext(".//DeviceConfigReference/Name"),
                )
        return data

    def _parse_n42(self):
        ns, data = self.N42_NS, {"name": self.file_name}
        instrument = self.root.findtext(".//n42:RadInstrumentIdentifier", namespaces=ns)

        data["instrument_model"] = self.root.findtext(
            ".//n42:RadInstrumentModelName", namespaces=ns
        )
        data["instrument_class_code"] = self.root.findtext(
            ".//n42:RadInstrumentClassCode", namespaces=ns
        )

        coeff = self.root.findtext(
            ".//n42:EnergyCalibration/n42:CoefficientValues", namespaces=ns
        )
        if coeff:
            data["calibration"] = [float(x) for x in coeff.split()][::-1]
            
        data["remark"] = self.root.findtext(
            ".//n42:Remark", namespaces=ns
        )

        for meas in self.root.xpath(".//n42:RadMeasurement", namespaces=ns):
            code = meas.findtext(
                "n42:MeasurementClassCode", default="", namespaces=ns
            ).lower()
            kind = "foreground" if "foreground" in code else "background"
            spec = meas.find("n42:Spectrum", namespaces=ns)
            if spec is None:
                continue
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
                _parse_iso8601_duration(
                    spec.findtext("n42:LiveTimeDuration", namespaces=ns)
                ),
                _parse_iso8601_duration(
                    meas.findtext("n42:RealTimeDuration", namespaces=ns)
                ),
                start_date=_safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=_safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=self.file_name,
                instrument=instrument,
            )

        lrc = self.root.find("./lrc:LuRaCs", namespaces=self.LRC_NS)
        if lrc is not None:
            data["peaks"], data["peak_data"] = self.parse_roisV1(lrc)
            data["instrument"] = self.parse_instrument(lrc)
        return data

    def parse_roisV1(self, ext_root):
        "Parse ROI data saved by LuRaCs"
        ns = self.LRC_NS
        peaks = []
        peak_data = []

        for roi in ext_root.xpath(".//n42:Roi", namespaces=self.N42_NS):
            roi_id, alias, spectrum = (
                roi.get("id"),
                roi.get("alias"),
                roi.get("spectrum_ref"),
            )
            cont = roi.xpath(".//n42:PeakContinuum", namespaces=ns)
            # --- Continuum ---
            cont = cont[0] if cont else None
            if cont is not None:
                meta_data = {
                    "merge": str_to_bool(cont.get("merge")),
                    "movable": str_to_bool(cont.get("movable")),
                }
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
                fit_params = _parse_array(
                    peak.xpath(".//n42:FitParameters", namespaces=ns)[0].text
                )
                fit_params_err = _parse_array(
                    peak.xpath(".//n42:FitParametersErr", namespaces=ns)[0].text
                )
                bkg_params = _parse_array(
                    peak.xpath(".//n42:BackgroundParameters", namespaces=ns)[0].text
                )
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
            emission_data = {}
            if nuclide:
                nuclide = nuclide[0]

                
                emission_data["parent_nuclide"] = nuclide.xpath("./n42:Name", namespaces=ns)[0].text

                emission_data["energy_keV"], emission_data["energy_error_keV"] = _parse_pair(nuclide, "./n42:Energy", ns=ns)
                
                emission_data["intensity_percent"], emission_data["intensity_error_percent"] = _parse_pair(nuclide, "./n42:Intensity", ns=ns)
                
                emission_data["type"] = nuclide.xpath("./n42:Type", namespaces=ns)[0].text
                
                emission_data["origin"] = nuclide.xpath("./n42:EmissionOrigin", namespaces=ns)[0].text


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
                "emission_data": emission_data,
            }

            peaks.append(_build_roi(general_kwargs, peak_kwargs))
            peak_data.append(misc_peak_kwargs)

        return peaks, peak_data
    
    def parse_instrument(self, ext_root):
        ns = self.N42_NS

        instrument_nodes = ext_root.xpath(".//n42:Instrument", namespaces=ns)
        if not instrument_nodes:
            return None

        instrument = instrument_nodes[0]

        # --- Type ---
        is_generic = str_to_bool(instrument.get("generic_instrument"))

        # --- Basic info ---
        instrument_data = {
            "is_generic": is_generic,
            "name": instrument.xpath("./n42:Name", namespaces=ns)[0].text,
            "model": instrument.xpath("./n42:Model", namespaces=ns)[0].text,
            "manufacturer": instrument.xpath("./n42:Manufacturer", namespaces=ns)[0].text,
            "detector_material": instrument.xpath("./n42:DetectorMaterial", namespaces=ns)[0].text,
            "detector_shape": instrument.xpath("./n42:DetectorShape", namespaces=ns)[0].text,
        }

        # --- Detector dimensions ---
        dim = instrument.xpath("./n42:DetectorDimensions", namespaces=ns)
        instrument_data["detector_dimensions_cm"] = (
            _parse_array(dim[0].text) if dim and dim[0].text else None
        )

        # --- Resolution ---
        res = instrument.xpath("./n42:Resolution", namespaces=ns)
        if res:
            res = res[0]

            resolution = {
                "function": res.xpath("./n42:Function", namespaces=ns)[0].text,
                "parameters": _parse_array(
                    res.xpath("./n42:Parameters", namespaces=ns)[0].text
                ),
                "E_points": [],
                "FWHM_points": [],
            }

            for pt in res.xpath(".//n42:Point", namespaces=ns):
                E = float(pt.xpath("./n42:Energy", namespaces=ns)[0].text)
                fwhm = float(pt.xpath("./n42:FWHM", namespaces=ns)[0].text)
                resolution["E_points"].append(E)
                resolution["FWHM_points"].append(fwhm)

            instrument_data["resolution"] = resolution
        else:
            instrument_data["resolution"] = None

        # --- Efficiency ---
        eff = instrument.xpath("./n42:Efficiency", namespaces=ns)
        if eff:
            eff = eff[0]

            instrument_data["efficiency"] = {
                "function": eff.xpath("./n42:Function", namespaces=ns)[0].text,
                "parameters": _parse_array(
                    eff.xpath("./n42:Parameters", namespaces=ns)[0].text
                ),
                "created": eff.xpath("./n42:Created", namespaces=ns)[0].text,
                "description": eff.xpath("./n42:Description", namespaces=ns)[0].text,
            }
        else:
            instrument_data["efficiency"] = None

        # --- Response Matrix ---
        rm = instrument.xpath("./n42:ResponseMatrix", namespaces=ns)
        if rm:
            rm = rm[0]

            shape = rm.get("matrix_shape")
            shape = tuple(map(int, shape.split())) if shape else None

            matrix = None
            if rm.text:
                decoded = decode_base64(rm.text)
                matrix = decompress_spectrum(decoded)

            instrument_data["response_matrix"] = matrix
            instrument_data["response_matrix_shape"] = shape
        else:
            instrument_data["response_matrix"] = None
            instrument_data["response_matrix_shape"] = None

        # --- Calibration ---
        cal = instrument.xpath("./n42:Calibration", namespaces=ns)
        if cal:
            cal = cal[0]

            calibration = {
                "poly_order": int(cal.xpath("./n42:PolynomialOrder", namespaces=ns)[0].text),
                "coefficients": _parse_array(
                    cal.xpath("./n42:Coefficients", namespaces=ns)[0].text
                ),
                "energy_points": [],
                "channel_points": [],
                "date": None,
            }

            for pt in cal.xpath(".//n42:Point", namespaces=ns):
                E = float(pt.xpath("./n42:Energy", namespaces=ns)[0].text)
                ch = float(pt.xpath("./n42:Channel", namespaces=ns)[0].text)
                calibration["energy_points"].append(E)
                calibration["channel_points"].append(ch)

            date = cal.xpath("./n42:Date", namespaces=ns)
            if date:
                calibration["date"] = date[0].text

            remark = cal.xpath("./n42:Remark", namespaces=ns)
            if remark:
                calibration["remark"] = remark[0].text

            instrument_data["calibration"] = calibration
        else:
            instrument_data["calibration"] = None
            
        remark = instrument.xpath("./n42:Remark", namespaces=ns)
        instrument_data["remark"] = remark[0].text if remark else ""

        return instrument_data
        
