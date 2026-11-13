//! Deconvolution window.
//!
//! Port of `luracs.gui.deconvolution_window.DeconvolutionWindow`. Renders as a
//! floating `egui::Window` owned by `LuRaCsApp`. The window checks its own
//! `open` flag; `render` is a no-op when `open == false`.
//!
//! ## Algorithm backends
//!
//! Both algorithms run on a background thread and report progress via an
//! `mpsc` channel of [`Event`]s drained each frame.
//!
//! - **Richardson-Lucy (RL)**: builds a Gaussian PSF from the instrument's
//!   `resolution_params[0]` (the *k* constant), then runs `richardson_lucy`.
//! - **ML-EM**: loads a `.npz` response matrix on the UI thread (matching the
//!   Python flow), then runs `ml_em` on a worker thread.
//!
//! ## Integration
//!
//! Follows the same contract as every other real window:
//! - Field `pub open: bool` — toggled by the menu bar.
//! - `render(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui)` — called from
//!   `LuRaCsApp::render_windows`; exits immediately when `open == false`.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use eframe::egui::{Align2, Color32, Id, Vec2};
use nalgebra::DVector;
use ndarray::{Array1, Array2};
use ndarray_npy::NpzReader;

use crate::app::LuRaCsApp;
use crate::utils::{MlEmOptions, ml_em, process_matrix, richardson_lucy};

// ===========================================================================
// Worker events
// ===========================================================================

#[derive(Debug)]
enum Event {
    Progress { percent: u8, message: String },
    Done(DVector<f64>),
    Error(String),
}

// ===========================================================================
// Algorithm selector
// ===========================================================================

#[derive(Copy, Clone, PartialEq, Eq)]
enum Algorithm {
    Rl,
    Mlem,
}

// ===========================================================================
// Pending confirmation (large spectrum warning)
// ===========================================================================

struct PendingConfirmation {
    algorithm: Algorithm,
    channels: usize,
}

// ===========================================================================
// DeconvolutionWindow
// ===========================================================================

pub struct DeconvolutionWindow {
    pub open: bool,

    // Algorithm selection
    algorithm: Algorithm,

    // RL params
    rl_iterations: i32,
    rl_use_resolution: bool,
    rl_use_efficiency: bool,

    // MLEM params
    mlem_iterations: i32,
    mlem_response_path: String,

    // Cached combo lists (refreshed when `lists_dirty`)
    instruments: Vec<(String, String)>,
    spectra: Vec<(String, String)>,
    selected_instrument: Option<String>,
    selected_spectrum: Option<String>,
    lists_dirty: bool,

    // Common
    subtract_background: bool,

    // Progress
    progress_percent: u8,
    progress_message: String,

    // Async plumbing
    rx: Option<Receiver<Event>>,
    pending_source_key: Option<String>,
    is_calculating: bool,

    // Deferred calculate request (set in draw, acted on in render)
    calculate_requested: bool,
    // Set by the confirmation modal to carry the confirmed algorithm forward
    confirmed_algorithm: Option<Algorithm>,

    // Local modals (not routed through the app modal stack because they are
    // tightly coupled to the in-progress calculation state)
    pending_confirmation: Option<PendingConfirmation>,
    error_message: Option<String>,
}

