//! Measure screen: timed foreground/background acquisition, live rate & spectrum
//! plots, and N42 measurement report export.
//!
//! Ported from `luracs/gui/measure_plot.py`.

use std::collections::HashMap;

use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::RenderState;
use ndarray::Array1;
use rsplot::core::items::CurveInterpolation;
use rsplot::{CurveSpec, LineStyle, Plot1D, YAxis};

use crate::app::LuRaCsApp;
use crate::clients::{WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage};
use crate::containers::{GroundMeasurement, MeasurementReport, Spectrum, SpectrumData};
use crate::core::SpectrumManager;
use crate::gui::color_rotator::{ColorRotator, ColorScheme};
use crate::gui::{ConfirmAction, ConfirmModal, InfoModal, Modal, ModalKind};
use crate::settings::SharedSettingsExt;
use crate::utils::N42ReportWriter;

// ---------------------------------------------------------------------------
// Measurement state (Foreground / Background)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MeasureKind {
    #[default]
    Foreground,
    Background,
}

#[derive(Debug, Clone, Default)]
pub struct Measurement {
    pub detector_name: String,
    pub start_time: Option<DateTime<Utc>>,
    pub duration_limit: u64,
    pub measurement_time: f64,
    pub accumulated_counts: f64,
    pub accumulated_dose: f64,
    pub mean_count_rate: f64,
    pub mean_dose_rate: f64,

    pub previous_spectrum: Option<Vec<f64>>,
    pub previous_live_time: Option<f64>,
    pub spectrum: Option<Vec<f64>>,

    pub count_rate_data: Vec<f64>,
    pub dose_rate_data: Vec<f64>,
    pub rate_axis: Vec<f64>,
    pub x_axis: Option<Vec<f64>>,
    pub calibration_coefficients: Option<Vec<f64>>,
    pub stop_timestamps: Vec<DateTime<Utc>>,
}

impl Measurement {
    pub fn set_calibration(&mut self, coeffs: Option<&[f64]>, channels: usize) {
        if let Some(c) = coeffs {
            let x: Vec<f64> = (0..channels)
                .map(|ch| {
                    let xf = ch as f64;
                    c.iter().fold(0.0_f64, |acc, &coef| acc.mul_add(xf, coef))
                })
                .collect();
            self.x_axis = Some(x);
            self.calibration_coefficients = Some(c.to_vec());
        } else {
            self.x_axis = Some((0..channels).map(|ch| ch as f64).collect());
            self.calibration_coefficients = None;
        }
    }

    pub fn update_real_time(&mut self, rt: &WrappedRealTimePackage, spectrum_update_delay: f64) {
        if self.previous_spectrum.is_none() {
            return;
        }

        self.count_rate_data.push(rt.cps);
        self.dose_rate_data.push(rt.dr);
        self.rate_axis.push(self.measurement_time);

        if !self.count_rate_data.is_empty() {
            let sum_cps: f64 = self.count_rate_data.iter().sum();
            self.mean_count_rate = sum_cps / (self.count_rate_data.len() as f64);
        } else {
            self.mean_count_rate = rt.cps;
        }

        if !self.dose_rate_data.is_empty() {
            let valid: Vec<f64> = self
                .dose_rate_data
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .collect();
            if !valid.is_empty() {
                self.mean_dose_rate = valid.iter().sum::<f64>() / (valid.len() as f64);
            }
        } else if rt.dr.is_finite() {
            self.mean_dose_rate = rt.dr;
        }

        if rt.dr.is_finite() {
            self.accumulated_dose += rt.dr * spectrum_update_delay / 3600.0;
        }
    }

    pub fn push_spectrum(&mut self, y_axis: &[f64], live_time: f64) {
        let Some(prev_spec) = &self.previous_spectrum else {
            self.previous_spectrum = Some(y_axis.to_vec());
            self.previous_live_time = Some(live_time);
            if self.spectrum.is_none() {
                self.spectrum = Some(vec![0.0; y_axis.len()]);
            }
            return;
        };

        let acc = self
            .spectrum
            .get_or_insert_with(|| vec![0.0; y_axis.len()]);
        if acc.len() != y_axis.len() {
            acc.resize(y_axis.len(), 0.0);
        }

        for (i, (&curr, &prev)) in y_axis.iter().zip(prev_spec.iter()).enumerate() {
            acc[i] += (curr - prev).max(0.0);
        }

        self.accumulated_counts = acc.iter().sum();
        if let Some(prev_live) = self.previous_live_time {
            self.measurement_time += (live_time - prev_live).max(0.0);
        }

        self.previous_spectrum = Some(y_axis.to_vec());
        self.previous_live_time = Some(live_time);
    }

