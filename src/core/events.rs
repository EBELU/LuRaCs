//! Centralized event system and dispatcher.
//!
//! Replaces direct cross-manager mutable borrows with an asynchronous
//! inbox/outbox event-driven architecture (EDA). Both Headless mode
//! and the GUI share this exact same event pipeline.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::clients::WrappedSpectrumPackage;
use crate::core::io_manager::{FileIndexEvent, IndexKind, IoManager};
use crate::core::roi_manager::{NewRoi, RoiManagerEvent};
use crate::core::run_manager::{RunManager, RunManagerEvent};
use crate::core::spectogram_manager::SpectrogramManagerEvent;
use crate::core::spectrum_manager::{
    ImportOptions, SpectrumImportBundle, SpectrumManager, SpectrumManagerEvent,
};
use crate::settings::{Category, SharedSettings, SharedSettingsExt};
use crate::theme_manager::{ThemeManager, ThemeManagerEvent};

// ===========================================================================
// Application Commands
// ===========================================================================

/// Commands requesting state changes across managers or from UI/CLI.
#[derive(Debug, Clone)]
pub enum AppCommand {
    CreateSpectrum {
        name: String,
        channels: usize,
        connection: Option<String>,
        load_calibration: bool,
    },
    RemoveSpectrum {
        name: String,
    },
    UpdateForeground {
        name: String,
        package: WrappedSpectrumPackage,
    },
    RefitRois {
        spectrum_name: Option<String>,
        roi_tag: Option<String>,
        use_chi2: bool,
    },
    AddRoi(Box<NewRoi>),
    RemoveRoi {
        tag: String,
    },
    CalibrateSpectrum {
        name: String,
        coefficients: Vec<f64>,
    },
    ImportSpectrumBundle(Box<SpectrumImportBundle>),
    SetTheme {
        name: String,
    },
}

// ===========================================================================
// Unified Application Event
// ===========================================================================

/// Unified event envelope.
#[derive(Debug, Clone)]
pub enum AppEvent {
    Run(RunManagerEvent),
    Spectrum(SpectrumManagerEvent),
    Roi(RoiManagerEvent),
    Spectrogram(SpectrogramManagerEvent),
    FileIndex(FileIndexEvent),
    Theme(ThemeManagerEvent),
    Command(AppCommand),
}

impl From<RunManagerEvent> for AppEvent {
    fn from(ev: RunManagerEvent) -> Self {
        Self::Run(ev)
    }
}

impl From<SpectrumManagerEvent> for AppEvent {
    fn from(ev: SpectrumManagerEvent) -> Self {
        Self::Spectrum(ev)
    }
}

impl From<RoiManagerEvent> for AppEvent {
    fn from(ev: RoiManagerEvent) -> Self {
        Self::Roi(ev)
    }
}

impl From<SpectrogramManagerEvent> for AppEvent {
    fn from(ev: SpectrogramManagerEvent) -> Self {
        Self::Spectrogram(ev)
    }
}

impl From<FileIndexEvent> for AppEvent {
    fn from(ev: FileIndexEvent) -> Self {
        Self::FileIndex(ev)
    }
}

impl From<ThemeManagerEvent> for AppEvent {
    fn from(ev: ThemeManagerEvent) -> Self {
        Self::Theme(ev)
    }
}

impl From<AppCommand> for AppEvent {
    fn from(cmd: AppCommand) -> Self {
        Self::Command(cmd)
    }
}

// ===========================================================================
// Event Dispatcher
// ===========================================================================

/// Central dispatcher routing events between managers and the GUI/Headless runner.
pub struct EventDispatcher {
    inbox: VecDeque<AppEvent>,
    frame_events: Vec<AppEvent>,
}

impl Default for EventDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EventDispatcher {
    pub const fn new() -> Self {
        Self {
            inbox: VecDeque::new(),
            frame_events: Vec::new(),
        }
    }

    /// Enqueue a single event or command.
    pub fn send(&mut self, event: impl Into<AppEvent>) {
        self.inbox.push_back(event.into());
    }

    /// Enqueue a batch of events.
    pub fn send_batch<I: IntoIterator<Item = AppEvent>>(&mut self, events: I) {
        self.inbox.extend(events);
    }

    /// Read events processed during the current frame / dispatch cycle.
    pub fn frame_events(&self) -> &[AppEvent] {
        &self.frame_events
    }

