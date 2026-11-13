//! Library of nuclide records with energy-to-nuclide matching and
//! multi-nuclide emission lookup.
//!
//! Direct port of `luracs.clients.nuclide_library.NuclideLibrary`.
//! Qt signals → `VecDeque<NuclideLibraryEvent>` (same pattern as
//! `Settings` / `InstrumentLibrary`).
//!
//! Deviations from Python:
//! - Library keys are `NuclideId` rather than the original JSON string.
//!   `get_nuclide_by_name` accepts any spelling (`"U-238"`, `"U238"`,
//!   `"U238m"`) via `NuclideId::from_name`.
//! - `match_roi_to_nuclide` takes a slice of ROIs instead of reaching into
//!   the spectrum manager, decoupling the library from the UI.
//! - `most_common(1)` on ties keeps the first-encountered match (Python's
//!   `Counter` behaviour), reproduced via a first-seen index.
//! - Emissions without a `parent_nuclide` or `energy_kev` are silently
//!   ineligible for matching (Python's frozen dataclass guarantees those
//!   fields are set; our `Option<Quantity>` allows placeholders).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use eframe::egui::Color32;
use nucleide_nuclei::NuclideId;
use thiserror::Error;

use crate::containers::{Emission, Nuclide};
use crate::containers::Roi;
use crate::utils::{NuclideLoadError, load_nuclide_data};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum NuclideLibraryError {
    #[error("failed to load nuclide data: {0}")]
    Load(#[from] NuclideLoadError),

    #[error("nuclide {0:?} is already present in the library")]
    DuplicateNuclide(String),

    #[error("invalid nuclide name {0:?}")]
    InvalidNuclideName(String),
}

pub type Result<T> = std::result::Result<T, NuclideLibraryError>;

// ---------------------------------------------------------------------------
// Events (replace Qt signals)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum NuclideLibraryEvent {
    /// Emitted from `track_selected_nuclide`. `color` is passed through
    /// unchanged so the plot can draw emission lines in the requested hue.
    ViewCheckChanged {
        nuclide: NuclideId,
        show: bool,
        color: Color32,
    },
}

// ---------------------------------------------------------------------------
// Match options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct MatchOptions {
    /// If true and the selection is non-empty, only match against selected
    /// nuclides. Falls through to "all nuclides" when nothing is selected.
    pub match_only_shown: bool,
    /// Energy search window in keV (must be > 0).
    pub window: f64,
    /// Weight each emission's effective energy by `intensity^0.7` before
    /// comparing (matches Python's heuristic).
    pub weight_by_intensity: bool,
    /// Reject emissions with intensity at or below this value. Default
    /// `-inf` keeps everything.
    pub filter_by_intensity_percent: f64,
}

impl Default for MatchOptions {
    fn default() -> Self {
        Self {
            match_only_shown: true,
            window: 50.0,
            weight_by_intensity: true,
            filter_by_intensity_percent: f64::NEG_INFINITY,
        }
    }
}

// ---------------------------------------------------------------------------
// Decay chains
// ---------------------------------------------------------------------------

/// Hard-coded decay chain lists (display strings, hyphenated).
const TH232_CHAIN: &[&str] = &[
    "Th-232", "Ra-228", "Ac-228", "Th-228", "Ra-224", "Rn-220", "Po-216", "Pb-212", "Bi-212",
    "Po-212", "Tl-208", "Pb-208",
];

const U238_CHAIN: &[&str] = &[
    "U-238", "Th-234", "Pa-234", "U-234", "Th-230", "Ra-226", "Rn-222", "Po-218", "Pb-214",
    "Bi-214", "Po-214", "Tl-210", "Pb-210", "Bi-210", "Po-210", "Tl-206", "Pb-206",
];

const DECAY_CHAINS: &[(&str, &[&str])] = &[
    ("Th-232 -- Chain", TH232_CHAIN),
    ("U-238 -- Chain", U238_CHAIN),
];

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

pub struct NuclideLibrary {
    nuclides: HashMap<NuclideId, Nuclide>,
    selected: HashSet<NuclideId>,
    events: VecDeque<NuclideLibraryEvent>,
}

impl NuclideLibrary {
    /// Load every `*.json` under `nuclide_data_dir`.
    ///
    /// Equivalent to `NuclideLibrary(spectrum_manager)` at the Python level,
    /// minus the manager dependency.
    pub fn load_from_dir(nuclide_data_dir: &Path) -> Result<Self> {
        let pattern = nuclide_data_dir.join("*.json");
        let pattern = pattern.to_string_lossy().to_string();
        let nuclides = load_nuclide_data(&pattern)?;

        let mut lib = Self {
            nuclides: HashMap::new(),
            selected: HashSet::new(),
            events: VecDeque::new(),
        };
        for n in nuclides {
            lib.insert_internal(n)?;
        }
        Ok(lib)
    }

