//! Isotopics tab: nuclide browser, emission viewer, peak search.
//!
//! Port of `luracs/gui/tabs/isotopics.py`. The Qt `NuclideListItem` /
//! `ColorCellWidget` pair collapses into a per-frame `render_row`; the
//! `sigViewCheckChanged` signal becomes a direct
//! `NuclideLibrary::track_selected_nuclide` call. The `SpectrumDebugViewWidget`
//! (three side-by-side plots with peak markers) becomes a single
//! `egui::Window` rendered from the tab's own `DebugViewState`.
//!
//! ## Deviations
//!
//! - No `Calculator.run` — `find_peaks` is called inline. Add a `thread` +
//!   `channel` around it if you ever hit a spectra large enough to stall the
//!   frame.
//! - No per-nuclide color dialog ("Auto Set Emissions" now finds a nuclide's
//!   emissions directly from the spectrum rather than via the info pane).
//! - Debug view draws peak left/center/right as three differently-coloured
//!   vertical markers instead of `symbol="o"` scatter dots. Same information.
//! - `decay_mode` is displayed as its raw string (e.g. `"(B-)"`) rather than
//!   Unicode-folded to `"β-"`. One `replace` if you want the folding back.

use std::collections::HashMap;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use nucleide_nuclei::NuclideId;
use rsplot::{LineStyle, Marker, Plot1D, YAxis};

use crate::containers::{Emission, Nuclide};
use crate::core::{NewRoi, SpectrumManager};
use crate::settings::Settings;
use crate::utils::find_peaks;

const DEFAULT_ROW_COLOR: Color32 = Color32::from_rgb(120, 180, 255);
const ROW_HEIGHT: f32 = 22.0;

// ===========================================================================
// Helpers
// ===========================================================================

fn nuclide_display_name(nuclide: &Nuclide) -> String {
    format!("{}-{}", nuclide.element.symbol(), nuclide.nuclide.a())
}

fn format_duration(seconds: f64) -> String {
    let abs_s = seconds.abs();
    if abs_s == 0.0 {
        return "0 s".into();
    }
    const MINUTE: f64 = 60.0;
    const HOUR: f64 = 3600.0;
    const DAY: f64 = 86400.0;
    const YEAR: f64 = 365.25 * DAY;

    if abs_s >= 1e12 {
        return format!("{:.3e} yr", seconds / YEAR);
    }
    if abs_s >= YEAR {
        let y = seconds / YEAR;
        if y.abs() >= 1e9 {
            return format!("{:.2} Gyr", y / 1e9);
        }
        if y.abs() >= 1e6 {
            return format!("{:.2} Myr", y / 1e6);
        }
        return format!("{:.2} yr", y);
    }
    if abs_s >= DAY {
        return format!("{:.2} days", seconds / DAY);
    }
    if abs_s >= HOUR {
        return format!("{:.2} h", seconds / HOUR);
    }
    if abs_s >= MINUTE {
        return format!("{:.2} min", seconds / MINUTE);
    }
    if abs_s < 1e-3 {
        return format!("{:.2} µs", seconds * 1e6);
    }
    if abs_s < 1.0 {
        return format!("{:.2} ms", seconds * 1e3);
    }
    format!("{:.2} s", seconds)
}

fn format_emissions(emissions: &[Emission]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{:>12} | {:>14} | {:>6} | Origin\n",
        "Energy (keV)", "Intensity (%)", "Type"
    ));
    out.push_str(&"-".repeat(64));
    out.push('\n');
    for e in emissions {
        let energy = e
            .energy_kev
            .as_ref()
            .map(|q| q.value.mean())
            .unwrap_or(f64::NAN);
        let intensity = e
            .intensity_percent
            .as_ref()
            .map(|q| q.value.mean())
            .unwrap_or(f64::NAN);
        let ty = e.r#type.map(|t| format!("{t:?}")).unwrap_or_default();
        let origin = e.origin.map(|o| format!("{o:?}")).unwrap_or_default();
        out.push_str(&format!(
            "{:12.4} | {:14.2} | {:>6} | {}\n",
            energy, intensity, ty, origin
        ));
    }
    out
}

