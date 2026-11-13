//! Manager for spectrum ROIs — owner of ROI settings, coordinator of
//! fits, and source of per-spectrum result data.
//!
//! Replaces `ROIManager` (Qt version). Qt signals → `RoiManagerEvent` queue
//! drained each frame. `spectrum_manager` dependency replaced by
//! `&mut HashMap<String, Spectrum>` parameters, so this module has no
//! circular dependency on the UI / spectrum-manager layer.
//!
//! Deviations from Python (documented inline too):
//! - `update_roi` marks **all** members of a group as evaluated after one
//!   fit, not just the triggering ROI. Python re-runs the group once per
//!   member (N× redundant work). Same results, less work.
//! - `selected_roi: Option<String>` replaces `select_roi`'s brush-color
//!   manipulation. The GUI reads it to highlight the selected ROI.
//! - `Roi.movable` is added to the model (was previously view-only).
//! - A `RoiSettingsPatch` replaces the `CoreSpectrumROI(**kwargs)` pattern.
//! - Errors are silently swallowed where Python would `KeyError` / no-op;
//!   see the methods for specifics.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use eframe::egui::Color32;
use serde_json::{Map, Value};

use crate::containers::{BkgType, FitType, Roi};
use crate::containers::{Emission, Fit};
use crate::containers::{Spectrum, SpectrumData};
use crate::utils::fit_gaussians;

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum RoiManagerEvent {
    /// A new ROI was added to the registry.
    Created { tag: String },
    /// An ROI's computed data was updated for a specific spectrum.
    Updated {
        tag: String,
        spectrum: String,
        data: Roi,
    },
    /// An ROI was removed from the registry.
    Deleted { tag: String },
    /// "Show spectrum as CPS" toggle changed.
    CpsChanged(bool),
    /// "Subtract background" toggle changed.
    BkgSubChanged(bool),
}

// ---------------------------------------------------------------------------
// Settings patch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct RoiSettingsPatch {
    pub alias: Option<String>,
    pub fit_type: Option<FitType>,
    pub bkg_type: Option<BkgType>,
    pub bkg_est_channels: Option<usize>,
    pub merge: Option<bool>,
    pub movable: Option<bool>,
    pub poisson_weights: Option<bool>,
}

// ---------------------------------------------------------------------------
// New ROI specification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewRoi {
    pub x_low: f64,
    pub x_high: f64,
    pub alias: Option<String>,
    pub owner_spectrum: Option<String>,
    pub fit_type: FitType,
    pub bkg_type: BkgType,
    pub bkg_est_channels: usize,
    pub merge: bool,
    pub movable: bool,
    pub poisson_weights: bool,
    pub emission: Option<Emission>,
    pub meta: Map<String, Value>,
}