    /// Run a dispatch cycle: collect produced events from active background services,
    /// route them through manager inboxes, cascade triggered events, and update state.
    pub fn dispatch(
        &mut self,
        spectrum_mgr: &mut SpectrumManager,
        run_mgr: &mut RunManager,
        io_mgr: &mut IoManager,
        mut theme_mgr: Option<&mut ThemeManager>,
        settings: &SharedSettings,
    ) {
        self.frame_events.clear();

        // 1. Ingest asynchronous events from background sources
        for ev in run_mgr.poll() {
            self.inbox.push_back(AppEvent::Run(ev));
        }

        for ev in io_mgr.file_index.drain_events() {
            match &ev {
                FileIndexEvent::ItemAdded { which, path } => match which {
                    IndexKind::UniqueInstrument => {
                        if let Some(p) = io_mgr
                            .file_index
                            .unique_instrument
                            .get(path)
                            .and_then(|f| f.as_xml())
                        {
                            let key = path.to_string_lossy().into_owned();
                            drop(
                                spectrum_mgr
                                    .unique_instrument_library
                                    .set_instrument_from_parser(&key, p),
                            );
                        }
                    }
                    IndexKind::GenericInstrument => {
                        if let Some(p) = io_mgr
                            .file_index
                            .generic_instrument
                            .get(path)
                            .and_then(|f| f.as_xml())
                        {
                            let key = path.to_string_lossy().into_owned();
                            drop(
                                spectrum_mgr
                                    .generic_instrument_library
                                    .set_instrument_from_parser(&key, p),
                            );
                        }
                    }
                    IndexKind::Spectrum | IndexKind::Spectrogram | IndexKind::Roi => {}
                },
                FileIndexEvent::IndexUpdated { .. } => {}
            }
            self.inbox.push_back(AppEvent::FileIndex(ev));
        }

        let (use_chi2, load_calibration) = {
            let s = settings.read_settings();
            (s.optimizer.use_chi2_weight, s.importing.load_calibration)
        };

        // 2. Cascade loop: dispatch events and collect freshly produced events
        let mut iterations = 0;
        const MAX_CASCADE_ITERATIONS: usize = 16;

        while !self.inbox.is_empty() && iterations < MAX_CASCADE_ITERATIONS {
            iterations += 1;
            let current_batch: Vec<AppEvent> = self.inbox.drain(..).collect();

            for ev in &current_batch {
                // Spectrum manager ingests device / command / index events
                spectrum_mgr.handle_event(ev, use_chi2, load_calibration);

                // Theme persistence
                if let AppEvent::Theme(ThemeManagerEvent::ThemeChanged { name }) = ev {
                    let mut s = settings.write_settings();
                    if let Err(e) = s.update(
                        Category::Theme,
                        "name",
                        serde_json::Value::String(name.clone()),
                    ) {
                        tracing::warn!("could not persist theme choice: {e}");
                    }
                }
            }

            self.frame_events.extend(current_batch);

            // Harvest cascading events from managers
            for ev in spectrum_mgr.drain_events() {
                self.inbox.push_back(AppEvent::Spectrum(ev));
            }
            for ev in spectrum_mgr.roi_manager.drain_events() {
                self.inbox.push_back(AppEvent::Roi(ev));
            }
            if let Some(tm) = theme_mgr.as_deref_mut() {
                for ev in tm.drain_events() {
                    self.inbox.push_back(AppEvent::Theme(ev));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::PollConfig;
    use crate::core::io_manager::SaveFn;
    use crate::gui::ColorScheme;
    use crate::settings::Settings;

    #[test]
    fn test_event_dispatcher_queue() {
        let mut dispatcher = EventDispatcher::default();
        assert!(dispatcher.frame_events().is_empty());

        dispatcher.send(AppCommand::RemoveSpectrum {
            name: "test".to_string(),
        });
        dispatcher.send_batch([AppEvent::Command(AppCommand::CreateSpectrum {
            name: "s1".to_string(),
            channels: 8,
            connection: None,
            load_calibration: false,
        })]);
        assert_eq!(dispatcher.inbox.len(), 2);

        let dir = std::env::temp_dir().join("luracs_test_dispatcher");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();

        let mut spec_mgr = SpectrumManager::new(
            ColorScheme::default(),
            dir.clone(),
            std::path::Path::new("resources/nuclide_data"),
        )
        .unwrap();
        let mut run_mgr = RunManager::new(8, PollConfig::default()).unwrap();
        let noop = || -> SaveFn { Box::new(|_| Err("noop".into())) };
        let mut io_mgr = IoManager::new(&dir, dir.clone(), false, noop(), noop(), noop(), noop());

        // Populate file_index with a UniqueInstrument XML and Spectrum XML before dispatch
        let ui_dir = dir.join("unique_instrument_library");
        let sp_dir = dir.join("spectrum_library");
        std::fs::create_dir_all(&ui_dir).unwrap();
        std::fs::create_dir_all(&sp_dir).unwrap();
        let mut tmp_spec = crate::containers::Spectrum::new(4, "IdxSpec");
        tmp_spec.set_foreground(
            crate::containers::SpectrumData {
                y_axis: ndarray::Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0]),
                channels: 4,
                total_counts: 10,
                live_time: 1.0,
                real_time: None,
                avg_dose_rate: None,
                avg_cps: None,
                start_date: None,
                end_date: None,
                spectrum_name: None,
                instrument: None,
            },
            None,
        );
        let mut u = crate::containers::UniqueInstrument::from_template(
            "IdxDet",
            &crate::containers::GenericInstrument::default(),
        );
        u.name = "IdxDet".to_string();
        tmp_spec.set_instrument(u);
        crate::utils::N42Writer::new(
            &tmp_spec,
            &ui_dir.join("IdxDet"),
            crate::utils::WriteOptions {
                export_spectrum: false,
                export_rois: false,
                export_instrument: true,
                comment: None,
            },
        )
        .unwrap()
        .write_out()
        .unwrap();
        crate::utils::N42Writer::new(
            &tmp_spec,
            &sp_dir.join("IdxSpec"),
            crate::utils::WriteOptions::default(),
        )
        .unwrap()
        .write_out()
        .unwrap();
        io_mgr.file_index.run_index_all();

        // Queue a RunManagerEvent and a ThemeManagerEvent
        run_mgr.resize_spectrogram_deque(8);
        let mut theme_mgr = ThemeManager::new(&dir, &dir, Some("dark-catppuccin"));
        let ctx = eframe::egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            drop(theme_mgr.apply(ui, "light-catppuccin"));
        });
        out.textures_delta.clear();

        let paths = crate::settings::Paths::with_overrides(
            crate::settings::PathOverrides::new()
                .base(dir.clone())
                .config(dir.join("cfg.json"))
                .appdata(dir.join("app"))
                .last_opened_dir(dir.clone()),
        )
        .unwrap();
        let settings = std::sync::Arc::new(std::sync::RwLock::new(
            Settings::builder()
                .headless(true)
                .paths(paths)
                .load_from_disk(false)
                .build()
                .unwrap(),
        ));

        dispatcher.send(AppCommand::CalibrateSpectrum {
            name: "s1".to_string(),
            coefficients: vec![2.0, 0.0],
        });
        dispatcher.send(AppCommand::AddRoi(Box::new(NewRoi::new(2.0, 6.0))));
        dispatcher.dispatch(
            &mut spec_mgr,
            &mut run_mgr,
            &mut io_mgr,
            Some(&mut theme_mgr),
            &settings,
        );

        assert!(!dispatcher.frame_events().is_empty());
        assert!(spec_mgr.get_spectrum("s1").is_some());
        assert_eq!(spec_mgr.roi_manager.registry().len(), 1);

        // Exercise From<...> impls, ThemeChanged persistence, and remaining AppCommands
        drop(AppEvent::from(RunManagerEvent::DeviceRemoved { name: "d".into() }));
        drop(AppEvent::from(SpectrumManagerEvent::SpectrumRemoved("s".into())));
        drop(AppEvent::from(RoiManagerEvent::Deleted { tag: "t".into() }));
        drop(AppEvent::from(SpectrogramManagerEvent::RoiCountsUpdated {
            logger_name: "l".into(),
            counts: std::sync::Arc::new(std::collections::HashMap::new()),
        }));
        drop(AppEvent::from(FileIndexEvent::IndexUpdated {
            which: IndexKind::Spectrum,
        }));
        dispatcher.send(ThemeManagerEvent::ThemeChanged {
            name: "light-catppuccin".into(),
        });
        dispatcher.send(AppCommand::UpdateForeground {
            name: "s1".into(),
            package: WrappedSpectrumPackage {
                y_axis: ndarray::Array1::from_vec(vec![1.0; 8]),
                live_time: 2.0,
                real_time: Some(2.0),
                calib_coeff: None,
                timestamp: 1.0,
            },
        });
        dispatcher.send(AppCommand::RemoveRoi {
            tag: "ROI_0".into(),
        });
        dispatcher.send(AppCommand::RemoveSpectrum { name: "s1".into() });
        dispatcher.dispatch(&mut spec_mgr, &mut run_mgr, &mut io_mgr, None, &settings);
        assert!(spec_mgr.get_spectrum("s1").is_none());

        run_mgr.shutdown();
        drop(std::fs::remove_dir_all(&dir));
    }
}