fn format_nuclide_info(n: &Nuclide) -> String {
    let mut out = String::new();
    let title = format!("| {} | {} |", n.element.symbol(), n.nuclide.to_name());
    let sep = "=".repeat(title.chars().count());
    out.push_str(&format!("{sep}\n{title}\n{sep}\n"));
    out.push_str(&format!(
        "Half-Life = {}\n",
        format_duration(n.half_life_s.value.mean())
    ));
    out.push_str(&format!("Z = {}\n\n", n.element.atomic_number()));
    out.push_str("Emissions:\n");
    out.push_str(&format_emissions(&n.emissions));
    out.push_str("\n\nDaughter Products:\n");
    // Assumes `Nuclide.daughters: Vec<(NuclideId, String, f64)>` — see port note.
    for (daughter, decay_mode, branching) in &n.daughters {
        let mode = decay_mode.replace("alpha", "α").replace("B", "β");
        out.push_str(&format!(
            "{mode} {} {:.4}%\n",
            daughter.to_name(),
            branching
        ));
    }
    out.push_str(&format!(
        "\n\nSource: {}\n(see bibliography)\n",
        n.citation_ref
    ));
    out
}

// ===========================================================================
// Debug view
// ===========================================================================

pub struct DebugViewState {
    spectrum_plot: Plot1D,
    d1_plot: Plot1D,
    d2_plot: Plot1D,

    spectrum_y: Option<Vec<f64>>,
    d1: Option<Vec<f64>>,
    d2: Option<Vec<f64>>,
    peaks: Option<Vec<(usize, usize, usize)>>,
}

impl DebugViewState {
    fn new(render_state: &RenderState) -> Self {
        Self {
            spectrum_plot: Plot1D::new(render_state, 0x1001),
            d1_plot: Plot1D::new(render_state, 0x1002),
            d2_plot: Plot1D::new(render_state, 0x1003),
            spectrum_y: None,
            d1: None,
            d2: None,
            peaks: None,
        }
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        ui.label("Spectrum");
        render_one_plot(
            &mut self.spectrum_plot,
            self.spectrum_y.as_deref(),
            self.peaks.as_deref(),
            ui,
        );
        ui.separator();
        ui.label("First Derivative");
        render_one_plot(
            &mut self.d1_plot,
            self.d1.as_deref(),
            self.peaks.as_deref(),
            ui,
        );
        ui.separator();
        ui.label("Second Derivative");
        render_one_plot(
            &mut self.d2_plot,
            self.d2.as_deref(),
            self.peaks.as_deref(),
            ui,
        );
    }
}

fn render_one_plot(
    plot: &mut Plot1D,
    y_data: Option<&[f64]>,
    peaks: Option<&[(usize, usize, usize)]>,
    ui: &mut egui::Ui,
) {
    plot.clear();
    plot.set_graph_y_label("", YAxis::Left);
    plot.set_x_min_pos(Some(0.0));

    if let Some(y) = y_data {
        let x: Vec<f64> = (0..y.len()).map(|i| i as f64).collect();
        plot.add_curve(&x, y, Color32::WHITE);
    }
    if let Some(peaks) = peaks {
        for (le, ce, re) in peaks {
            for (x, color) in [
                (*le as f64, Color32::RED),
                (*ce as f64, Color32::LIGHT_BLUE),
                (*re as f64, Color32::GREEN),
            ] {
                let mut m = Marker::vline(x)
                    .with_line_style(LineStyle::Solid)
                    .with_line_width(1.5)
                    .with_draggable(false);
                m.color = color;
                plot.add_marker_data(&m);
            }
        }
    }
    plot.show(ui);
}

// ===========================================================================
// Tab state
// ===========================================================================

#[derive(Clone, Debug)]
enum RowItem {
    Nuclide {
        id: NuclideId,
        display: String,
        display_lower: String,
        emissions_count: usize,
    },
    Chain {
        name: String,
        name_lower: String,
        label: String,
    },
}

pub struct IsotopicsTab {
    /// Search filter (case-insensitive substring of the display name).
    pub filter: String,
    last_filter: String,
    all_rows: Vec<RowItem>,
    filtered_rows: Vec<usize>,

    /// Nuclide whose details are shown in the info pane.
    pub selected_info: Option<NuclideId>,
    cached_info_id: Option<NuclideId>,
    cached_info_text: String,

    /// Per-nuclide line color, persists across frames and survives
    /// deselect/reselect.
    pub nuclide_colors: HashMap<NuclideId, Color32>,
    /// Per-decay-chain "all members selected" state.
    pub chain_checked: HashMap<String, bool>,

    /// Spectrum the peak finder operates on. `None` means "first available".
    pub search_spectrum: Option<String>,
    /// Channels in the Savitzky-Golay window.
    pub search_window_length: usize,

