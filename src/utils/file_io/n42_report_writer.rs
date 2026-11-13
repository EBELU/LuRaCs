//! N42 report writer — the `xml_report_writer` counterpart.
//!
//! Reuses `N42Writer` for the spectrum preamble, adds a `LRC:LuRaCs /
//! MeasurementReport` extension, and stores a human-readable summary of the
//! report as an XML comment above the root element.

use std::path::PathBuf;

use chrono::{DateTime, SecondsFormat, Utc};
use thiserror::Error;

use crate::containers::Spectrum;
use crate::containers::{GroundMeasurement, MeasurementReport};

use super::n42_writer::{
    N42Result, N42Writer, WriteOptions, XmlNode, XmlValue, append_text, append_text_attrs,
};

pub struct N42ReportWriter<'a> {
    inner: N42Writer<'a>,
    report: &'a MeasurementReport,
}

impl<'a> N42ReportWriter<'a> {
    pub fn new(
        spectrum: &'a Spectrum,
        file_name: impl Into<PathBuf>,
        report: &'a MeasurementReport,
    ) -> N42Result<Self> {
        let comment = format_text_report(report);
        let inner = N42Writer::new(
            spectrum,
            file_name,
            WriteOptions {
                export_spectrum: true,
                export_rois: false,
                export_instrument: false,
                comment: Some(comment),
            },
        )?;
        Ok(Self { inner, report })
    }

    pub fn write_out(&self) -> N42Result<()> {
        // Build the standard root (preamble + spectrum), then append our
        // own LRC:LuRaCs section.
        let mut root = self.inner.build_root()?;

        let mut ext = XmlNode::new("LRC:LuRaCs").attr("version", "1");
        self.write_measurement_report(&mut ext);
        root.push(ext);

        self.inner.write_root(&root)
    }

    fn write_measurement_report(&self, ext: &mut XmlNode) {
        let mut branch = XmlNode::new("MeasurementReport");

        // <n42:Date> inside an LRC section is what Python produces; preserved.
        append_text(
            &mut branch,
            "n42:Date",
            XmlValue::scalar(py_isoformat(&self.report.date)),
        );

        // --- MetaData ---
        let mut meta = XmlNode::new("MetaData");
        append_text(
            &mut meta,
            "Creator",
            XmlValue::scalar(self.report.author_name.clone()),
        );
        append_text_attrs(
            &mut meta,
            "Location",
            XmlValue::scalar(self.report.location.clone()),
            &[
                ("longitude", &self.report.longitude.to_string()),
                ("latitude", &self.report.latitude.to_string()),
            ],
        );
        append_text(
            &mut meta,
            "Weather",
            XmlValue::scalar(self.report.weather.clone()),
        );
        append_text(
            &mut meta,
            "WindDirection",
            XmlValue::scalar(self.report.wind_direction.clone()),
        );
        append_text_attrs(
            &mut meta,
            "WindSpeed",
            XmlValue::float(self.report.wind_speed),
            &[("unit", "m/s")],
        );
        append_text_attrs(
            &mut meta,
            "AirTemperature",
            XmlValue::float(self.report.air_temperature),
            &[("unit", "C")],
        );
        append_text(
            &mut meta,
            "Remarks",
            XmlValue::scalar(self.report.remark.clone()),
        );
        branch.push(meta);

        // --- Foreground / Background ---
        // Python skips a ground entirely when `start_time is None`.
        for (name, data) in [
            ("Foreground", &self.report.fg_measurement),
            ("Background", &self.report.bg_measurement),
        ] {
            if data.start_time.is_none() {
                continue;
            }
            write_ground_measurement(&mut branch, name, data);
        }

        ext.push(branch);
    }
}

fn write_ground_measurement(branch: &mut XmlNode, name: &str, data: &GroundMeasurement) {
    let mut sec = XmlNode::new(name);

    // Guarded by the caller, but keep the check here for direct callers.
    if let Some(st) = data.start_time {
        append_text(&mut sec, "Start", XmlValue::scalar(py_isoformat(&st)));
    }

    append_text_attrs(
        &mut sec,
        "MeanCountRate",
        XmlValue::scalar(py_round_str(data.mean_count_rate, 4)),
        &[("unit", "/s")],
    );
    append_text_attrs(
        &mut sec,
        "MeanDoseRate",
        XmlValue::scalar(py_round_str(data.mean_dose_rate, 4)),
        &[("unit", "uSv/h")],
    );

    append_text(
        &mut sec,
        "Stops",
        XmlValue::strings(data.stop_timestamps.iter().map(py_isoformat)),
    );

    append_text_attrs(
        &mut sec,
        "CountRatePoints",
        XmlValue::strings(data.count_rate_data.iter().map(|v| py_round_str(*v, 2))),
        &[("unit", "/s")],
    );
    append_text_attrs(
        &mut sec,
        "DoseRatePoints",
        XmlValue::strings(data.dose_rate_data.iter().map(|v| py_round_str(*v, 4))),
        &[("unit", "uSv/h")],
    );

    append_text(
        &mut sec,
        "TimePoints",
        XmlValue::strings(data.rate_axis.iter().map(|v| py_round_str(*v, 2))),
    );

    branch.push(sec);
}