impl NewRoi {
    pub fn new(x_low: f64, x_high: f64) -> Self {
        Self {
            x_low,
            x_high,
            alias: None,
            owner_spectrum: None,
            fit_type: FitType::Gaussian,
            bkg_type: BkgType::Linear,
            bkg_est_channels: 3,
            merge: true,
            movable: true,
            poisson_weights: false,
            emission: None,
            meta: Map::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct RoiManager {
    /// Authoritative ROI settings per tag. `fit` is always `None` here —
    /// resolved fits live on each `Spectrum.rois`.
    roi_registry: HashMap<String, Roi>,
    roi_counter: usize,

    spectrum_is_cps: bool,
    spectrum_is_bkg_sub: bool,

    roi_groupings: Vec<HashSet<String>>,
    selected_roi: Option<String>,

    events: VecDeque<RoiManagerEvent>,
}

impl Default for RoiManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RoiManager {
    pub fn new() -> Self {
        Self {
            roi_registry: HashMap::new(),
            roi_counter: 0,
            spectrum_is_cps: false,
            spectrum_is_bkg_sub: false,
            roi_groupings: Vec::new(),
            selected_roi: None,
            events: VecDeque::new(),
        }
    }

    // ---- Flags / accessors ---------------------------------------------

    pub fn set_cps(&mut self, on: bool) {
        self.spectrum_is_cps = on;
        self.emit(RoiManagerEvent::CpsChanged(on));
    }

    pub fn set_bkg_sub(&mut self, on: bool) {
        self.spectrum_is_bkg_sub = on;
        self.emit(RoiManagerEvent::BkgSubChanged(on));
    }

    pub fn spectrum_is_cps(&self) -> bool {
        self.spectrum_is_cps
    }
    pub fn spectrum_is_bkg_sub(&self) -> bool {
        self.spectrum_is_bkg_sub
    }

    pub fn registry(&self) -> &HashMap<String, Roi> {
        &self.roi_registry
    }
    pub fn get_roi(&self, tag: &str) -> Option<&Roi> {
        self.roi_registry.get(tag)
    }
    pub fn groupings(&self) -> &[HashSet<String>] {
        &self.roi_groupings
    }
    pub fn selected_roi(&self) -> Option<&str> {
        self.selected_roi.as_deref()
    }

    pub fn select_roi(&mut self, tag: &str) {
        self.selected_roi = Some(tag.to_string());
    }

    pub fn clear_selection(&mut self) {
        self.selected_roi = None;
    }

    pub fn rename_owner_spectrum(&mut self, old: &str, new: &str) {
        for r in self.roi_registry.values_mut() {
            if r.owner_spectrum.as_deref() == Some(old) {
                r.owner_spectrum = Some(new.to_string());
            }
        }
    }

    /// Color for an ROI's plot region, derived from grouping size.
    /// (`set_brushes` in Python — computed lazily here.)
    pub fn get_roi_color(&self, tag: &str) -> Color32 {
        for group in &self.roi_groupings {
            if group.contains(tag) {
                return if group.len() == 1 {
                    Color32::from_rgba_premultiplied(0, 0, 255, 20)
                } else {
                    Color32::from_rgba_premultiplied(0, 255, 0, 20)
                };
            }
        }
        Color32::TRANSPARENT
    }

    // ---- Event queue ---------------------------------------------------

    pub fn drain_events(&mut self) -> Vec<RoiManagerEvent> {
        self.events.drain(..).collect()
    }

    fn emit(&mut self, ev: RoiManagerEvent) {
        self.events.push_back(ev);
    }

    // ---- CRUD ----------------------------------------------------------

    pub fn add_roi(&mut self, spec: NewRoi) -> String {
        let tag = format!("ROI_{}", self.roi_counter);
        self.roi_counter += 1;

        let alias = spec.alias.unwrap_or_else(|| tag.clone());
        let roi = Roi {
            tag: tag.clone(),
            alias,
            spectrum: String::new(),
            roi_bound: (spec.x_low, spec.x_high),
            region_bound: (0.0, 0.0),
            fit_type: spec.fit_type,
            bkg_type: spec.bkg_type,
            bkg_est_channels: spec.bkg_est_channels as i32,
            fit: None,
            roi_counts: 0.0,
            live_time: 0.0,
            emission: spec.emission,
            meta: spec.meta,
            merge: spec.merge,
            poisson_weights: spec.poisson_weights,
            owner_spectrum: spec.owner_spectrum,
            movable: spec.movable,
        };
        self.roi_registry.insert(tag.clone(), roi);
        self.emit(RoiManagerEvent::Created { tag: tag.clone() });
        tag
    }

    pub fn remove_roi(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        tag: &str,
        update_state: bool,
        optimizer_use_chi2_weight: bool,
    ) {
        if self.roi_registry.remove(tag).is_none() {
            return;
        }

        // Clear the resolved ROI from every spectrum.
        for spec in spectra.values_mut() {
            spec.remove_roi(tag);
        }

        self.emit(RoiManagerEvent::Deleted {
            tag: tag.to_string(),
        });
        self.roi_groupings = self.calculate_roi_groups();

        if !update_state {
            return;
        }

        // Re-eval the groups touched by the removal. Python tracked which
        // tags were "updated"; we just dedupe via a set.
        let mut updated: HashSet<String> = HashSet::new();
        for group in self.roi_groupings.clone() {
            for t in &group {
                if updated.insert(t.clone()) {
                    self.on_roi_change(spectra, t, optimizer_use_chi2_weight, true);
                }
            }
        }
    }

    pub fn clear_all(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        optimizer_use_chi2_weight: bool,
    ) {
        let tags: Vec<String> = self.roi_registry.keys().cloned().collect();
        for tag in tags {
            self.remove_roi(spectra, &tag, false, optimizer_use_chi2_weight);
        }
    }

    // ---- Settings / bounds updates -------------------------------------

    pub fn update_roi_bounds(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        tag: &str,
        bounds: (f64, f64),
        optimizer_use_chi2_weight: bool,
    ) {
        if let Some(r) = self.roi_registry.get_mut(tag) {
            r.roi_bound = bounds;
        }
        self.on_roi_change(spectra, tag, optimizer_use_chi2_weight, true);
    }

    /// Apply a settings patch and propagate shared settings across the
    /// group. Re-evaluates the group afterwards.
    pub fn update_roi_settings(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        tag: &str,
        patch: RoiSettingsPatch,
        optimizer_use_chi2_weight: bool,
    ) {
        // Apply to the source ROI
        let Some(source) = self.roi_registry.get_mut(tag) else {
            return;
        };
        if let Some(v) = patch.alias {
            source.alias = v;
        }
        if let Some(v) = patch.fit_type {
            source.fit_type = v;
        }
        if let Some(v) = patch.bkg_type {
            source.bkg_type = v;
        }
        if let Some(v) = patch.bkg_est_channels {
            source.bkg_est_channels = v as i32;
        }
        if let Some(v) = patch.merge {
            source.merge = v;
        }
        if let Some(v) = patch.movable {
            source.movable = v;
        }
        if let Some(v) = patch.poisson_weights {
            source.poisson_weights = v;
        }

        // Find the group (may be the singleton).
        let group = self
            .roi_groupings
            .iter()
            .find(|g| g.contains(tag))
            .cloned()
            .unwrap_or_else(|| HashSet::from([tag.to_string()]));

        // Propagate the shared fields to the rest of the group.
        // (`fit_type`, `bkg_type`, `poisson_weights`, `bkg_est_channels`.)
        let snapshot = self.roi_registry.get(tag).map(|r| {
            (
                r.fit_type,
                r.bkg_type,
                r.poisson_weights,
                r.bkg_est_channels,
            )
        });
        if let Some((ft, bt, pw, bkg)) = snapshot {
            for t in &group {
                if t == tag {
                    continue;
                }
                if let Some(r) = self.roi_registry.get_mut(t) {
                    r.fit_type = ft;
                    r.bkg_type = bt;
                    r.poisson_weights = pw;
                    r.bkg_est_channels = bkg;
                }
            }
        }

        // Re-eval the whole group.
        for t in &group {
            self.update_roi(spectra, None, Some(t), optimizer_use_chi2_weight);
        }
    }

    pub fn set_roi_emission(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        tag: &str,
        emission: Option<Emission>,
        optimizer_use_chi2_weight: bool,
    ) {
        if let Some(r) = self.roi_registry.get_mut(tag) {
            r.emission = emission;
        }
        self.on_roi_change(spectra, tag, optimizer_use_chi2_weight, true);
    }

    // ---- Getters over spectra ------------------------------------------

    /// All resolved ROI data for `tag`, keyed by the spectrum that owns it.
    pub fn get_data_from_roi<'a>(
        &self,
        spectra: &'a HashMap<String, Spectrum>,
        tag: &str,
    ) -> Vec<(&'a String, &'a Roi)> {
        spectra
            .iter()
            .filter_map(|(name, spec)| spec.rois.get(tag).map(|roi| (name, roi)))
            .collect()
    }

    /// All resolved ROI data for one spectrum, keyed by tag.
    pub fn get_data_from_spectrum<'a>(
        &self,
        spectra: &'a HashMap<String, Spectrum>,
        spectrum_name: &str,
    ) -> Option<&'a HashMap<String, Roi>> {
        spectra.get(spectrum_name).map(|s| &s.rois)
    }