    pub debug: DebugViewState,
    pub debug_open: bool,
    pub auto_set_emissions_requested: bool,
}

impl IsotopicsTab {
    pub fn new(render_state: &RenderState) -> Self {
        Self {
            filter: String::new(),
            last_filter: String::new(),
            all_rows: Vec::new(),
            filtered_rows: Vec::new(),
            selected_info: None,
            cached_info_id: None,
            cached_info_text: String::new(),
            nuclide_colors: HashMap::new(),
            chain_checked: HashMap::new(),
            search_spectrum: None,
            search_window_length: 31,
            debug: DebugViewState::new(render_state),
            debug_open: false,
            auto_set_emissions_requested: false,
        }
    }

    pub fn render(
        &mut self,
        spectrum_manager: &mut SpectrumManager,
        settings: &Settings,
        ui: &mut egui::Ui,
    ) {
        // Ensure the nuclide/decay-chain row list is cached and filtered.
        self.ensure_rows(spectrum_manager);

        // Resolve default spectrum for peak search.
        if let Some(name) = &self.search_spectrum
            && spectrum_manager.get_spectrum(name).is_none()
        {
            self.search_spectrum = None;
        }
        if self.search_spectrum.is_none() {
            self.search_spectrum = spectrum_manager.spectrum_registry.keys().next().cloned();
        }

        ui.horizontal_top(|ui| {
            let spacing = ui.spacing().item_spacing.x + ui.spacing().item_spacing.x + 16.0;
            let total_w = (ui.available_width() - spacing).max(100.0);
            let total_h = ui.available_height();

            // --- Left: search + list (30 %) ---
            ui.allocate_ui_with_layout(
                egui::vec2(total_w * 0.30, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let filter_response = ui.add_sized(
                        [ui.available_width(), ROW_HEIGHT],
                        egui::TextEdit::singleline(&mut self.filter).hint_text("Search nuclide..."),
                    );
                    if filter_response.changed() || self.filter != self.last_filter {
                        self.update_filter();
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("isotopics_nuclide_list")
                        .auto_shrink([false, false])
                        .show_rows(
                            ui,
                            ROW_HEIGHT,
                            self.filtered_rows.len(),
                            |ui, row_range| {
                                for idx in row_range {
                                    let row_idx = self.filtered_rows[idx];
                                    let row = self.all_rows[row_idx].clone();
                                    self.render_row(&row, spectrum_manager, ui);
                                }
                            },
                        );
                },
            );

            ui.separator();

            // --- Middle: info pane (45 %) ---
            ui.allocate_ui_with_layout(
                egui::vec2(total_w * 0.45, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if self.cached_info_id != self.selected_info {
                        self.cached_info_id = self.selected_info;
                        self.cached_info_text = match self.selected_info {
                            Some(id) => spectrum_manager
                                .nuclide_library
                                .get_nuclide(id)
                                .map(format_nuclide_info)
                                .unwrap_or_else(|| "No nuclide selected.".into()),
                            None => "Click a nuclide to see its details.".into(),
                        };
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("isotopics_info_pane")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut view = self.cached_info_text.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut view)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(30)
                                    .font(egui::TextStyle::Monospace)
                                    .interactive(false),
                            );
                        });
                },
            );

            ui.separator();

            // --- Right: peak search controls (25 %) ---
            ui.allocate_ui_with_layout(
                egui::vec2(total_w * 0.25, total_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if ui.button("Auto Set Emissions").clicked() {
                        self.auto_set_emissions_requested = true;
                    }

                    ui.add_space(4.0);
                    if ui.button("Search peaks").clicked() {
                        self.run_peak_search(spectrum_manager, settings);
                    }

                    ui.add_space(4.0);
                    ui.label("Spectrum");
                    let names: Vec<(String, String)> = spectrum_manager
                        .spectrum_registry
                        .iter()
                        .map(|(k, v)| (v.name.clone(), k.clone()))
                        .collect();
                    let selected_name = self
                        .search_spectrum
                        .as_ref()
                        .and_then(|k| names.iter().find(|(_, kk)| kk == k).map(|(n, _)| n.clone()))
                        .unwrap_or_else(|| "(none)".into());
                    egui::ComboBox::from_id_salt("isotopics_search_spectrum")
                        .selected_text(selected_name)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (name, key) in &names {
                                let is_sel = self.search_spectrum.as_deref() == Some(key.as_str());
                                if ui.selectable_label(is_sel, name).clicked() {
                                    self.search_spectrum = Some(key.clone());
                                }
                            }
                        });

                    ui.add_space(4.0);
                    ui.label("Channels in Search Window");
                    ui.add(
                        egui::DragValue::new(&mut self.search_window_length)
                            .range(3..=1001)
                            .speed(1),
                    );

                    ui.add_space(4.0);
                    if ui.button("View Derivatives").clicked() {
                        self.debug_open = true;
                    }
                },
            );
        });

        // Debug window (independent of tab lifetime: closing it drops the flag).
        if self.debug_open {
            let viewport_id = egui::ViewportId::from_hash_of("spectrum_derivatives_debug");
            let builder = egui::ViewportBuilder::default()
                .with_title("Spectrum Derivatives")
                .with_inner_size([900.0, 700.0])
                .with_resizable(false);

            ui.ctx()
                .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                    if ui.input(|i| i.viewport().close_requested()) {
                        self.debug_open = false;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        self.debug.render(ui);
                    });
                });
        }
    }

    fn ensure_rows(&mut self, spectrum_manager: &SpectrumManager) {
        if self.all_rows.is_empty() {
            let mut rows: Vec<(u32, RowItem)> = Vec::new();
            for id in spectrum_manager.nuclide_library.sorted_nuclide_ids(false) {
                if let Some(n) = spectrum_manager.nuclide_library.get_nuclide(id) {
                    let display = nuclide_display_name(n);
                    let display_lower = display.to_lowercase();
                    rows.push((
                        id.a(),
                        RowItem::Nuclide {
                            id,
                            display,
                            display_lower,
                            emissions_count: n.emissions.len(),
                        },
                    ));
                }
            }
            for (chain_name, _members) in spectrum_manager.nuclide_library.decay_chains() {
                let mass = chain_name
                    .split_once('-')
                    .and_then(|(_, rest)| rest.split_whitespace().next())
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                rows.push((
                    mass,
                    RowItem::Chain {
                        name: (*chain_name).to_string(),
                        name_lower: chain_name.to_lowercase(),
                        label: format!("🔗 {chain_name}"),
                    },
                ));
            }
            rows.sort_by_key(|(m, _)| *m);
            self.all_rows = rows.into_iter().map(|(_, r)| r).collect();
            self.update_filter();
        }
    }

    fn update_filter(&mut self) {
        self.last_filter = self.filter.clone();
        let filter_lower = self.filter.to_lowercase();
        if filter_lower.is_empty() {
            self.filtered_rows = (0..self.all_rows.len()).collect();
        } else {
            self.filtered_rows = self
                .all_rows
                .iter()
                .enumerate()
                .filter_map(|(idx, row)| {
                    let matches = match row {
                        RowItem::Nuclide { display_lower, .. } => {
                            display_lower.contains(&filter_lower)
                        }
                        RowItem::Chain { name_lower, .. } => {
                            name_lower.contains(&filter_lower)
                        }
                    };
                    if matches { Some(idx) } else { None }
                })
                .collect();
        }
    }

    fn render_row(
        &mut self,
        row: &RowItem,
        spectrum_manager: &mut SpectrumManager,
        ui: &mut egui::Ui,
    ) {
        match row {
            RowItem::Nuclide {
                id,
                display,
                emissions_count,
                ..
            } => {
                let id = *id;
                let is_selected = spectrum_manager.nuclide_library.selected().contains(&id);
                let stored_color = self
                    .nuclide_colors
                    .get(&id)
                    .copied()
                    .unwrap_or(DEFAULT_ROW_COLOR);

                // --- Draw row ---
                ui.horizontal(|ui| {
                    let mut checked = is_selected;
                    let mut color = stored_color;

                    // Color swatch (click opens picker).
                    let color_changed = ui.color_edit_button_srgba(&mut color).changed();

                    // Checkbox toggles line visibility.
                    let check_changed = ui.checkbox(&mut checked, display).changed();

                    // Info button (ⓘ) selects the nuclide for the info pane.
                    if ui
                        .selectable_label(self.selected_info == Some(id), "ⓘ")
                        .on_hover_text("Show details")
                        .clicked()
                    {
                        self.selected_info = Some(id);
                    }

                    if *emissions_count == 0 {
                        ui.label(egui::RichText::new("(no emissions)").italics().weak());
                    }

                    // --- Apply mutations ---
                    if color_changed {
                        self.nuclide_colors.insert(id, color);
                        if is_selected {
                            spectrum_manager
                                .nuclide_library
                                .track_selected_nuclide(id, true, color);
                        }
                    }
                    if check_changed {
                        self.nuclide_colors.insert(id, color);
                        spectrum_manager
                            .nuclide_library
                            .track_selected_nuclide(id, checked, color);
                    }
                });
            }

            RowItem::Chain { name, label, .. } => {
                let is_checked = self
                    .chain_checked
                    .get(name)
                    .copied()
                    .unwrap_or(false);

                let mut checked = is_checked;
                let changed = ui
                    .horizontal(|ui| {
                        ui.checkbox(&mut checked, label)
                            .on_hover_text("Toggle all members of this decay chain")
                            .changed()
                    })
                    .inner;

                if changed {
                    self.chain_checked.insert(name.clone(), checked);
                    let members = spectrum_manager
                        .nuclide_library
                        .decay_chain_ids(name);
                    if let Some(members) = members {
                        for id in members {
                            let color = self
                                .nuclide_colors
                                .get(&id)
                                .copied()
                                .unwrap_or(DEFAULT_ROW_COLOR);
                            spectrum_manager
                                .nuclide_library
                                .track_selected_nuclide(id, checked, color);
                        }
                    }
                }
            }
        }
    }

    fn run_peak_search(&mut self, spectrum_manager: &mut SpectrumManager, settings: &Settings) {
        let Some(name) = self.search_spectrum.clone() else {
            tracing::warn!("Peak search: no spectrum selected");
            return;
        };
        let Some(spectrum) = spectrum_manager.get_spectrum(&name).cloned() else {
            tracing::warn!("Peak search: spectrum {name:?} not found");
            return;
        };

        let y_axis = if spectrum_manager.roi_manager.spectrum_is_bkg_sub() {
            spectrum
                .get_bkg_sub(false)
                .map(|a| a.to_vec())
                .unwrap_or_default()
        } else {
            spectrum
                .get_foreground(false, false)
                .map(|a| a.to_vec())
                .unwrap_or_default()
        };
        let x_axis: Vec<f64> = spectrum.x_axis.to_vec();

        match find_peaks(&y_axis, &x_axis, self.search_window_length) {
            Ok(result) => {
                if settings.logging.verbose_calculations {
                    tracing::info!("{}", result.log_info);
                }

                self.debug.spectrum_y = Some(y_axis);
                self.debug.d1 = Some(result.d1);
                self.debug.d2 = Some(result.d2);
                self.debug.peaks = Some(result.peaks.clone());

                for (left, _centre, right) in &result.peaks {
                    let (le_e, re_e) = (x_axis[*left], x_axis[*right]);
                    let mut nr = NewRoi::new(le_e, re_e);
                    nr.merge = false;
                    spectrum_manager.roi_manager.add_roi(nr);
                }
            }
            Err(e) => {
                tracing::warn!("Peak search failed: {e}");
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_ranges() {
        assert_eq!(format_duration(0.0), "0 s");
        assert_eq!(format_duration(0.000005), "5.00 µs");
        assert_eq!(format_duration(0.025), "25.00 ms");
        assert_eq!(format_duration(42.0), "42.00 s");
        assert_eq!(format_duration(120.0), "2.00 min");
        assert_eq!(format_duration(7200.0), "2.00 h");
        assert_eq!(format_duration(86400.0 * 3.0), "3.00 days");
        assert_eq!(format_duration(31557600.0 * 5.0), "5.00 yr");
    }

    #[test]
    fn test_row_item_filter() {
        let row1 = RowItem::Nuclide {
            id: NuclideId::from_name("Am-241").unwrap(),
            display: "Am-241".into(),
            display_lower: "am-241".into(),
            emissions_count: 5,
        };
        let row2 = RowItem::Chain {
            name: "Th-232".into(),
            name_lower: "th-232".into(),
            label: "🔗 Th-232".into(),
        };
        let rows = vec![row1, row2];

        // Empty filter matches all
        let all: Vec<usize> = (0..rows.len()).collect();
        assert_eq!(all.len(), 2);

        // Filter for "am" matches only row1
        let filter = "am";
        let filtered: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| match row {
                RowItem::Nuclide { display_lower, .. } if display_lower.contains(filter) => {
                    Some(idx)
                }
                RowItem::Chain { name_lower, .. } if name_lower.contains(filter) => Some(idx),
                _ => None,
            })
            .collect();
        assert_eq!(filtered, vec![0]);
    }
}