    #[must_use]
    pub fn to_ground_measurement(&self) -> GroundMeasurement {
        GroundMeasurement {
            detector_name: self.detector_name.clone(),
            start_time: self.start_time,
            measurement_time: self.measurement_time,
            accumulated_counts: self.accumulated_counts,
            accumulated_dose: self.accumulated_dose,
            mean_count_rate: self.mean_count_rate,
            mean_dose_rate: self.mean_dose_rate,
            stop_timestamps: self.stop_timestamps.clone(),
            count_rate_data: self.count_rate_data.clone(),
            dose_rate_data: self.dose_rate_data.clone(),
            rate_axis: self.rate_axis.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Report Form Dialog
// ---------------------------------------------------------------------------

const WIND_DIRECTIONS: [&str; 9] = ["-", "N", "NW", "W", "SW", "S", "SE", "E", "NE"];

#[derive(Debug, Clone, Default)]
pub struct ReportFormDialog {
    pub open: bool,
    pub author_name: String,
    pub location: String,
    pub longitude: f64,
    pub latitude: f64,
    pub weather: String,
    pub wind_direction_idx: usize,
    pub wind_speed: f64,
    pub air_temperature: f64,
    pub remark: String,
}

impl ReportFormDialog {
    pub const fn open(&mut self) {
        self.open = true;
    }

    fn render(&mut self, ui: &egui::Ui) -> bool {
        if !self.open {
            return false;
        }

        let mut submitted = false;
        let mut close_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("measure_report_form_dialog");
        let builder = egui::ViewportBuilder::default()
            .with_title("Measurement Report Details")
            .with_inner_size([430.0, 440.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    let (sub, close) = self.render_contents(ui);
                    submitted |= sub;
                    close_requested |= close;
                });
            });

        if close_requested {
            self.open = false;
        }

        submitted
    }

    pub fn render_contents(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let mut submitted = false;
        let mut close_requested = false;

        ui.heading("Enter additional report information");
        ui.add_space(6.0);

        egui::Grid::new("report_form_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut self.author_name);
                ui.end_row();

                ui.label("Location:");
                ui.text_edit_singleline(&mut self.location);
                ui.end_row();

                ui.label("Longitude (°):");
                ui.add(
                    egui::DragValue::new(&mut self.longitude)
                        .range(-180.0..=180.0)
                        .speed(0.0001)
                        .max_decimals(6),
                );
                ui.end_row();

                ui.label("Latitude (°):");
                ui.add(
                    egui::DragValue::new(&mut self.latitude)
                        .range(-90.0..=90.0)
                        .speed(0.0001)
                        .max_decimals(6),
                );
                ui.end_row();

                ui.label("Weather:");
                ui.text_edit_singleline(&mut self.weather);
                ui.end_row();

                ui.label("Wind Direction:");
                egui::ComboBox::from_id_salt("report_wind_dir")
                    .selected_text(WIND_DIRECTIONS[self.wind_direction_idx])
                    .show_ui(ui, |ui| {
                        for (i, dir) in WIND_DIRECTIONS.iter().enumerate() {
                            ui.selectable_value(&mut self.wind_direction_idx, i, *dir);
                        }
                    });
                ui.end_row();

                ui.label("Wind Speed (m/s):");
                ui.add(
                    egui::DragValue::new(&mut self.wind_speed)
                        .range(0.0..=100.0)
                        .speed(0.1)
                        .max_decimals(1),
                );
                ui.end_row();

                ui.label("Air Temperature (°C):");
                ui.add(
                    egui::DragValue::new(&mut self.air_temperature)
                        .range(-100.0..=100.0)
                        .speed(0.1)
                        .max_decimals(1),
                );
                ui.end_row();

                ui.label("Comments:");
                ui.add(
                    egui::TextEdit::multiline(&mut self.remark)
                        .desired_rows(3)
                        .desired_width(240.0),
                );
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close_requested = true;
            }
            if ui.button("Next →").clicked() {
                submitted = true;
                close_requested = true;
            }
        });

        (submitted, close_requested)
    }
}

// ---------------------------------------------------------------------------
// MeasureScreen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct DetectorStatusSnapshot {
    battery: String,
    state: String,
    connection: String,
}

pub struct MeasureScreen {
    detectors: Vec<String>,
    selected_detector: String,
    statuses: HashMap<String, DetectorStatusSnapshot>,

    hours: u32,
    minutes: u32,
    seconds: u32,
    time_limit_seconds: f64,

    active_kind: MeasureKind,
    measuring: bool,
    fg: Measurement,
    bg: Measurement,
    buffer_real_time: Option<WrappedRealTimePackage>,

    spectrum_plot: Option<Plot1D>,
    count_rate_plot: Option<Plot1D>,
    dose_rate_plot: Option<Plot1D>,
    fg_color: Color32,
    bg_color: Color32,

    report_dialog: ReportFormDialog,
    completed_notified: bool,
}