    // ---- Grouping ------------------------------------------------------

    pub fn calculate_roi_groups(&self) -> Vec<HashSet<String>> {
        // Group by owner_spectrum. BTreeMap gives deterministic order
        // (None first, then by name) — HashMap iteration order is unstable
        // and Python's implicit dict order has no analogue here.
        let mut by_owner: BTreeMap<Option<String>, (Vec<(String, f64, f64)>, Vec<String>)> =
            BTreeMap::new();

        for r in self.roi_registry.values() {
            let entry = by_owner.entry(r.owner_spectrum.clone()).or_default();
            if r.merge && r.fit_type != FitType::None {
                entry.0.push((r.tag.clone(), r.roi_bound.0, r.roi_bound.1));
            } else {
                entry.1.push(r.tag.clone());
            }
        }

        let mut groups: Vec<HashSet<String>> = Vec::new();
        for (_, (mut mergeable, singles)) in by_owner {
            mergeable.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            let mut current: HashSet<String> = HashSet::new();
            let mut current_end: f64 = 0.0;
            let mut has_current = false;

            for (tag, low, high) in mergeable {
                if !has_current {
                    current.insert(tag);
                    current_end = high;
                    has_current = true;
                } else if low < current_end {
                    current.insert(tag);
                    current_end = current_end.max(high);
                } else {
                    groups.push(std::mem::take(&mut current));
                    current.insert(tag);
                    current_end = high;
                }
            }
            if !current.is_empty() {
                groups.push(current);
            }

            for tag in singles {
                groups.push(HashSet::from([tag]));
            }
        }
        groups
    }