impl DeconvolutionWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            algorithm: Algorithm::Mlem,
            rl_iterations: 30,
            rl_use_resolution: true,
            rl_use_efficiency: false,
            mlem_iterations: 150,
            mlem_response_path: String::new(),
            instruments: Vec::new(),
            spectra: Vec::new(),
            selected_instrument: None,
            selected_spectrum: None,
            lists_dirty: true,
            subtract_background: false,
            progress_percent: 0,
            progress_message: String::new(),
            rx: None,
            pending_source_key: None,
            is_calculating: false,
            calculate_requested: false,
            confirmed_algorithm: None,
            pending_confirmation: None,
            error_message: None,
        }
    }

    /// Pre-fill the last-used response-matrix path from persisted settings.
    /// Call once after `Settings::load`.
    pub fn apply_settings(&mut self, app: &LuRaCsApp) {
        let settings = app.settings.read().unwrap();
        self.mlem_response_path = settings.state.deconv_last_response_file.clone();
    }

    // -----------------------------------------------------------------------
    // Public entry point
    // -----------------------------------------------------------------------

    /// Called every frame from `LuRaCsApp::render_windows`.
    pub fn render(&mut self, app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        if !self.open {
            return;
        }

        if self.lists_dirty {
            self.refresh_lists(app);
            self.lists_dirty = false;
        }

        self.poll_events(app);

        // Act on the Calculate button click (deferred from the previous frame's
        // draw because egui's Grid closure captures `&mut self` exclusively).
        if self.calculate_requested {
            self.calculate_requested = false;
            self.on_calculate(app, ui.ctx());
        }

        // Act on a confirmed large-spectrum calculation.
        if let Some(algo) = self.confirmed_algorithm.take() {
            self.dispatch_calculation(app, ui.ctx(), algo);
        }

        let modal_open = self.pending_confirmation.is_some() || self.error_message.is_some();

        let mut open = self.open;
        egui::Window::new("Deconvolution")
            .open(&mut open)
            .default_size([520.0, 320.0])
            .resizable(true)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                ui.add_enabled_ui(!modal_open, |ui| {
                    self.draw_main_ui(ui);
                });
                self.draw_modal(ui.ctx());
            });
        self.open = open;

        if self.is_calculating {
            ui.ctx().request_repaint();
        }
    }

    // -----------------------------------------------------------------------
    // List refresh
    // -----------------------------------------------------------------------

    fn refresh_lists(&mut self, app: &LuRaCsApp) {
        let mut instruments: Vec<(String, String)> = app
            .spectrum_manager
            .generic_instrument_library
            .iter()
            .map(|(k, inst)| (k.clone(), inst.name_or_model().to_string()))
            .collect();
        instruments.extend(
            app.spectrum_manager
                .unique_instrument_library
                .iter()
                .map(|(k, inst)| (k.clone(), inst.name_or_model().to_string())),
        );
        self.instruments = instruments;

        let mut spectra: Vec<(String, String)> = app
            .spectrum_manager
            .spectrum_registry
            .keys()
            .map(|k| (k.clone(), k.clone()))
            .collect();
        spectra.sort_by(|(a, _), (b, _)| a.cmp(b));
        self.spectra = spectra;

        if self.selected_spectrum.is_none() {
            self.selected_spectrum = self.spectra.first().map(|(k, _)| k.clone());
        }
        if self.selected_instrument.is_none() {
            self.selected_instrument = self.instruments.first().map(|(k, _)| k.clone());
        }
    }

    // -----------------------------------------------------------------------
    // Event polling
    // -----------------------------------------------------------------------

    fn poll_events(&mut self, app: &mut LuRaCsApp) {
        let Some(rx) = self.rx.take() else { return };
        let mut keep = true;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Progress { percent, message } => {
                    self.progress_percent = percent;
                    self.progress_message = message;
                }
                Event::Done(result) => {
                    let key = self.pending_source_key.take();
                    self.is_calculating = false;
                    keep = false;
                    if let Some(key) = key {
                        self.catch_result(app, &key, result);
                    }
                }
                Event::Error(msg) => {
                    self.is_calculating = false;
                    keep = false;
                    self.set_error(msg);
                }
            }
        }
        if keep {
            self.rx = Some(rx);
        }
    }

    // -----------------------------------------------------------------------
    // Result handling
    // -----------------------------------------------------------------------

    fn catch_result(&mut self, app: &mut LuRaCsApp, source_key: &str, data: DVector<f64>) {
        use crate::containers::{Spectrum, SpectrumData};
        use crate::core::ForegroundInput;

        let deconv_name = format!("{source_key}_deconvolved");
        let n = data.len();

        let y_array: Array1<f64> = Array1::from_iter(data.iter().copied());

        let (channels, x_axis) = app
            .spectrum_manager
            .spectrum_registry
            .get(source_key)
            .map(|s| (s.channels, s.x_axis.clone()))
            .unwrap_or_else(|| (n, Array1::from_iter((0..n).map(|i| i as f64))));

        let sd = SpectrumData {
            y_axis: y_array,
            channels,
            total_counts: 0,
            live_time: 1.0,
            real_time: None,
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: Some(deconv_name.clone()),
            instrument: None,
        };

        // Create the spectrum slot, then set its foreground data and x-axis.
        match app
            .spectrum_manager
            .create_spectrum(&deconv_name, channels, None, false)
        {
            Ok(_) => {
                // Overwrite the x-axis (create_spectrum uses channel indices).
                if let Some(spec) = app.spectrum_manager.spectrum_registry.get_mut(&deconv_name) {
                    spec.x_axis = x_axis;
                }
                if let Err(e) = app
                    .spectrum_manager
                    .set_foreground_spectrum(&deconv_name, ForegroundInput::Raw(sd))
                {
                    self.set_error(format!("Could not set deconvolved foreground: {e}"));
                }
            }
            Err(e) => {
                self.set_error(format!("Could not create deconvolved spectrum slot: {e}"));
            }
        }

        self.lists_dirty = true;
    }

    // -----------------------------------------------------------------------
    // Error helper
    // -----------------------------------------------------------------------

    fn set_error(&mut self, msg: impl Into<String>) {
        self.error_message = Some(msg.into());
    }

    // -----------------------------------------------------------------------
    // Validation
    // -----------------------------------------------------------------------

    fn validate_spectrum(&mut self, app: &LuRaCsApp) -> Option<(String, usize)> {
        let key = self.selected_spectrum.clone().or_else(|| {
            self.set_error("No spectrum selected");
            None
        })?;
        let spec = app
            .spectrum_manager
            .spectrum_registry
            .get(&key)
            .or_else(|| {
                self.set_error(format!("Spectrum '{key}' not found"));
                None
            })?;
        if self.subtract_background && spec.background.is_none() {
            self.set_error("Selected spectrum has no background to subtract. Aborting.");
            return None;
        }
        Some((key, spec.channels))
    }

    // -----------------------------------------------------------------------
    // Calculate dispatch
    // -----------------------------------------------------------------------

    fn on_calculate(&mut self, app: &mut LuRaCsApp, ctx: &egui::Context) {
        let (_, channels) = match self.validate_spectrum(app) {
            Some(v) => v,
            None => return,
        };
        if channels > 5000 {
            self.pending_confirmation = Some(PendingConfirmation {
                algorithm: self.algorithm,
                channels,
            });
            return;
        }
        self.dispatch_calculation(app, ctx, self.algorithm);
    }

    fn dispatch_calculation(&mut self, app: &mut LuRaCsApp, ctx: &egui::Context, algo: Algorithm) {
        match algo {
            Algorithm::Rl => self.calculate_rl(app, ctx),
            Algorithm::Mlem => self.calculate_mlem(app, ctx),
        }
    }

    fn calculate_rl(&mut self, app: &mut LuRaCsApp, ctx: &egui::Context) {
        let source_key = match self.selected_spectrum.clone() {
            Some(k) => k,
            None => return,
        };
        let instrument_key = match self.selected_instrument.clone() {
            Some(k) => k,
            None => {
                self.set_error("No instrument selected");
                return;
            }
        };

        // Extract the resolution k parameter (first element of resolution_params).
        // Try the unique library first, fall back to the generic one.
        let k = {
            let from_unique = app
                .spectrum_manager
                .unique_instrument_library
                .get(&instrument_key)
                .and_then(|inst| inst.as_unique())
                .and_then(|u| u.resolution_params.as_ref())
                .and_then(|p| p.first().copied());

            from_unique.or_else(|| {
                app.spectrum_manager
                    .generic_instrument_library
                    .get(&instrument_key)
                    .and_then(|inst| inst.as_generic())
                    .and_then(|g| g.resolution_params.as_ref())
                    .and_then(|p| p.first().copied())
            })
        };

        let k = match k {
            Some(v) => v,
            None => {
                self.set_error("Selected instrument has no resolution parameters");
                return;
            }
        };

        let (y_axis, x_axis) = match app.spectrum_manager.spectrum_registry.get(&source_key) {
            Some(spec) => (
                effective_y_axis(spec, self.subtract_background),
                dvector_from_array1(&spec.x_axis),
            ),
            None => return,
        };

        let iterations = self.rl_iterations as usize;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || run_rl_worker(tx, x_axis, y_axis, k, iterations));

        self.rx = Some(rx);
        self.pending_source_key = Some(source_key);
        self.is_calculating = true;
        self.progress_percent = 0;
        self.progress_message = "Starting RL…".into();
        ctx.request_repaint();
    }

    fn calculate_mlem(&mut self, app: &mut LuRaCsApp, ctx: &egui::Context) {
        let path = self.mlem_response_path.trim().to_string();
        if path.is_empty() {
            self.set_error("No response matrix path set");
            return;
        }
        if !Path::new(&path).is_file() {
            self.set_error(format!("{path} is not a file!"));
            return;
        }

        let source_key = match self.selected_spectrum.clone() {
            Some(k) => k,
            None => return,
        };

        let (y_axis, x_axis) = match app.spectrum_manager.spectrum_registry.get(&source_key) {
            Some(spec) => (
                effective_y_axis(spec, self.subtract_background),
                dvector_from_array1(&spec.x_axis),
            ),
            None => return,
        };

        let (resp, idx, bin) = match load_response_npz(&path) {
            Ok(v) => v,
            Err(e) => {
                self.set_error(e);
                return;
            }
        };

        // Persist the last-used path.
        {
            let mut settings = app.settings.write().unwrap();
            settings.state.deconv_last_response_file = path;
            let _ = settings.save();
        }

        let iterations = self.mlem_iterations as usize;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || run_mlem_worker(tx, y_axis, resp, idx, bin, x_axis, iterations));

        self.rx = Some(rx);
        self.pending_source_key = Some(source_key);
        self.is_calculating = true;
        self.progress_percent = 0;
        self.progress_message = "Starting ML-EM…".into();
        ctx.request_repaint();
    }

    // -----------------------------------------------------------------------
    // UI drawing
    // -----------------------------------------------------------------------

    fn draw_main_ui(&mut self, ui: &mut egui::Ui) {
        let mut calc_clicked = false;

        egui::Grid::new("deconv_form")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                // Algorithm selector
                ui.label("Algorithm");
                let current = match self.algorithm {
                    Algorithm::Rl => "Generalized Richardson-Lucy",
                    Algorithm::Mlem => "ML-EM",
                };
                egui::ComboBox::from_id_salt("deconv_algorithm_combo")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                self.algorithm == Algorithm::Rl,
                                "Generalized Richardson-Lucy",
                            )
                            .clicked()
                        {
                            self.algorithm = Algorithm::Rl;
                        }
                        if ui
                            .selectable_label(self.algorithm == Algorithm::Mlem, "ML-EM")
                            .clicked()
                        {
                            self.algorithm = Algorithm::Mlem;
                        }
                    });
                ui.end_row();

                // Algorithm-specific parameters
                ui.label("");
                match self.algorithm {
                    Algorithm::Rl => self.draw_rl_form(ui),
                    Algorithm::Mlem => self.draw_mlem_form(ui),
                }
                ui.end_row();

                // Instrument
                ui.label("Instrument");
                self.draw_instrument_combo(ui);
                ui.end_row();

                // Spectrum
                ui.label("Spectrum");
                self.draw_spectrum_combo(ui);
                ui.end_row();

                // Background subtraction toggle
                ui.label("");
                ui.checkbox(&mut self.subtract_background, "Subtract Background");
                ui.end_row();

                // Progress bar
                ui.label("Progress");
                let frac = self.progress_percent as f32 / 100.0;
                ui.add(
                    egui::ProgressBar::new(frac)
                        .text(if self.progress_message.is_empty() {
                            format!("{}%", self.progress_percent)
                        } else {
                            self.progress_message.clone()
                        })
                        .desired_width(400.0),
                );
                ui.end_row();

                // Calculate button
                ui.label("");
                let enabled = !self.is_calculating;
                if ui
                    .add_enabled(enabled, egui::Button::new("Calculate"))
                    .clicked()
                {
                    calc_clicked = true;
                }
                ui.end_row();
            });

        // Defer the actual calculation to the next render call so that
        // `app` is accessible (the Grid closure only holds `&mut self`).
        if calc_clicked {
            self.calculate_requested = true;
        }

        ui.add_space(8.0);
        ui.separator();
    }

    fn draw_rl_form(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("rl_form").num_columns(2).show(ui, |ui| {
            ui.label("Iterations");
            ui.add(egui::DragValue::new(&mut self.rl_iterations).range(1..=100_000));
            ui.end_row();

            ui.label("Priors");
            ui.horizontal(|ui| {
                // Resolution prior is always applied; the checkbox is shown but
                // disabled to communicate that (matches the Python original).
                ui.add_enabled(
                    false,
                    egui::Checkbox::new(&mut self.rl_use_resolution, "Use Resolution"),
                );
                ui.checkbox(&mut self.rl_use_efficiency, "Use Efficiency");
            });
            ui.end_row();
        });
    }

    fn draw_mlem_form(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("mlem_form").num_columns(2).show(ui, |ui| {
            ui.label("Iterations");
            ui.add(egui::DragValue::new(&mut self.mlem_iterations).range(1..=100_000));
            ui.end_row();

            ui.label("Response Matrix");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.mlem_response_path).desired_width(300.0),
                );
                if ui.button("Browse…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("NPZ Files", &["npz"])
                        .pick_file()
                    {
                        self.mlem_response_path = path.display().to_string();
                    }
                }
            });
            ui.end_row();
        });
    }

    fn draw_instrument_combo(&mut self, ui: &mut egui::Ui) {
        let current = self
            .selected_instrument
            .as_ref()
            .and_then(|k| self.instruments.iter().find(|(kk, _)| kk == k))
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| "—".into());
        egui::ComboBox::from_id_salt("deconv_instrument_combo")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (key, display) in self.instruments.clone() {
                    let selected = self.selected_instrument.as_deref() == Some(&key);
                    if ui.selectable_label(selected, &display).clicked() {
                        self.selected_instrument = Some(key);
                    }
                }
            });
    }

    fn draw_spectrum_combo(&mut self, ui: &mut egui::Ui) {
        let current = self
            .selected_spectrum
            .as_ref()
            .and_then(|k| self.spectra.iter().find(|(kk, _)| kk == k))
            .map(|(_, d)| d.clone())
            .unwrap_or_else(|| "—".into());
        egui::ComboBox::from_id_salt("deconv_spectrum_combo")
            .selected_text(current)
            .show_ui(ui, |ui| {
                for (key, display) in self.spectra.clone() {
                    let selected = self.selected_spectrum.as_deref() == Some(&key);
                    if ui.selectable_label(selected, &display).clicked() {
                        self.selected_spectrum = Some(key);
                    }
                }
            });
    }

    fn draw_modal(&mut self, ctx: &egui::Context) {
        // Error modal
        if let Some(msg) = self.error_message.clone() {
            let mut dismiss = false;
            egui::Window::new("⚠ Error")
                .id(Id::new("deconv_error_modal"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.colored_label(Color32::from_rgb(220, 60, 60), &msg);
                    ui.add_space(6.0);
                    if ui.button("OK").clicked() {
                        dismiss = true;
                    }
                });
            if dismiss {
                self.error_message = None;
            }
        }

        // Confirmation modal (large spectrum warning)
        if let Some(pending) = self.pending_confirmation.take() {
            let mut choice: Option<bool> = None;
            egui::Window::new("Proceed?")
                .id(Id::new("deconv_confirm_modal"))
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "The chosen spectrum has a large number of channels ({}).\n\
                         This operation can use a lot of memory. Proceed?",
                        pending.channels
                    ));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Yes").clicked() {
                            choice = Some(true);
                        }
                        if ui.button("No").clicked() {
                            choice = Some(false);
                        }
                    });
                });
            match choice {
                Some(true) => self.confirmed_algorithm = Some(pending.algorithm),
                Some(false) => {}
                None => self.pending_confirmation = Some(pending),
            }
        }
    }
}