impl MeasureScreen {
    pub fn new(render_state: &RenderState, scheme: ColorScheme) -> Self {
        let mut spectrum_plot = Plot1D::new(render_state, 0x4001);
        spectrum_plot.set_graph_x_label("Energy [keV]");
        spectrum_plot.set_graph_y_label("Counts per Channel per Second", YAxis::Left);

        let mut count_rate_plot = Plot1D::new(render_state, 0x4002);
        count_rate_plot.set_graph_x_label("Measurement Time [s]");
        count_rate_plot.set_graph_y_label("Count Rate [/s]", YAxis::Left);

        let mut dose_rate_plot = Plot1D::new(render_state, 0x4003);
        dose_rate_plot.set_graph_x_label("Measurement Time [s]");
        dose_rate_plot.set_graph_y_label("Dose Rate [µSv/h]", YAxis::Left);

        Self::with_plots(
            scheme,
            Some(spectrum_plot),
            Some(count_rate_plot),
            Some(dose_rate_plot),
        )
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_headless(scheme: ColorScheme) -> Self {
        Self::with_plots(scheme, None, None, None)
    }

    fn with_plots(
        scheme: ColorScheme,
        spectrum_plot: Option<Plot1D>,
        count_rate_plot: Option<Plot1D>,
        dose_rate_plot: Option<Plot1D>,
    ) -> Self {
        let mut rotator = ColorRotator::new(scheme);
        let fg_color = rotator.next_color();
        let _ = rotator.next_color(); // skip orange, matching Python
        let bg_color = rotator.next_color();

        Self {
            detectors: Vec::new(),
            selected_detector: String::new(),
            statuses: HashMap::new(),
            hours: 0,
            minutes: 0,
            seconds: 0,
            time_limit_seconds: 0.0,
            active_kind: MeasureKind::Foreground,
            measuring: false,
            fg: Measurement::default(),
            bg: Measurement::default(),
            buffer_real_time: None,
            spectrum_plot,
            count_rate_plot,
            dose_rate_plot,
            fg_color,
            bg_color,
            report_dialog: ReportFormDialog::default(),
            completed_notified: false,
        }
    }

    pub fn retheme(&mut self, colors: &crate::theme_manager::ResolvedPlotColors) {
        if let Some(plot) = &mut self.spectrum_plot {
            colors.apply_to(plot);
        }
        if let Some(plot) = &mut self.count_rate_plot {
            colors.apply_to(plot);
        }
        if let Some(plot) = &mut self.dose_rate_plot {
            colors.apply_to(plot);
        }
    }

    const fn active_measurement(&self) -> &Measurement {
        match self.active_kind {
            MeasureKind::Foreground => &self.fg,
            MeasureKind::Background => &self.bg,
        }
    }

    const fn active_measurement_mut(&mut self) -> &mut Measurement {
        match self.active_kind {
            MeasureKind::Foreground => &mut self.fg,
            MeasureKind::Background => &mut self.bg,
        }
    }

    // -----------------------------------------------------------------------
    // Event hooks (called from LuRaCsApp::dispatch_all_events)
    // -----------------------------------------------------------------------

    pub fn on_device_connected(&mut self, name: &str) {
        if !self.detectors.iter().any(|d| d == name) {
            self.detectors.push(name.to_string());
        }
        if self.selected_detector.is_empty() {
            self.selected_detector = name.to_string();
        }
        self.statuses.entry(name.to_string()).or_default();
    }

    pub fn on_device_removed(&mut self, name: &str) {
        self.detectors.retain(|d| d != name);
        self.statuses.remove(name);
        if self.selected_detector == name {
            self.selected_detector = self.detectors.first().cloned().unwrap_or_default();
        }
    }

    pub fn on_status_updated(&mut self, name: &str, status: &WrappedStatusPackage) {
        let entry = self.statuses.entry(name.to_string()).or_default();
        entry.battery = status
            .battery
            .map_or_else(|| "N/A".into(), |b| format!("{b}%"));
        entry.state = status
            .device_state
            .map_or_else(|| "Unknown".into(), |s| format!("{s:?}"));
        entry.connection = status
            .connection_type
            .map_or_else(|| "Unknown".into(), |c| format!("{c:?}"));
    }

    pub fn on_realtime_updated(&mut self, name: &str, rt: &WrappedRealTimePackage) {
        if !self.measuring || name != self.selected_detector {
            return;
        }
        self.buffer_real_time = Some(rt.clone());
    }

    /// Returns `Some((title, message))` if a timed measurement just completed.
    pub fn on_spectrum_updated(
        &mut self,
        name: &str,
        spectrum: &WrappedSpectrumPackage,
        spectrum_manager: &SpectrumManager,
        spectrum_update_delay: f64,
    ) -> Option<(String, String)> {
        if !self.measuring || name != self.selected_detector {
            return None;
        }

        if self.time_limit_seconds > 0.0
            && (self.time_limit_seconds - self.active_measurement().measurement_time) < 0.05
        {
            self.stop_measurement();
            if !self.completed_notified {
                self.completed_notified = true;
                return Some((
                    "Info".into(),
                    "Measurement time reached. Increase the measurement duration to continue."
                        .into(),
                ));
            }
            return None;
        }

        let mut calib_from_mgr: Option<(Option<Vec<f64>>, usize)> = None;
        if self.active_measurement().x_axis.is_none() {
            if spectrum.calib_coeff.is_none()
                && let Some(mgr_spec) = spectrum_manager.get_spectrum(name)
                && mgr_spec.calibrated
            {
                calib_from_mgr = Some((
                    mgr_spec.calibration_coefficients.clone(),
                    mgr_spec.channels,
                ));
            } else {
                calib_from_mgr = Some((spectrum.calib_coeff.clone(), spectrum.y_axis.len()));
            }
        }

        let rt_buf = self.buffer_real_time.clone();
        let m = self.active_measurement_mut();
        if m.measurement_time == 0.0 && m.start_time.is_none() {
            m.start_time = Some(Utc::now());
        }
        if let Some((coeffs, ch)) = calib_from_mgr {
            m.set_calibration(coeffs.as_deref(), ch);
        }
        if let Some(rt) = &rt_buf {
            m.update_real_time(rt, spectrum_update_delay);
        }
        if let Some(slice) = spectrum.y_axis.as_slice() {
            m.push_spectrum(slice, spectrum.live_time);
        } else {
            let owned: Vec<f64> = spectrum.y_axis.iter().copied().collect();
            m.push_spectrum(&owned, spectrum.live_time);
        }

        if self.time_limit_seconds > 0.0
            && (self.time_limit_seconds - self.active_measurement().measurement_time) < 0.05
        {
            self.stop_measurement();
            if !self.completed_notified {
                self.completed_notified = true;
                return Some((
                    "Measurement Completed".into(),
                    "The measurement has completed successfully.\nData collection has stopped because the configured collection time was reached.".into(),
                ));
            }
        }

        None
    }

    // -----------------------------------------------------------------------
    // Controls
    // -----------------------------------------------------------------------

    pub fn stop_measurement(&mut self) {
        self.measuring = false;
        let det = self.selected_detector.clone();
        let m = self.active_measurement_mut();
        if m.previous_spectrum.is_some() {
            m.stop_timestamps.push(Utc::now());
        }
        m.detector_name = det;
        m.previous_spectrum = None;
        m.previous_live_time = None;
    }

    pub fn clear_active_measurement(&mut self) {
        self.measuring = false;
        self.completed_notified = false;
        match self.active_kind {
            MeasureKind::Foreground => self.fg = Measurement::default(),
            MeasureKind::Background => self.bg = Measurement::default(),
        }
    }

    pub fn reset_all_measurements(&mut self) {
        self.measuring = false;
        self.completed_notified = false;
        self.fg = Measurement::default();
        self.bg = Measurement::default();
    }

    fn confirm_time(&mut self) {
        let total = u64::from(self.hours) * 3600
            + u64::from(self.minutes) * 60
            + u64::from(self.seconds);
        self.time_limit_seconds = total as f64;
        self.completed_notified = false;
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    pub fn render_screen(
        &mut self,
        spectrum_update_delay: f64,
        ui: &mut egui::Ui,
    ) -> (Option<String>, bool, bool, bool) {
        let mut pending_detector_change: Option<String> = None;
        let mut request_start = false;
        let mut request_clear = false;
        let mut request_report = false;

        // --- 1. Top Detector Status Row ---
        ui.horizontal(|ui| {
            ui.label("Detector:");
            let current_det = self.selected_detector.clone();
            let mut chosen = current_det.clone();
            egui::ComboBox::from_id_salt("measure_detector_combo")
                .width(180.0)
                .selected_text(if current_det.is_empty() {
                    "(none)"
                } else {
                    &current_det
                })
                .show_ui(ui, |ui| {
                    for det in &self.detectors {
                        ui.selectable_value(&mut chosen, det.clone(), det);
                    }
                });
            if chosen != current_det {
                pending_detector_change = Some(chosen);
            }

            let mut snap = self
                .statuses
                .get(&self.selected_detector)
                .cloned()
                .unwrap_or_default();

            ui.label("Battery:");
            ui.add(
                egui::TextEdit::singleline(&mut snap.battery)
                    .desired_width(60.0)
                    .interactive(false),
            );
            ui.label("Status:");
            ui.add(
                egui::TextEdit::singleline(&mut snap.state)
                    .desired_width(100.0)
                    .interactive(false),
            );
            ui.label("Connection:");
            ui.add(
                egui::TextEdit::singleline(&mut snap.connection)
                    .desired_width(80.0)
                    .interactive(false),
            );
        });

        // --- 2. Timer Row ---
        ui.horizontal(|ui| {
            ui.strong("Timer");
            ui.label("Hours:");
            ui.add(egui::DragValue::new(&mut self.hours).range(0..=100));
            ui.label("Minutes:");
            ui.add(egui::DragValue::new(&mut self.minutes).range(0..=59));
            ui.label("Seconds:");
            ui.add(egui::DragValue::new(&mut self.seconds).range(0..=59));

            if ui.button("Confirm Time").clicked() {
                self.confirm_time();
            }

            let elapsed = self.active_measurement().measurement_time;
            let limit = self.time_limit_seconds;
            let (fraction, text) = if limit > 0.0 {
                let rem = (limit - elapsed).max(0.0).round() as u64;
                (
                    (elapsed / limit).clamp(0.0, 1.0) as f32,
                    format!("Remaining: {}", format_hms(rem)),
                )
            } else {
                (0.0, "Remaining: 00:00:00".to_string())
            };
            ui.add(egui::ProgressBar::new(fraction).text(text));
        });

        // --- 3. Controls Row ---
        ui.horizontal(|ui| {
            ui.strong("Controls");

            let prev_kind = self.active_kind;
            ui.radio_value(&mut self.active_kind, MeasureKind::Foreground, "Foreground");
            ui.radio_value(&mut self.active_kind, MeasureKind::Background, "Background");
            if self.active_kind != prev_kind {
                let new_kind = self.active_kind;
                self.active_kind = prev_kind;
                self.stop_measurement();
                self.active_kind = new_kind;
            }

            let start_label = if self.measuring {
                "Measuring..."
            } else {
                "Start Measurement"
            };
            if ui
                .add_enabled(!self.measuring, egui::Button::new(start_label))
                .clicked()
            {
                request_start = true;
            }

            if ui
                .add_enabled(self.measuring, egui::Button::new("Stop Measurement"))
                .clicked()
            {
                self.stop_measurement();
            }

            if ui.button("Clear Measurement").clicked() {
                request_clear = true;
            }

            if ui.button("Make Report").clicked() {
                request_report = true;
            }
        });

        ui.separator();

        // --- 4. Central 2x2 Grid ---
        let avail = ui.available_size();
        let col_w = ((avail.x - 16.0) / 2.0).max(200.0);
        let row_h = ((avail.y - 12.0) / 2.0).max(200.0);

        egui::ScrollArea::vertical()
            .id_salt("measure_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Top-left: Summary text
                    ui.allocate_ui_with_layout(
                        egui::vec2(col_w, row_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.set_max_height(row_h);
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                let inner_h = (row_h - 16.0).max(40.0);
                                ui.set_min_size(egui::vec2((col_w - 16.0).max(120.0), inner_h));
                                egui::ScrollArea::vertical()
                                    .id_salt("measure_summary_scroll")
                                    .max_height(inner_h)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.monospace(format_summary(
                                            &self.fg,
                                            &self.bg,
                                            spectrum_update_delay,
                                        ));
                                    });
                            });
                        },
                    );

