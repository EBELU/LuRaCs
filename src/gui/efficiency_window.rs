//! Intrinsic Efficiency Calibration Window.
//!
//! Ported from `luracs/gui/windows/calc_efficiency_window/`.

use std::collections::HashSet;
use std::f64::consts::{LN_2, PI};

use chrono::{DateTime, NaiveDateTime, Utc};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use physure_core::quantity::Quantity;
use rsplot::{Marker, Plot1D, YAxis, leastsq};
use serde::{Deserialize, Serialize};

use crate::containers::{CountField, dimensionless};
use crate::core::SpectrumManager;
use crate::utils::EfficiencySignature;
use crate::utils::numerics::calibration::physics_models::efficiency::lookup as lookup_efficiency;
use crate::utils::numerics::calibration::u_intrinsic_efficiency;

// ---------------------------------------------------------------------------
// Units & Formatting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Us,
    Ms,
    S,
    Min,
    H,
    D,
    Yr,
}

impl TimeUnit {
    pub const ALL: [Self; 7] = [
        Self::Us,
        Self::Ms,
        Self::S,
        Self::Min,
        Self::H,
        Self::D,
        Self::Yr,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Ms => "ms",
            Self::S => "s",
            Self::Min => "min",
            Self::H => "h",
            Self::D => "d",
            Self::Yr => "yr",
        }
    }

    #[must_use]
    pub const fn seconds(self) -> f64 {
        match self {
            Self::Us => 1e-6,
            Self::Ms => 1e-3,
            Self::S => 1.0,
            Self::Min => 60.0,
            Self::H => 3600.0,
            Self::D => 86_400.0,
            Self::Yr => 86_400.0 * 365.24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityUnit {
    Bq,
    KBq,
    MBq,
    GBq,
    TBq,
    Ci,
    MCi,
    UCi,
    NCi,
}

impl ActivityUnit {
    pub const ALL: [Self; 9] = [
        Self::Bq,
        Self::KBq,
        Self::MBq,
        Self::GBq,
        Self::TBq,
        Self::Ci,
        Self::MCi,
        Self::UCi,
        Self::NCi,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bq => "Bq",
            Self::KBq => "kBq",
            Self::MBq => "MBq",
            Self::GBq => "GBq",
            Self::TBq => "TBq",
            Self::Ci => "Ci",
            Self::MCi => "mCi",
            Self::UCi => "uCi",
            Self::NCi => "nCi",
        }
    }

    #[must_use]
    pub const fn bq(self) -> f64 {
        match self {
            Self::Bq => 1.0,
            Self::KBq => 1e3,
            Self::MBq => 1e6,
            Self::GBq => 1e9,
            Self::TBq => 1e12,
            Self::Ci => 3.7e10,
            Self::MCi => 3.7e7,
            Self::UCi => 3.7e4,
            Self::NCi => 3.7e1,
        }
    }
}

#[must_use]
pub fn format_duration_unit(seconds: f64) -> (f64, TimeUnit) {
    if seconds == 0.0 {
        return (0.0, TimeUnit::S);
    }
    let abs_s = seconds.abs();
    for unit in [
        TimeUnit::Yr,
        TimeUnit::D,
        TimeUnit::H,
        TimeUnit::Min,
        TimeUnit::S,
        TimeUnit::Ms,
        TimeUnit::Us,
    ] {
        if abs_s >= unit.seconds() {
            return (seconds / unit.seconds(), unit);
        }
    }
    (seconds / TimeUnit::Us.seconds(), TimeUnit::Us)
}

#[must_use]
pub fn format_activity_unit(activity_bq: f64, uncert_bq: f64) -> (f64, f64, ActivityUnit) {
    if activity_bq == 0.0 {
        return (0.0, 0.0, ActivityUnit::Bq);
    }
    let abs_a = activity_bq.abs();
    for unit in [
        ActivityUnit::TBq,
        ActivityUnit::GBq,
        ActivityUnit::MBq,
        ActivityUnit::KBq,
        ActivityUnit::Bq,
    ] {
        if abs_a >= unit.bq() {
            return (activity_bq / unit.bq(), uncert_bq / unit.bq(), unit);
        }
    }
    (activity_bq, uncert_bq, ActivityUnit::Bq)
}

// ---------------------------------------------------------------------------
// Source & Physics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EfficiencySource {
    pub name: String,
    pub nuclide: String,
    pub spectrum: String,
    pub half_life_s: f64,
    #[serde(rename = "ref_activity_Bq")]
    pub ref_activity_bq: f64,
    #[serde(rename = "ref_activity_uncert_Bq")]
    pub ref_activity_uncert_bq: f64,
    pub calibration_time: DateTime<Utc>,
    pub measurement_start: DateTime<Utc>,
    pub measurement_end: DateTime<Utc>,
    pub distance_cm: f64,
    pub distance_uncert_cm: f64,
    pub solid_angle: f64,
    pub solid_angle_uncert: f64,
    pub measurement_decay_compensate: bool,
    pub manual_solid_angle: bool,
}

