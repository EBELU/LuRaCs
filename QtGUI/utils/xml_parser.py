from dataclasses import dataclass
from datetime import datetime, timedelta
import numpy as np
from lxml import etree
import os
try:
    import matplotlib.pyplot as plt
except:
    pass

from ..SpectrumClasses import SpectrumData

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

class SpectrumParser:
    def __init__(self, path: str):
        self.parser = etree.XMLParser(recover=True, remove_comments=True)
        self.path = path
        filename = os.path.basename(path)
        name_without_ext = os.path.splitext(filename)[0]
        self.file_name = name_without_ext
        self.tree = etree.parse(path, self.parser)
        self.root = self.tree.getroot()
        self.kwargs = self._dispatch_parser()

    def _dispatch_parser(self):
        root_tag = etree.QName(self.root).localname.lower()
        if root_tag == 'radinstrumentdata':
            instrument_node = self.root.find(".//n42:RadInstrumentInformation", namespaces={'n42': 'http://physics.nist.gov/N42/2011/N42'})
            # Extract the id attribute
            if instrument_node is not None:
                if instrument_node.get("id") == "Raysid":
                    return self._parse_raysid()
                else:
                    return self._parse_n42_generic()
        elif root_tag == 'resultdatafile':
            return self._parse_radiacode()
        else:
            raise ValueError(f"Unknown XML format: {root_tag}")

    def _parse_raysid(self):
        ns = {'dhs': 'DHS', 'n42': 'http://physics.nist.gov/N42/2011/N42'}
        kwargs = {}
        kwargs["name"] = self.file_name
        
        boundary_node = self.root.find(".//n42:EnergyCalibration/n42:EnergyBoundaryValues", namespaces=ns)
        if boundary_node is not None and boundary_node.text:
            boundaries = [float(b) for b in boundary_node.text.split()]
            kwargs["calibration"] = np.polyfit(np.arange(len(boundaries)), boundaries, 4)
            
            
        for meas in self.root.xpath('.//n42:RadMeasurement', namespaces=ns):
            code = meas.findtext('n42:MeasurementClassCode', namespaces=ns, default='').lower()
            kind = 'foreground' if 'foreground' in code else 'background'

            spec_node = meas.find('n42:Spectrum', namespaces=ns)
            if spec_node is None:
                continue

            chan_data = spec_node.findtext('n42:ChannelData', namespaces=ns, default='')
            y_axis = np.array([float(x) for x in chan_data.split() if x.strip()])

            live_time = _parse_iso8601_duration(spec_node.findtext('n42:LiveTimeDuration', namespaces=ns))
            real_time = _parse_iso8601_duration(meas.findtext('n42:RealTimeDuration', namespaces=ns))

            dhs = meas.find('dhs:Raysid', namespaces=ns)
            avg_dose_rate = float(dhs.findtext('dhs:AverageDoseRate', default='0', namespaces=ns)) if dhs is not None else None

            total_counts = int(y_axis.sum())
            channels = len(y_axis)
            avg_cps = total_counts / live_time if live_time else None

            kwargs[kind] = SpectrumData(
                y_axis=y_axis,
                channels=channels,
                total_counts=total_counts,
                live_time=live_time,
                real_time=real_time,
                avg_dose_rate=avg_dose_rate,
                avg_cps=avg_cps,
                start_date=datetime.fromisoformat(meas.findtext('n42:StartDateTime', namespaces=ns)) if meas.findtext('n42:StartDateTime', namespaces=ns) else None,
                end_date=datetime.fromisoformat(meas.findtext('n42:EndDateTime', namespaces=ns)) if meas.findtext('n42:EndDateTime', namespaces=ns) else None,
                spectrum_name=spec_node.get('id'),
                instrument=spec_node.get('radDetectorInformationReference')
            )
        return kwargs

    def _parse_radiacode(self):
        kwargs = {}
        kwargs["name"] = self.file_name
        for result in self.root.findall('.//ResultData'):
            # Foreground
            fg_spec = result.find('.//EnergySpectrum/Spectrum')
            
            if fg_spec is not None:
                coeff_nodes = self.root.findall(".//EnergyCalibration/Coefficients/Coefficient")
                if coeff_nodes:
                    kwargs["calibration"] = [float(c.text) for c in coeff_nodes][::-1]
                    
                y_axis = np.array([float(x.text) for x in fg_spec.findall('DataPoint')])
                live_time = float(result.findtext('.//EnergySpectrum/LiveTime', '0'))
                real_time = float(result.findtext('.//EnergySpectrum/MeasurementTime', '0'))
                total_counts = int(y_axis.sum())
                channels = len(y_axis)
                avg_cps = total_counts / live_time if live_time else None
                kwargs['foreground'] = SpectrumData(
                    y_axis=y_axis,
                    channels=channels,
                    total_counts=total_counts,
                    live_time=live_time,
                    real_time=real_time,
                    avg_cps=avg_cps,
                    start_date=datetime.fromisoformat(result.findtext('.//StartTime')) if result.findtext('.//StartTime') else None,
                    end_date=datetime.fromisoformat(result.findtext('.//EndTime')) if result.findtext('.//EndTime') else None,
                    spectrum_name=fg_spec.findtext('../SpectrumName'),
                    instrument=result.findtext('.//DeviceConfigReference/Name')
                )

            # Background
            bg_spec = result.find('.//BackgroundEnergySpectrum/Spectrum')
            if bg_spec is not None:
                y_axis = np.array([float(x.text) for x in bg_spec.findall('DataPoint')])
                live_time = float(result.findtext('.//BackgroundEnergySpectrum/LiveTime', '0'))
                real_time = float(result.findtext('.//BackgroundEnergySpectrum/MeasurementTime', '0'))
                total_counts = int(y_axis.sum())
                channels = len(y_axis)
                avg_cps = total_counts / live_time if live_time else None
                kwargs['background'] = SpectrumData(
                    y_axis=y_axis,
                    channels=channels,
                    total_counts=total_counts,
                    live_time=live_time,
                    real_time=real_time,
                    avg_cps=avg_cps,
                    start_date=datetime.fromisoformat(result.findtext('.//StartTime')) if result.findtext('.//StartTime') else None,
                    end_date=datetime.fromisoformat(result.findtext('.//EndTime')) if result.findtext('.//EndTime') else None,
                    spectrum_name=bg_spec.findtext('../SpectrumName'),
                    instrument=result.findtext('.//DeviceConfigReference/Name')
                )
        return kwargs

    def _parse_n42_generic(self):
        # Generic N42 parser for other instruments like Detective X
        ns = {'n42': 'http://physics.nist.gov/N42/2011/N42'}
        kwargs = {}
        
        instrument_id = self.root.findtext(
            './/n42:RadInstrumentIdentifier',
            namespaces=ns
        )
        
        coeff_text = self.root.findtext(
            './/n42:EnergyCalibration/n42:CoefficientValues',
            namespaces=ns
        )
        if coeff_text is not None:
            kwargs["calibration"] = [float(x) for x in coeff_text.split()][::-1]
        kwargs["name"] = self.file_name
        
        for meas in self.root.xpath('.//n42:RadMeasurement', namespaces=ns):
            code = meas.findtext('n42:MeasurementClassCode', namespaces=ns, default='').lower()
            kind = 'foreground' if 'foreground' in code else 'background'
            spec_node = meas.find('n42:Spectrum', namespaces=ns)
            if spec_node is None:
                continue
            chan_node = spec_node.find('n42:ChannelData', namespaces=ns)

            chan_data = chan_node.text.strip() if chan_node is not None else ""
            compression = chan_node.get("compressionCode") if chan_node is not None else None
            y_axis = np.array([float(x) for x in chan_data.split() if x.strip()])

            if compression == "CountedZeroes":
                y_axis = decompress_counted_zeroes(y_axis)

            live_time = _parse_iso8601_duration(spec_node.findtext('n42:LiveTimeDuration', namespaces=ns))
            real_time = _parse_iso8601_duration(meas.findtext('n42:RealTimeDuration', namespaces=ns))
            total_counts = int(y_axis.sum())
            channels = len(y_axis)
            avg_cps = total_counts / live_time if live_time else None
            kwargs[kind] = SpectrumData(
                y_axis=y_axis,
                channels=channels,
                total_counts=total_counts,
                live_time=live_time,
                real_time=real_time,
                avg_cps=avg_cps,
                start_date=datetime.fromisoformat(meas.findtext('n42:StartDateTime', namespaces=ns)) if meas.findtext('n42:StartDateTime', namespaces=ns) else None,
                end_date=datetime.fromisoformat(meas.findtext('n42:EndDateTime', namespaces=ns)) if meas.findtext('n42:EndDateTime', namespaces=ns) else None,
                spectrum_name=self.file_name,
                instrument=instrument_id
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

