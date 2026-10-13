from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.report_template import MeasurementReport
    from luracs.containers.spectrum_classes import Spectrum
    
from textwrap import dedent

from lxml import etree

from .xml_writer import LRC, n42, write_text_to_SubElement, xml_writer


class xml_report_writer(xml_writer):
    def __init__(self, spectrum: Spectrum, file_name, report: MeasurementReport):
        
        
        txt_report = f"""
        ======================
        | Measurement Report |
        ======================
        
        Date: {report.date}
        Creator: {report.author_name}
        Location: {report.location} (Long: {report.longitude}, Lat: {report.latitude})
        Detector: {report.fg_measurement.detector_name}
        
        Remarks: {report.remark}
        
        == Foreground Summary ==
        Start: {report.fg_measurement.start_time}
        Measurement Time: {report.fg_measurement.measurement_time} s
        Accumulated Counts: {report.fg_measurement.accumulated_counts}
        Accumulated Dose: {report.fg_measurement.accumulated_dose} uSv
        Mean Count Rate: {report.fg_measurement.mean_count_rate} /s
        Mean Dose Rate: {report.fg_measurement.mean_dose_rate} uSv/h
        
        == Background Summary ==
        

        ========================
        """
        
        writer = xml_writer(
            spectrum=spectrum,
            file_name=file_name,
            export_spectrum=True,
            export_instrument=False,
            export_rois=False,
            comment=dedent(txt_report),
            write_in_constructor=False
        )
        
        
        extension_section = (
            etree.SubElement(writer.root, LRC("LuRaCs"), version="1")
        )
        branch = etree.SubElement(extension_section, "MeasurementReport")
        
        
        write_text_to_SubElement(branch, n42("Date"), str(report.date.isoformat()))
        
        fg_section = etree.SubElement(branch, "Foreground")
        
        write_text_to_SubElement(
            fg_section, "MeanCountRate",
            round(report.fg_measurement.mean_count_rate, 4),
            unit="/s"
        )

        write_text_to_SubElement(
            fg_section, "MeanDoseRate",
            round(report.fg_measurement.mean_dose_rate, 4),
            unit="uSv/h"
        )

        write_text_to_SubElement(
            fg_section, "CountRatePoints",
            [round(v, 4) for v in report.fg_measurement.count_rate_data],
            unit="/s"
        )

        write_text_to_SubElement(
            fg_section, "DoseRatePoints",
            [round(v, 4) for v in report.fg_measurement.dose_rate_data],
            unit="uSv/h"
        )

        write_text_to_SubElement(
            fg_section, "TimePoints",
            [round(v, 4) for v in report.fg_measurement.rate_axis]
        )
        
        writer.write_out()