    // ---- Fit coordination ----------------------------------------------

    pub fn on_roi_change(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        tag: &str,
        optimizer_use_chi2_weight: bool,
        update_all: bool,
    ) {
        let old_group = self
            .roi_groupings
            .iter()
            .find(|g| g.contains(tag))
            .cloned()
            .unwrap_or_else(|| HashSet::from([tag.to_string()]));

        self.roi_groupings = self.calculate_roi_groups();

        if update_all {
            for t in &old_group {
                self.update_roi(spectra, None, Some(t), optimizer_use_chi2_weight);
            }
        } else {
            self.update_roi(spectra, None, Some(tag), optimizer_use_chi2_weight);
        }
    }

    pub fn update_roi(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        spectrum_name: Option<&str>,
        roi_tag: Option<&str>,
        optimizer_use_chi2_weight: bool,
    ) {
        if self.roi_registry.is_empty() {
            return;
        }

        let names: Vec<String> = match spectrum_name {
            Some(n) => vec![n.to_string()],
            None => spectra.keys().cloned().collect(),
        };
        let tags: Vec<String> = match roi_tag {
            Some(t) => vec![t.to_string()],
            None => self.roi_registry.keys().cloned().collect(),
        };

        for name in names {
            let Some(spectrum) = spectra.get_mut(&name) else {
                continue;
            };
            let mut updated: HashSet<String> = HashSet::new();

            for tag in &tags {
                if updated.contains(tag) {
                    continue;
                }

                // Owner check.
                match self.roi_registry.get(tag) {
                    Some(r) => {
                        if let Some(o) = &r.owner_spectrum {
                            if o != &spectrum.name {
                                continue;
                            }
                        }
                    }
                    None => continue,
                }

                // Find and evaluate its group.
                let group = match self.roi_groupings.iter().find(|g| g.contains(tag)).cloned() {
                    Some(g) => g,
                    None => continue,
                };
                // FIX vs Python: mark the *whole* group as updated.
                updated.extend(group.iter().cloned());
                self.eval_roi(spectrum, &group, optimizer_use_chi2_weight);
            }
        }
    }

    // For refitting on live messurements, we want to refit the ROI bounds without triggering a full re-evaluation of the group.
    pub fn refit_roi(
        &mut self,
        spectra: &mut HashMap<String, Spectrum>,
        spectrum_name: Option<&str>,
        roi_tag: Option<&str>,
        optimizer_use_chi2_weight: bool,
    ) {
        if self.roi_registry.is_empty() {
            return;
        }

        let names: Vec<String> = match spectrum_name {
            Some(n) => vec![n.to_string()],
            None => spectra.keys().cloned().collect(),
        };
        let tags: Vec<String> = match roi_tag {
            Some(t) => vec![t.to_string()],
            None => self.roi_registry.keys().cloned().collect(),
        };

        for name in names {
            let Some(spectrum) = spectra.get_mut(&name) else {
                continue;
            };
            let mut updated: HashSet<String> = HashSet::new();

            for tag in &tags {
                if updated.contains(tag) {
                    continue;
                }

                // Owner check.
                match self.roi_registry.get(tag) {
                    Some(r) => {
                        if let Some(o) = &r.owner_spectrum {
                            if o != &spectrum.name {
                                continue;
                            }
                        }
                    }
                    None => continue,
                }

                // Find and evaluate its group.
                let group = match self.roi_groupings.iter().find(|g| g.contains(tag)).cloned() {
                    Some(g) => g,
                    None => continue,
                };
                // FIX vs Python: mark the *whole* group as updated.
                updated.extend(group.iter().cloned());
                self.eval_roi(spectrum, &group, optimizer_use_chi2_weight);
            }
        }
    }

