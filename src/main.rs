#![allow(
    unknown_lints,
    reason = "allow lint names unavailable on older toolchains"
)]
#![allow(clippy::cargo_common_metadata)]
#![allow(clippy::multiple_crate_versions)]
#![allow(unfulfilled_lint_expectations, reason = "I know what I want")]
// === Style and Philosophy ===
// #![warn(clippy::arithmetic_side_effects)]
// #![warn(clippy::shadow_reuse, clippy::shadow_same, clippy::shadow_unrelated)]
// #![warn(clippy::indexing_slicing)]
// === Allowed stylistic choices ===
#![expect(clippy::mod_module_files, reason = "self-imposed: allow mod files")]
#![expect(
    clippy::missing_docs_in_private_items,
    reason = "self-imposed: prefer explicit documentation control"
)]
#![expect(
    clippy::blanket_clippy_restriction_lints,
    reason = "intentional: want clippy as a second brain"
)]
#![expect(
    clippy::unseparated_literal_suffix,
    reason = "self-imposed style choice"
)]
// #![expect(
//     clippy::missing_trait_methods,
//     reason = "self-imposed: allow partial trait implementations"
// )]
// #![expect(dead_code, reason = "self-imposed: allow staged development")]
#![expect(
    clippy::implicit_return,
    reason = "self-imposed: prefer explicit returns for clarity"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "self-imposed: allow short identifiers"
)]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "self-imposed: prefer logical grouping over ordering"
)]
#![expect(
    clippy::multiple_inherent_impl,
    reason = "self-imposed: can improve readability if unconnected parts of impl are in separate impls across files"
)]
#![expect(
    clippy::partial_pub_fields,
    reason = "self-imposed: allow mixed pub and priv"
)]
// === Explicitly allowed runtime patterns ===
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::dbg_macro,
    reason = "self-imposed: allow printing for diagnostics"
)]
// === Platform and environment ===
#![expect(
    clippy::host_endian_bytes,
    reason = "intentional: allow endian-specific code"
)]
#![expect(
    clippy::little_endian_bytes,
    reason = "intentional: allow endian-specific code"
)]
#![expect(
    clippy::big_endian_bytes,
    reason = "intentional: allow endian-specific code"
)]
#![expect(
    clippy::std_instead_of_alloc,
    reason = "intentional: prefer std over alloc"
)]
// === Language constructs ===
#![expect(
    clippy::question_mark_used,
    reason = "intentional: prefer ? for readability"
)]
#![expect(
    clippy::as_conversions,
    reason = "intentional: Other lints will warn for weird casts"
)]
// === Cryptographically important ===
#![expect(
    clippy::integer_division_remainder_used,
    reason = "intentional: Comment out if intent is to be cryptographically sound"
)]
#![expect(clippy::float_arithmetic, reason = "Allow float ops")]
#![expect(clippy::expect_used, reason = "Allow float ops")]
#![allow(dead_code, reason = "too many errors")]
#![allow(unused, reason = "too many errors")]

//! LuRaCs entry point.
//!
//! Bootstrap sequence:
//!
//! 1. Parse CLI (`--headless`, `--debug`).
//! 2. Initialise logging with an in-memory ring buffer.
//! 3. Load settings from disk (missing file → defaults).
//! 4. Construct `RunManager`, `SpectrumManager`, `IoManager`, `ThemeManager`.
//! 5. Run the file indexer once and dispatch discovered instruments to the
//!    libraries (startup discovery).
//! 6. Either:
//!    - GUI: `eframe::run_native` with `LuRaCsApp` (shutdown via `on_exit`).
//!    - Headless: a tokio poll loop until Ctrl+C (shutdown inline).
//!
//! ## Splash screen
//!
//! Python has a `QSplashScreen` because building the Qt window is slow.
//! eframe shows the window on the first frame, so a splash would be a
//! visual delay, not a cover for delay. Not implemented; add a deferred
//! viewport later if branding matters.
//!
//! ## Not ported here
//!
//! - `parse_cli_args(win, script_engine)` — script-engine commands. Not
//!   relevant without the script engine.
//! - `ascii_art.logo(...)` welcome banner — cosmetic. Add later if wanted.

mod app;
mod clients;
mod containers;
mod core;
mod gui;
mod logging;
mod settings;
mod spectogram;
mod theme_manager;
mod utils;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use clap::Parser;
use eframe::egui;