impl Default for EfficiencySource {
    fn default() -> Self {
        let now = Utc::now();
        Self {
            name: "Source 1".to_string(),
            nuclide: "Cs-137".to_string(),
            spectrum: "None".to_string(),
            half_life_s: 30.08 * 86_400.0 * 365.24,
            ref_activity_bq: 37_000.0,
            ref_activity_uncert_bq: 1_850.0,
            calibration_time: now,
            measurement_start: now,
            measurement_end: now,
            distance_cm: 10.0,
            distance_uncert_cm: 0.2,
            solid_angle: 0.0,
            solid_angle_uncert: 0.0,
            measurement_decay_compensate: true,
            manual_solid_angle: false,
        }
    }
}

#[derive(Debug, Clone)]
struct SourceUiDraft {
    name: String,
    nuclide: String,
    spectrum: String,
    half_life_val: f64,
    half_life_unit: TimeUnit,
    activity_val: f64,
    activity_uncert: f64,
    activity_unit: ActivityUnit,
    calibration_str: String,
    meas_start_str: String,
    meas_end_str: String,
    distance_cm: f64,
    distance_uncert_cm: f64,
    solid_angle: f64,
    solid_angle_uncert: f64,
    measurement_decay_compensate: bool,
    manual_solid_angle: bool,
}

impl SourceUiDraft {
    fn from_source(src: &EfficiencySource) -> Self {
        let (hl_v, hl_u) = format_duration_unit(src.half_life_s);
        let (act_v, act_s, act_u) =
            format_activity_unit(src.ref_activity_bq, src.ref_activity_uncert_bq);
        Self {
            name: src.name.clone(),
            nuclide: src.nuclide.clone(),
            spectrum: src.spectrum.clone(),
            half_life_val: hl_v,
            half_life_unit: hl_u,
            activity_val: act_v,
            activity_uncert: act_s,
            activity_unit: act_u,
            calibration_str: src.calibration_time.format("%Y-%m-%d %H:%M:%S").to_string(),
            meas_start_str: src.measurement_start.format("%Y-%m-%d %H:%M:%S").to_string(),
            meas_end_str: src.measurement_end.format("%Y-%m-%d %H:%M:%S").to_string(),
            distance_cm: src.distance_cm,
            distance_uncert_cm: src.distance_uncert_cm,
            solid_angle: src.solid_angle,
            solid_angle_uncert: src.solid_angle_uncert,
            measurement_decay_compensate: src.measurement_decay_compensate,
            manual_solid_angle: src.manual_solid_angle,
        }
    }

    fn to_source(&self) -> EfficiencySource {
        EfficiencySource {
            name: self.name.clone(),
            nuclide: self.nuclide.clone(),
            spectrum: self.spectrum.clone(),
            half_life_s: self.half_life_val * self.half_life_unit.seconds(),
            ref_activity_bq: self.activity_val * self.activity_unit.bq(),
            ref_activity_uncert_bq: self.activity_uncert * self.activity_unit.bq(),
            calibration_time: parse_datetime_utc(&self.calibration_str).unwrap_or_else(Utc::now),
            measurement_start: parse_datetime_utc(&self.meas_start_str).unwrap_or_else(Utc::now),
            measurement_end: parse_datetime_utc(&self.meas_end_str).unwrap_or_else(Utc::now),
            distance_cm: self.distance_cm,
            distance_uncert_cm: self.distance_uncert_cm,
            solid_angle: self.solid_angle,
            solid_angle_uncert: self.solid_angle_uncert,
            measurement_decay_compensate: self.measurement_decay_compensate,
            manual_solid_angle: self.manual_solid_angle,
        }
    }
}

fn parse_datetime_utc(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s.trim()) {
        return Some(dt.with_timezone(&Utc));
    }
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|ndt| ndt.and_utc())
}

