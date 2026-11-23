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

    #[test]
    fn test_event_dispatcher_queue() {
        let mut dispatcher = EventDispatcher::new();
        assert!(dispatcher.frame_events().is_empty());

        dispatcher.send(AppCommand::RemoveSpectrum {
            name: "test".to_string(),
        });
        assert_eq!(dispatcher.inbox.len(), 1);
    }
}