use crate::app::LuRaCsApp;
use crate::clients::{PollConfig, WrappedSpectrumPackage};
use crate::containers::SpectrumData;
use crate::core::{FileIndexEvent, IndexKind, IoManager, NewRoi, SaveFn, SaveInput};
use crate::core::{ForegroundInput, SpectrumManager, SpectrumManagerError};
use crate::core::{RunManager, RunManagerEvent};
use crate::gui::{ColorScheme, SpectrumScreen};
use crate::logging::LogBuffer;
use crate::settings::{Settings, SharedSettings, SharedSettingsExt};
use crate::theme_manager::ThemeManager;
use crate::utils::{
    InstrumentRef, save_instrument_to_library, save_rois_to_library, save_spectrum_to_library,
};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "LuRaCs", version = env!("CARGO_PKG_VERSION"))]
struct Cli {
    #[arg(long)]
    headless: bool,

    #[arg(long)]
    debug: bool,

    #[arg(long, num_args = 1..)]
    import_spectrum: Option<Vec<PathBuf>>,

    #[arg(long, num_args = 1..)]
    nuclides: Option<Vec<String>>,

    #[arg(long)]
    clear_local_cache: bool,

    #[arg(long)]
    map_url: Option<String>,

    #[arg(long)]
    map_file: Option<PathBuf>,

    #[arg(long)]
    import_rois: Option<PathBuf>,

    /// `-roi <lower> <upper>`, repeatable.
    #[arg(long = "roi", num_args = 2, value_names = ["LOWER", "UPPER"], action = clap::ArgAction::Append)]
    roi: Option<Vec<f64>>,

    #[arg(long = "bluetooth", alias = "ble", num_args = 1..)]
    bluetooth: Option<Vec<String>>,

    #[arg(long = "usb", num_args = 1..)]
    usb: Option<Vec<String>>,

    #[arg(long = "network", alias = "nw", num_args = 1..)]
    network: Option<Vec<String>>,

    #[arg(long)]
    serial_gps: Option<String>,
}

