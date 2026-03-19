from dataclasses import dataclass
from datetime import datetime, timedelta
import numpy as np
from lxml import etree
import os
try:
    import matplotlib.pyplot as plt
except:
    pass

from SpectrumClasses import SpectrumData

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
    
def strip_ns(root):
    """Remove all namespace prefixes in an lxml tree."""
    for elem in root.getiterator():
        if not hasattr(elem.tag, 'find'):
            continue  # skip comments or processing instructions
        i = elem.tag.find('}')
        if i >= 0:
            elem.tag = elem.tag[i+1:]
    etree.cleanup_namespaces(root)


def _parse_iso8601_duration(duration: str) -> float:
    """Parses ISO 8601 duration like PT1234.56S into seconds."""
    if not duration:
        return None
    try:
        if duration.startswith('PT') and duration.endswith('S'):
            return float(duration[2:-1])
    except Exception:
        return None
    


class ExternalSpectrumParser:
    N42_NS = {'n42': 'http://physics.nist.gov/N42/2011/N42'}
    DHS_NS = {'dhs': 'DHS', **N42_NS}

    def __init__(self, path: str):
        self.path = path
        self.file_name = os.path.splitext(os.path.basename(path))[0]
        parser = etree.XMLParser(recover=True, remove_comments=True)
        self.tree = etree.parse(path, parser)
        self.root = self.tree.getroot()
        self.kwargs = self._dispatch_parser()

    def _dispatch_parser(self):
        root_tag = etree.QName(self.root).localname.lower()
        if root_tag == "radinstrumentdata":
            node = self.root.find(".//n42:RadInstrumentInformation", namespaces=self.N42_NS)
            return self._parse_raysid() if node is not None and node.get("id") == "Raysid" else self._parse_n42_generic()
        if root_tag == "resultdatafile":
            return self._parse_radiacode()
        raise ValueError(f"Unknown XML format: {root_tag}")

    def _parse_array(self, text: str):
        return np.array([float(x) for x in text.split() if x.strip()]) if text else np.array([])

    def _safe_iso(self, value: str):
        return datetime.fromisoformat(value) if value else None

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
        ns, kwargs = self.DHS_NS, {"name": self.file_name}
        node = self.root.find(".//n42:EnergyCalibration/n42:EnergyBoundaryValues", namespaces=ns)
        if node is not None and node.text:
            b = self._parse_array(node.text)
            kwargs["calibration"] = np.polyfit(np.arange(len(b)), b, 4)

        for meas in self.root.xpath(".//n42:RadMeasurement", namespaces=ns):
            code = meas.findtext("n42:MeasurementClassCode", default="", namespaces=ns).lower()
            kind = "foreground" if "foreground" in code else "background"
            spec = meas.find("n42:Spectrum", namespaces=ns)
            if spec is None: continue

            y = self._parse_array(spec.findtext("n42:ChannelData", default="", namespaces=ns))
            live = _parse_iso8601_duration(spec.findtext("n42:LiveTimeDuration", namespaces=ns))
            real = _parse_iso8601_duration(meas.findtext("n42:RealTimeDuration", namespaces=ns))

            dhs = meas.find("dhs:Raysid", namespaces=ns)
            dose = float(dhs.findtext("dhs:AverageDoseRate", "0", namespaces=ns)) if dhs is not None else None

            kwargs[kind] = self._build_spectrum(
                y, live, real,
                avg_dose_rate=dose,
                start_date=self._safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=self._safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=spec.get("id"),
                instrument=spec.get("radDetectorInformationReference")
            )
        return kwargs

    def _parse_radiacode(self):
        kwargs = {"name": self.file_name}
        coeffs = self.root.findall(".//EnergyCalibration/Coefficients/Coefficient")
        if coeffs:
            kwargs["calibration"] = [float(c.text) for c in coeffs][::-1]

        for result in self.root.findall(".//ResultData"):
            fg = result.find(".//EnergySpectrum/Spectrum")
            if fg is not None:
                y = np.array([float(x.text) for x in fg.findall("DataPoint")])
                kwargs["foreground"] = self._build_spectrum(
                    y,
                    float(result.findtext(".//EnergySpectrum/LiveTime", "0")),
                    float(result.findtext(".//EnergySpectrum/MeasurementTime", "0")),
                    start_date=self._safe_iso(result.findtext(".//StartTime")),
                    end_date=self._safe_iso(result.findtext(".//EndTime")),
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
                    start_date=self._safe_iso(result.findtext(".//StartTime")),
                    end_date=self._safe_iso(result.findtext(".//EndTime")),
                    spectrum_name=bg.findtext("../SpectrumName"),
                    instrument=result.findtext(".//DeviceConfigReference/Name")
                )
        return kwargs

    def _parse_n42_generic(self):
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

            chan = spec.find("n42:ChannelData", namespaces=ns)
            data = chan.text.strip() if chan is not None else ""
            y = self._parse_array(data)

            if chan is not None and chan.get("compressionCode") == "CountedZeroes":
                y = decompress_counted_zeroes(y)

            kwargs[kind] = self._build_spectrum(
                y,
                _parse_iso8601_duration(spec.findtext("n42:LiveTimeDuration", namespaces=ns)),
                _parse_iso8601_duration(meas.findtext("n42:RealTimeDuration", namespaces=ns)),
                start_date=self._safe_iso(meas.findtext("n42:StartDateTime", namespaces=ns)),
                end_date=self._safe_iso(meas.findtext("n42:EndDateTime", namespaces=ns)),
                spectrum_name=self.file_name,
                instrument=instrument
            )
        return kwargs

if __name__ == "__main__":
    # Example usage:

    parser = SpectrumParser("/home/eewa/Documents/git/MySpect/debug/xml/Cyklotron_Ba.n42")
    # parser = SpectrumParser("/home/eewa/Documents/git/MySpect/debug/xml/103-GRF-Ba133.xml")
    # parser = SpectrumParser("/home/eewa/Documents/git/MySpect/debug/xml/Th.n42")
    fg_spectrum = parser.kwargs.get("foreground")
    plt.plot(fg_spectrum.y_axis)
    plt.show()
    print(parser.kwargs)
    bg_spectrum = parser.kwargs.get("background")

