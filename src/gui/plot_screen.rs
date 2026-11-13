//! Container that switches between the single combined plot and a tabbed
//! view with one plot per spectrum. Port of
//! `luracs.gui.spectrum_plot_container.SpectrumPlotContainer`.
//!
//! All actual rendering is in `crate::gui::spectrum_plot::render`, which
//! uses **rsplot 0.6.1** via `rsplot::Plot1D`. This file only manages:
//!
//! - which mode is active (Combined vs Tabbed)
//! - the collection of `SpectrumPlotState`s (one per spectrum in tabbed mode)
//! - reconciliation against the current `SpectrumManager::spectrum_registry`
//! - nuclide → ROI matching
//! - feature-line broadcast
//!
//! ## Structural notes
//!
//! - **`render` calls `sync_spectra` internally.** It needs the wgpu
//!   `RenderState` to construct fresh tab plots anyway, so threading it
//!   through the render signature is more compact than exposing `sync_spectra`
//!   as a separate step the caller has to remember.
//! - **Feature lines are persistent.** The peak-features tool can fire
//!   `set_feature_line` at any time; we store them on the container so tabs
//!   created *after* the emission still see the line.
//! - **Per-frame ROI cleanup.** `sync_spectra` removes ROIs whose
//!   `owner_spectrum` points at a spectrum that was just dropped. Python did
//!   this in `remove_tab`, but only on the `spectrumRemoved` signal — which
//!   silently left orphans on rename. This version handles both paths
//!   uniformly.
//!
//! ## Deliberate deviations
//!
//! - **`match_nuclide_to_rois` in combined mode** filters to *all* ROIs
//!   (Python's version dereferenced `tabs.currentWidget()` unconditionally,
//!   which would `AttributeError` if no tab was visible — the modal was only
//!   safely callable from tabbed mode). Matching all ROIs is the intuitive
//!   behaviour for the combined view.
//! - **Tab order is alphabetical.** Python used insertion order, which
//!   depended on the order of `spectrumCreated` signals. We sort names to
//!   keep the tab strip deterministic across sessions.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;

use crate::containers::Emission;
use crate::containers::Roi;
use crate::core::SpectrumManager;
use crate::gui::spectrum_plot::{self, FeatureLine, SpectrumPlotState};
use crate::settings::Settings;
use crate::theme_manager::Palette;

// ===========================================================================
// Mode
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlotMode {
    #[default]
    Combined,
    Tabbed,
}

// ===========================================================================
// Nuclide match (returned to the caller for the InfoModal)
// ===========================================================================

#[derive(Debug, Clone)]
pub struct NuclideMatch {
    pub alias: String,
    pub nuclide: String,
    pub energy_kev: f64,
    pub intensity_percent: f64,
}

impl NuclideMatch {
    pub fn display_line(&self) -> String {
        format!(
            "{} --> {} | [{:.1} keV - {:.2} %]",
            self.alias, self.nuclide, self.energy_kev, self.intensity_percent
        )
    }
}

/// Format the "Nuclides matched to ROIs" modal message body.
pub fn format_match_result(matches: &[NuclideMatch]) -> String {
    if matches.is_empty() {
        return "No matching nuclides were found".to_string();
    }
    let mut out = String::from("Nuclides matched to ROIs:\n\n");
    for m in matches {
        out.push_str(&m.display_line());
        out.push('\n');
    }
    out
}

// ===========================================================================
// SpectrumScreen
// ===========================================================================

pub struct SpectrumScreen {
    pub mode: PlotMode,

    /// Combined-mode plot (all spectra overlaid).
    pub combined: SpectrumPlotState,
    /// Tabbed-mode plots, one per spectrum.
    pub tabs: HashMap<String, SpectrumPlotState>,
    /// Stable tab order (sorted alphabetically for determinism).
    pub tab_order: Vec<String>,
    /// Which tab is currently visible in tabbed mode.
    pub current_tab: Option<String>,

    /// Persistent feature lines (peak-features gamma tool). Survives tab
    /// creation: any new `SpectrumPlotState` inherits these.
    feature_lines: HashMap<String, FeatureLine>,
}

