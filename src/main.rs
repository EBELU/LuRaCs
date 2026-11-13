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
use crate::settings::{Settings, SharedSettings};
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
        // TODO: RunManager::connect_bluetooth_list(names)
        tracing::info!(?names, "--bluetooth: BLE scan not yet implemented");
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
        for chunk in bounds.chunks_exact(2) {
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
        .read()
        .unwrap()
        .advanced
        .real_time_values_deque_length;

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

    // --- 7. Theme manager ---
    let theme_manager = ThemeManager::new(shared_settings.read().unwrap().paths.themes.clone());

    // --- 8. GUI or headless ---
    if cli.headless {
        run_headless(run_manager, spectrum_manager, io_manager, shared_settings)?;
    } else {
        run_gui(
            shared_settings,
            run_manager,
            spectrum_manager,
            io_manager,
            theme_manager,
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
    theme_manager: ThemeManager,
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
                theme_manager,
                log_buffer,
            );
            Ok(Box::new(app))
        }),
    )?;

    Ok(())
}

fn app_title() -> String {
    let suffix = match (cfg!(feature = "si"), cfg!(feature = "h3")) {
        (true, _) => "-Si",
        (_, true) => "-H3",
        _ => "",
    };
    format!("LuRaCs{suffix}")
}

// ---------------------------------------------------------------------------
// Headless
// ---------------------------------------------------------------------------

fn run_headless(
    mut run_manager: RunManager,
    mut spectrum_manager: SpectrumManager,
    _io_manager: IoManager,
    shared_settings: SharedSettings,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Headless mode active — Ctrl+C to exit");

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })?;

    loop {
        // Drain run-manager events and forward them to the spectrum manager.
        let events = run_manager.poll();
        for ev in events {
            handle_headless_event(ev, &mut spectrum_manager, &shared_settings);
        }

        // Drain spectrum-manager events (no consumer, but keeps the queue
        // small — cheap enough not to matter).
        let _ = spectrum_manager.drain_events();

        if stop_rx.recv_timeout(Duration::from_millis(50)).is_ok() {
            break;
        }
    }

    tracing::info!("Headless mode stopping");
    shutdown_all(&mut run_manager, &shared_settings);
    Ok(())
}

fn handle_headless_event(
    ev: RunManagerEvent,
    spectrum_manager: &mut SpectrumManager,
    shared_settings: &SharedSettings,
) {
    match ev {
        RunManagerEvent::CreateDeviceSpectrum {
            device_name,
            channels,
            connection,
        } => {
            let load_cal = shared_settings
                .read()
                .unwrap()
                .appearance
                .load_calibration_from_instrument;
            if let Err(e) = spectrum_manager.create_spectrum(
                &device_name,
                channels,
                Some(&connection),
                load_cal,
            ) {
                tracing::warn!(device = %device_name, "create_spectrum failed: {e}");
            }
        }
        RunManagerEvent::RemoveDeviceSpectrum { name } => {
            spectrum_manager.remove_spectrum(&name);
        }
        RunManagerEvent::SpectrumUpdated { name, data } => {
            let sd = spectrum_data_from_wrapped(&data, Some(name.clone()));
            if let Err(e) =
                spectrum_manager.set_foreground_spectrum(&name, ForegroundInput::Raw(sd))
            {
                tracing::warn!(device = %name, "set_foreground_spectrum failed: {e}");
            }
        }
        // All other events are GUI-only concerns (status, GPS, spectrograms).
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Shutdown
// ---------------------------------------------------------------------------

/// Persist settings, close every loaded spectrogram, and stop the run manager.
///
/// Called from `LuRaCsApp::on_exit` in the GUI path (via `App::shutdown`) and
/// from `run_headless` on Ctrl+C.
fn shutdown_all(run_manager: &mut RunManager, shared_settings: &SharedSettings) {
    if let Err(e) = shared_settings.read().unwrap().save() {
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

fn poll_config_from(s: &Settings) -> PollConfig {
    PollConfig {
        update_loop_delay: s.advanced.update_loop_delay,
        spectrum_update_delay: s.advanced.spectrum_update_delay,
    }
}

fn build_spectrum_manager(
    settings: &SharedSettings,
) -> Result<SpectrumManager, Box<dyn std::error::Error>> {
    let s = settings.read().unwrap();
    let scheme: ColorScheme = s.appearance.color_rotator_scheme;
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
        s.appearance.tabbed_spectrum_view,
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
                        let _ = spectrum_manager
                            .unique_instrument_library
                            .set_instrument_from_parser(&key, p);
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
                        let _ = spectrum_manager
                            .generic_instrument_library
                            .set_instrument_from_parser(&key, p);
                    }
                }
                IndexKind::Spectrum | IndexKind::Spectrogram | IndexKind::Roi => {}
            },
            FileIndexEvent::IndexUpdated { .. } => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Wrapped → SpectrumData conversion
// ---------------------------------------------------------------------------
//
// Used by headless mode here and by `LuRaCsApp::ui` in the GUI path. Kept as
// a free function for now; will move to a shared module once the App-side
// implementation settles. If app.rs needs it, duplicate-and-note or import
// from this module.

fn spectrum_data_from_wrapped(
    spectrum: &WrappedSpectrumPackage,
    spectrum_name: Option<String>,
) -> SpectrumData {
    let total: f64 = spectrum.y_axis.iter().sum();
    let channels = spectrum.y_axis.len();
    let live_time = spectrum.live_time;
    let end = chrono::Utc::now();
    let start = end
        - chrono::Duration::milliseconds((spectrum.real_time.unwrap_or(live_time) * 1000.0) as i64);
    SpectrumData {
        y_axis: spectrum.y_axis.clone(),
        channels,
        total_counts: total.max(0.0) as u64,
        live_time,
        real_time: spectrum.real_time,
        avg_dose_rate: None,
        avg_cps: Some(total / live_time.max(1.0)),
        start_date: Some(start),
        end_date: Some(end),
        spectrum_name,
        instrument: None,
    }
}