                    // Top-right: Measured Spectrum Plot
                    ui.allocate_ui_with_layout(
                        egui::vec2(col_w, row_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            draw_spectrum_plot(
                                &mut self.spectrum_plot,
                                &self.fg,
                                &self.bg,
                                self.fg_color,
                                self.bg_color,
                                ui,
                            );
                        },
                    );
                });

                ui.horizontal(|ui| {
                    // Bottom-left: Count Rate Plot
                    ui.allocate_ui_with_layout(
                        egui::vec2(col_w, row_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            draw_rate_plot(
                                &mut self.count_rate_plot,
                                &self.fg.rate_axis,
                                &self.fg.count_rate_data,
                                &self.bg.rate_axis,
                                &self.bg.count_rate_data,
                                self.fg_color,
                                self.bg_color,
                                spectrum_update_delay,
                                ui,
                            );
                        },
                    );

                    // Bottom-right: Dose Rate Plot
                    ui.allocate_ui_with_layout(
                        egui::vec2(col_w, row_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            draw_rate_plot(
                                &mut self.dose_rate_plot,
                                &self.fg.rate_axis,
                                &self.fg.dose_rate_data,
                                &self.bg.rate_axis,
                                &self.bg.dose_rate_data,
                                self.fg_color,
                                self.bg_color,
                                spectrum_update_delay,
                                ui,
                            );
                        },
                    );
                });
            });

        (
            pending_detector_change,
            request_start,
            request_clear,
            request_report,
        )
    }

    pub fn render(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        let spectrum_update_delay = app
            .settings
            .read_settings()
            .scanning
            .spectrum_update_delay_s
            .max(0.05);

        let (pending_detector_change, request_start, request_clear, request_report) =
            app.measure_screen.render_screen(spectrum_update_delay, ui);

        // --- Deferred Actions & Modals ---
        if let Some(new_det) = pending_detector_change {
            if app.measure_screen.fg.measurement_time > 0.0
                || app.measure_screen.bg.measurement_time > 0.0
            {
                app.modal_stack.push(Modal::root(ModalKind::Confirm(
                    ConfirmModal::new(
                        "Change Detector",
                        "Changing detector will reset all previous measurements and all unsaved data will be lost!",
                        ConfirmAction::Custom(Box::new(move |app| {
                            app.measure_screen.reset_all_measurements();
                            app.measure_screen.selected_detector = new_det;
                        })),
                    ),
                )));
            } else {
                app.measure_screen.selected_detector = new_det;
            }
        }

        if request_start {
            if app.measure_screen.selected_detector.is_empty() {
                app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
                    "No Detector",
                    "The measurement could not be started because no detector is connected.",
                ))));
            } else if app.measure_screen.time_limit_seconds == 0.0 {
                app.modal_stack.push(Modal::root(ModalKind::Confirm(
                    ConfirmModal::new(
                        "Confirm",
                        "No time limit for the measurement has been set. Are you sure you want to continue?",
                        ConfirmAction::Custom(Box::new(|app| {
                            app.measure_screen.completed_notified = false;
                            app.measure_screen.measuring = true;
                        })),
                    ),
                )));
            } else {
                app.measure_screen.completed_notified = false;
                app.measure_screen.measuring = true;
            }
        }

        if request_clear {
            app.modal_stack.push(Modal::root(ModalKind::Confirm(
                ConfirmModal::new(
                    "Confirm",
                    "Do you want to clear the current measurement? All acquired data will be lost.",
                    ConfirmAction::Custom(Box::new(|app| {
                        app.measure_screen.clear_active_measurement();
                    })),
                ),
            )));
        }

        if request_report {
            if app.measure_screen.active_measurement().measurement_time <= 0.0 {
                app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
                    "No Measurement",
                    "No measurement to report on.",
                ))));
            } else {
                app.measure_screen.report_dialog.open();
            }
        }

        if app.measure_screen.report_dialog.render(ui) {
            export_measurement_report(app, ui.ctx());
        }
    }
}

