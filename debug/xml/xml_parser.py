#!/home/eewa/anaconda3/bin/python3
# -*- coding: utf-8 -*-
"""
Created on Tue Feb 10 19:27:04 2026

@author: Erik Ewald
"""

import os
os.chdir(os.path.dirname(os.path.realpath(__file__)))
import numpy as np
import matplotlib.pyplot as plt


from lxml import etree as et

# tree = et.parse("/home/eewa/Documents/SommarProjekt/RadiaCode/Spectroscopy/RadiacodeSpectra/Cyklotron_Ba.xml", parser1)
# tree = et.parse("/home/eewa/Documents/git/ConsumerSpectrometers/Measurments/Calibration/Raysid/Raysid-GRF-Ba133.xml", parser1)



from dataclasses import dataclass
from datetime import datetime, timedelta
import numpy as np
from lxml import etree

@dataclass
class SpectrumData:
    y_axis: np.array
    channels: int
    total_counts: int
    live_time: float
    real_time: float = None
    avg_dose_rate: float = None
    avg_cps: float = None
    start_date: datetime = None
    end_date: datetime = None
    spectrum_name: str = None
    instrument: str = None
    
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
        self.tree = etree.parse(path, self.parser)
        self.root = self.tree.getroot()
        self.spectra = self._dispatch_parser()

    def _dispatch_parser(self):
        root_tag = etree.QName(self.root).localname.lower()
        if root_tag == 'radinstrumentdata':
            # Check if Raysid specific by presence of DHS:Raysid
            if self.root.xpath('.//DHS:Raysid', namespaces={'DHS': 'DHS'}):
                return self._parse_raysid()
            else:
                return self._parse_n42_generic()
        elif root_tag == 'resultdatafile':
            return self._parse_radiacode()
        else:
            raise ValueError(f"Unknown XML format: {root_tag}")

    def _parse_raysid(self):
        ns = {'dhs': 'DHS', 'n42': 'http://physics.nist.gov/N42/2011/N42'}
        spectra = {}
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

            spectra[kind] = SpectrumData(
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
        return spectra

    def _parse_radiacode(self):
        spectra = {}
        for result in self.root.findall('.//ResultData'):
            # Foreground
            fg_spec = result.find('.//EnergySpectrum/Spectrum')
            if fg_spec is not None:
                y_axis = np.array([float(x.text) for x in fg_spec.findall('DataPoint')])
                live_time = float(result.findtext('.//EnergySpectrum/LiveTime', '0'))
                real_time = float(result.findtext('.//EnergySpectrum/MeasurementTime', '0'))
                total_counts = int(y_axis.sum())
                channels = len(y_axis)
                avg_cps = total_counts / live_time if live_time else None
                spectra['foreground'] = SpectrumData(
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
                spectra['background'] = SpectrumData(
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
        return spectra

    def _parse_n42_generic(self):
        # Generic N42 parser for other instruments like Detective X
        ns = {'n42': 'http://physics.nist.gov/N42/2011/N42'}
        spectra = {}
        for meas in self.root.xpath('.//n42:RadMeasurement', namespaces=ns):
            print(meas)
            code = meas.findtext('n42:MeasurementClassCode', namespaces=ns, default='').lower()
            kind = 'foreground' if 'foreground' in code else 'background'
            spec_node = meas.find('n42:Spectrum', namespaces=ns)
            if spec_node is None:
                continue
            chan_data = spec_node.findtext('n42:ChannelData', namespaces=ns, default='')
            y_axis = np.array([float(x) for x in chan_data.split() if x.strip()])
            live_time = _parse_iso8601_duration(spec_node.findtext('n42:LiveTimeDuration', namespaces=ns))
            real_time = _parse_iso8601_duration(meas.findtext('n42:RealTimeDuration', namespaces=ns))
            total_counts = int(y_axis.sum())
            channels = len(y_axis)
            avg_cps = total_counts / live_time if live_time else None
            spectra[kind] = SpectrumData(
                y_axis=y_axis,
                channels=channels,
                total_counts=total_counts,
                live_time=live_time,
                real_time=real_time,
                avg_cps=avg_cps,
                start_date=datetime.fromisoformat(meas.findtext('n42:StartDateTime', namespaces=ns)) if meas.findtext('n42:StartDateTime', namespaces=ns) else None,
                end_date=datetime.fromisoformat(meas.findtext('n42:EndDateTime', namespaces=ns)) if meas.findtext('n42:EndDateTime', namespaces=ns) else None,
                spectrum_name=spec_node.get('id'),
                instrument=spec_node.get('radDetectorInformationReference')
            )
        return spectra

# Example usage:
parser = SpectrumParser("/home/eewa/Documents/SommarProjekt/RadiaCode/Spectroscopy/RadiacodeSpectra/InterspecFitted/bkgsub_Cyklotron_Ba.n42")
fg_spectrum = parser.spectra.get("foreground")
plt.plot(fg_spectrum.y_axis)
print(fg_spectrum.__dict__)
bg_spectrum = parser.spectra.get("background")

