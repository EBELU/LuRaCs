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
        
        Weather: {report.weather}
        Wind Direction: {report.wind_direction}
        Wind Speed: {report.wind_speed} m/s
        Air Temperature: {report.air_temperature} C
        
        Remarks: {report.remark}
        
        == Foreground Summary ==
        Start: {report.fg_measurement.start_time}
        Measurement Time: {report.fg_measurement.measurement_time} s
        Accumulated Counts: {report.fg_measurement.accumulated_counts}
        Accumulated Dose: {report.fg_measurement.accumulated_dose} uSv
        Mean Count Rate: {report.fg_measurement.mean_count_rate} /s
        Mean Dose Rate: {report.fg_measurement.mean_dose_rate} uSv/h
        Stops: {[str(v) for v  in report.fg_measurement.stop_timestamps]}
        
        == Background Summary ==
        Start: {report.bg_measurement.start_time}
        Measurement Time: {report.bg_measurement.measurement_time} s
        Accumulated Counts: {report.bg_measurement.accumulated_counts}
        Accumulated Dose: {report.bg_measurement.accumulated_dose} uSv
        Mean Count Rate: {report.bg_measurement.mean_count_rate} /s
        Mean Dose Rate: {report.bg_measurement.mean_dose_rate} uSv/h
        Stops: {[str(v) for v  in report.bg_measurement.stop_timestamps]}

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
        
        meta_data_section = etree.SubElement(extension_section, "MetaData")
        
        write_text_to_SubElement(meta_data_section, "Creator", report.author_name)
        write_text_to_SubElement(meta_data_section, "Location", report.location, 
                                 longitude=str(report.longitude), latitude=str(report.latitude))
        write_text_to_SubElement(meta_data_section, "Weather", report.weather)
        write_text_to_SubElement(meta_data_section, "WindDirection", report.wind_direction)
        write_text_to_SubElement(meta_data_section, "WindSpeed", report.wind_speed, unit="m/s")
        write_text_to_SubElement(meta_data_section, "AirTemperature", report.air_temperature, unit="C")
        write_text_to_SubElement(meta_data_section, "Remarks", report.remark)
        
        for name, data in zip(["Foreground", "Background"], [report.fg_measurement, report.fg_measurement]):
            fg_section = etree.SubElement(branch, name)
            
            write_text_to_SubElement(
                fg_section,
                "Start",
                data.start_time.isoformat()
            )
            
            write_text_to_SubElement(
                fg_section, "MeanCountRate",
                round(data.mean_count_rate, 4),
                unit="/s"
            )

            write_text_to_SubElement(
                fg_section, "MeanDoseRate",
                round(data.mean_dose_rate, 4),
                unit="uSv/h"
            )
            
            write_text_to_SubElement(
                fg_section, 
                "Stops",
                [v.isoformat() for v in data.stop_timestamps]
            )

            write_text_to_SubElement(
                fg_section, "CountRatePoints",
                [round(v, 4) for v in data.count_rate_data],
                unit="/s"
            )

            write_text_to_SubElement(
                fg_section, "DoseRatePoints",
                [round(v, 4) for v in data.dose_rate_data],
                unit="uSv/h"
            )

            write_text_to_SubElement(
                fg_section, "TimePoints",
                [round(v, 4) for v in data.rate_axis]
            )
            
        
        writer.write_out()