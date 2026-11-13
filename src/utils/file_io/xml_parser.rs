//! N42 / Raysid / RadiaCode XML parsers.
//!
//! Detects the format from the root element:
//!   * `<ResultDataFile>` → RadiaCode
//!   * `<RadInstrumentData>` with `RadInstrumentInformation[@id="Raysid"]` → Raysid
//!   * `<RadInstrumentData>` otherwise → N42 (with LuRaCs extensions)
//!
//! Bugs in the Python preserved vs. fixed:
//!   * **Preserved:** `Roi.tag` is discarded (Python sets `tag=""` even though
//!     `<Roi id="...">` is present).
//!   * **Preserved:** `chi2_weighted_err` meta stored as-is from the attribute
//!     (further readers should use `meta_bool` with a default).
//!   * **Fixed:** `Description` for efficiency is looked up under `Efficiency`
//!     (Python searches under `<Instrument>`, never finds it).

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use ndarray::Array1;
use physure_core::quantity::Quantity;
use roxmltree::{Document, Node};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::containers::{BkgType, FitType};
use crate::containers::{
    Emission, EmissionOrigin, EmissionType, Fit, GenericInstrument, Roi, Spectrum, SpectrumData,
    UniqueInstrument,
};
use crate::containers::{cm, dimensionless, kev, percent};
use crate::utils::numerics::{PolyfitError, polyfit_weighted};
use crate::utils::{EfficiencySignature, ResolutionSignature};

const N42: &str = "http://physics.nist.gov/N42/2011/N42";
const LRC: &str = "https://example.com/n42/extensions";
const DHS: &str = "DHS";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum XmlParseError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("XML parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("unknown XML root format: {0}")]
    UnknownFormat(String),
    #[error("polyfit failed: {0}")]
    Polyfit(#[from] PolyfitError),
}

pub type XmlParserResult<T> = std::result::Result<T, XmlParseError>;

// ---------------------------------------------------------------------------
// Parsed data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedData {
    pub name: String,
    pub instrument_model: Option<String>,
    pub instrument_id: Option<String>,
    pub instrument_class_code: Option<String>,
    pub calibration: Option<Vec<f64>>,
    pub remark: Option<String>,
    pub foreground: Option<SpectrumData>,
    pub background: Option<SpectrumData>,
    pub peaks: Vec<Roi>,
    pub peak_data: Vec<Option<PeakExtras>>,
    pub instrument: Option<InstrumentData>,
}