    /// Set the ROI's bounds without triggering a refit. Used during a live
    /// drag; the caller is responsible for calling `on_roi_change` when the
    /// drag ends.
    pub fn set_roi_bound_raw(&mut self, tag: &str, bounds: (f64, f64)) {
        if let Some(r) = self.roi_registry.get_mut(tag) {
            // Enforce lower < upper so the group calculation doesn't see
            // inverted bounds mid-drag.
            let (lo, hi) = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
            r.roi_bound = (lo, hi);
        }
    }

    fn eval_roi(
        &mut self,
        spectrum: &mut Spectrum,
        roi_group: &HashSet<String>,
        optimizer_use_chi2_weight: bool,
    ) {
        let Some(fg_ref) = spectrum.foreground.as_ref() else {
            return;
        };
        let fg_live_time = effective_live_time(fg_ref);

        // Materialize group members; skip unknown tags (Python would KeyError).
        let roi_list: Vec<&Roi> = roi_group
            .iter()
            .filter_map(|t| self.roi_registry.get(t))
            .collect();
        if roi_list.is_empty() {
            return;
        }

        let bounds: Vec<(f64, f64)> = roi_list.iter().map(|r| r.roi_bound).collect();
        let group_min = bounds
            .iter()
            .map(|b| b.0.min(b.1))
            .fold(f64::INFINITY, f64::min);
        let group_max = bounds
            .iter()
            .map(|b| b.0.max(b.1))
            .fold(f64::NEG_INFINITY, f64::max);

        // Group-wide settings come from the first member (Python does the
        // same; propagation keeps them identical).
        let first = roi_list[0];
        let fit_type = first.fit_type;
        let bkg_type = first.bkg_type;
        let bkg_est_channels = first.bkg_est_channels.max(0) as usize;

        let mut meta_data = Map::new();
        meta_data.insert(
            "background_subtracted".into(),
            Value::Bool(self.spectrum_is_bkg_sub),
        );
        meta_data.insert(
            "chi2_weighted_err".into(),
            Value::Bool(optimizer_use_chi2_weight),
        );

        // Special case: bkg_sub requested but this spectrum has no bg.
        if spectrum.background.is_none() && self.spectrum_is_bkg_sub {
            let results: Vec<Roi> = roi_list
                .iter()
                .map(|r| {
                    build_result_roi(
                        r,
                        &spectrum.name,
                        group_min,
                        group_max,
                        fit_type,
                        bkg_type,
                        fg_live_time,
                        None,
                        0.0,
                        &meta_data,
                        true,
                    )
                })
                .collect();
            self.commit_results(spectrum, results);
            return;
        }

        // y_axis for counts (this is independent of fit_gaussians).
        let y_axis: Vec<f64> = if !self.spectrum_is_bkg_sub {
            spectrum
                .get_foreground(false, false)
                .map(|a| a.to_vec())
                .unwrap_or_default()
        } else {
            match spectrum.get_bkg_sub(false) {
                Some(arr) => arr.iter().map(|v| v * fg_live_time).collect(),
                None => return,
            }
        };
        let x_axis: Vec<f64> = spectrum.x_axis.to_vec();

        // Per-ROI counts.
        let per_counts: Vec<f64> = bounds
            .iter()
            .map(|&(lo, hi)| get_roi_counts(&y_axis, &x_axis, lo, hi))
            .collect();
        let total = get_roi_counts(&y_axis, &x_axis, group_min, group_max);

        // Fit only if there is enough signal and the user asked for one.
        let fits_opt: Option<Vec<Fit>> = if total > 32.0 && fit_type != FitType::None {
            let fg_y_axis = spectrum
                .get_foreground(false, false)
                .map(|a| a.to_vec())
                .unwrap_or_default();
            let fg_live = effective_live_time(spectrum.foreground.as_ref().unwrap());

            let (bg_y_axis, bg_live) = if self.spectrum_is_bkg_sub {
                let bg = spectrum.get_background(false, false).map(|a| a.to_vec());
                let bg_lt = spectrum.background.as_ref().map(effective_live_time);
                (bg, bg_lt)
            } else {
                (None, None)
            };

            match fit_gaussians(
                &spectrum.x_axis.to_vec(),
                &fg_y_axis,
                fg_live,
                bg_y_axis.as_deref(),
                bg_live,
                &bounds,
                fit_type,
                first.poisson_weights,
                optimizer_use_chi2_weight,
                bkg_type,
                bkg_est_channels,
            ) {
                Ok(outcome) if outcome.converged => Some(outcome.fits),
                Ok(_) => {
                    tracing::warn!("fit did not converge");
                    None
                }
                Err(e) => {
                    tracing::warn!("fit errored: {e}");
                    None
                }
            }
        } else {
            None
        };

        let results: Vec<Roi> = match fits_opt {
            Some(fit_list) if fit_list.len() == roi_list.len() => roi_list
                .iter()
                .zip(fit_list.iter())
                .zip(per_counts.iter())
                .map(|((r, f), &cnt)| {
                    build_result_roi(
                        r,
                        &spectrum.name,
                        group_min,
                        group_max,
                        fit_type,
                        bkg_type,
                        fg_live_time,
                        Some(f.clone()),
                        cnt,
                        &meta_data,
                        false,
                    )
                })
                .collect(),
            _ => roi_list
                .iter()
                .zip(per_counts.iter())
                .map(|(r, &cnt)| {
                    build_result_roi(
                        r,
                        &spectrum.name,
                        group_min,
                        group_max,
                        fit_type,
                        bkg_type,
                        fg_live_time,
                        None,
                        cnt,
                        &meta_data,
                        true,
                    )
                })
                .collect(),
        };

        self.commit_results(spectrum, results);
    }