impl SpectrumScreen {
    pub fn new(render_state: &RenderState) -> Self {
        Self {
            mode: PlotMode::Combined,
            combined: SpectrumPlotState::new(render_state),
            tabs: HashMap::new(),
            tab_order: Vec::new(),
            current_tab: None,
            feature_lines: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Mode
    // -----------------------------------------------------------------------

    /// Switch modes. Clears all ROIs (Python's `sigModeChanged → clear_all`)
    /// because ROI ownership is mode-specific: combined uses `None`, tabbed
    /// uses `Some(name)`.
    pub fn set_mode(&mut self, mode: PlotMode, spectrum_manager: &mut SpectrumManager) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;

        spectrum_manager
            .roi_manager
            .clear_all(&mut spectrum_manager.spectrum_registry, true);

        if mode == PlotMode::Combined {
            self.combined.request_reset();
        } else if let Some(name) = &self.current_tab {
            if let Some(state) = self.tabs.get_mut(name) {
                state.request_reset();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Reconciliation (called automatically from `render`)
    // -----------------------------------------------------------------------

    /// Sync `tabs` / `tab_order` / `current_tab` against the current
    /// spectrum registry. Safe to call every frame.
    pub fn sync_spectra(
        &mut self,
        spectrum_manager: &mut SpectrumManager,
        render_state: &RenderState,
    ) {
        let alive: HashSet<String> = spectrum_manager
            .get_spectra_dict()
            .keys()
            .cloned()
            .collect();

        // --- Remove dead tabs (and their owned ROIs) ---
        let dead: Vec<String> = self
            .tab_order
            .iter()
            .filter(|n| !alive.contains(*n))
            .cloned()
            .collect();

        for name in &dead {
            self.tabs.remove(name);

            // Collect the doomed ROI tags first, then remove them, so we
            // don't hold a borrow on `roi_manager` across a mutable call.
            let doomed: Vec<String> = spectrum_manager
                .roi_manager
                .registry()
                .values()
                .filter(|r| r.owner_spectrum.as_deref() == Some(name.as_str()))
                .map(|r| r.tag.clone())
                .collect();

            for tag in doomed {
                spectrum_manager.roi_manager.remove_roi(
                    &mut spectrum_manager.spectrum_registry,
                    &tag,
                    false, // update_state = false: the tab is going away entirely
                    true,  // optimizer_use_chi2_weight
                );
            }
        }

        self.tab_order.retain(|n| alive.contains(n));

        // --- Add new tabs, inheriting feature lines ---
        let mut new_names: Vec<String> = alive
            .iter()
            .filter(|n| !self.tabs.contains_key(*n))
            .cloned()
            .collect();
        new_names.sort();

        for name in &new_names {
            let mut state = SpectrumPlotState::for_spectrum(render_state, name.clone());
            state.feature_lines = self.feature_lines.clone();
            self.tabs.insert(name.clone(), state);
            self.tab_order.push(name.clone());
        }

        // New spectra appeared — force a re-fit on the combined plot so its curve
        // is visible without the user having to toggle Tabbed → Combined. In tabbed
        // mode this is harmless (each new tab already starts with reset_pending:
        // true), and in combined mode it's what makes data visible on first arrival.
        if !new_names.is_empty() {
            self.combined.request_reset();
        }

        // --- Fix current_tab if it points at a removed spectrum ---
        if self
            .current_tab
            .as_ref()
            .map_or(true, |n| !alive.contains(n))
        {
            self.current_tab = self.tab_order.first().cloned();
        }
    }

    // -----------------------------------------------------------------------
    // Feature lines
    // -----------------------------------------------------------------------

    /// Broadcast a feature line to every existing plot, and remember it so
    /// future tabs inherit it. Called from the peak-features dialog.
    pub fn set_feature_line(&mut self, name: &str, enabled: bool, value: f64, color: Color32) {
        if enabled {
            let line = FeatureLine { value, color };
            self.feature_lines.insert(name.to_string(), line.clone());

            self.combined
                .feature_lines
                .insert(name.to_string(), line.clone());
            for state in self.tabs.values_mut() {
                state.feature_lines.insert(name.to_string(), line.clone());
            }
        } else {
            self.feature_lines.remove(name);

            self.combined.feature_lines.remove(name);
            for state in self.tabs.values_mut() {
                state.feature_lines.remove(name);
            }
        }
    }

    // -----------------------------------------------------------------------
    // ROI adoption (for imported ROIs)
    // -----------------------------------------------------------------------

    /// Called from `App::dispatch_all_events` on
    /// `RoiManagerEvent::Created`. In tabbed mode, an imported ROI without
    /// an owner adopts the current tab; then the fit is evaluated.
    ///
    /// (Button-created ROIs already carry `owner_spectrum = current tab`,
    /// so the adoption step is a no-op for them — harmless.)
    pub fn on_roi_created(
        &mut self,
        tag: &str,
        spectrum_manager: &mut SpectrumManager,
        settings: &Settings,
    ) {
        if self.mode == PlotMode::Tabbed {
            if spectrum_manager.spectrum_registry.is_empty() {
                return;
            }
            let needs_owner = spectrum_manager
                .roi_manager
                .get_roi(tag)
                .map(|r| r.owner_spectrum.is_none())
                .unwrap_or(false);

            if needs_owner {
                if let Some(current) = self.current_tab.clone() {
                    spectrum_manager
                        .roi_manager
                        .set_owner_spectrum(tag, Some(current));
                }
            }
        }

        let use_chi2 = settings.advanced.optimizer_use_chi2_weight;
        spectrum_manager.roi_manager.on_roi_change(
            &mut spectrum_manager.spectrum_registry,
            tag,
            use_chi2,
            true,
        );
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Current owner spectrum in tabbed mode; `None` in combined mode.
    pub fn current_owner(&self) -> Option<&str> {
        match self.mode {
            PlotMode::Combined => None,
            PlotMode::Tabbed => self.current_tab.as_deref(),
        }
    }

    // -----------------------------------------------------------------------
    // Render (also performs `sync_spectra`)
    // -----------------------------------------------------------------------

    /// Render the whole screen. Internally calls `sync_spectra` so callers
    /// only need to invoke this once per frame.
    pub fn render(
        &mut self,
        spectrum_manager: &mut SpectrumManager,
        settings: &Settings,
        palette: &Palette,
        render_state: &RenderState,
        ui: &mut egui::Ui,
    ) {
        self.sync_spectra(spectrum_manager, render_state);

        match self.mode {
            PlotMode::Combined => {
                spectrum_plot::render(&mut self.combined, spectrum_manager, settings, palette, ui);
            }
            PlotMode::Tabbed => {
                self.render_tab_strip(ui);
                ui.separator();

                match self.current_tab.clone() {
                    Some(current) => {
                        if let Some(state) = self.tabs.get_mut(&current) {
                            spectrum_plot::render(state, spectrum_manager, settings, palette, ui);
                        } else {
                            ui.label("Selected tab is stale — please reload");
                        }
                    }
                    None => {
                        ui.label("No spectra loaded — connect a detector or import a file");
                    }
                }
            }
        }
    }

    fn render_tab_strip(&mut self, ui: &mut egui::Ui) {
        let tab_names = self.tab_order.clone();
        let mut clicked: Option<String> = None;

        ui.horizontal_wrapped(|ui| {
            for tab_name in &tab_names {
                let is_current = self.current_tab.as_deref() == Some(tab_name.as_str());
                if ui.selectable_label(is_current, tab_name).clicked() {
                    clicked = Some(tab_name.clone());
                }
            }
        });

        if let Some(name) = clicked {
            self.current_tab = Some(name);
        }
    }

    // -----------------------------------------------------------------------
    // Nuclide → ROI matching
    // -----------------------------------------------------------------------

    /// Match ROIs against `nuclide_library`, updating each ROI's `emission`
    /// in place. Returns one `NuclideMatch` per successful match.
    ///
    /// Scope:
    /// - **Tabbed mode**: only ROIs owned by `current_tab`, plus unowned ROIs.
    /// - **Combined mode**: all ROIs (they're all visible).
    ///
    /// The caller formats via `format_match_result` and pushes a
    /// `Modal::root(ModalKind::Info(...))`.
    pub fn match_nuclide_to_rois(
        &self,
        spectrum_manager: &mut SpectrumManager,
        energy_search_window: f64,
        optimizer_use_chi2_weight: bool,
    ) -> Vec<NuclideMatch> {
        let current_owner: Option<String> = self.current_owner().map(String::from);

        let mut pending: Vec<(String, Emission)> = Vec::new();
        let mut results: Vec<NuclideMatch> = Vec::new();

        let roi_tags: Vec<String> = spectrum_manager
            .roi_manager
            .registry()
            .keys()
            .cloned()
            .collect();

        for tag in &roi_tags {
            let Some(roi) = spectrum_manager.roi_manager.get_roi(tag) else {
                continue;
            };

            // Owner filter: in tabbed mode, only unowned ROIs and ROIs owned
            // by the current tab qualify. Combined mode matches everything.
            if let Some(owner) = &current_owner {
                if let Some(r_owner) = &roi.owner_spectrum {
                    if r_owner != owner {
                        continue;
                    }
                }
            }
            let alias = roi.alias.clone();

            // Build a list of resolved ROI data for this tag across all
            // spectra that hold one. `match_roi_to_nuclide` uses their fit
            // centroids (counts common nuclide across spectra).
            let data = spectrum_manager
                .roi_manager
                .get_data_from_roi(&spectrum_manager.spectrum_registry, tag);
            let rois_refs: Vec<&Roi> = data.iter().map(|(_, r)| *r).collect();
            if rois_refs.is_empty() {
                continue;
            }

            let matched = spectrum_manager
                .nuclide_library
                .match_roi_to_nuclide(rois_refs.iter().copied(), energy_search_window);
            let Some(em) = matched else { continue };

            let parent = em.parent_nuclide.map(|id| id.to_name()).unwrap_or_default();
            let energy_kev = em
                .energy_kev
                .as_ref()
                .map(|q| q.value.mean())
                .unwrap_or(0.0);
            let intensity_percent = em
                .intensity_percent
                .as_ref()
                .map(|q| q.value.mean())
                .unwrap_or(0.0);

            results.push(NuclideMatch {
                alias,
                nuclide: parent,
                energy_kev,
                intensity_percent,
            });
            pending.push((tag.clone(), em));
        }

        // Apply in a second pass (mutable borrow of spectrum_manager).
        for (tag, em) in pending {
            spectrum_manager.roi_manager.set_roi_emission(
                &mut spectrum_manager.spectrum_registry,
                &tag,
                Some(em),
                optimizer_use_chi2_weight,
            );
        }

        results
    }
}