impl ParsedData {
    fn new(name: String) -> Self {
        Self {
            name,
            instrument_model: None,
            instrument_id: None,
            instrument_class_code: None,
            calibration: None,
            remark: None,
            foreground: None,
            background: None,
            peaks: Vec::new(),
            peak_data: Vec::new(),
            instrument: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum InstrumentData {
    Generic(GenericInstrument),
    Unique(UniqueInstrument),
}

#[derive(Debug, Clone, Default)]
pub struct PeakExtras {
    pub centroid: Option<f64>,
    pub centroid_err: Option<f64>,
    pub sigma: Option<f64>,
    pub sigma_err: Option<f64>,
    pub amplitude: Option<f64>,
    pub amplitude_err: Option<f64>,
    pub peak_area: Option<f64>,
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

pub struct XmlHeader {
    pub name: String,
    pub instrument_model: Option<String>,
    pub instrument_id: Option<String>,
    pub instrument_class_code: Option<String>,
    pub calibration: Option<Vec<f64>>,
    pub remark: Option<String>,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

pub struct XmlParser {
    pub path: PathBuf,
    pub file_name: String,
    pub meta_only: bool,
    data: ParsedData,
}

impl XmlParser {
    pub fn open(path: impl AsRef<Path>, meta_only: bool) -> XmlParserResult<Self> {
        let path = path.as_ref().to_path_buf();
        let file_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let content = std::fs::read_to_string(&path)?;
        let doc = Document::parse(&content)?;
        let root = doc.root_element();
        let data = Self::dispatch(root, &file_name, meta_only)?;
        Ok(Self {
            path,
            file_name,
            meta_only,
            data,
        })
    }

    pub fn get_spectrum(&self) -> Option<Spectrum> {
        let fg = self.get_foreground_spectrum()?;
        let header = self.get_header();

        let mut spec = Spectrum::new(fg.channels, header.name.clone());
        spec.instrument_id = header.instrument_id.clone();
        spec.instrument_model = header.instrument_model.clone();
        spec.remark = header.remark.clone().unwrap_or_default();

        spec.set_foreground(fg, None);
        if let Some(bg) = self.get_background_spectrum() {
            spec.set_background(bg, None);
        }
        if let Some(InstrumentData::Unique(u)) = &self.data.instrument {
            spec.set_instrument((*u).clone());
        }
        if let Some(cal) = &header.calibration {
            if !cal.is_empty() {
                spec.apply_calibration(cal.clone());
            }
        }
        for roi in self.data.peaks.clone() {
            spec.set_roi(roi);
        }
        Some(spec)
    }

    pub fn get_foreground_spectrum(&self) -> Option<SpectrumData> {
        self.data.foreground.clone()
    }

    pub fn get_background_spectrum(&self) -> Option<SpectrumData> {
        self.data.background.clone()
    }

    pub fn get_rois(&self) -> &[Roi] {
        &self.data.peaks
    }

    pub fn get_roi_data(&self) -> &[Option<PeakExtras>] {
        &self.data.peak_data
    }

    pub fn get_instrument(&self) -> Option<&InstrumentData> {
        self.data.instrument.as_ref()
    }

    pub fn get_header(&self) -> XmlHeader {
        XmlHeader {
            name: self.data.name.clone(),
            instrument_model: self.data.instrument_model.clone(),
            instrument_id: self.data.instrument_id.clone(),
            instrument_class_code: self.data.instrument_class_code.clone(),
            calibration: self.data.calibration.clone(),
            remark: self.data.remark.clone(),
        }
    }

    fn dispatch(root: Node, name: &str, meta_only: bool) -> XmlParserResult<ParsedData> {
        match root.tag_name().name().to_lowercase().as_str() {
            "radinstrumentdata" => {
                let is_raysid = descendant(root, Some(N42), "RadInstrumentInformation")
                    .and_then(|n| n.attribute("id"))
                    .map(|id| id == "Raysid")
                    .unwrap_or(false);
                if is_raysid {
                    Self::parse_raysid(root, name)
                } else {
                    Self::parse_n42(root, name, meta_only)
                }
            }
            "resultdatafile" => Self::parse_radiacode(root, name),
            other => Err(XmlParseError::UnknownFormat(other.to_string())),
        }
    }
}

// ---------------------------------------------------------------------------
// Text / attribute helpers
// ---------------------------------------------------------------------------

fn child<'a, 'input>(
    node: Node<'a, 'input>,
    ns: Option<&'a str>,
    local: &'a str,
) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().namespace() == ns && c.tag_name().name() == local)
}

fn descendant<'a, 'input>(
    node: Node<'a, 'input>,
    ns: Option<&'a str>,
    local: &'a str,
) -> Option<Node<'a, 'input>> {
    node.descendants()
        .find(|c| c.is_element() && c.tag_name().namespace() == ns && c.tag_name().name() == local)
}

fn descendants<'a, 'input>(
    node: Node<'a, 'input>,
    ns: Option<&'a str>,
    local: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.descendants().filter(move |c| {
        c.is_element() && c.tag_name().namespace() == ns && c.tag_name().name() == local
    })
}

fn get_text(node: Node, ns: Option<&str>, local: &str) -> Option<String> {
    child(node, ns, local)
        .and_then(|n| n.text())
        .map(String::from)
}

fn get_descendant_text(node: Node, ns: Option<&str>, local: &str) -> Option<String> {
    descendant(node, ns, local)
        .and_then(|n| n.text())
        .map(String::from)
}

fn get_float(node: Node, ns: Option<&str>, local: &str) -> f64 {
    get_descendant_text(node, ns, local)
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0.0)
}

fn parse_array(text: &str) -> Array1<f64> {
    Array1::from_iter(
        text.split_whitespace()
            .filter_map(|s| s.parse::<f64>().ok()),
    )
}

fn parse_array_opt(text: Option<&str>) -> Option<Array1<f64>> {
    let text = text?;
    if text.trim().is_empty() {
        return None;
    }
    Some(parse_array(text))
}

fn parse_pair(text: Option<&str>) -> Option<(f64, f64)> {
    let mut it = text?.split_whitespace();
    let a: f64 = it.next()?.parse().ok()?;
    let b: f64 = it.next()?.parse().ok()?;
    Some((a, b))
}

fn parse_pair_at(node: Node, ns: Option<&str>, local: &str) -> Option<(f64, f64)> {
    parse_pair(child(node, ns, local).and_then(|n| n.text()))
}

fn parse_iso8601_duration(text: Option<&str>) -> Option<f64> {
    let s = text?;
    if s.starts_with("PT") && s.ends_with('S') {
        s[2..s.len() - 1].parse().ok()
    } else {
        None
    }
}

/// Strip trailing `Z` and parse as naive; interpret the result as UTC.
/// Matches Python's `datetime.fromisoformat(value.removesuffix("Z")).replace(tzinfo=None)`.
fn safe_iso(text: Option<&str>) -> Option<DateTime<Utc>> {
    let s = text?;
    let s = s.strip_suffix('Z').unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f"))
        .ok()
        .map(|ndt| Utc.from_utc_datetime(&ndt))
}

fn str_to_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_emission_type(s: &str) -> Option<EmissionType> {
    match s.trim().to_ascii_lowercase().as_str() {
        "gamma" | "g" => Some(EmissionType::Gamma),
        "beta-" | "beta minus" | "b-" | "β-" | "negatron" => Some(EmissionType::BetaMinus),
        "beta+" | "beta plus" | "b+" | "β+" | "positron" => Some(EmissionType::BetaPlus),
        "alpha" | "a" | "α" => Some(EmissionType::Alpha),
        "x-ray" | "xray" | "x" => Some(EmissionType::XRay),
        "ce" | "conversion electron" => Some(EmissionType::ConversionElectron),
        "annih." | "annih" | "annihilation" => Some(EmissionType::Annihilation),
        _ => None,
    }
}

fn parse_emission_origin(s: &str) -> Option<EmissionOrigin> {
    match s.trim().to_ascii_lowercase().as_str() {
        "annih." | "annih" | "annihilation" => Some(EmissionOrigin::Annihilation),
        "decay" | "nuclear decay" => Some(EmissionOrigin::NuclearDecay),
        _ => None,
    }
}

fn build_spectrum(
    y_axis: Array1<f64>,
    live_time: Option<f64>,
    real_time: Option<f64>,
    avg_dose_rate: Option<f64>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    spectrum_name: Option<String>,
) -> SpectrumData {
    let channels = y_axis.len();
    let total: u64 = y_axis.iter().map(|v| v.max(0.0) as u64).sum();
    let lt = live_time.unwrap_or(0.0);
    let avg_cps = if lt > 0.0 {
        Some(total as f64 / lt)
    } else {
        None
    };
    SpectrumData {
        y_axis,
        channels,
        total_counts: total,
        live_time: lt,
        real_time,
        avg_dose_rate,
        avg_cps,
        start_date,
        end_date,
        spectrum_name,
        instrument: None,
    }
}

// ---------------------------------------------------------------------------
// Raysid
// ---------------------------------------------------------------------------

impl XmlParser {
    // In XmlParser:
    pub fn into_data(self) -> ParsedData {
        self.data
    }
    pub fn get_instrument_owned(&self) -> Option<InstrumentData> {
        self.data.instrument.clone() // if InstrumentData: Clone
    }

    fn parse_raysid(root: Node, name: &str) -> XmlParserResult<ParsedData> {
        let mut data = ParsedData::new(name.to_string());

        // Calibration is a degree-3 polynomial fit to energy boundary values.
        if let Some(ec) = descendant(root, Some(N42), "EnergyCalibration") {
            if let Some(ebv) = child(ec, Some(N42), "EnergyBoundaryValues") {
                if let Some(text) = ebv.text() {
                    let b = parse_array(text);
                    let xs: Vec<f64> = (0..b.len())
                        .filter(|&i| b[i] > 0.0)
                        .map(|i| i as f64)
                        .collect();
                    let ys: Vec<f64> = (0..b.len()).filter(|&i| b[i] > 0.0).map(|i| b[i]).collect();
                    if xs.len() > 3 {
                        let pf = polyfit_weighted(&xs, &ys, None, 3, false)?;
                        data.calibration = Some(pf.coefficients);
                    }
                }
            }
        }

        for meas in descendants(root, Some(N42), "RadMeasurement") {
            let code = get_text(meas, Some(N42), "MeasurementClassCode").unwrap_or_default();
            let kind = if code.to_lowercase().contains("foreground") {
                "fg"
            } else {
                "bg"
            };

            let Some(spec) = child(meas, Some(N42), "Spectrum") else {
                continue;
            };

            let y = get_text(spec, Some(N42), "ChannelData")
                .map(|t| parse_array(&t))
                .unwrap_or_else(|| Array1::zeros(0));

            let live = parse_iso8601_duration(
                child(spec, Some(N42), "LiveTimeDuration").and_then(|n| n.text()),
            );
            let real = parse_iso8601_duration(
                child(meas, Some(N42), "RealTimeDuration").and_then(|n| n.text()),
            );

            let dose = child(meas, Some(DHS), "Raysid")
                .and_then(|n| child(n, Some(DHS), "AverageDoseRate"))
                .and_then(|n| n.text())
                .and_then(|t| t.parse().ok());

            let sd = build_spectrum(
                y,
                live,
                real,
                dose,
                safe_iso(child(meas, Some(N42), "StartDateTime").and_then(|n| n.text())),
                safe_iso(child(meas, Some(N42), "EndDateTime").and_then(|n| n.text())),
                spec.attribute("id").map(String::from),
            );

            if kind == "fg" {
                data.foreground = Some(sd);
            } else {
                data.background = Some(sd);
            }
        }
        Ok(data)
    }

    // -----------------------------------------------------------------------
    // RadiaCode
    // -----------------------------------------------------------------------

    fn parse_radiacode(root: Node, name: &str) -> XmlParserResult<ParsedData> {
        let mut data = ParsedData::new(name.to_string());

        data.instrument_model = descendant_no_ns(root, "Name").or_else(|| {
            descendant(root, Some(""), "Name")
                .and_then(|n| n.text())
                .map(String::from)
        });
        // The above is approximate; the Python locates it via
        // `.//ResultDataList/ResultData/DeviceConfigReference/Name`. Since the
        // RadiaCode schema uses no namespaces, walk descendants directly:
        data.instrument_model = Some(
            descendant(root, None, "DeviceConfigReference")
                .and_then(|n| child(n, None, "Name"))
                .and_then(|n| n.text())
                .unwrap_or("Unknown")
                .to_string(),
        );

        if let Some(cal) = descendant(root, None, "EnergyCalibration") {
            if let Some(coeffs_node) = descendant(cal, None, "Coefficients") {
                let mut coeffs: Vec<f64> = descendants(coeffs_node, None, "Coefficient")
                    .filter_map(|n| n.text().and_then(|t| t.parse().ok()))
                    .collect();
                coeffs.reverse();
                coeffs.truncate(3);
                if !coeffs.is_empty() {
                    data.calibration = Some(coeffs);
                }
            }
        }

        for result in descendants(root, None, "ResultData") {
            if let Some(fg_spec) =
                descendant(result, None, "EnergySpectrum").and_then(|n| child(n, None, "Spectrum"))
            {
                data.instrument_id = descendant(result, None, "SerialNumber")
                    .and_then(|n| n.text())
                    .map(String::from);

                let y: Array1<f64> = descendants(fg_spec, None, "DataPoint")
                    .filter_map(|n| n.text().and_then(|t| t.parse().ok()))
                    .collect();
                let live = descendant(result, None, "EnergySpectrum")
                    .and_then(|n| child(n, None, "LiveTime"))
                    .and_then(|n| n.text())
                    .and_then(|t| t.parse().ok());
                let real = descendant(result, None, "EnergySpectrum")
                    .and_then(|n| child(n, None, "MeasurementTime"))
                    .and_then(|n| n.text())
                    .and_then(|t| t.parse().ok());
                let spectrum_name = descendant(result, None, "SpectrumName")
                    .and_then(|n| n.text())
                    .map(String::from);
                let instrument = descendant(result, None, "DeviceConfigReference")
                    .and_then(|n| child(n, None, "Name"))
                    .and_then(|n| n.text())
                    .map(String::from);

                data.foreground = Some(build_spectrum(
                    y,
                    live,
                    real,
                    None,
                    safe_iso(descendant(result, None, "StartTime").and_then(|n| n.text())),
                    safe_iso(descendant(result, None, "EndTime").and_then(|n| n.text())),
                    spectrum_name,
                ));
                let _ = instrument; // Python passes as `instrument` kwarg; our SpectrumData can't hold a string.
            }

            if let Some(bg_spec) = descendant(result, None, "BackgroundEnergySpectrum")
                .and_then(|n| child(n, None, "Spectrum"))
            {
                let y: Array1<f64> = descendants(bg_spec, None, "DataPoint")
                    .filter_map(|n| n.text().and_then(|t| t.parse().ok()))
                    .collect();
                let live = descendant(result, None, "BackgroundEnergySpectrum")
                    .and_then(|n| child(n, None, "LiveTime"))
                    .and_then(|n| n.text())
                    .and_then(|t| t.parse().ok());
                let real = descendant(result, None, "BackgroundEnergySpectrum")
                    .and_then(|n| child(n, None, "MeasurementTime"))
                    .and_then(|n| n.text())
                    .and_then(|t| t.parse().ok());
                let spectrum_name = descendant(result, None, "SpectrumName")
                    .and_then(|n| n.text())
                    .map(String::from);

                data.background = Some(build_spectrum(
                    y,
                    live,
                    real,
                    None,
                    safe_iso(descendant(result, None, "StartTime").and_then(|n| n.text())),
                    safe_iso(descendant(result, None, "EndTime").and_then(|n| n.text())),
                    spectrum_name,
                ));
            }
        }
        Ok(data)
    }

    // -----------------------------------------------------------------------
    // N42 (with LuRaCs extensions)
    // -----------------------------------------------------------------------

    fn parse_n42(root: Node, name: &str, meta_only: bool) -> XmlParserResult<ParsedData> {
        let mut data = ParsedData::new(name.to_string());

        data.instrument_id = get_descendant_text(root, Some(N42), "RadInstrumentIdentifier");
        data.instrument_model = get_descendant_text(root, Some(N42), "RadInstrumentModelName");
        data.instrument_class_code = get_descendant_text(root, Some(N42), "RadInstrumentClassCode");
        data.remark = get_descendant_text(root, Some(N42), "Remark");

        if let Some(ec) = descendant(root, Some(N42), "EnergyCalibration") {
            if let Some(text) = child(ec, Some(N42), "CoefficientValues").and_then(|n| n.text()) {
                let mut coeffs: Vec<f64> = text
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                coeffs.reverse();
                if !coeffs.is_empty() {
                    data.calibration = Some(coeffs);
                }
            }
        }

        for meas in descendants(root, Some(N42), "RadMeasurement") {
            let code = get_text(meas, Some(N42), "MeasurementClassCode")
                .unwrap_or_default()
                .to_lowercase();
            let kind = if code.contains("foreground") {
                "fg"
            } else {
                "bg"
            };
            let Some(spec) = child(meas, Some(N42), "Spectrum") else {
                continue;
            };

            let y = if meta_only {
                Array1::zeros(0)
            } else {
                let chan = child(spec, Some(N42), "ChannelData");
                let mut parsed = chan
                    .and_then(|n| n.text())
                    .map(parse_array)
                    .unwrap_or_else(|| Array1::zeros(0));
                if chan.and_then(|n| n.attribute("compressionCode")) == Some("CountedZeroes") {
                    parsed = decompress_counted_zeroes(&parsed);
                }
                parsed
            };

            let live = parse_iso8601_duration(
                child(spec, Some(N42), "LiveTimeDuration").and_then(|n| n.text()),
            );
            let real = parse_iso8601_duration(
                child(meas, Some(N42), "RealTimeDuration").and_then(|n| n.text()),
            );
            let sd = build_spectrum(
                y,
                live,
                real,
                None,
                safe_iso(child(meas, Some(N42), "StartDateTime").and_then(|n| n.text())),
                safe_iso(child(meas, Some(N42), "EndDateTime").and_then(|n| n.text())),
                Some(name.to_string()),
            );

            if kind == "fg" {
                data.foreground = Some(sd);
            } else {
                data.background = Some(sd);
            }
        }

        if let Some(lrc) = child(root, Some(LRC), "LuRaCs") {
            let (peaks, peak_data) = Self::parse_rois_v1(lrc);
            data.peaks = peaks;
            data.peak_data = peak_data;
            data.instrument = Self::parse_instrument(lrc)?;
        }

        Ok(data)
    }

    // -----------------------------------------------------------------------
    // ROIs
    // -----------------------------------------------------------------------

    fn parse_rois_v1(ext_root: Node) -> (Vec<Roi>, Vec<Option<PeakExtras>>) {
        let mut peaks = Vec::new();
        let mut peak_data = Vec::new();

        for roi in descendants(ext_root, Some(N42), "Roi") {
            let alias = roi.attribute("alias").unwrap_or("").to_string();
            let spectrum = roi.attribute("spectrum_ref").unwrap_or("").to_string();

            let cont = child(roi, Some(N42), "PeakContinuum");
            let (region_counts, live_time, e_low, e_high) = match cont {
                Some(c) => (
                    get_float(c, Some(N42), "RegionCounts"),
                    get_float(c, Some(N42), "LiveTime"),
                    get_float(c, Some(N42), "LowerEnergy"),
                    get_float(c, Some(N42), "UpperEnergy"),
                ),
                None => (0.0, 0.0, 0.0, 0.0),
            };

            let mut meta: Map<String, Value> = Map::new();
            if let Some(c) = cont {
                meta_from_attr(&mut meta, "merge", c.attribute("merge"));
                meta_from_attr(&mut meta, "movable", c.attribute("movable"));
                meta_from_attr(
                    &mut meta,
                    "background_subtracted",
                    c.attribute("background_subtracted"),
                );
            }

            let Some(fit_node) = child(roi, Some(N42), "Fit") else {
                continue;
            };
            let fit_type = fit_node
                .text()
                .and_then(|t| t.parse::<FitType>().ok())
                .unwrap_or(FitType::None);
            let bkg_type = descendant(roi, Some(N42), "BackgroundType")
                .and_then(|n| n.text())
                .and_then(|t| t.parse::<BkgType>().ok())
                .unwrap_or(BkgType::None);
            meta_from_attr(
                &mut meta,
                "chi2_weighted_err",
                fit_node.attribute("chi2_weighted_err"),
            );

            // Peak fit results
            let (fit, extras) = match child(roi, Some(N42), "Peak") {
                Some(peak) => {
                    let params = parse_array_opt(
                        child(peak, Some(N42), "FitParameters").and_then(|n| n.text()),
                    );
                    let param_errs = parse_array_opt(
                        child(peak, Some(N42), "FitParametersErr").and_then(|n| n.text()),
                    );
                    let bkg_params = parse_array_opt(
                        child(peak, Some(N42), "BackgroundParameters").and_then(|n| n.text()),
                    );
                    let centroid_pair = parse_pair_at(peak, Some(N42), "Centroid");
                    let sigma_pair = parse_pair_at(peak, Some(N42), "StandardDeviation");
                    let amp_pair = parse_pair_at(peak, Some(N42), "Amplitude");
                    let peak_area_pair = parse_pair_at(peak, Some(N42), "PeakArea");
                    let n_pair = parse_pair_at(peak, Some(N42), "NetCounts");
                    let b_pair = parse_pair_at(peak, Some(N42), "BkgCounts");
                    let g_pair = parse_pair_at(peak, Some(N42), "GrossCounts");

                    let fit = match (params, param_errs) {
                        (Some(p), Some(pe)) => Some(Fit {
                            region_lower: None,
                            region_upper: None,
                            lower: e_low,
                            upper: e_high,
                            params: p,
                            param_errs: pe,
                            bkg_params,
                            bkg_est_channels: 3,
                            g: g_pair.map(|(v, _)| v).unwrap_or(0.0),
                            b: b_pair.map(|(v, _)| v).unwrap_or(0.0),
                            n: n_pair.map(|(v, _)| v).unwrap_or(0.0),
                            peak_area: peak_area_pair.map(|(v, _)| v).unwrap_or(0.0),
                            g_err: g_pair.map(|(_, e)| e).unwrap_or(0.0),
                            b_err: b_pair.map(|(_, e)| e).unwrap_or(0.0),
                            n_err: n_pair.map(|(_, e)| e).unwrap_or(0.0),
                            peak_area_err: peak_area_pair.map(|(_, e)| e).unwrap_or(0.0),
                        }),
                        _ => None,
                    };
                    let extras = PeakExtras {
                        centroid: centroid_pair.map(|(a, _)| a),
                        centroid_err: centroid_pair.map(|(_, b)| b),
                        sigma: sigma_pair.map(|(a, _)| a),
                        sigma_err: sigma_pair.map(|(_, b)| b),
                        amplitude: amp_pair.map(|(a, _)| a),
                        amplitude_err: amp_pair.map(|(_, b)| b),
                        peak_area: peak_area_pair.map(|(a, _)| a),
                    };
                    (fit, extras)
                }
                None => (None, PeakExtras::default()),
            };

            // Emission (nuclide)
            let emission = child(roi, Some(N42), "Nuclide").and_then(parse_emission);

            let movable = roi
                .attribute("movable")
                .and_then(str_to_bool)
                .unwrap_or(false);

            peaks.push(Roi {
                tag: String::new(),
                alias,
                spectrum,
                roi_bound: (e_low, e_high),
                region_bound: (0.0, 0.0),
                fit_type,
                bkg_type,
                bkg_est_channels: 3,
                fit,
                roi_counts: region_counts,
                live_time,
                emission,
                meta,
                merge: true,
                movable,
                poisson_weights: false,
                owner_spectrum: None,
            });
            peak_data.push(Some(extras));
        }

        (peaks, peak_data)
    }

    // -----------------------------------------------------------------------
    // Instrument
    // -----------------------------------------------------------------------

    fn parse_instrument(ext_root: Node) -> XmlParserResult<Option<InstrumentData>> {
        let Some(instrument) = descendant(ext_root, Some(N42), "Instrument") else {
            return Ok(None);
        };

        let is_generic = instrument
            .attribute("generic_instrument")
            .and_then(str_to_bool)
            .unwrap_or(false);

        // Shared fields
        let model = get_text(instrument, Some(N42), "Model").unwrap_or_default();
        let manufacturer = get_text(instrument, Some(N42), "Manufacturer").unwrap_or_default();
        let detector_type = get_text(instrument, Some(N42), "DetectorType").unwrap_or_default();
        let detector_material = get_text(instrument, Some(N42), "DetectorMaterial");
        let detector_shape = get_text(instrument, Some(N42), "DetectorShape");
        let remark = get_text(instrument, Some(N42), "Remark").unwrap_or_default();

        let dims = combine_dims(
            parse_array_opt(get_text(instrument, Some(N42), "DetectorDimensions").as_deref()),
            parse_array_opt(
                get_text(instrument, Some(N42), "DetectorDimensionsUncertainty").as_deref(),
            ),
            cm,
        );

        // Resolution
        let (res_fn, res_params, res_fwhm, res_e, res_created) =
            if let Some(res) = child(instrument, Some(N42), "Resolution") {
                // was: let fn_name = get_text(res, Some(N42), "Function");
                let fn_name: Option<ResolutionSignature> = get_text(res, Some(N42), "Function")
                    .and_then(|t| t.parse::<ResolutionSignature>().ok());

                let params = parse_array_opt(get_text(res, Some(N42), "Parameters").as_deref())
                    .map(|a| a.to_vec());
                let created = get_text(res, Some(N42), "CreatedTS")
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                let mut fwhm: Vec<Quantity> = Vec::new();
                let mut e_pts: Vec<f64> = Vec::new();
                for dp in descendants(res, Some(N42), "DataPoint") {
                    let f = get_float(dp, Some(N42), "FWHM");
                    let fu = get_float(dp, Some(N42), "FWHMUncertainty");
                    let e = get_float(dp, Some(N42), "Energy");
                    fwhm.push(Quantity::new_scalar(f, fu, kev(), None, None));
                    e_pts.push(e);
                }
                (
                    fn_name,
                    params,
                    if fwhm.is_empty() { None } else { Some(fwhm) },
                    if e_pts.is_empty() { None } else { Some(e_pts) },
                    created,
                )
            } else {
                (None, None, None, None, None)
            };

        // Efficiency
        let (eff_fn, eff_params, eff_points, eff_e_pts, eff_created, eff_desc) =
            if let Some(eff) = child(instrument, Some(N42), "Efficiency") {
                // was: let fn_name = get_text(eff, Some(N42), "Function");
                let fn_name: Option<EfficiencySignature> = get_text(eff, Some(N42), "Function")
                    .and_then(|t| t.parse::<EfficiencySignature>().ok());

                let means = parse_array_opt(get_text(eff, Some(N42), "Parameters").as_deref());
                let uncerts =
                    parse_array_opt(get_text(eff, Some(N42), "ParametersUncertainty").as_deref());
                let params = combine_quantity_pairs(means, uncerts, dimensionless);
                let created = get_text(eff, Some(N42), "CreatedTS")
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|dt| dt.with_timezone(&Utc));
                // BUG FIX: Python looks for Description under Instrument;
                // it's actually under Efficiency.
                let desc = get_text(eff, Some(N42), "Description");

                let mut eff_pts: Vec<Quantity> = Vec::new();
                let mut e_pts: Vec<f64> = Vec::new();
                for dp in descendants(eff, Some(N42), "DataPoint") {
                    let v = get_float(dp, Some(N42), "Efficiency");
                    let vu = get_float(dp, Some(N42), "EfficiencyUncertainty");
                    let e = get_float(dp, Some(N42), "Energy");
                    eff_pts.push(Quantity::new_scalar(v, vu, dimensionless(), None, None));
                    e_pts.push(e);
                }
                (
                    fn_name,
                    params,
                    if eff_pts.is_empty() {
                        None
                    } else {
                        Some(eff_pts)
                    },
                    if e_pts.is_empty() { None } else { Some(e_pts) },
                    created,
                    desc,
                )
            } else {
                (None, None, None, None, None, None)
            };

        let generic = GenericInstrument {
            model,
            manufacturer,
            detector_type,
            detector_material,
            detector_shape,
            detector_dimensions_cm: dims,
            resolution_fn: res_fn,
            resolution_params: res_params,
            resolution_e_points: res_e,
            resolution_fwhm_points: res_fwhm,
            resolution_created: res_created,
            int_efficiency_fn: eff_fn,
            int_efficiency_params: eff_params,
            int_efficiency_e_points: eff_e_pts,
            int_efficiency_eff_points: eff_points,
            int_efficiency_created: eff_created,
            int_efficiency_description: eff_desc,
            response_matrix: None,
            response_matrix_shape: None,
            remark,
        };

        if is_generic {
            return Ok(Some(InstrumentData::Generic(generic)));
        }

        // Unique-only fields
        let name = get_text(instrument, Some(N42), "Name").unwrap_or_default();

        let (poly_order, coeffs, cal_e_pts, cal_c_pts, cal_date) =
            if let Some(cal) = child(instrument, Some(N42), "Calibration") {
                let order = get_text(cal, Some(N42), "PolynomialOrder")
                    .and_then(|t| t.parse::<u32>().ok())
                    .unwrap_or(1);
                let coeffs = parse_array_opt(get_text(cal, Some(N42), "Coefficients").as_deref())
                    .map(|a| a.to_vec());

                let mut e_pts: Vec<f64> = Vec::new();
                let mut c_pts: Vec<f64> = Vec::new();
                for dp in descendants(cal, Some(N42), "DataPoint") {
                    e_pts.push(get_float(dp, Some(N42), "Energy"));
                    c_pts.push(get_float(dp, Some(N42), "Channel"));
                }
                let date = get_text(cal, Some(N42), "Date")
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|dt| dt.with_timezone(&Utc));
                (
                    Some(order),
                    coeffs,
                    if e_pts.is_empty() { None } else { Some(e_pts) },
                    if c_pts.is_empty() { None } else { Some(c_pts) },
                    date,
                )
            } else {
                (None, None, None, None, None)
            };

        Ok(Some(InstrumentData::Unique(UniqueInstrument {
            name,
            calibration_poly_order: poly_order,
            calibration_coefficients: coeffs,
            calibration_energy_points: cal_e_pts,
            calibration_channel_points: cal_c_pts,
            calibration_date: cal_date,
            generic,
        })))
    }
}

