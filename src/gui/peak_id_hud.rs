//! Live Nuclide ID / Peak-Match HUD for the spectrum plot.
//!
//! Runs `find_peaks` on the active calibrated spectrum, matches candidate peaks
//! against `NuclideLibrary`, and renders clickable peak badges on the plot that
//! create fitted ROIs in one click.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};

use crate::containers::Emission;
use crate::core::{NewRoi, NuclideLibrary, SpectrumManager};
use crate::gui::spectrum_plot::{BkgDisplayMode, SpectrumPlotState};
use crate::utils::numerics::find_peaks;

/// Default energy matching tolerance in keV.
pub const DEFAULT_MATCH_TOLERANCE_KEV: f64 = 12.0;

/// Minimum interval between re-running peak detection on live spectra.
const RECOMPUTE_INTERVAL: Duration = Duration::from_millis(500);

/// Minimum integrated counts before attempting peak detection.
const MIN_TOTAL_COUNTS: f64 = 50.0;

#[derive(Debug, Clone)]
pub struct PeakMatchCandidate {
    pub nuclide_name: String,
    pub emission_kev: f64,
    pub delta_kev: f64,
    pub intensity_percent: Option<f64>,
    pub emission: Emission,
}

#[derive(Debug, Clone)]
pub struct DetectedPeakBadge {
    pub spectrum_name: String,
    pub energy_kev: f64,
    pub lo_kev: f64,
    pub hi_kev: f64,
    pub peak_counts: f64,
    pub peak_cps: f64,
    pub matches: Vec<PeakMatchCandidate>,
}

#[derive(Debug, Clone, Default)]
pub struct PeakIdCache {
    last_key: Option<(String, u64, u8)>,
    last_eval: Option<Instant>,
    pub badges: Vec<DetectedPeakBadge>,
}

impl PeakIdCache {
    pub fn clear(&mut self) {
        self.last_key = None;
        self.last_eval = None;
        self.badges.clear();
    }

    pub fn update_if_needed(
        &mut self,
        state: &SpectrumPlotState,
        spectrum_manager: &SpectrumManager,
    ) {
        if !state.peak_id_enabled {
            if !self.badges.is_empty() {
                self.clear();
            }
            return;
        }

        let Some((name, spectrum)) = select_target_spectrum(state, spectrum_manager) else {
            self.clear();
            return;
        };

        if !spectrum.calibrated {
            self.clear();
            return;
        }

        let Some(x_axis) = spectrum.x_axis.as_slice() else {
            self.clear();
            return;
        };

        let linear_y = match state.bkg_mode {
            BkgDisplayMode::Subtract => spectrum.get_bkg_sub(false),
            _ => spectrum.get_foreground(false, false),
        };
        let Some(y_arr) = linear_y else {
            self.clear();
            return;
        };
        let Some(y_axis) = y_arr.as_slice() else {
            self.clear();
            return;
        };

        let total_counts_key = spectrum
            .foreground
            .as_ref()
            .map_or(0, |f| f.total_counts);
        let bkg_key = match state.bkg_mode {
            BkgDisplayMode::None => 0,
            BkgDisplayMode::Overlay => 1,
            BkgDisplayMode::Subtract => 2,
        };
        let key = (name.clone(), total_counts_key, bkg_key);

        let same_key = self.last_key.as_ref() == Some(&key);
        let within_throttle = self
            .last_eval
            .is_some_and(|t| t.elapsed() < RECOMPUTE_INTERVAL);
        if same_key || within_throttle {
            return;
        }

        let live_time = spectrum
            .foreground
            .as_ref()
            .map_or(1.0, |f| f.live_time)
            .max(1e-9);

        self.badges = detect_and_match_peaks(
            &name,
            x_axis,
            y_axis,
            live_time,
            &spectrum_manager.nuclide_library,
            DEFAULT_MATCH_TOLERANCE_KEV,
        );
        self.last_key = Some(key);
        self.last_eval = Some(Instant::now());
    }
}