    /// Create an empty library. Useful for tests and dynamic building.
    pub fn empty() -> Self {
        Self {
            nuclides: HashMap::new(),
            selected: HashSet::new(),
            events: VecDeque::new(),
        }
    }

    fn insert_internal(&mut self, nuclide: Nuclide) -> Result<()> {
        let id = nuclide.nuclide;
        if self.nuclides.contains_key(&id) {
            return Err(NuclideLibraryError::DuplicateNuclide(id.to_name()));
        }
        self.nuclides.insert(id, nuclide);
        Ok(())
    }

    /// Public insertion. Errors if `nuclide.nuclide` is already present.
    pub fn add_nuclide(&mut self, nuclide: Nuclide) -> Result<()> {
        self.insert_internal(nuclide)
    }

    // ---- Queries --------------------------------------------------------

    pub fn get_nuclide(&self, id: NuclideId) -> Option<&Nuclide> {
        self.nuclides.get(&id)
    }

    /// Look up by any accepted spelling (`"U-238"`, `"U238"`, `"Am242m"`, …).
    pub fn get_nuclide_by_name(&self, name: &str) -> Option<&Nuclide> {
        let id = NuclideId::from_name(name).ok()?;
        self.nuclides.get(&id)
    }

    /// Sort nuclide IDs by mass number. If `require_photon_emissions` is
    /// true, only nuclides with at least one emission are returned.
    pub fn sorted_nuclide_ids(&self, require_photon_emissions: bool) -> Vec<NuclideId> {
        let mut ids: Vec<NuclideId> = self
            .nuclides
            .iter()
            .filter(|(_, n)| !require_photon_emissions || !n.emissions.is_empty())
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by_key(|id| id.a());
        ids
    }

    /// Convenience wrapper that returns the display names (canonical form,
    /// e.g. `"U238"`, `"Tc99m"`).
    pub fn sorted_nuclide_names(&self, require_photon_emissions: bool) -> Vec<String> {
        self.sorted_nuclide_ids(require_photon_emissions)
            .into_iter()
            .map(|id| id.to_name())
            .collect()
    }

    pub fn all_emissions(&self) -> impl Iterator<Item = &Emission> {
        self.nuclides.values().flat_map(|n| n.emissions.iter())
    }

    pub fn selected(&self) -> &HashSet<NuclideId> {
        &self.selected
    }

    // ---- Decay chains ---------------------------------------------------