#[must_use]
pub fn decay_corrected_activity(
    a0_bq: f64,
    a0_uncert_bq: f64,
    half_life_s: f64,
    elapsed_s: f64,
) -> (f64, f64) {
    if half_life_s <= 0.0 || !half_life_s.is_finite() {
        return (a0_bq, a0_uncert_bq);
    }
    let factor = (-elapsed_s / half_life_s).exp2();
    (a0_bq * factor, a0_uncert_bq * factor)
}

#[must_use]
pub fn number_of_decays(
    a_start_bq: f64,
    a_start_unc_bq: f64,
    half_life_s: f64,
    duration_s: f64,
) -> (f64, f64) {
    if duration_s <= 0.0 {
        return (0.0, 0.0);
    }
    if half_life_s <= 0.0 || !half_life_s.is_finite() {
        return (a_start_bq * duration_s, a_start_unc_bq * duration_s);
    }
    let factor = (half_life_s / LN_2) * (1.0 - (-duration_s / half_life_s).exp2());
    (a_start_bq * factor, a_start_unc_bq * factor)
}

#[must_use]
pub fn compute_solid_angle(
    area_cm2: f64,
    area_unc_cm2: f64,
    distance_cm: f64,
    distance_unc_cm: f64,
) -> (f64, f64) {
    if distance_cm <= 0.0 || area_cm2 <= 0.0 {
        return (0.0, 0.0);
    }
    let omega = area_cm2 / (4.0 * PI * distance_cm * distance_cm);
    let rel_area = area_unc_cm2 / area_cm2;
    let rel_dist = 2.0 * distance_unc_cm / distance_cm;
    let rel_unc = rel_area.hypot(rel_dist);
    (omega, omega * rel_unc)
}

pub fn compute_point_efficiency(
    source: &EfficiencySource,
    detector_area_cm2: f64,
    detector_area_unc_cm2: f64,
    counts: f64,
    counts_uncert: f64,
    yield_frac: f64,
    yield_uncert: f64,
) -> Result<(f64, f64), String> {
    if yield_frac <= 0.0 || !yield_frac.is_finite() {
        return Err("Invalid emission yield".into());
    }
    let elapsed_s = (source.measurement_start - source.calibration_time).num_seconds() as f64;
    let (a_start, a_start_unc) = decay_corrected_activity(
        source.ref_activity_bq,
        source.ref_activity_uncert_bq,
        source.half_life_s,
        elapsed_s,
    );

    let duration_s = (source.measurement_end - source.measurement_start)
        .num_seconds()
        .max(1) as f64;

    let (cps, cps_unc) = if source.measurement_decay_compensate {
        let (decays, decays_unc) =
            number_of_decays(a_start, a_start_unc, source.half_life_s, duration_s);
        if decays <= 0.0 {
            return Err("Expected decays <= 0".into());
        }
        let geom = if source.manual_solid_angle {
            if source.solid_angle <= 0.0 {
                return Err("Solid angle must be > 0".into());
            }
            (source.solid_angle, source.solid_angle_uncert)
        } else {
            compute_solid_angle(
                detector_area_cm2,
                detector_area_unc_cm2,
                source.distance_cm,
                source.distance_uncert_cm,
            )
        };
        if geom.0 <= 0.0 {
            return Err("Geometric solid angle <= 0".into());
        }
        let eff = counts / (decays * yield_frac * geom.0);
        let rel = (counts_uncert / counts.max(1e-12))
            .hypot(decays_unc / decays)
            .hypot(yield_uncert / yield_frac)
            .hypot(geom.1 / geom.0);
        return Ok((eff, eff * rel));
    } else {
        (counts / duration_s, counts_uncert / duration_s)
    };

    let q_act = Quantity::new_scalar(a_start, a_start_unc, dimensionless(), None, None);
    let q_dist = Quantity::new_scalar(
        source.distance_cm,
        source.distance_uncert_cm,
        dimensionless(),
        None,
        None,
    );
    let q_yield = Quantity::new_scalar(yield_frac, yield_uncert, dimensionless(), None, None);
    let q_area = Quantity::new_scalar(
        detector_area_cm2,
        detector_area_unc_cm2,
        dimensionless(),
        None,
        None,
    );
    let q_cps = Quantity::new_scalar(cps, cps_unc, dimensionless(), None, None);
    let eff = u_intrinsic_efficiency(&q_act, &q_dist, &q_yield, &q_area, &q_cps);
    Ok((eff.value.mean(), eff.value.std_dev()))
}