    fn commit_results(&mut self, spectrum: &mut Spectrum, results: Vec<Roi>) {
        for roi in results {
            let tag = roi.tag.clone();
            let spectrum_name = spectrum.name.clone();
            spectrum.set_roi(roi.clone());
            self.emit(RoiManagerEvent::Updated {
                tag,
                spectrum: spectrum_name,
                data: roi,
            });
        }
    }

    pub fn set_owner_spectrum(&mut self, tag: &str, owner: Option<String>) -> bool {
        if let Some(r) = self.roi_registry.get_mut(tag) {
            r.owner_spectrum = owner;
            true
        } else {
            false
        }
    }

    pub fn load_rois(&mut self, rois: &[Roi]) {
        for roi in rois {
            self.roi_registry.insert(roi.tag.clone(), roi.clone());
            self.emit(RoiManagerEvent::Created {
                tag: roi.tag.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn effective_live_time(data: &SpectrumData) -> f64 {
    if data.live_time > 0.0 {
        data.live_time
    } else {
        data.real_time.unwrap_or(0.0)
    }
}

fn get_roi_counts(y_axis: &[f64], x_axis: &[f64], lower: f64, upper: f64) -> f64 {
    x_axis
        .iter()
        .zip(y_axis.iter())
        .filter(|(x, _)| **x > lower && **x < upper)
        .map(|(_, y)| *y)
        .sum()
}

#[allow(clippy::too_many_arguments)]
fn build_result_roi(
    source: &Roi,
    spectrum_name: &str,
    group_min: f64,
    group_max: f64,
    fit_type: FitType,
    bkg_type: BkgType,
    fg_live_time: f64,
    fit: Option<Fit>,
    roi_counts: f64,
    meta_extra: &Map<String, Value>,
    include_poisson_weights_in_meta: bool,
) -> Roi {
    let mut meta = Map::new();
    meta.insert("merge".into(), Value::Bool(source.merge));
    meta.insert("movable".into(), Value::Bool(source.movable));
    if include_poisson_weights_in_meta {
        meta.insert(
            "poisson_weights".into(),
            Value::Bool(source.poisson_weights),
        );
    }
    for (k, v) in meta_extra {
        meta.insert(k.clone(), v.clone());
    }

    Roi {
        tag: source.tag.clone(),
        alias: source.alias.clone(),
        spectrum: spectrum_name.to_string(),
        roi_bound: source.roi_bound,
        region_bound: (group_min, group_max),
        fit_type,
        bkg_type,
        bkg_est_channels: source.bkg_est_channels,
        fit,
        roi_counts,
        live_time: fg_live_time,
        emission: source.emission.clone(),
        meta,
        merge: source.merge,
        poisson_weights: source.poisson_weights,
        owner_spectrum: source.owner_spectrum.clone(),
        movable: source.movable,
    }
}