// ---------------------------------------------------------------------------
// Format-specific helpers
// ---------------------------------------------------------------------------

fn meta_from_attr(meta: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(s) = value {
        match str_to_bool(s) {
            Some(b) => {
                meta.insert(key.to_string(), Value::Bool(b));
            }
            None => {
                meta.insert(key.to_string(), Value::String(s.to_string()));
            }
        }
    }
}

fn combine_dims(
    means: Option<Array1<f64>>,
    uncerts: Option<Array1<f64>>,
    unit_fn: fn() -> physure_core::units::rational::RationalUnit,
) -> Option<Vec<Quantity>> {
    combine_quantity_pairs(means, uncerts, unit_fn)
}

fn combine_quantity_pairs(
    means: Option<Array1<f64>>,
    uncerts: Option<Array1<f64>>,
    unit_fn: fn() -> physure_core::units::rational::RationalUnit,
) -> Option<Vec<Quantity>> {
    let means = means?;
    match uncerts {
        Some(u) if u.len() == means.len() => Some(
            means
                .iter()
                .zip(u.iter())
                .map(|(m, s)| Quantity::new_scalar(*m, *s, unit_fn(), None, None))
                .collect(),
        ),
        _ => Some(
            means
                .iter()
                .map(|m| Quantity::new_scalar(*m, 0.0, unit_fn(), None, None))
                .collect(),
        ),
    }
}