// ===========================================================================
// y-axis helper (free function so the worker thread can use it too)
// ===========================================================================

fn effective_y_axis(spec: &crate::containers::Spectrum, subtract_background: bool) -> DVector<f64> {
    let arr = if subtract_background {
        // Falls back to foreground CPS when background is absent.
        spec.get_bkg_sub(false)
            .unwrap_or_else(|| Array1::zeros(spec.channels))
    } else {
        spec.get_foreground(false, false)
            .unwrap_or_else(|| Array1::zeros(spec.channels))
    };
    DVector::from_iterator(arr.len(), arr.iter().copied())
}

fn dvector_from_array1(a: &Array1<f64>) -> DVector<f64> {
    DVector::from_iterator(a.len(), a.iter().copied())
}

// ===========================================================================
// Worker functions (run on background threads)
// ===========================================================================

type ResponseData = (Array2<f64>, Array1<f64>, Array1<f64>);

/// Load + validate the `.npz` response matrix on the calling thread.
fn load_response_npz(path: &str) -> Result<ResponseData, String> {
    use std::fs::File;
    let file = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut npz = NpzReader::new(file).map_err(|e| format!("parse npz: {e}"))?;

    let names: Vec<String> = npz.names().map_err(|e| format!("list names: {e}"))?;
    for req in ["response_matrix", "indices", "bin_centres"] {
        if !names.iter().any(|n| n == req) {
            return Err(format!("Missing array in response matrix: {req}"));
        }
    }

    let response_matrix: Array2<f64> = npz
        .by_name("response_matrix")
        .map_err(|e| format!("response_matrix: {e}"))?;
    let indices: Array1<f64> = npz
        .by_name("indices")
        .map_err(|e| format!("indices: {e}"))?;
    let bin_centres: Array1<f64> = npz
        .by_name("bin_centres")
        .map_err(|e| format!("bin_centres: {e}"))?;

    Ok((response_matrix, indices, bin_centres))
}