// ---------------------------------------------------------------------------
// Efficiency Window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PeakRow {
    tag: String,
    source_name: String,
    roi_alias: String,
    spectrum: String,
    counts: f64,
    counts_uncert: f64,
    yield_frac: f64,
    yield_uncert: f64,
    energy_kev: f64,
    nuclide: String,
}

pub struct EfficiencyWindow {
    pub open: bool,
    sources: Vec<EfficiencySource>,
    selected_source: Option<usize>,
    draft: Option<SourceUiDraft>,
    selected_peaks: HashSet<String>,
    selected_instrument: String,
    detector_area_cm2: f64,
    detector_area_unc_cm2: f64,
    weighted_fit: bool,
    energies: Vec<f64>,
    efficiencies: Vec<(f64, f64)>,
    fit_params: Vec<f64>,
    fit_uncerts: Vec<f64>,
    status_msg: String,
    plot: Plot1D,
    import_slot: crate::gui::PendingFileSlot<Option<std::path::PathBuf>>,
    export_slot: crate::gui::PendingFileSlot<Option<std::path::PathBuf>>,
}

impl EfficiencyWindow {
    #[must_use]
    pub fn new(render_state: &RenderState) -> Self {
        let mut plot = Plot1D::new(render_state, 0x3001);
        plot.set_graph_x_label("Energy [keV]");
        plot.set_graph_y_label("Intrinsic Efficiency", YAxis::Left);
        Self {
            open: false,
            sources: vec![EfficiencySource::default()],
            selected_source: Some(0),
            draft: Some(SourceUiDraft::from_source(&EfficiencySource::default())),
            selected_peaks: HashSet::new(),
            selected_instrument: String::new(),
            detector_area_cm2: 1.0,
            detector_area_unc_cm2: 0.05,
            weighted_fit: true,
            energies: Vec::new(),
            efficiencies: Vec::new(),
            fit_params: Vec::new(),
            fit_uncerts: Vec::new(),
            status_msg: String::new(),
            plot,
            import_slot: crate::gui::PendingFileSlot::default(),
            export_slot: crate::gui::PendingFileSlot::default(),
        }
    }

    pub const fn open(&mut self) {
        self.open = true;
    }

    pub fn retheme(&mut self, colors: &crate::theme_manager::ResolvedPlotColors) {
        colors.apply_to(&mut self.plot);
    }

    fn collect_peaks(&self, spectrum_manager: &SpectrumManager) -> Vec<PeakRow> {
        let mut out = Vec::new();
        for roi in spectrum_manager.roi_manager.registry().values() {
            let Some(em) = &roi.emission else {
                continue;
            };
            let nuclide = em
                .parent_nuclide
                .map_or_else(|| "Other".to_string(), |n| n.to_string());
            let energy_kev = em.energy_kev.as_ref().map_or(0.0, |q| q.value.mean());
            let (yield_frac, yield_uncert) = em.intensity_percent.as_ref().map_or((0.0, 0.0), |q| {
                (q.value.mean() / 100.0, q.value.std_dev() / 100.0)
            });
            let counts = roi
                .get_count_data(CountField::PeakArea, false)
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(roi.roi_counts);
            let counts_uncert = roi
                .get_count_data(CountField::PeakAreaErr, false)
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or_else(|| counts.max(1.0).sqrt());
            let source_name = self
                .sources
                .iter()
                .find(|s| s.nuclide == nuclide && (s.spectrum == "None" || s.spectrum == roi.spectrum))
                .map_or_else(|| "—".to_string(), |s| s.name.clone());

            out.push(PeakRow {
                tag: roi.tag.clone(),
                source_name,
                roi_alias: roi.alias.clone(),
                spectrum: roi.spectrum.clone(),
                counts,
                counts_uncert,
                yield_frac,
                yield_uncert,
                energy_kev,
                nuclide,
            });
        }
        out
    }

    fn sync_draft_to_selected(&mut self) {
        if let (Some(idx), Some(draft)) = (self.selected_source, &self.draft)
            && let Some(slot) = self.sources.get_mut(idx)
        {
            *slot = draft.to_source();
        }
    }