// ---------------------------------------------------------------------------
// Plotting & Formatting Helpers
// ---------------------------------------------------------------------------

fn draw_spectrum_plot(
    plot_opt: &mut Option<Plot1D>,
    fg: &Measurement,
    bg: &Measurement,
    fg_color: Color32,
    bg_color: Color32,
    ui: &mut egui::Ui,
) {
    let Some(plot) = plot_opt else {
        return;
    };
    plot.clear();

    for (label, m, color) in [("Foreground", fg, fg_color), ("Background", bg, bg_color)] {
        if let (Some(x), Some(y)) = (&m.x_axis, &m.spectrum)
            && !x.is_empty()
            && x.len() == y.len()
        {
            let div = m.measurement_time.max(1.0);
            let scaled: Vec<f64> = y.iter().map(|&v| v / div).collect();

            let fill_color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 80);
            let fill_h = plot.add_curve(x, &scaled, fill_color);
            let mut fill_spec = CurveSpec::new(x, &scaled, fill_color);
            fill_spec.line_style = LineStyle::None;
            fill_spec.fill = true;
            fill_spec.interpolation = CurveInterpolation::StepPost;
            plot.update_curve_spec(fill_h, fill_spec);

            plot.add_curve_with_legend_and_interpolation(
                x,
                &scaled,
                color,
                label,
                CurveInterpolation::StepPost,
            );
        }
    }

    plot.show(ui);
}