/// RL worker: build a Gaussian PSF and run ML-EM iterations.
fn run_rl_worker(
    tx: Sender<Event>,
    x_axis: DVector<f64>,
    y_axis: DVector<f64>,
    k: f64,
    iterations: usize,
) {
    let _ = tx.send(Event::Progress {
        percent: 0,
        message: "Running RL…".into(),
    });

    let n = y_axis.len();
    let sensitivity = DVector::from_element(n, 1.0);
    let opts = MlEmOptions {
        iterations,
        ..Default::default()
    };

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        richardson_lucy(&x_axis, &y_axis, k, &sensitivity, &opts)
    })) {
        Ok(result) => {
            let _ = tx.send(Event::Progress {
                percent: 100,
                message: "Done!".into(),
            });
            let _ = tx.send(Event::Done(result));
        }
        Err(_) => {
            let _ = tx.send(Event::Error("RL calculation panicked".into()));
        }
    }
}

/// MLEM worker: process the response matrix, then run `ml_em`.
fn run_mlem_worker(
    tx: Sender<Event>,
    y_axis: DVector<f64>,
    response_matrix: Array2<f64>,
    indices: Array1<f64>,
    bin_centres: Array1<f64>,
    x_axis: DVector<f64>,
    iterations: usize,
) {
    let _ = tx.send(Event::Progress {
        percent: 0,
        message: "Processing response matrix…".into(),
    });

    let idx_slice = indices.as_slice().unwrap_or(&[]);
    let bin_slice = bin_centres.as_slice().unwrap_or(&[]);
    let x_slice = x_axis.as_slice();

    let csr = process_matrix(&response_matrix, idx_slice, bin_slice, x_slice, x_slice);

    let _ = tx.send(Event::Progress {
        percent: 50,
        message: "Running ML-EM…".into(),
    });

    let n = y_axis.len();
    let sensitivity = DVector::from_element(n, 1.0);
    let opts = MlEmOptions {
        iterations,
        ..Default::default()
    };

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ml_em(&y_axis, &csr, &sensitivity, &opts)
    })) {
        Ok(result) => {
            let _ = tx.send(Event::Progress {
                percent: 100,
                message: "Done!".into(),
            });
            let _ = tx.send(Event::Done(result));
        }
        Err(_) => {
            let _ = tx.send(Event::Error("ML-EM calculation panicked".into()));
        }
    }
}