    fn calculate(&mut self, peaks: &[PeakRow]) {
        self.sync_draft_to_selected();
        self.energies.clear();
        self.efficiencies.clear();
        self.fit_params.clear();
        self.fit_uncerts.clear();

        for row in peaks {
            if !self.selected_peaks.contains(&row.tag) {
                continue;
            }
            let Some(src) = self
                .sources
                .iter()
                .find(|s| s.nuclide == row.nuclide && (s.spectrum == "None" || s.spectrum == row.spectrum))
            else {
                continue;
            };
            match compute_point_efficiency(
                src,
                self.detector_area_cm2,
                self.detector_area_unc_cm2,
                row.counts,
                row.counts_uncert,
                row.yield_frac,
                row.yield_uncert,
            ) {
                Ok((eff, unc)) if eff.is_finite() && eff > 0.0 => {
                    self.energies.push(row.energy_kev);
                    self.efficiencies.push((eff, unc.max(1e-12)));
                }
                Ok(_) => {}
                Err(e) => {
                    self.status_msg = format!("Skipped {}: {e}", row.roi_alias);
                }
            }
        }

        if self.efficiencies.len() < 4 {
            self.status_msg = format!(
                "Calculated {} points (need >= 4 points to fit exp_polynomial).",
                self.efficiencies.len()
            );
            self.rebuild_plot();
            return;
        }

        let ydata: Vec<f64> = self.efficiencies.iter().map(|(e, _)| *e).collect();
        let sigma: Vec<f64> = self.efficiencies.iter().map(|(_, s)| *s).collect();
        let eval_fn = lookup_efficiency(EfficiencySignature::ExpPoly);
        let model = |xs: &[f64], params: &[f64]| -> Vec<f64> {
            xs.iter().map(|&e| eval_fn(e, params).unwrap_or(0.0)).collect()
        };
        let sig_opt = if self.weighted_fit {
            Some(sigma.as_slice())
        } else {
            None
        };
        match leastsq(
            model,
            &self.energies,
            &ydata,
            &[1.0, -1.0, 0.0, 0.0],
            sig_opt,
            200,
            1e-8,
            false,
        ) {
            Ok(res) => {
                self.fit_params = res.parameters;
                self.fit_uncerts = res.uncertainties;
                self.status_msg = format!(
                    "Fitted {} points (chi2 = {:.4}).",
                    self.efficiencies.len(),
                    res.chisq
                );
            }
            Err(e) => {
                self.status_msg = format!("Efficiency fit failed: {e:?}");
            }
        }
        self.rebuild_plot();
    }

    fn rebuild_plot(&mut self) {
        self.plot.clear();
        if self.energies.is_empty() {
            return;
        }
        let ys: Vec<f64> = self.efficiencies.iter().map(|(y, _)| *y).collect();
        let max_e = self.energies.iter().copied().fold(100.0_f64, f64::max);

        for (&x, &y) in self.energies.iter().zip(ys.iter()) {
            let mut m = Marker::point(x, y);
            m.color = Color32::from_rgb(80, 200, 255);
            self.plot.add_marker_data(&m);
        }

        if self.fit_params.len() == 4 {
            let eval_fn = lookup_efficiency(EfficiencySignature::ExpPoly);
            let n_curve = 250usize;
            let e_hi = max_e + 500.0;
            let xs: Vec<f64> = (0..n_curve)
                .map(|i| 25.0 + (i as f64) * (e_hi - 25.0) / ((n_curve - 1) as f64))
                .collect();
            let fy: Vec<f64> = xs
                .iter()
                .map(|&e| eval_fn(e, &self.fit_params).unwrap_or(0.0))
                .collect();
            self.plot
                .add_curve(&xs, &fy, Color32::from_rgb(255, 200, 60));
        }
        self.plot.set_auto_reset_zoom(true);
    }