fn parse_emission(nuclide: Node) -> Option<Emission> {
    let parent_text = get_text(nuclide, Some(N42), "Name")?;
    let parent_nuclide = nucleide_nuclei::NuclideId::from_name(&parent_text).ok();

    let energy_pair = parse_pair_at(nuclide, Some(N42), "Energy");
    let intensity_pair = parse_pair_at(nuclide, Some(N42), "Intensity");
    let r#type = get_text(nuclide, Some(N42), "Type").and_then(|t| parse_emission_type(&t));
    let origin =
        get_text(nuclide, Some(N42), "EmissionOrigin").and_then(|t| parse_emission_origin(&t));

    Some(Emission {
        parent_nuclide,
        energy_kev: energy_pair.map(|(m, u)| Quantity::new_scalar(m, u, kev(), None, None)),
        intensity_percent: intensity_pair
            .map(|(m, u)| Quantity::new_scalar(m, u, percent(), None, None)),
        r#type,
        origin,
    })
}

fn descendant_no_ns<'a, 'input>(node: Node<'a, 'input>, local: &'a str) -> Option<String> {
    descendant(node, None, local)
        .and_then(|n| n.text())
        .map(String::from)
}

fn decompress_counted_zeroes(values: &Array1<f64>) -> Array1<f64> {
    let mut result: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < values.len() {
        let v = values[i] as i64;
        if v == 0 && i + 1 < values.len() {
            let run = values[i + 1] as usize;
            result.extend(std::iter::repeat_n(0.0, run));
            i += 2;
        } else {
            result.push(values[i]);
            i += 1;
        }
    }
    Array1::from_vec(result)
}
