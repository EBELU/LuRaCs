//! N42 XML writer for spectra and (LuRaCs-extension) ROIs / instruments.
//!
//! Namespace note: the Python uses `nsmap={None: N42, "LRC": ext}`, so N42
//! tags serialize unprefixed (inheriting the root default NS) and extension
//! tags get an `LRC:` prefix. Some elements in the Python (`EnergyCalibration`,
//! `ChannelData`, `Peaks`, `Roi`, …) are created without an explicit namespace
//! but serialize with the same bytes as `n42(...)`/`LRC(...)` versions. We
//! reproduce the bytes exactly; consumers that parse with namespace support
//! see the elements in N42 or LRC depending on context, matching what any
//! N42-aware tool would see from the Python output.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use ndarray::Array2;
use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use thiserror::Error;
use uuid::Uuid;

use crate::containers::BkgType;
use crate::containers::FitType;
use crate::containers::{
    Emission, EmissionOrigin, EmissionType, Fit, GenericInstrument, Roi, Spectrum, SpectrumData,
    UniqueInstrument,
};
use crate::utils::{compress_spectrum, encode_base64};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NS: &str = "http://physics.nist.gov/N42/2011/N42";
const MY: &str = "https://example.com/n42/extensions";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum N42Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML write error: {0}")]
    Xml(String),
    #[error("compression / base64 error: {0}")]
    Codec(String),
}

pub type N42Result<T> = std::result::Result<T, N42Error>;