    fn assign_to_instruments(
        &mut self,
        spectrum_manager: &mut SpectrumManager,
        include_all_of_model: bool,
    ) {
        if self.fit_params.len() != 4 {
            self.status_msg = "Calculate a valid 4-parameter fit first.".into();
            return;
        }
        let params: Vec<(f64, f64)> = self
            .fit_params
            .iter()
            .copied()
            .zip(self.fit_uncerts.iter().copied().chain(std::iter::repeat(0.0)))
            .collect();

        let target_key = self.selected_instrument.clone();
        let mut count = 0usize;
        if let Some(instr) = spectrum_manager
            .unique_instrument_library
            .get_instrument_by_name(&target_key)
        {
            let model = instr.model().to_string();
            let keys: Vec<String> = if include_all_of_model {
                spectrum_manager
                    .unique_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .unique_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect()
            } else {
                vec![target_key]
            };
            for k in &keys {
                if spectrum_manager
                    .unique_instrument_library
                    .set_efficiency(k, &params, &self.energies, &self.efficiencies)
                    .is_ok()
                {
                    count += 1;
                }
            }
            if include_all_of_model {
                let g_keys: Vec<String> = spectrum_manager
                    .generic_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .generic_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect();
                for k in &g_keys {
                    if spectrum_manager
                        .generic_instrument_library
                        .set_efficiency(k, &params, &self.energies, &self.efficiencies)
                        .is_ok()
                    {
                        count += 1;
                    }
                }
            }
        } else if let Some(instr) = spectrum_manager
            .generic_instrument_library
            .get_instrument_by_name(&target_key)
        {
            let model = instr.model().to_string();
            let keys: Vec<String> = if include_all_of_model {
                spectrum_manager
                    .generic_instrument_library
                    .get_instrument_names()
                    .into_iter()
                    .filter(|k| {
                        spectrum_manager
                            .generic_instrument_library
                            .get_instrument_by_name(k)
                            .is_some_and(|i| i.model() == model)
                    })
                    .collect()
            } else {
                vec![target_key]
            };
            for k in &keys {
                if spectrum_manager
                    .generic_instrument_library
                    .set_efficiency(k, &params, &self.energies, &self.efficiencies)
                    .is_ok()
                {
                    count += 1;
                }
            }
        }
        self.status_msg = format!("Assigned efficiency curve to {count} instrument(s).");
    }