fn dispatch_cli_args(
    cli: &Cli,
    shared_settings: &SharedSettings,
    run_manager: &mut RunManager,
    spectrum_manager: &mut SpectrumManager,
) -> Result<(), Box<dyn std::error::Error>> {
    if cli.debug {
        run_manager.add_device(
            crate::clients::DeviceAddress::Other("None".into()),
            "mock".to_string(),
            crate::clients::ConnectionType::Ble,
        );
        tracing::info!("--debug: mock device added");
    }

    if let Some(names) = &cli.bluetooth {
        run_manager.connect_bluetooth_list(names, Duration::from_secs(5))?;
    }

    if let Some(usb_targets) = &cli.usb {
        let devices = run_manager.scan_all_usb();
        for dev in devices {
            // Match by product name against registered wrapper types…
            tracing::debug!(?dev, "USB device found");
        }
    }

    if let Some(ips) = &cli.network {
        for ip in ips {
            // run_manager.add_device(ip, "detective_x", ConnectionType::Network);
            tracing::info!(ip, "--network: network devices not yet implemented");
        }
    }

    if let Some(paths) = &cli.import_spectrum {
        for path in paths {
            if path.is_file() {
                // io_manager.importer.parse_spectrum(path) → spectrum_manager.import_spectrum(...)
                tracing::info!(?path, "--import_spectrum: dispatch via IoManager");
            }
        }
    }

    if let Some(bounds) = &cli.roi {
        for chunk in bounds.as_chunks::<2>().0 {
            spectrum_manager
                .roi_manager
                .add_roi(NewRoi::new(chunk[0], chunk[1]));
        }
    }

    if let Some(url) = &cli.map_url {
        if cli.map_file.is_some() {
            tracing::warn!("Both --map_url and --map_file given; ignoring both");
        } else {
            tracing::info!(url, "--map_url: map widget not yet ported");
        }
    } else if cli.map_file.is_some() {
        tracing::info!("--map_file: map widget not yet ported");
    }

    if let Some(port) = &cli.serial_gps {
        run_manager.connect_serial_gps(port, 9600)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    drop(rustls::crypto::ring::default_provider().install_default());
    let cli = Cli::parse();

    // --- 1. Logging ---
    let log_buffer = logging::init_logging(cli.debug)?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        headless = cli.headless,
        "LuRaCs starting"
    );

    // --- 2. Register the mock device (unconditional; `--debug` selects it) ---
    crate::clients::register_mock_device();

    // --- 3. Settings ---
    let shared_settings = load_settings(cli.headless)?;

    // --- 4. Managers ---
    let poll_cfg = poll_config_from(&shared_settings.read().unwrap());
    let queue_len = shared_settings
        .read_settings()
        .real_time
        .values_buffer_length;

    let mut run_manager = RunManager::new(queue_len, poll_cfg)?;
    let mut spectrum_manager = build_spectrum_manager(&shared_settings)?;
    let mut io_manager = build_io_manager(&shared_settings)?;

    // --- 5. Startup file discovery ---
    io_manager.file_index.run_index_all();
    dispatch_file_index_events(&mut io_manager, &mut spectrum_manager);

    // --- 6. CLI argument dispatch (was Python's `parse_cli_args` after startup) ---
    dispatch_cli_args(
        &cli,
        &shared_settings,
        &mut run_manager,
        &mut spectrum_manager,
    )?;

    // --- 8. GUI or headless ---
    if cli.headless {
        run_headless(run_manager, spectrum_manager, io_manager, shared_settings)?;
    } else {
        run_gui(
            shared_settings,
            run_manager,
            spectrum_manager,
            io_manager,
            log_buffer,
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// GUI
// ---------------------------------------------------------------------------

fn run_gui(
    shared_settings: SharedSettings,
    run_manager: RunManager,
    spectrum_manager: SpectrumManager,
    io_manager: IoManager,
    log_buffer: Arc<LogBuffer>,
) -> Result<(), Box<dyn std::error::Error>> {
    let title = app_title();

    let options = eframe::NativeOptions {
        // rsplot needs wgpu for its paint callback. Set explicitly so that
        // `CreationContext::wgpu_render_state` is populated in `App::new`.
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_app_id("luracs"),
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            let app = LuRaCsApp::new(
                cc,
                shared_settings,
                run_manager,
                spectrum_manager,
                io_manager,
                log_buffer,
            );
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}

fn app_title() -> String {
    "LuRaCs".to_string()
}

// ---------------------------------------------------------------------------
// Headless
// ---------------------------------------------------------------------------

fn run_headless(
    mut run_manager: RunManager,
    mut spectrum_manager: SpectrumManager,
    mut io_manager: IoManager,
    shared_settings: SharedSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Headless mode active — Ctrl+C to exit");

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })?;

    let mut dispatcher = crate::core::EventDispatcher::new();

    loop {
        dispatcher.dispatch(
            &mut spectrum_manager,
            &mut run_manager,
            &mut io_manager,
            None,
            &shared_settings,
        );

        if stop_rx.recv_timeout(Duration::from_millis(50)).is_ok() {
            break;
        }
    }

    tracing::info!("Headless mode stopping");
    shutdown_all(&mut run_manager, &shared_settings);
    Ok(())
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

/// Persist settings, close every loaded spectrogram, and stop the run manager.
///
/// Called from `LuRaCsApp::on_exit` in the GUI path (via `App::shutdown`) and
/// from `run_headless` on Ctrl+C.
fn shutdown_all(run_manager: &mut RunManager, shared_settings: &SharedSettings) {
    let value = shared_settings.read().unwrap().save();
    if let Err(e) = value {
        tracing::error!("Save settings failed: {e}");
    }

    // Close spectrograms first — they hold SQLite handles that must be
    // released before the runtime stops.
    let names: Vec<String> = run_manager
        .spectrogram_manager()
        .spectrogram_registry
        .iter()
        .cloned()
        .collect();
    for name in names {
        run_manager.close_spectrogram(&name);
    }

    run_manager.shutdown();
    tracing::info!("Shutdown complete");
}

// ---------------------------------------------------------------------------
// Bootstrap helpers
// ---------------------------------------------------------------------------

fn load_settings(headless: bool) -> Result<SharedSettings, Box<dyn std::error::Error>> {
    let mut settings = Settings::new()?;
    if let Err(e) = settings.load() {
        // First run is not an error; but a corrupt file should surface.
        tracing::warn!("Settings load warning: {e}");
    }
    settings.headless = headless;
    Ok(Arc::new(RwLock::new(settings)))
}

const fn poll_config_from(s: &Settings) -> PollConfig {
    PollConfig {
        update_loop_delay: s.scanning.update_loop_delay_s,
        spectrum_update_delay: s.scanning.spectrum_update_delay_s,
    }
}

fn build_spectrum_manager(
    settings: &SharedSettings,
) -> Result<SpectrumManager, Box<dyn std::error::Error>> {
    let s = settings.read_settings();
    let scheme: ColorScheme = s.theme.color_rotator_scheme;
    Ok(SpectrumManager::new(
        scheme,
        s.paths.unique_instrument_library.clone(),
        &s.paths.nuclide_data,
    )?)
}

fn build_io_manager(settings: &SharedSettings) -> Result<IoManager, Box<dyn std::error::Error>> {
    let s = settings.read().unwrap();
    let spectrum_lib = s.paths.spectrum_library.clone();
    let roi_lib = s.paths.roi_library.clone();
    let unique_lib = s.paths.unique_instrument_library.clone();
    let generic_lib = s.paths.generic_instrument_library.clone();
    let unique_lib_for_generic = unique_lib.clone();
    let generic_lib_for_generic = generic_lib.clone();

    let spectrum_save: SaveFn = Box::new(move |input| match input {
        SaveInput::Spectrum(spec) => {
            save_spectrum_to_library(spec, &spectrum_lib).map_err(|e| e.to_string())
        }
        _ => Err("wrong input for spectrum indexer".into()),
    });
    let roi_save: SaveFn = Box::new(move |input| match input {
        SaveInput::Rois { rois, name } => {
            let mut dummy = crate::containers::Spectrum::new(1, name);
            for r in rois {
                dummy.set_roi(r.clone());
            }
            save_rois_to_library(&dummy, &roi_lib).map_err(|e| e.to_string())
        }
        _ => Err("wrong input for roi indexer".into()),
    });
    let unique_save: SaveFn = Box::new(move |input| match input {
        SaveInput::UniqueInstrument(u) => {
            save_instrument_to_library(InstrumentRef::Unique(u), &unique_lib, &generic_lib)
                .map_err(|e| e.to_string())
        }
        _ => Err("wrong input for unique-instrument indexer".into()),
    });
    let generic_save: SaveFn = Box::new(move |input| match input {
        SaveInput::GenericInstrument(g) => save_instrument_to_library(
            InstrumentRef::Generic(g),
            &unique_lib_for_generic,
            &generic_lib_for_generic,
        )
        .map_err(|e| e.to_string()),
        _ => Err("wrong input for generic-instrument indexer".into()),
    });

    Ok(IoManager::new(
        &s.paths.appdata,
        s.paths.last_opened_dir.clone(),
        s.plotting.tabbed_spectrum_view,
        spectrum_save,
        roi_save,
        unique_save,
        generic_save,
    ))
}

/// Forward indexer discoveries into the instrument libraries.
///
/// On startup, this loads every saved instrument so `run_manager` can find
/// them when a device with a matching serial arrives.
fn dispatch_file_index_events(io: &mut IoManager, spectrum_manager: &mut SpectrumManager) {
    for ev in io.file_index.drain_events() {
        match ev {
            FileIndexEvent::ItemAdded { which, path } => match which {
                IndexKind::UniqueInstrument => {
                    if let Some(p) = io
                        .file_index
                        .unique_instrument
                        .get(&path)
                        .and_then(|f| f.as_xml())
                    {
                        let key = path.to_string_lossy().into_owned();
                        drop(
                            spectrum_manager
                                .unique_instrument_library
                                .set_instrument_from_parser(&key, p),
                        );
                    }
                }
                IndexKind::GenericInstrument => {
                    if let Some(p) = io
                        .file_index
                        .generic_instrument
                        .get(&path)
                        .and_then(|f| f.as_xml())
                    {
                        let key = path.to_string_lossy().into_owned();
                        drop(
                            spectrum_manager
                                .generic_instrument_library
                                .set_instrument_from_parser(&key, p),
                        );
                    }
                }
                IndexKind::Spectrum | IndexKind::Spectrogram | IndexKind::Roi => {}
            },
            FileIndexEvent::IndexUpdated { .. } => {}
        }
    }
}