// ---------------------------------------------------------------------------
// Python-compatible formatting helpers
// ---------------------------------------------------------------------------

/// `datetime.isoformat()` for a UTC datetime: `"2024-01-01T00:00:00+00:00"`.
fn py_isoformat(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::AutoSi, false)
}

/// `str(round(v, n))` — round to `n` decimals then format without trailing
/// zeros beyond the requested precision (Rust's `Debug` for `f64` matches
/// Python's `str(float)` for finite values).
fn py_round_str(v: f64, decimals: u32) -> String {
    let rounded: f64 = format!("{:.*}", decimals as usize, v).parse().unwrap_or(v);
    format!("{:?}", rounded)
}

/// `str(round(v))` — round-half-to-even, integer result.
fn py_round_int(v: f64) -> String {
    format!("{}", v.round_ties_even() as i64)
}

/// `str(datetime)` — `"2024-01-01 00:00:00+00:00"` (space instead of `T`).
fn py_str_dt(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S%:z").to_string()
}

fn py_str_opt_dt(dt: &Option<DateTime<Utc>>) -> String {
    dt.map(|d| py_str_dt(&d)).unwrap_or_else(|| "None".into())
}

/// `str([...])` for a list of datetimes.
fn py_list_dt(items: &[DateTime<Utc>]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|d| format!("'{}'", py_str_dt(d)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

// ---------------------------------------------------------------------------
// Text report (embedded as an XML comment)
// ---------------------------------------------------------------------------

fn format_text_report(r: &MeasurementReport) -> String {
    format!(
        "\n\
======================\n\
| Measurement Report |\n\
======================\n\
\n\
Date: {date}\n\
Creator: {author}\n\
Location: {location} (Long: {lon}, Lat: {lat})\n\
Detector: {detector}\n\
\n\
Weather: {weather}\n\
Wind Direction: {wind_dir}\n\
Wind Speed: {wind_speed} m/s\n\
Air Temperature: {air_temp} C\n\
\n\
Remarks: {remark}\n\
\n\
== Foreground Summary ==\n\
Start: {fg_start}\n\
Measurement Time: {fg_time:.2} s\n\
Accumulated Counts: {fg_counts}\n\
Accumulated Dose: {fg_dose:.4} uSv\n\
Mean Count Rate: {fg_cr:.2} /s\n\
Mean Dose Rate: {fg_dr:.3} uSv/h\n\
Stops: {fg_stops}\n\
\n\
== Background Summary ==\n\
Start: {bg_start}\n\
Measurement Time: {bg_time:.2} s\n\
Accumulated Counts: {bg_counts}\n\
Accumulated Dose: {bg_dose:.4} uSv\n\
Mean Count Rate: {bg_cr:.2} /s\n\
Mean Dose Rate: {bg_dr:.3} uSv/h\n\
Stops: {bg_stops}\n\
\n\
========================\n",
        date = py_str_dt(&r.date),
        author = r.author_name,
        location = r.location,
        lon = r.longitude,
        lat = r.latitude,
        detector = r.fg_measurement.detector_name,
        weather = r.weather,
        wind_dir = r.wind_direction,
        wind_speed = r.wind_speed,
        air_temp = r.air_temperature,
        remark = r.remark,
        fg_start = py_str_opt_dt(&r.fg_measurement.start_time),
        fg_time = r.fg_measurement.measurement_time,
        fg_counts = py_round_int(r.fg_measurement.accumulated_counts),
        fg_dose = r.fg_measurement.accumulated_dose,
        fg_cr = r.fg_measurement.mean_count_rate,
        fg_dr = r.fg_measurement.mean_dose_rate,
        fg_stops = py_list_dt(&r.fg_measurement.stop_timestamps),
        bg_start = py_str_opt_dt(&r.bg_measurement.start_time),
        bg_time = r.bg_measurement.measurement_time,
        bg_counts = py_round_int(r.bg_measurement.accumulated_counts),
        bg_dose = r.bg_measurement.accumulated_dose,
        bg_cr = r.bg_measurement.mean_count_rate,
        bg_dr = r.bg_measurement.mean_dose_rate,
        bg_stops = py_list_dt(&r.bg_measurement.stop_timestamps),
    )
}