    pub fn decay_chains(&self) -> &'static [(&'static str, &'static [&'static str])] {
        DECAY_CHAINS
    }

    /// Resolve a chain name to `NuclideId`s, skipping entries that aren't in
    /// the loaded library.
    pub fn decay_chain_ids(&self, chain_name: &str) -> Option<Vec<NuclideId>> {
        let (_, names) = DECAY_CHAINS.iter().find(|(n, _)| *n == chain_name)?;
        Some(
            names
                .iter()
                .filter_map(|n| NuclideId::from_name(n).ok())
                .collect(),
        )
    }

    // ---- Energy matching ------------------------------------------------

    /// Return the single best matching emission within `opts.window` keV of
    /// `energy`, or `None` if there are no eligible candidates.
    ///
    /// A match within `isclose` tolerance of 511 keV is replaced by the
    /// canonical `Emission::annihilation()` value, matching the Python's
    /// `if isclose(energy, 511): return AnnihilationEmission`.
    pub fn match_energy_to_nuclide(&self, energy: f64) -> Option<Emission> {
        self.match_energy_with(energy, MatchOptions::default())
    }

    pub fn match_energy_with(&self, energy: f64, opts: MatchOptions) -> Option<Emission> {
        let candidates = self.collect_matching_emissions(energy, &opts);

        // Weighted energy per candidate (skips ones with missing intensity
        // when weight_by_intensity is true).
        let weighted: Vec<(usize, f64)> = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, e)| weighted_energy(e, opts.weight_by_intensity).map(|w| (i, w)))
            .collect();

        let (best_idx, _) = weighted
            .iter()
            .min_by(|(_, a), (_, b)| {
                let da = (a - energy).powi(2);
                let db = (b - energy).powi(2);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()?;

        let closest = candidates[best_idx].clone();

        let e_val = closest
            .energy_kev
            .as_ref()
            .map(|q| q.value.mean())
            .unwrap_or(f64::NAN);
        if (e_val - 511.0).abs() <= 1e-8 + 1e-5 * 511.0 {
            return Some(Emission::annihilation());
        }
        Some(closest)
    }

    fn collect_matching_emissions(&self, energy: f64, opts: &MatchOptions) -> Vec<&Emission> {
        if !opts.match_only_shown || self.selected.is_empty() {
            self.all_emissions()
                .filter(|e| is_eligible(e, energy, opts))
                .collect()
        } else {
            self.selected
                .iter()
                .filter_map(|id| self.nuclides.get(id))
                .flat_map(|n| n.emissions.iter())
                .filter(|e| is_eligible(e, energy, opts))
                .collect()
        }
    }

    // ---- ROI matching ---------------------------------------------------

    /// Match a group of ROIs to the most common emission across their fit
    /// centroids. Callers supply the ROIs (rather than the library reaching
    /// into a `ROIManager`), which decouples this from the UI layer.
    pub fn match_roi_to_nuclide<'a, I>(
        &self,
        rois: I,
        energy_search_window: f64,
    ) -> Option<Emission>
    where
        I: IntoIterator<Item = &'a Roi>,
    {
        let opts = MatchOptions {
            window: energy_search_window,
            ..MatchOptions::default()
        };

        // Collect non-None matches, tracking first-seen index for the
        // Counter-style tie-break.
        let mut counts: HashMap<EmissionKey, (Emission, usize, usize)> = HashMap::new();
        let mut order = 0usize;
        for roi in rois {
            let Some(fit) = &roi.fit else { continue };
            let mu = fit.mu();
            if let Some(e) = self.match_energy_with(mu, opts) {
                let key = EmissionKey::from(&e);
                counts
                    .entry(key)
                    .and_modify(|(_, c, _)| *c += 1)
                    .or_insert((e, 1, order));
            }
            order += 1;
        }

        // Python: `most_common(1)` → first item on tie.
        counts
            .into_values()
            .max_by(|(_, ac, ai), (_, bc, bi)| ac.cmp(bc).then_with(|| bi.cmp(ai)))
            .map(|(e, _, _)| e)
    }

    // ---- Selection tracking --------------------------------------------

    /// Add or remove a nuclide from the visible set and emit
    /// `ViewCheckChanged`. `color` is forwarded to the event without being
    /// stored — the receiver decides how to use it.
    pub fn track_selected_nuclide(&mut self, nuclide: NuclideId, show: bool, color: Color32) {
        if show {
            self.selected.insert(nuclide);
        } else {
            self.selected.remove(&nuclide);
        }
        self.events
            .push_back(NuclideLibraryEvent::ViewCheckChanged {
                nuclide,
                show,
                color,
            });
    }

    pub fn drain_events(&mut self) -> Vec<NuclideLibraryEvent> {
        self.events.drain(..).collect()
    }
}

// ---------------------------------------------------------------------------
// Matching helpers
// ---------------------------------------------------------------------------

fn is_eligible(e: &Emission, energy: f64, opts: &MatchOptions) -> bool {
    let Some(e_val) = e.energy_kev.as_ref().map(|q| q.value.mean()) else {
        return false;
    };
    if (e_val - energy).abs() >= opts.window {
        return false;
    }
    // Python's `Emission` treats missing intensity as 0.0 by dataclass default.
    let intensity = e
        .intensity_percent
        .as_ref()
        .map(|q| q.value.mean())
        .unwrap_or(0.0);
    intensity > opts.filter_by_intensity_percent
}

fn weighted_energy(e: &Emission, weight_by_intensity: bool) -> Option<f64> {
    let e_val = e.energy_kev.as_ref()?.value.mean();
    if !weight_by_intensity {
        return Some(e_val);
    }
    // Without an intensity value the weighting is undefined — reject.
    let intensity = e.intensity_percent.as_ref()?.value.mean();
    Some(e_val * (intensity * 0.01).powf(0.7))
}

// ---------------------------------------------------------------------------
// Emission hash key
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct EmissionKey {
    parent: Option<NuclideId>,
    energy_micro: i64,
    intensity_micro: i64,
}

impl From<&Emission> for EmissionKey {
    fn from(e: &Emission) -> Self {
        let round_micro = |q: Option<&physure_core::quantity::Quantity>| {
            q.map(|v| (v.value.mean() * 1e6).round() as i64)
                .unwrap_or(i64::MIN)
        };
        Self {
            parent: e.parent_nuclide,
            energy_micro: round_micro(e.energy_kev.as_ref()),
            intensity_micro: round_micro(e.intensity_percent.as_ref()),
        }
    }
}