fn select_target_spectrum<'a>(
    state: &SpectrumPlotState,
    spectrum_manager: &'a SpectrumManager,
) -> Option<(String, &'a crate::containers::Spectrum)> {
    if let Some(owned) = &state.owned_spectrum {
        return spectrum_manager
            .spectrum_registry
            .get(owned)
            .filter(|s| s.show_in_plot)
            .map(|s| (owned.clone(), s));
    }

    // In combined mode, pick the first visible calibrated spectrum (deterministic order).
    let mut names: Vec<&String> = spectrum_manager
        .spectrum_registry
        .iter()
        .filter(|(_, s)| s.show_in_plot && s.calibrated)
        .map(|(n, _)| n)
        .collect();
    names.sort();
    let first = names.first().copied()?;
    spectrum_manager
        .spectrum_registry
        .get(first)
        .map(|s| (first.clone(), s))
}

/// Run Savitzky–Golay peak detection on `(x_axis, y_axis)` and match each
/// peak's centroid against `nuclide_lib` within `tolerance_kev`.
#[must_use]
pub fn detect_and_match_peaks(
    spectrum_name: &str,
    x_axis: &[f64],
    y_axis: &[f64],
    live_time: f64,
    nuclide_lib: &NuclideLibrary,
    tolerance_kev: f64,
) -> Vec<DetectedPeakBadge> {
    if x_axis.len() != y_axis.len() || x_axis.len() < 15 {
        return Vec::new();
    }

    let total: f64 = y_axis.iter().copied().filter(|v| v.is_finite() && *v > 0.0).sum();
    if total < MIN_TOTAL_COUNTS {
        return Vec::new();
    }

    let Ok(found) = find_peaks(y_axis, x_axis, 7) else {
        return Vec::new();
    };

    let mut badges = Vec::with_capacity(found.peaks.len());
    for (left, centre, right) in found.peaks {
        if centre >= x_axis.len() || left >= x_axis.len() {
            continue;
        }
        let right_idx = right.min(x_axis.len() - 1);
        let energy_kev = x_axis[centre];
        let lo_kev = x_axis[left];
        let hi_kev = x_axis[right_idx];
        if !energy_kev.is_finite() || !lo_kev.is_finite() || !hi_kev.is_finite() || hi_kev <= lo_kev
        {
            continue;
        }

        let peak_counts = y_axis[centre].max(0.0);
        let peak_cps = if live_time > 0.0 {
            peak_counts / live_time
        } else {
            peak_counts
        };

        let matches = match_candidates(energy_kev, nuclide_lib, tolerance_kev);

        badges.push(DetectedPeakBadge {
            spectrum_name: spectrum_name.to_string(),
            energy_kev,
            lo_kev,
            hi_kev,
            peak_counts,
            peak_cps,
            matches,
        });
    }

    badges
}