    pub fn render(&mut self, ui: &egui::Ui, spectrum_manager: &mut SpectrumManager) {
        if !self.open {
            return;
        }

        let peaks = self.collect_peaks(spectrum_manager);
        let nuclides = spectrum_manager.nuclide_library.sorted_nuclide_names(false);
        let spectra: Vec<String> = std::iter::once("None".to_string())
            .chain(spectrum_manager.spectrum_registry.keys().cloned())
            .collect();
        let mut instruments = spectrum_manager
            .unique_instrument_library
            .get_instrument_names();
        instruments.extend(
            spectrum_manager
                .generic_instrument_library
                .get_instrument_names(),
        );
        if self.selected_instrument.is_empty()
            && let Some(first) = instruments.first()
        {
            self.selected_instrument = first.clone();
        }

        if let Some(Some(path)) = self.import_slot.take_ready() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    serde_json::from_str::<Vec<EfficiencySource>>(&s).map_err(|e| e.to_string())
                }) {
                Ok(loaded) if !loaded.is_empty() => {
                    self.sources = loaded;
                    self.selected_source = Some(0);
                    self.draft = self.sources.first().map(SourceUiDraft::from_source);
                    self.status_msg = format!("Loaded sources from {}", path.display());
                }
                Ok(_) => self.status_msg = "JSON contained no sources.".into(),
                Err(e) => self.status_msg = format!("Load failed: {e}"),
            }
        }
        if let Some(Some(path)) = self.export_slot.take_ready() {
            self.sync_draft_to_selected();
            match serde_json::to_string_pretty(&self.sources)
                .map_err(|e| e.to_string())
                .and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string()))
            {
                Ok(()) => self.status_msg = format!("Saved {}", path.display()),
                Err(e) => self.status_msg = format!("Save failed: {e}"),
            }
        }

        let mut close_requested = false;
        let mut do_calc = false;
        let mut do_assign_one = false;
        let mut do_assign_model = false;

        let viewport_id = egui::ViewportId::from_hash_of("efficiency_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Efficiency Window")
            .with_inner_size([1020.0, 640.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("Peaks:");
                egui::ScrollArea::vertical()
                    .id_salt("eff_peaks_scroll")
                    .max_height(130.0)
                    .show(ui, |ui| {
                        egui::Grid::new("eff_peaks_grid")
                            .striped(true)
                            .num_columns(8)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                for h in [
                                    "", "Source", "ROI", "Spectrum", "Counts", "Yield", "Energy",
                                    "Nuclide",
                                ] {
                                    ui.strong(h);
                                }
                                ui.end_row();
                                for row in &peaks {
                                    let mut checked = self.selected_peaks.contains(&row.tag);
                                    if ui.checkbox(&mut checked, "").changed() {
                                        if checked {
                                            self.selected_peaks.insert(row.tag.clone());
                                        } else {
                                            self.selected_peaks.remove(&row.tag);
                                        }
                                    }
                                    ui.label(&row.source_name);
                                    ui.label(&row.roi_alias);
                                    ui.label(&row.spectrum);
                                    ui.label(format!("{:.0}", row.counts));
                                    ui.label(format!("{:.2}%", row.yield_frac * 100.0));
                                    ui.label(format!("{:.2} keV", row.energy_kev));
                                    ui.label(&row.nuclide);
                                    ui.end_row();
                                }
                            });
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Add Source").clicked() {
                        self.sync_draft_to_selected();
                        let n = self.sources.len() + 1;
                        let src = EfficiencySource {
                            name: format!("Source {n}"),
                            ..EfficiencySource::default()
                        };
                        self.sources.push(src.clone());
                        self.selected_source = Some(self.sources.len() - 1);
                        self.draft = Some(SourceUiDraft::from_source(&src));
                    }
                    if ui.button("Remove Source").clicked()
                        && let Some(idx) = self.selected_source
                        && self.sources.len() > 1
                    {
                        self.sources.remove(idx);
                        let next = idx.min(self.sources.len() - 1);
                        self.selected_source = Some(next);
                        self.draft = self.sources.get(next).map(SourceUiDraft::from_source);
                    }
                    if ui.button("From JSON").clicked() {
                        let dlg = rfd::FileDialog::new().add_filter("JSON", &["json"]);
                        self.import_slot.spawn(ui.ctx(), move || dlg.pick_file());
                    }
                    if ui.button("To JSON").clicked() {
                        let dlg = rfd::FileDialog::new()
                            .add_filter("JSON", &["json"])
                            .set_file_name("sources.json");
                        self.export_slot.spawn(ui.ctx(), move || dlg.save_file());
                    }
                });

                ui.columns(2, |cols| {
                    cols[0].group(|ui| {
                        ui.label("Sources:");
                        let mut clicked_idx = None;
                        for (i, src) in self.sources.iter().enumerate() {
                            if ui
                                .selectable_label(self.selected_source == Some(i), &src.name)
                                .clicked()
                            {
                                clicked_idx = Some(i);
                            }
                        }
                        if let Some(i) = clicked_idx {
                            self.sync_draft_to_selected();
                            self.selected_source = Some(i);
                            self.draft = self.sources.get(i).map(SourceUiDraft::from_source);
                        }
                    });

                    cols[1].group(|ui| {
                        if let Some(d) = &mut self.draft {
                            egui::Grid::new("eff_source_form")
                                .num_columns(2)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    ui.label("Nuclide:");
                                    let prev_nuc = d.nuclide.clone();
                                    egui::ComboBox::from_id_salt("eff_src_nuc")
                                        .selected_text(&d.nuclide)
                                        .show_ui(ui, |ui| {
                                            for n in &nuclides {
                                                ui.selectable_value(&mut d.nuclide, n.clone(), n);
                                            }
                                        });
                                    if d.nuclide != prev_nuc
                                        && let Some(nuc) =
                                            spectrum_manager.nuclide_library.get_nuclide_by_name(&d.nuclide)
                                    {
                                        let hl = nuc.half_life_s.value.mean();
                                        if hl.is_finite() && hl > 0.0 {
                                            let (v, u) = format_duration_unit(hl);
                                            d.half_life_val = v;
                                            d.half_life_unit = u;
                                        }
                                    }
                                    ui.end_row();

                                    ui.label("Spectrum:");
                                    egui::ComboBox::from_id_salt("eff_src_spec")
                                        .selected_text(&d.spectrum)
                                        .show_ui(ui, |ui| {
                                            for s in &spectra {
                                                ui.selectable_value(&mut d.spectrum, s.clone(), s);
                                            }
                                        });
                                    ui.end_row();

                                    ui.label("Half-life:");
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut d.half_life_val).speed(0.1));
                                        egui::ComboBox::from_id_salt("eff_hl_u")
                                            .width(55.0)
                                            .selected_text(d.half_life_unit.label())
                                            .show_ui(ui, |ui| {
                                                for u in TimeUnit::ALL {
                                                    ui.selectable_value(
                                                        &mut d.half_life_unit,
                                                        u,
                                                        u.label(),
                                                    );
                                                }
                                            });
                                    });
                                    ui.end_row();

                                    ui.label("Ref Activity:");
                                    ui.horizontal(|ui| {
                                        ui.add(egui::DragValue::new(&mut d.activity_val).speed(1.0));
                                        ui.label("±");
                                        ui.add(
                                            egui::DragValue::new(&mut d.activity_uncert).speed(0.5),
                                        );
                                        egui::ComboBox::from_id_salt("eff_act_u")
                                            .width(55.0)
                                            .selected_text(d.activity_unit.label())
                                            .show_ui(ui, |ui| {
                                                for u in ActivityUnit::ALL {
                                                    ui.selectable_value(
                                                        &mut d.activity_unit,
                                                        u,
                                                        u.label(),
                                                    );
                                                }
                                            });
                                    });
                                    ui.end_row();

                                    ui.label("Distance [cm]:");
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::DragValue::new(&mut d.distance_cm)
                                                .range(0.01..=1e5)
                                                .speed(0.1),
                                        );
                                        ui.label("±");
                                        ui.add(
                                            egui::DragValue::new(&mut d.distance_uncert_cm)
                                                .range(0.0..=1e4)
                                                .speed(0.05),
                                        );
                                    });
                                    ui.end_row();

                                    ui.label("Options:");
                                    ui.horizontal(|ui| {
                                        ui.checkbox(
                                            &mut d.measurement_decay_compensate,
                                            "Decay Compensate",
                                        );
                                        ui.checkbox(&mut d.manual_solid_angle, "Manual Ω");
                                    });
                                    ui.end_row();
                                });
                        }
                    });
                });

                ui.horizontal(|ui| {
                    ui.label("Instrument:");
                    egui::ComboBox::from_id_salt("eff_instr_combo")
                        .selected_text(&self.selected_instrument)
                        .show_ui(ui, |ui| {
                            for k in &instruments {
                                ui.selectable_value(&mut self.selected_instrument, k.clone(), k);
                            }
                        });
                    ui.label("Detector Area [cm²]:");
                    ui.add(
                        egui::DragValue::new(&mut self.detector_area_cm2)
                            .range(0.01..=1e5)
                            .speed(0.1),
                    );
                    ui.label("±");
                    ui.add(
                        egui::DragValue::new(&mut self.detector_area_unc_cm2)
                            .range(0.0..=1e4)
                            .speed(0.01),
                    );
                });

                ui.horizontal(|ui| {
                    if ui.button("Calculate").clicked() {
                        do_calc = true;
                    }
                    ui.checkbox(&mut self.weighted_fit, "Weighted Fit");
                    if ui.button("Assign to Instrument").clicked() {
                        do_assign_one = true;
                    }
                    if ui.button("Assign to All of Model").clicked() {
                        do_assign_model = true;
                    }
                    if !self.status_msg.is_empty() {
                        ui.label(&self.status_msg);
                    }
                });

                ui.allocate_ui(egui::vec2(ui.available_width(), 200.0), |ui| {
                    self.plot.show(ui);
                });
                });
            });

        if do_calc {
            self.calculate(&peaks);
        }
        if do_assign_one {
            self.assign_to_instruments(spectrum_manager, false);
        }
        if do_assign_model {
            self.assign_to_instruments(spectrum_manager, true);
        }
        self.open = !close_requested;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decay_and_efficiency_math() {
        let (a_half, _) = decay_corrected_activity(1000.0, 50.0, 100.0, 100.0);
        assert!((a_half - 500.0).abs() < 1e-6);

        let (decays, _) = number_of_decays(1000.0, 10.0, 100.0, 100.0);
        let expected = 1000.0 * (100.0 / LN_2) * 0.5;
        assert!((decays - expected).abs() < 1e-6);

        let src = EfficiencySource {
            half_life_s: 1e12,
            ref_activity_bq: 1000.0,
            ref_activity_uncert_bq: 10.0,
            distance_cm: 10.0,
            distance_uncert_cm: 0.0,
            measurement_decay_compensate: false,
            ..EfficiencySource::default()
        };
        let (eff, unc) =
            compute_point_efficiency(&src, 4.0 * PI * 100.0, 0.0, 50.0, 5.0, 1.0, 0.0).unwrap();
        assert!(eff > 0.0 && unc > 0.0);
    }

    #[test]
    fn test_source_json_roundtrip() {
        let src = EfficiencySource::default();
        let json = serde_json::to_string(&vec![src.clone()]).unwrap();
        let back: Vec<EfficiencySource> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, src.name);
        assert_eq!(back[0].nuclide, src.nuclide);
    }
}