impl From<quick_xml::Error> for N42Error {
    fn from(e: quick_xml::Error) -> Self {
        N42Error::Xml(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Minimal XML tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum XmlNode {
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        text: Option<String>,
        children: Vec<XmlNode>,
    },
    Comment(String),
}

impl XmlNode {
    pub fn new(name: impl Into<String>) -> Self {
        XmlNode::Element {
            name: name.into(),
            attrs: Vec::new(),
            text: None,
            children: Vec::new(),
        }
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let XmlNode::Element { attrs, .. } = &mut self {
            attrs.push((key.into(), value.into()));
        }
        self
    }

    fn text(mut self, text: impl Into<String>) -> Self {
        if let XmlNode::Element { text: t, .. } = &mut self {
            *t = Some(text.into());
        }
        self
    }

    pub fn push(&mut self, child: XmlNode) {
        if let XmlNode::Element { children, .. } = self {
            children.push(child);
        }
    }

    fn write_to<W: Write>(&self, writer: &mut Writer<W>) -> N42Result<()> {
        match self {
            XmlNode::Element {
                name,
                attrs,
                text,
                children,
            } => {
                let mut start = BytesStart::new(name.as_str());
                for (k, v) in attrs {
                    start.push_attribute((k.as_str(), v.as_str()));
                }
                if children.is_empty() && text.is_none() {
                    writer.write_event(Event::Empty(start))?;
                } else {
                    writer.write_event(Event::Start(start))?;
                    if let Some(t) = text {
                        if !t.is_empty() {
                            writer.write_event(Event::Text(BytesText::new(t)))?;
                        }
                    }
                    for c in children {
                        c.write_to(writer)?;
                    }
                    writer.write_event(Event::End(BytesEnd::new(name.as_str())))?;
                }
            }
            XmlNode::Comment(c) => {
                writer.write_event(Event::Comment(BytesText::new(c.as_str())))?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Value helper (mirrors `write_text_to_SubElement`'s data)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum XmlValue {
    Scalar(String),
    List(Vec<String>),
}

impl XmlValue {
    pub fn scalar(s: impl Into<String>) -> Self {
        XmlValue::Scalar(s.into())
    }
    pub fn float(v: f64) -> Self {
        XmlValue::Scalar(v.to_string())
    }
    fn int(v: i64) -> Self {
        XmlValue::Scalar(v.to_string())
    }
    fn floats(v: impl IntoIterator<Item = f64>) -> Self {
        XmlValue::List(v.into_iter().map(|x| x.to_string()).collect())
    }
    fn ints(v: impl IntoIterator<Item = i64>) -> Self {
        XmlValue::List(v.into_iter().map(|x| x.to_string()).collect())
    }
    pub fn strings(v: impl IntoIterator<Item = impl Into<String>>) -> Self {
        XmlValue::List(v.into_iter().map(|x| x.into()).collect())
    }
    fn text(&self) -> String {
        match self {
            XmlValue::Scalar(s) => s.clone(),
            XmlValue::List(v) => v.join(" "),
        }
    }
}

fn text_node(name: &str, value: XmlValue, attrs: &[(&str, &str)]) -> XmlNode {
    let mut n = XmlNode::new(name).text(value.text());
    for (k, v) in attrs {
        n = n.attr(*k, *v);
    }
    n
}

pub fn append_text(parent: &mut XmlNode, name: &str, value: XmlValue) {
    parent.push(text_node(name, value, &[]));
}

pub fn append_text_attrs(
    parent: &mut XmlNode,
    name: &str,
    value: XmlValue,
    attrs: &[(&str, &str)],
) {
    parent.push(text_node(name, value, attrs));
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WriteOptions {
    pub export_spectrum: bool,
    pub export_rois: bool,
    pub export_instrument: bool,
    pub comment: Option<String>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            export_spectrum: true,
            export_rois: true,
            export_instrument: true,
            comment: None,
        }
    }
}

/// Convenience function matching the Python's default `write_in_constructor=True`.
pub fn write_n42_file(spectrum: &Spectrum, path: impl AsRef<Path>) -> N42Result<()> {
    N42Writer::new(
        spectrum,
        path.as_ref().to_path_buf(),
        WriteOptions::default(),
    )?
    .write_out()
}

pub struct N42Writer<'a> {
    spectrum: &'a Spectrum,
    file_name: PathBuf,
    opts: WriteOptions,
}

impl<'a> N42Writer<'a> {
    pub fn new(
        spectrum: &'a Spectrum,
        file_name: impl Into<PathBuf>,
        opts: WriteOptions,
    ) -> N42Result<Self> {
        Ok(Self {
            spectrum,
            file_name: file_name.into(),
            opts,
        })
    }

    pub fn write_out(&self) -> N42Result<()> {
        let root = self.build_root()?;
        self.write_root(&root)
    }

    pub fn build_root(&self) -> N42Result<XmlNode> {
        let uuid = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);

        let mut root = XmlNode::new("RadInstrumentData")
            .attr("xmlns", NS)
            .attr("xmlns:LRC", MY)
            .attr("n42DocUUID", uuid)
            .attr("n42DocDateTime", now);

        // --- Preamble ---
        append_text(
            &mut root,
            "RadInstrumentDataCreatorName",
            XmlValue::scalar("LuRaCs"),
        );

        let mut rad_info = XmlNode::new("RadInstrumentInformation");
        append_text(
            &mut rad_info,
            "RadInstrumentIdentifier",
            XmlValue::scalar(
                self.spectrum
                    .instrument_id
                    .clone()
                    .unwrap_or_else(|| "Unknown".into()),
            ),
        );
        append_text(
            &mut rad_info,
            "RadInstrumentModelName",
            XmlValue::scalar(
                self.spectrum
                    .instrument_model
                    .clone()
                    .unwrap_or_else(|| "Unknown".into()),
            ),
        );
        root.push(rad_info);

        // --- Spectrum ---
        if self.opts.export_spectrum {
            self.write_spectrum_section(&mut root)?;
        }

        // --- Extensions ---
        let mut extension: Option<XmlNode> = None;

        if self.opts.export_rois && !self.spectrum.rois.is_empty() {
            let ext =
                extension.get_or_insert_with(|| XmlNode::new("LRC:LuRaCs").attr("version", "1"));
            let mut peaks = XmlNode::new("Peaks");
            write_roi_data(&self.spectrum.rois, &mut peaks);
            ext.push(peaks);
        }

        if self.opts.export_instrument {
            if let Some(inst) = &self.spectrum.instrument {
                let ext = extension
                    .get_or_insert_with(|| XmlNode::new("LRC:LuRaCs").attr("version", "1"));
                write_instrument_data(inst, ext)?;
            }
        }

        if let Some(ext) = extension {
            root.push(ext);
        }

        Ok(root)
    }

    /// Serialize an arbitrary root (e.g. one a caller has extended).
    pub fn write_root(&self, root: &XmlNode) -> N42Result<()> {
        let path = self.file_name.with_extension("xml");
        let mut buf = Vec::with_capacity(64 * 1024);
        {
            let mut writer = Writer::new_with_indent(&mut buf, b' ', 2);
            writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
            if let Some(c) = &self.opts.comment {
                writer.write_event(Event::Comment(BytesText::new(c.as_str())))?;
            }
            root.write_to(&mut writer)?;
        }
        std::fs::write(&path, buf)?;
        Ok(())
    }

    fn write_spectrum_section(&self, root: &mut XmlNode) -> N42Result<()> {
        let s = self.spectrum;

        let fg_id = s
            .foreground
            .as_ref()
            .and_then(|d| d.spectrum_name.clone())
            .unwrap_or_else(|| s.name.clone());

        let bg_id = s
            .background
            .as_ref()
            .and_then(|d| d.spectrum_name.clone())
            .unwrap_or_else(|| "BkgSample0".into());

        if s.calibrated {
            if let Some(coeffs) = &s.calibration_coefficients {
                let mut energy_cal = XmlNode::new("EnergyCalibration").attr("id", "EnergyCal0");
                // Reverse: Spectrum stores highest-degree-first; N42 wants
                // lowest-degree-first.
                let reversed: Vec<f64> = coeffs.iter().rev().copied().collect();
                append_text(
                    &mut energy_cal,
                    "CoefficientValues",
                    XmlValue::floats(reversed),
                );
                root.push(energy_cal);
            }
        }

        if !s.remark.is_empty() {
            append_text(root, "Remark", XmlValue::scalar(s.remark.clone()));
        }

        if let Some(fg) = &s.foreground {
            write_spectrum_data(root, fg, "foreground", &fg_id);
        }
        if let Some(bg) = &s.background {
            write_spectrum_data(root, bg, "background", &bg_id);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// RadMeasurement / Spectrum
// ---------------------------------------------------------------------------

fn write_spectrum_data(root: &mut XmlNode, data: &SpectrumData, kind: &str, spec_id: &str) {
    let mut rad = XmlNode::new("RadMeasurement").attr("id", spec_id);

    append_text(&mut rad, "MeasurementClassCode", XmlValue::scalar(kind));

    // StartDateTime is always present; empty text when start_date is None.
    let start_text = match &data.start_date {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        None => String::new(),
    };
    append_text(&mut rad, "StartDateTime", XmlValue::scalar(start_text));

    // RealTimeDuration: skip when zero (Python truthiness).
    if let Some(rt) = data.real_time {
        if rt != 0.0 {
            append_text(
                &mut rad,
                "RealTimeDuration",
                XmlValue::scalar(format!("PT{:.2}S", rt)),
            );
        }
    }

    let id = data.spectrum_name.clone().unwrap_or_else(|| match kind {
        "foreground" => "Sample0Spectrum".into(),
        _ => "Sample1Spectrum".into(),
    });

    let mut spectrum = XmlNode::new("Spectrum")
        .attr("energyCalibrationReference", "EnergyCal0")
        .attr("id", id);

    append_text(
        &mut spectrum,
        "LiveTimeDuration",
        XmlValue::scalar(format!("PT{:.2}S", data.live_time)),
    );

    // Rounded counts, space-separated. `ChannelData` is unprefixed in the
    // Python (inconsistent with siblings but preserved).
    let rounded: Vec<i64> = data.y_axis.iter().map(|v| v.round() as i64).collect();
    append_text_attrs(
        &mut spectrum,
        "ChannelData",
        XmlValue::ints(rounded),
        &[("compressionCode", "None")],
    );

    rad.push(spectrum);
    root.push(rad);
}

// ---------------------------------------------------------------------------
// ROI / Peaks extension
// ---------------------------------------------------------------------------

fn write_roi_data(rois: &std::collections::HashMap<String, Roi>, peaks_section: &mut XmlNode) {
    append_text(
        peaks_section,
        "NumberOfPeaks",
        XmlValue::int(rois.len() as i64),
    );

    for (tag, roi) in rois {
        write_one_roi(peaks_section, tag, roi);
    }
}

fn write_one_roi(parent: &mut XmlNode, tag: &str, roi: &Roi) {
    let mut peak = XmlNode::new("Roi")
        .attr("id", tag)
        .attr("alias", roi.alias.clone())
        .attr("spectrum_ref", roi.spectrum.clone())
        .attr("version", "1");

    // --- Continuum ---
    let mut continuum = XmlNode::new("PeakContinuum")
        .attr("merge", bool_py(meta_bool(&roi.meta, "merge", true)))
        .attr("movable", bool_py(meta_bool(&roi.meta, "movable", true)))
        .attr(
            "background_subtracted",
            bool_py(meta_bool(&roi.meta, "background_subtracted", false)),
        );

    append_text(
        &mut continuum,
        "RegionCounts",
        XmlValue::float(roi.roi_counts),
    );
    append_text(&mut continuum, "LiveTime", XmlValue::float(roi.live_time));
    append_text(
        &mut continuum,
        "LowerEnergy",
        XmlValue::float(roi.roi_bound.0),
    );
    append_text(
        &mut continuum,
        "UpperEnergy",
        XmlValue::float(roi.roi_bound.1),
    );
    peak.push(continuum);

    // --- Fit type / background type ---
    append_text_attrs(
        &mut peak,
        "Fit",
        XmlValue::scalar(fit_type_str(roi.fit_type)),
        &[(
            "chi2_weighted_err",
            bool_py(meta_bool(&roi.meta, "chi2_weighted_err", true)),
        )],
    );
    append_text(
        &mut peak,
        "BackgroundType",
        XmlValue::scalar(bkg_type_str(roi.bkg_type)),
    );

    // --- Fit results ---
    if let Some(fit) = &roi.fit {
        let mut p = XmlNode::new("Peak");

        // Python writes BackgroundParameters unconditionally, which would
        // TypeError if bkg_params were None. Guard here.
        if let Some(bp) = &fit.bkg_params {
            append_text(
                &mut p,
                "BackgroundParameters",
                XmlValue::floats(bp.iter().copied()),
            );
        }

        append_text(
            &mut p,
            "FitParameters",
            XmlValue::floats(fit.params.iter().copied()),
        );
        append_text(
            &mut p,
            "FitParametersErr",
            XmlValue::floats(fit.param_errs.iter().copied()),
        );

        append_text(
            &mut p,
            "Centroid",
            XmlValue::floats([fit.mu(), fit.mu_err()]),
        );
        append_text(
            &mut p,
            "StandardDeviation",
            XmlValue::floats([fit.sigma(), fit.sigma_err()]),
        );
        append_text(
            &mut p,
            "Amplitude",
            XmlValue::floats([fit.a(), fit.a_err()]),
        );
        append_text(
            &mut p,
            "PeakArea",
            XmlValue::floats([fit.peak_area, fit.peak_area_err]),
        );
        append_text(&mut p, "NetCounts", XmlValue::floats([fit.n, fit.n_err]));
        append_text(&mut p, "BkgCounts", XmlValue::floats([fit.b, fit.b_err]));
        // BUG preserved from Python: GrossCounts carries B_err, not G_err.
        append_text(&mut p, "GrossCounts", XmlValue::floats([fit.g, fit.b_err]));

        peak.push(p);
    }

    // --- Nuclide ---
    if let Some(em) = &roi.emission {
        if let Some(nuclide_id) = &em.parent_nuclide {
            let mut nuclide = XmlNode::new("Nuclide");
            append_text(&mut nuclide, "Name", XmlValue::scalar(nuclide_id.to_name()));

            if let Some(q) = &em.energy_kev {
                append_text_attrs(
                    &mut nuclide,
                    "Energy",
                    XmlValue::floats([q.value.mean(), q.value.std_dev()]),
                    &[("unit", "keV")],
                );
            }
            if let Some(q) = &em.intensity_percent {
                append_text_attrs(
                    &mut nuclide,
                    "Intensity",
                    XmlValue::floats([q.value.mean(), q.value.std_dev()]),
                    &[("unit", "%")],
                );
            }
            if let Some(t) = em.r#type {
                append_text(&mut nuclide, "Type", XmlValue::scalar(emission_type_str(t)));
            }
            if let Some(o) = em.origin {
                append_text(
                    &mut nuclide,
                    "EmissionOrigin",
                    XmlValue::scalar(emission_origin_str(o)),
                );
            }
            peak.push(nuclide);
        }
    }

    parent.push(peak);
}

// ---------------------------------------------------------------------------
// Instrument extension
// ---------------------------------------------------------------------------

fn write_instrument_data(instrument: &UniqueInstrument, root: &mut XmlNode) -> N42Result<()> {
    let mut section = XmlNode::new("Instrument").attr("generic_instrument", "false");

    // --- Basic info ---
    append_text(
        &mut section,
        "Name",
        XmlValue::scalar(instrument.name.clone()),
    );
    append_text(
        &mut section,
        "Model",
        XmlValue::scalar(instrument.model.clone()),
    );
    append_text(
        &mut section,
        "Manufacturer",
        XmlValue::scalar(instrument.manufacturer.clone()),
    );

    if let Some(m) = &instrument.detector_material {
        append_text(
            &mut section,
            "DetectorMaterial",
            XmlValue::scalar(m.clone()),
        );
    }
    if let Some(s) = &instrument.detector_shape {
        append_text(&mut section, "DetectorShape", XmlValue::scalar(s.clone()));
    }
    append_text(
        &mut section,
        "DetectorType",
        XmlValue::scalar(instrument.detector_type.clone()),
    );

    if let Some(dims) = &instrument.detector_dimensions_cm {
        append_text_attrs(
            &mut section,
            "DetectorDimensions",
            XmlValue::floats(dims.iter().map(|q| q.value.mean())),
            &[("unit", "cm")],
        );
        append_text_attrs(
            &mut section,
            "DetectorDimensionsUncertainty",
            XmlValue::floats(dims.iter().map(|q| q.value.std_dev())),
            &[("unit", "cm")],
        );
    }

    // --- Resolution ---
    if let Some(fn_name) = &instrument.resolution_fn {
        let mut res = XmlNode::new("Resolution").attr("type", "energy");

        if let Some(dt) = &instrument.resolution_created {
            append_text(&mut res, "CreatedTS", XmlValue::scalar(dt.to_rfc3339()));
        }
        append_text(
            &mut res,
            "Function",
            XmlValue::scalar(format!("{fn_name:?}")),
        );
        if let Some(params) = &instrument.resolution_params {
            append_text(
                &mut res,
                "Parameters",
                XmlValue::floats(params.iter().copied()),
            );
        }

        let mut points = XmlNode::new("ResolutionPoints")
            .attr("energy_unit", "keV")
            .attr("FWHM_unit", "keV");
        if let (Some(fwhm), Some(e_pts)) = (
            &instrument.resolution_fwhm_points,
            &instrument.resolution_e_points,
        ) {
            let cols = vec![
                fwhm.iter().map(|q| q.value.mean().to_string()).collect(),
                fwhm.iter().map(|q| q.value.std_dev().to_string()).collect(),
                e_pts.iter().map(|v| v.to_string()).collect(),
            ];
            write_coupled_points(
                &mut points,
                "DataPoint",
                &["FWHM", "FWHMUncertainty", "Energy"],
                &cols,
            );
        }
        res.push(points);
        section.push(res);
    }

    // --- Efficiency ---
    if let Some(fn_name) = &instrument.int_efficiency_fn {
        let mut eff = XmlNode::new("Efficiency").attr("type", "intrinsic");

        if let Some(dt) = &instrument.int_efficiency_created {
            append_text(&mut eff, "CreatedTS", XmlValue::scalar(dt.to_rfc3339()));
        }
        append_text(
            &mut eff,
            "Function",
            XmlValue::scalar(format!("{fn_name:?}")),
        );

        if let Some(params) = &instrument.int_efficiency_params {
            append_text(
                &mut eff,
                "Parameters",
                XmlValue::floats(params.iter().map(|q| q.value.mean())),
            );
            append_text(
                &mut eff,
                "ParametersUncertainty",
                XmlValue::floats(params.iter().map(|q| q.value.std_dev())),
            );
        }

        let mut points = XmlNode::new("EfficiencyPoints")
            .attr("energy_unit", "keV")
            .attr("efficiency_unit", "unitless");
        if let (Some(eff_pts), Some(e_pts)) = (
            &instrument.int_efficiency_eff_points,
            &instrument.int_efficiency_e_points,
        ) {
            let cols = vec![
                eff_pts.iter().map(|q| q.value.mean().to_string()).collect(),
                eff_pts
                    .iter()
                    .map(|q| q.value.std_dev().to_string())
                    .collect(),
                e_pts.iter().map(|v| v.to_string()).collect(),
            ];
            write_coupled_points(
                &mut points,
                "DataPoint",
                &["Efficiency", "EfficiencyUncertainty", "Energy"],
                &cols,
            );
        }
        eff.push(points);

        if let Some(dt) = &instrument.int_efficiency_created {
            append_text(&mut eff, "Created", XmlValue::scalar(dt.to_rfc3339()));
        }
        if let Some(desc) = &instrument.int_efficiency_description {
            append_text(&mut eff, "Description", XmlValue::scalar(desc.clone()));
        }
        section.push(eff);
    }

    // --- Response matrix ---
    if let Some(matrix) = &instrument.response_matrix {
        let encoded = encode_response_matrix(matrix)?;
        let shape = match &instrument.response_matrix_shape {
            Some(s) => format!("{} {}", s[0], s[1]),
            None => format!("{} {}", matrix.nrows(), matrix.ncols()),
        };
        append_text_attrs(
            &mut section,
            "ResponseMatrix",
            XmlValue::scalar(encoded),
            &[
                ("matrix_shape", shape.as_str()),
                ("compression", "Base64-zLib"),
            ],
        );
    }

    // --- Calibration (UniqueInstrument only) ---
    if let Some(coeffs) = &instrument.calibration_coefficients {
        let mut cal = XmlNode::new("Calibration");
        append_text(
            &mut cal,
            "PolynomialOrder",
            XmlValue::int(instrument.calibration_poly_order.unwrap_or(0) as i64),
        );
        append_text(
            &mut cal,
            "Coefficients",
            XmlValue::floats(coeffs.iter().copied()),
        );

        let mut points = XmlNode::new("CalibrationPoints").attr("energy_unit", "keV");
        if let (Some(e_pts), Some(c_pts)) = (
            &instrument.calibration_energy_points,
            &instrument.calibration_channel_points,
        ) {
            let cols = vec![
                e_pts.iter().map(|v| v.to_string()).collect(),
                c_pts.iter().map(|v| v.to_string()).collect(),
            ];
            write_coupled_points(&mut points, "DataPoint", &["Energy", "Channel"], &cols);
        }
        cal.push(points);

        if let Some(dt) = &instrument.calibration_date {
            append_text(&mut cal, "CreatedTS", XmlValue::scalar(dt.to_rfc3339()));
        }
        section.push(cal);
    }

    if !instrument.remark.is_empty() {
        append_text(
            &mut section,
            "Remark",
            XmlValue::scalar(instrument.remark.clone()),
        );
    }

    root.push(section);
    Ok(())
}

fn encode_response_matrix(matrix: &Array2<f64>) -> N42Result<String> {
    // Flatten row-major, cast to u32, compress, base64.
    let flat: Vec<u32> = matrix.iter().map(|&v| v as u32).collect();
    let compressed = compress_spectrum(&flat).map_err(|e| N42Error::Codec(e.to_string()))?;
    Ok(encode_base64(&compressed))
}

// ---------------------------------------------------------------------------
// Coupled points helper
// ---------------------------------------------------------------------------

fn write_coupled_points(
    parent: &mut XmlNode,
    point_name: &str,
    column_names: &[&str],
    columns: &[Vec<String>],
) {
    let n = columns.iter().map(Vec::len).min().unwrap_or(0);
    for i in 0..n {
        let mut dp = XmlNode::new(point_name);
        for (j, col) in columns.iter().enumerate() {
            append_text(&mut dp, column_names[j], XmlValue::scalar(col[i].clone()));
        }
        parent.push(dp);
    }
}

// ---------------------------------------------------------------------------
// Enum / meta helpers
// ---------------------------------------------------------------------------

fn fit_type_str(t: FitType) -> String {
    match t {
        FitType::Gaussian => "Gaussian".into(),
        FitType::None => "None".into(),
    }
}

fn bkg_type_str(t: BkgType) -> String {
    match t {
        BkgType::None => "None".into(),
        BkgType::Linear => "Linear".into(),
        BkgType::Quadratic => "Quadratic".into(),
    }
}

fn emission_type_str(t: EmissionType) -> String {
    match t {
        EmissionType::Gamma => "gamma".into(),
        EmissionType::Annihilation => "annih.".into(),
        EmissionType::BetaMinus => "beta-".into(),
        EmissionType::BetaPlus => "beta+".into(),
        EmissionType::Alpha => "alpha".into(),
        EmissionType::XRay => "x-ray".into(),
        EmissionType::ConversionElectron => "ce".into(),
    }
}

fn emission_origin_str(o: EmissionOrigin) -> String {
    match o {
        EmissionOrigin::Annihilation => "annih.".into(),
        EmissionOrigin::NuclearDecay => "decay".into(),
    }
}

/// Python `str(bool)` → "True"/"False". Preserved for XML-attribute fidelity.
fn bool_py(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

fn meta_bool(meta: &serde_json::Map<String, serde_json::Value>, key: &str, default: bool) -> bool {
    meta.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}