fn match_candidates(
    energy_kev: f64,
    nuclide_lib: &NuclideLibrary,
    tolerance_kev: f64,
) -> Vec<PeakMatchCandidate> {
    let mut candidates: Vec<PeakMatchCandidate> = Vec::new();

    for em in nuclide_lib.all_emissions() {
        let Some(e_val) = em.energy_kev.as_ref().map(|q| q.value.mean()) else {
            continue;
        };
        let delta = energy_kev - e_val;
        if delta.abs() > tolerance_kev {
            continue;
        }
        let nuclide_name = em
            .parent_nuclide
            .as_ref()
            .map_or_else(|| "Annihilation".to_string(), |n| n.to_name());
        let intensity = em.intensity_percent.as_ref().map(|q| q.value.mean());
        candidates.push(PeakMatchCandidate {
            nuclide_name,
            emission_kev: e_val,
            delta_kev: delta,
            intensity_percent: intensity,
            emission: em.clone(),
        });
    }

    if (energy_kev - 511.0).abs() <= tolerance_kev
        && !candidates.iter().any(|c| (c.emission_kev - 511.0).abs() < 0.5)
    {
        let ann = Emission::annihilation();
        candidates.push(PeakMatchCandidate {
            nuclide_name: "Annihilation".to_string(),
            emission_kev: 511.0,
            delta_kev: energy_kev - 511.0,
            intensity_percent: None,
            emission: ann,
        });
    }

    candidates.sort_by(|a, b| {
        a.delta_kev
            .abs()
            .partial_cmp(&b.delta_kev.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let ia = a.intensity_percent.unwrap_or(0.0);
                let ib = b.intensity_percent.unwrap_or(0.0);
                ib.partial_cmp(&ia).unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    candidates.truncate(3);
    candidates
}

/// Render clickable Peak ID badges inside `plot_area`.
#[allow(clippy::too_many_arguments)]
pub fn draw_peak_id_hud(
    ui: &egui::Ui,
    plot_area: Rect,
    badges: &[DetectedPeakBadge],
    state: &SpectrumPlotState,
    spectrum_manager: &mut SpectrumManager,
    use_chi2: bool,
    x_to_px: impl Fn(f64) -> f32,
    y_to_px: impl Fn(f64) -> f32,
) {
    if !state.peak_id_enabled || badges.is_empty() {
        return;
    }

    let painter = ui.painter_at(plot_area);
    let font = FontId::proportional(11.0);
    let mut placed_rects: Vec<Rect> = Vec::with_capacity(badges.len());
    let mut clicked_badge: Option<&DetectedPeakBadge> = None;

    for (idx, badge) in badges.iter().enumerate() {
        let raw_y = if state.bkg_mode == BkgDisplayMode::Subtract || state.cps {
            badge.peak_cps
        } else {
            badge.peak_counts
        };
        let y_data = if state.log {
            if raw_y > 0.0 {
                raw_y.log10()
            } else {
                continue;
            }
        } else {
            raw_y
        };

        let px_x = x_to_px(badge.energy_kev);
        let px_y = y_to_px(y_data);
        if !px_x.is_finite()
            || !px_y.is_finite()
            || px_x < plot_area.left()
            || px_x > plot_area.right()
        {
            continue;
        }

        let covered_by_roi = spectrum_manager
            .roi_manager
            .registry()
            .values()
            .any(|roi| {
                let owner_ok = state
                    .owned_spectrum
                    .as_ref()
                    .is_none_or(|o| roi.owner_spectrum.as_deref() == Some(o.as_str()));
                owner_ok && badge.energy_kev >= roi.roi_bound.0 && badge.energy_kev <= roi.roi_bound.1
            });

        let label = badge.matches.first().map_or_else(
            || format!("? {:.1} keV", badge.energy_kev),
            |top| format!("{} · {:.1}", top.nuclide_name, badge.energy_kev),
        );

        let galley = painter.layout_no_wrap(label, font.clone(), Color32::WHITE);
        let pad = Vec2::new(6.0, 3.0);
        let size = galley.size() + pad * 2.0;

        let mut center_x = px_x.clamp(
            size.x.mul_add(0.5, plot_area.left()) + 2.0,
            size.x.mul_add(-0.5, plot_area.right()) - 2.0,
        );
        let mut bottom_y = (px_y - 10.0).clamp(plot_area.top() + size.y + 2.0, plot_area.bottom() - 2.0);

        let mut rect = Rect::from_min_size(
            Pos2::new(size.x.mul_add(-0.5, center_x), bottom_y - size.y),
            size,
        );

        // Simple vertical de-overlapping so nearby peak labels stack cleanly.
        for _ in 0..6 {
            if let Some(hit) = placed_rects.iter().find(|r| r.intersects(rect)) {
                bottom_y = (hit.top() - 4.0).max(plot_area.top() + size.y + 2.0);
                center_x = rect.center().x;
                rect = Rect::from_min_size(
                    Pos2::new(size.x.mul_add(-0.5, center_x), bottom_y - size.y),
                    size,
                );
            } else {
                break;
            }
        }
        placed_rects.push(rect);

        let id = ui.scope_id().with(("peak_id_badge", idx, badge.energy_kev.to_bits()));
        let resp = ui.interact(rect, id, egui::Sense::click());
        let hovered = resp.hovered();

        let (bg, border, text_col) = if covered_by_roi {
            (
                Color32::from_rgba_unmultiplied(30, 40, 55, if hovered { 210 } else { 120 }),
                Color32::from_rgba_unmultiplied(120, 140, 170, if hovered { 220 } else { 110 }),
                Color32::from_rgba_unmultiplied(210, 220, 235, if hovered { 240 } else { 160 }),
            )
        } else if badge.matches.is_empty() {
            (
                Color32::from_rgba_unmultiplied(55, 50, 35, if hovered { 235 } else { 195 }),
                Color32::from_rgb(230, 185, 90),
                Color32::from_rgb(255, 240, 200),
            )
        } else {
            (
                Color32::from_rgba_unmultiplied(25, 55, 85, if hovered { 240 } else { 205 }),
                Color32::from_rgb(100, 210, 255),
                Color32::WHITE,
            )
        };

        // Subtle leader line from peak apex to badge bottom.
        let apex = Pos2::new(px_x, px_y.clamp(plot_area.top(), plot_area.bottom()));
        painter.line_segment(
            [apex, Pos2::new(rect.center().x, rect.bottom())],
            Stroke::new(1.0, border.gamma_multiply(0.7)),
        );
        painter.rect_filled(rect, CornerRadius::same(4), bg);
        painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, border), egui::StrokeKind::Inside);
        painter.galley(rect.min + pad, galley, text_col);

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let resp = resp.on_hover_ui(|ui| {
            ui.strong(format!("Peak @ {:.2} keV", badge.energy_kev));
            ui.label(format!("Window: {:.1} – {:.1} keV", badge.lo_kev, badge.hi_kev));
            if badge.matches.is_empty() {
                ui.weak("No nuclide match within tolerance");
            } else {
                ui.separator();
                for m in &badge.matches {
                    let int_str = m
                        .intensity_percent
                        .map_or_else(String::new, |p| format!(" ({p:.1}%)"));
                    ui.label(format!(
                        "{} @ {:.2} keV (Δ {:+.2} keV){}",
                        m.nuclide_name, m.emission_kev, m.delta_kev, int_str
                    ));
                }
            }
            ui.separator();
            ui.weak(if covered_by_roi {
                "ROI already covers this peak (click to add another)"
            } else {
                "Click to create ROI"
            });
        });

        if resp.clicked() {
            clicked_badge = Some(badge);
        }
    }

    if let Some(badge) = clicked_badge {
        let mut nr = NewRoi::new(badge.lo_kev, badge.hi_kev);
        nr.owner_spectrum = state.owned_spectrum.clone();
        if let Some(top) = badge.matches.first() {
            nr.alias = Some(top.nuclide_name.clone());
            nr.emission = Some(top.emission.clone());
        } else {
            nr.alias = Some(format!("{:.1} keV", badge.energy_kev));
        }
        let tag = spectrum_manager.roi_manager.add_roi(nr);
        spectrum_manager.roi_manager.on_roi_change(
            &mut spectrum_manager.spectrum_registry,
            &tag,
            use_chi2,
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_detect_and_match_peaks_finds_cs137_and_k40() {
        let x: Vec<f64> = (0..1024).map(|i| i as f64 * 2.0).collect();
        let mut y = vec![10.0_f64; 1024];
        for (i, &e) in x.iter().enumerate() {
            let cs = 1500.0 * (-0.5 * ((e - 661.7) / 8.0).powi(2)).exp();
            let k = 800.0 * (-0.5 * ((e - 1460.8) / 10.0).powi(2)).exp();
            y[i] += cs + k;
        }

        let lib = NuclideLibrary::load_from_dir(Path::new("resources/nuclide_data"))
            .expect("nuclide_data should load");
        let badges = detect_and_match_peaks("test", &x, &y, 10.0, &lib, 12.0);
        assert!(badges.len() >= 2, "expected at least 2 peaks, got {}", badges.len());

        let cs_badge = badges
            .iter()
            .find(|b| (b.energy_kev - 661.7).abs() < 6.0)
            .expect("Cs-137 peak should be detected");
        assert!(
            cs_badge.matches.iter().any(|m| m.nuclide_name == "Cs137"),
            "expected Cs137 among {:?}",
            cs_badge.matches.iter().map(|m| &m.nuclide_name).collect::<Vec<_>>()
        );

        let k_badge = badges
            .iter()
            .find(|b| (b.energy_kev - 1460.8).abs() < 6.0)
            .expect("K-40 peak should be detected");
        assert!(
            k_badge.matches.iter().any(|m| m.nuclide_name == "K40"),
            "expected K40 among {:?}",
            k_badge.matches.iter().map(|m| &m.nuclide_name).collect::<Vec<_>>()
        );
    }
}