#[allow(clippy::too_many_arguments)]
fn draw_rate_plot(
    plot_opt: &mut Option<Plot1D>,
    fg_x: &[f64],
    fg_y: &[f64],
    bg_x: &[f64],
    bg_y: &[f64],
    fg_color: Color32,
    bg_color: Color32,
    delay: f64,
    ui: &mut egui::Ui,
) {
    let Some(plot) = plot_opt else {
        return;
    };
    plot.clear();

    for (label, xs, ys, color) in [
        ("Foreground", fg_x, fg_y, fg_color),
        ("Background", bg_x, bg_y, bg_color),
    ] {
        let pairs: Vec<(f64, f64)> = xs
            .iter()
            .zip(ys.iter())
            .filter(|(_, y)| y.is_finite())
            .map(|(&x, &y)| (x + delay, y))
            .collect();
        if !pairs.is_empty() {
            let px: Vec<f64> = pairs.iter().map(|p| p.0).collect();
            let py: Vec<f64> = pairs.iter().map(|p| p.1).collect();
            plot.add_curve_with_legend_and_interpolation(
                &px,
                &py,
                color,
                label,
                CurveInterpolation::Linear,
            );
        }
    }

    plot.show(ui);
}

#[must_use]
pub fn format_hms(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

#[must_use]
pub fn format_summary(fg: &Measurement, bg: &Measurement, spectrum_update_delay: f64) -> String {
    let rate = 1.0 / spectrum_update_delay.max(1e-4);
    format!(
        "Detector: {}\n\
         --- Foreground Summary ---\n\
         Measurement Rate: {:.2}/s\n\
         Measurement Time: {}\n\n\
         Total Counts: {}\n\
         Mean Count Rate: {:.2} /s\n\
         Total Dose: {:.4} µSv\n\
         Mean Dose Rate: {:.3} µSv/h\n\n\
         --- Background Summary ---\n\
         Measurement Rate: {:.2}/s\n\
         Measurement Time: {}\n\n\
         Total Counts: {}\n\
         Mean Count Rate: {:.2} /s\n\
         Total Dose: {:.4} µSv\n\
         Mean Dose Rate: {:.3} µSv/h\n",
        fg.detector_name,
        rate,
        format_hms(fg.measurement_time.round() as u64),
        fg.accumulated_counts.round() as u64,
        fg.mean_count_rate,
        fg.accumulated_dose,
        fg.mean_dose_rate,
        rate,
        format_hms(bg.measurement_time.round() as u64),
        bg.accumulated_counts.round() as u64,
        bg.mean_count_rate,
        bg.accumulated_dose,
        bg.mean_dose_rate,
    )
}

fn export_measurement_report(app: &mut LuRaCsApp, ctx: &egui::Context) {
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    let default_name = format!("MeasurementReport_{timestamp}.xml");

    let mut dialog = rfd::FileDialog::new()
        .add_filter("XML Report", &["xml"])
        .set_file_name(&default_name);

    let last_dir = app.settings.read_settings().paths.last_opened_dir.clone();
    if last_dir.is_dir() {
        dialog = dialog.set_directory(&last_dir);
    }

    app.file_dialogs.save_file(ctx, dialog, |app, path| {
        let Some(path) = path else {
            return;
        };
        finish_export_measurement_report(app, path);
    });
}

fn finish_export_measurement_report(app: &mut LuRaCsApp, path: std::path::PathBuf) {

    if let Some(parent) = path.parent() {
        app.settings.write_settings().paths.last_opened_dir = parent.to_path_buf();
    }

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("MeasurementReport")
        .to_string();

    let fg_y = app.measure_screen.fg.spectrum.clone().unwrap_or_default();
    let channels = fg_y.len().max(
        app.measure_screen
            .bg
            .spectrum
            .as_ref()
            .map_or(0, Vec::len),
    );
    if channels == 0 {
        app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
            "Report Error",
            "No spectrum channels recorded to export.",
        ))));
        return;
    }

    let mut spec = Spectrum::new(channels, &stem);
    spec.instrument_id = Some(app.measure_screen.fg.detector_name.clone());

    if !fg_y.is_empty() {
        spec.set_foreground(
            SpectrumData {
                y_axis: Array1::from_vec(fg_y),
                channels,
                total_counts: app.measure_screen.fg.accumulated_counts.round() as u64,
                live_time: app.measure_screen.fg.measurement_time,
                real_time: None,
                avg_dose_rate: Some(app.measure_screen.fg.mean_dose_rate),
                avg_cps: Some(app.measure_screen.fg.mean_count_rate),
                start_date: app.measure_screen.fg.start_time,
                end_date: None,
                spectrum_name: Some(stem.clone()),
                instrument: None,
            },
            None,
        );
    }

    if app.measure_screen.bg.measurement_time > 0.0
        && let Some(bg_y) = app.measure_screen.bg.spectrum.clone()
    {
        spec.set_background(
            SpectrumData {
                y_axis: Array1::from_vec(bg_y),
                channels,
                total_counts: app.measure_screen.bg.accumulated_counts.round() as u64,
                live_time: app.measure_screen.bg.measurement_time,
                real_time: None,
                avg_dose_rate: Some(app.measure_screen.bg.mean_dose_rate),
                avg_cps: Some(app.measure_screen.bg.mean_count_rate),
                start_date: app.measure_screen.bg.start_time,
                end_date: None,
                spectrum_name: Some(stem),
                instrument: None,
            },
            None,
        );
    }

    if let Some(cal) = app
        .measure_screen
        .fg
        .calibration_coefficients
        .clone()
        .or_else(|| app.measure_screen.bg.calibration_coefficients.clone())
    {
        spec.apply_calibration(cal);
    }

    let form = &app.measure_screen.report_dialog;
    let report = MeasurementReport {
        date: Utc::now(),
        author_name: form.author_name.clone(),
        location: form.location.clone(),
        longitude: form.longitude,
        latitude: form.latitude,
        weather: form.weather.clone(),
        wind_direction: WIND_DIRECTIONS[form.wind_direction_idx].to_string(),
        wind_speed: form.wind_speed,
        air_temperature: form.air_temperature,
        remark: form.remark.replace('\n', "; ").trim().to_string(),
        fg_measurement: app.measure_screen.fg.to_ground_measurement(),
        bg_measurement: app.measure_screen.bg.to_ground_measurement(),
    };

    match N42ReportWriter::new(&spec, &path, &report).and_then(|w| w.write_out()) {
        Ok(()) => {
            app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
                "Report Exported",
                format!("Saved measurement report to {}", path.display()),
            ))));
        }
        Err(e) => {
            app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
                "Report Export Failed",
                format!("Failed to write report: {e}"),
            ))));
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measurement_accumulation_and_rates() {
        let mut m = Measurement::default();
        m.set_calibration(Some(&[0.0, 2.0, 10.0]), 4);
        assert_eq!(m.x_axis.as_ref().unwrap(), &vec![10.0, 12.0, 14.0, 16.0]);

        // First spectrum establishes baseline
        m.push_spectrum(&[10.0, 20.0, 30.0, 40.0], 5.0);
        assert_eq!(m.measurement_time, 0.0);
        assert_eq!(m.accumulated_counts, 0.0);

        // Real-time update after baseline
        let rt = WrappedRealTimePackage {
            cps: 100.0,
            dr: 0.36,
            ..Default::default()
        };
        m.update_real_time(&rt, 1.0);
        assert!((m.mean_count_rate - 100.0).abs() < 1e-6);
        assert!((m.accumulated_dose - 0.0001).abs() < 1e-9);

        // Second spectrum adds differential counts and livetime
        m.push_spectrum(&[15.0, 30.0, 45.0, 60.0], 7.0);
        assert!((m.measurement_time - 2.0).abs() < 1e-6);
        assert!((m.accumulated_counts - 50.0).abs() < 1e-6);
        assert_eq!(m.spectrum.as_ref().unwrap(), &vec![5.0, 10.0, 15.0, 20.0]);
    }

    #[test]
    fn test_format_hms() {
        assert_eq!(format_hms(0), "00:00:00");
        assert_eq!(format_hms(3661), "01:01:01");
    }

    #[test]
    fn test_measure_screen_hooks_and_timed_completion() {
        use std::path::Path;

        let dir = std::env::temp_dir().join("luracs_measure_hooks_sm");
        drop(std::fs::create_dir_all(&dir));
        let sm = SpectrumManager::new(
            ColorScheme::default(),
            dir,
            Path::new("resources/nuclide_data"),
        )
        .unwrap();

        let mut ms = MeasureScreen::new_headless(ColorScheme::default());
        ms.on_device_connected("Det1");
        ms.on_device_connected("Det2");
        assert_eq!(ms.selected_detector, "Det1");

        ms.on_status_updated(
            "Det1",
            &WrappedStatusPackage {
                battery: Some(85),
                ..Default::default()
            },
        );
        assert_eq!(ms.statuses["Det1"].battery, "85%");

        ms.hours = 0;
        ms.minutes = 0;
        ms.seconds = 2;
        ms.confirm_time();
        assert!((ms.time_limit_seconds - 2.0).abs() < 1e-6);

        ms.measuring = true;
        ms.on_realtime_updated(
            "Det1",
            &WrappedRealTimePackage {
                cps: 12.0,
                dr: 0.25,
                ..Default::default()
            },
        );

        let pkg1 = WrappedSpectrumPackage {
            y_axis: Array1::from_vec(vec![1.0, 2.0, 3.0]),
            live_time: 1.0,
            calib_coeff: Some(vec![0.0, 1.0]),
            ..Default::default()
        };
        assert!(ms.on_spectrum_updated("Det1", &pkg1, &sm, 1.0).is_none());

        let pkg2 = WrappedSpectrumPackage {
            y_axis: Array1::from_vec(vec![3.0, 5.0, 8.0]),
            live_time: 3.0,
            calib_coeff: None,
            ..Default::default()
        };
        let done = ms.on_spectrum_updated("Det1", &pkg2, &sm, 1.0);
        assert!(done.is_some());
        assert!(!ms.measuring);

        let gm = ms.fg.to_ground_measurement();
        assert!((gm.accumulated_counts - 10.0).abs() < 1e-6);
        let summary = format_summary(&ms.fg, &ms.bg, 1.0);
        assert!(summary.contains("Foreground Summary"));

        ms.clear_active_measurement();
        assert_eq!(ms.fg.measurement_time, 0.0);
        ms.reset_all_measurements();
        ms.on_device_removed("Det1");
        assert_eq!(ms.selected_detector, "Det2");
    }

    #[test]
    fn test_kittest_report_dialog_and_measure_screen() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        // 1. ReportFormDialog closed noop & open form
        let mut dlg = ReportFormDialog::default();
        let mut closed = Harness::builder().build_ui(|ui| {
            assert!(!dlg.render(ui));
        });
        closed.run();
        drop(closed);

        dlg.open();
        struct DlgState {
            dlg: ReportFormDialog,
            submitted: bool,
            closed: bool,
        }
        let mut dlg_harness = Harness::builder()
            .with_size(egui::vec2(450.0, 460.0))
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, st: &mut DlgState| {
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    let (sub, c) = st.dlg.render_contents(ui);
                    st.submitted |= sub;
                    st.closed |= c;
                },
                DlgState {
                    dlg,
                    submitted: false,
                    closed: false,
                },
            );
        dlg_harness.run();
        dlg_harness.get_by_label("Next →").click();
        dlg_harness.run();
        assert!(dlg_harness.state().submitted);
        assert!(dlg_harness.state().closed);

        // 2. MeasureScreen::render_screen UI
        let mut ms = MeasureScreen::new_headless(ColorScheme::default());
        ms.on_device_connected("DetA");
        ms.on_status_updated(
            "DetA",
            &WrappedStatusPackage {
                battery: Some(90),
                ..Default::default()
            },
        );
        ms.fg.measurement_time = 10.0;
        ms.fg.accumulated_counts = 100.0;
        ms.fg.set_calibration(None, 4);
        ms.fg.spectrum = Some(vec![10.0, 20.0, 30.0, 40.0]);
        ms.fg.rate_axis = vec![1.0, 2.0];
        ms.fg.count_rate_data = vec![50.0, 50.0];
        ms.fg.dose_rate_data = vec![0.1, 0.12];

        struct MsState {
            ms: MeasureScreen,
            start: bool,
            clear: bool,
            report: bool,
        }
        let mut ms_harness = Harness::builder()
            .with_size(egui::vec2(920.0, 640.0))
            .with_accessibility_check(false)
            .build_ui_state(
                |ui, st: &mut MsState| {
                    ui.ctx()
                        .set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
                    let (_, start, clear, report) = st.ms.render_screen(1.0, ui);
                    st.start |= start;
                    st.clear |= clear;
                    st.report |= report;
                },
                MsState {
                    ms,
                    start: false,
                    clear: false,
                    report: false,
                },
            );
        ms_harness.run();

        ms_harness.get_by_label("Confirm Time").click();
        ms_harness.run();
        ms_harness.get_by_label("Start Measurement").click();
        ms_harness.run();
        assert!(ms_harness.state().start);

        ms_harness.state_mut().ms.measuring = true;
        ms_harness.run();
        ms_harness.get_by_label("Stop Measurement").click();
        ms_harness.run();
        assert!(!ms_harness.state().ms.measuring);

        ms_harness.get_by_label("Background").click();
        ms_harness.run();
        assert_eq!(ms_harness.state().ms.active_kind, MeasureKind::Background);

        ms_harness.get_by_label("Clear Measurement").click();
        ms_harness.run();
        assert!(ms_harness.state().clear);

        ms_harness.get_by_label("Make Report").click();
        ms_harness.run();
        assert!(ms_harness.state().report);
    }
}
