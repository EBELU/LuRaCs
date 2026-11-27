//! Isolated device runner.
//!
//! Supervises one `DeviceDriver` and `DeviceLink` inside an isolated Tokio task,
//! guaranteeing that a panic, disconnection, or failure in one detector cannot
//! spill over to any other detector.

pub mod events;

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use futures_util::FutureExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub use events::{DeviceCommand, DeviceEvent};
use crate::clients::device_wrapper_base::{DeviceState, PollConfig};
use crate::clients::driver::{DeviceDriver, DeviceError};
use crate::clients::link::{DeviceLink, LinkError};

/// Public handle to an active, supervised detector task.
pub struct DeviceHandle {
    name: String,
    command_tx: mpsc::Sender<DeviceCommand>,
    join_handle: JoinHandle<()>,
}

impl DeviceHandle {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Send a command to this detector (e.g. start, stop, reset spectrum).
    pub async fn send_command(&self, cmd: DeviceCommand) -> Result<(), mpsc::error::SendError<DeviceCommand>> {
        self.command_tx.send(cmd).await
    }

    /// Check if the background task has completed or exited.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.join_handle.is_finished()
    }

    /// Gracefully request the runner to stop.
    pub async fn stop(&self) {
        drop(self.command_tx.send(DeviceCommand::Stop).await);
    }

    /// Abort the runner task immediately.
    pub fn abort(&self) {
        self.join_handle.abort();
    }
}

/// The background supervisor engine for a single detector.
pub struct DeviceRunner {
    name: String,
    driver: Box<dyn DeviceDriver>,
    link: Box<dyn DeviceLink>,
    events_tx: mpsc::UnboundedSender<DeviceEvent>,
    command_rx: mpsc::Receiver<DeviceCommand>,
    config: Arc<RwLock<PollConfig>>,
}

impl DeviceRunner {
    /// Spawn an isolated detector runner task.
    ///
    /// Even if the underlying driver panics, the panic is caught and isolated,
    /// emitting `DeviceEvent::TaskPanicked` and marking only this detector as `Error`.
    pub fn spawn(
        driver: Box<dyn DeviceDriver>,
        link: Box<dyn DeviceLink>,
        events_tx: mpsc::UnboundedSender<DeviceEvent>,
        config: Arc<RwLock<PollConfig>>,
    ) -> DeviceHandle {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let name = driver.info().name;

        let mut runner = Self {
            name: name.clone(),
            driver,
            link,
            events_tx: events_tx.clone(),
            command_rx: cmd_rx,
            config,
        };

        let runner_name = name.clone();
        let panic_events_tx = events_tx;
        let panic_name = name;

        let join_handle = tokio::spawn(async move {
            // Panic isolation boundary
            let run_result = std::panic::AssertUnwindSafe(runner.run())
                .catch_unwind()
                .await;

            if let Err(panic_err) = run_result {
                let reason = panic_err.downcast_ref::<&str>().map_or_else(
                    || {
                        panic_err
                            .downcast_ref::<String>()
                            .cloned()
                            .unwrap_or_else(|| "unspecified panic in device runner".to_string())
                    },
                    |s| (*s).to_string(),
                );

                tracing::error!(device = %panic_name, "Detector crashed (isolated panic): {reason}");

                drop(panic_events_tx.send(DeviceEvent::TaskPanicked {
                    name: panic_name.clone(),
                    reason,
                }));
                drop(panic_events_tx.send(DeviceEvent::DeviceStateUpdated {
                    name: panic_name,
                    state: DeviceState::Error,
                }));
            }
        });

        DeviceHandle {
            name: runner_name,
            command_tx: cmd_tx,
            join_handle,
        }
    }

    async fn run(&mut self) {
        let name = self.name.clone();
        tracing::info!(device = %name, "Detector runner started");

        let events_tx = self.events_tx.clone();
        let send_event = move |ev: DeviceEvent| {
            drop(events_tx.send(ev));
        };

        send_event(DeviceEvent::DeviceStateUpdated {
            name: name.clone(),
            state: DeviceState::Connected,
        });

        let mut loops_since_spectrum: usize = 0;
        let mut last_cfg = *self.config.read().unwrap();

        loop {
            // 1. Process pending commands
            while let Ok(cmd) = self.command_rx.try_recv() {
                match cmd {
                    DeviceCommand::ResetSpectrum => {
                        drop(self.driver.reset_spectrum().await);
                    }
                    DeviceCommand::StartAcquisition => {
                        drop(self.driver.start_acquisition().await);
                    }
                    DeviceCommand::StopAcquisition => {
                        drop(self.driver.stop_acquisition().await);
                    }
                    DeviceCommand::SetCalibration(coeffs) => {
                        drop(self.driver.set_calibration(&coeffs).await);
                    }
                    DeviceCommand::Stop => {
                        tracing::info!(device = %name, "Detector received stop command");
                        drop(self.driver.stop_acquisition().await);
                        drop(self.link.close().await);
                        send_event(DeviceEvent::DeviceStateUpdated {
                            name: name.clone(),
                            state: DeviceState::Stopped,
                        });
                        return;
                    }
                }
            }

            // 2. Refresh poll delays
            let cfg = *self.config.read().unwrap();
            if cfg.update_loop_delay != last_cfg.update_loop_delay
                || cfg.spectrum_update_delay != last_cfg.spectrum_update_delay
            {
                last_cfg = cfg;
                loops_since_spectrum = 0;
            }

            let spectrum_interval = ((cfg.spectrum_update_delay / cfg.update_loop_delay).ceil() as usize).max(1);

            // 3. Poll Real-Time measurements
            match self.driver.read_realtime().await {
                Ok(data) => send_event(DeviceEvent::CurrentUpdated {
                    name: name.clone(),
                    data,
                }),
                Err(e) => {
                    if !self.handle_error(e, &name).await {
                        break;
                    }
                    continue;
                }
            }

            // 4. Poll Status
            match self.driver.read_status().await {
                Ok(data) => send_event(DeviceEvent::StatusUpdated {
                    name: name.clone(),
                    data,
                }),
                Err(e) => {
                    if !self.handle_error(e, &name).await {
                        break;
                    }
                    continue;
                }
            }

            // 5. Poll Spectrum (at spectrum interval)
            loops_since_spectrum += 1;
            if loops_since_spectrum >= spectrum_interval {
                loops_since_spectrum = 0;
                match self.driver.read_spectrum().await {
                    Ok(data) => send_event(DeviceEvent::SpectrumUpdated {
                        name: name.clone(),
                        data,
                    }),
                    Err(e) => {
                        if !self.handle_error(e, &name).await {
                            break;
                        }
                        continue;
                    }
                }
            }

            // 6. Poll GPS if supported
            if let Some(gps) = self.driver.read_gps().await {
                send_event(DeviceEvent::GpsUpdated {
                    name: name.clone(),
                    data: gps,
                });
            }

            // 7. Sleep for loop delay
            let delay = Duration::from_secs_f64(cfg.update_loop_delay.max(0.05));
            tokio::select! {
                _ = tokio::time::sleep(delay) => {},
                cmd = self.command_rx.recv() => {
                    if matches!(cmd, Some(DeviceCommand::Stop)) {
                        drop(self.driver.stop_acquisition().await);
                        drop(self.link.close().await);
                        send_event(DeviceEvent::DeviceStateUpdated {
                            name,
                            state: DeviceState::Stopped,
                        });
                        return;
                    }
                }
            }
        }
    }

    /// Handles an error during polling. Returns `true` if polling should continue,
    /// or `false` if the runner must terminate.
    async fn handle_error(&self, err: DeviceError, name: &str) -> bool {
        let is_recoverable = match &err {
            DeviceError::Link(link_err) => {
                link_err.is_recoverable() && self.link.reconnect_policy().is_reconnectable()
            }
            _ => false,
        };

        if is_recoverable {
            tracing::warn!(device = %name, "Recoverable link drop detected, entering reconnect loop: {err}");

            drop(self.events_tx.send(DeviceEvent::DeviceStateUpdated {
                name: name.to_string(),
                state: DeviceState::Connecting,
            }));

            match self.link.reconnect().await {
                Ok(()) => {
                    tracing::info!(device = %name, "Link reconnected successfully, restoring driver state");
                    drop(self.driver.on_reconnect().await);

                    drop(self.events_tx.send(DeviceEvent::Reconnected {
                        name: name.to_string(),
                    }));
                    drop(self.events_tx.send(DeviceEvent::DeviceStateUpdated {
                        name: name.to_string(),
                        state: DeviceState::Connected,
                    }));
                    return true;
                }
                Err(reconnect_err) => {
                    tracing::error!(device = %name, "Reconnection failed or limits exhausted: {reconnect_err}");
                    drop(self.events_tx.send(DeviceEvent::DeviceStateUpdated {
                        name: name.to_string(),
                        state: DeviceState::ConnectionLost,
                    }));
                    drop(self.events_tx.send(DeviceEvent::PollFailed {
                        name: name.to_string(),
                        state: DeviceState::ConnectionLost,
                        reason: reconnect_err.to_string(),
                    }));
                    return false;
                }
            }
        }

        tracing::error!(device = %name, "Unrecoverable detector error: {err}");
        drop(self.events_tx.send(DeviceEvent::DeviceStateUpdated {
            name: name.to_string(),
            state: DeviceState::Error,
        }));
        drop(self.events_tx.send(DeviceEvent::PollFailed {
            name: name.to_string(),
            state: DeviceState::Error,
            reason: err.to_string(),
        }));
        false
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ndarray::Array1;
    use crate::clients::device_wrapper_base::{
        ConnectionType, WrappedRealTimePackage, WrappedSpectrumPackage, WrappedStatusPackage,
    };
    use crate::clients::driver::DeviceInfo;
    use crate::clients::link::MockLink;

    struct TestDriver {
        name: String,
        should_panic: bool,
    }

    #[async_trait]
    impl DeviceDriver for TestDriver {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                name: self.name.clone(),
                model: "TestModel".into(),
                channels: 1024,
                connection_type: ConnectionType::Ble,
                supported_settings: Vec::new(),
            }
        }

        async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
            if self.should_panic {
                panic!("intentional driver panic for isolation test");
            }
            Ok(WrappedRealTimePackage {
                cps: 42.0,
                dr: 0.15,
                timestamp: 100.0,
                ..Default::default()
            })
        }

        async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
            Ok(WrappedStatusPackage {
                battery: Some(85),
                ..Default::default()
            })
        }

        async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
            Ok(WrappedSpectrumPackage {
                y_axis: Array1::zeros(1024),
                live_time: 1.0,
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn test_runner_fault_isolation_catches_panic() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));

        let driver = Box::new(TestDriver {
            name: "CrashingDetector".into(),
            should_panic: true,
        });
        let link = Box::new(MockLink::ble_mock("BLE:CrashingDetector"));

        let handle = DeviceRunner::spawn(driver, link, tx, config);

        // Await runner completion
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(handle.is_finished());

        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }

        // Verify that TaskPanicked was emitted and the device state is Error
        let panicked = events.iter().any(|ev| matches!(ev, DeviceEvent::TaskPanicked { .. }));
        let state_err = events.iter().any(|ev| {
            matches!(
                ev,
                DeviceEvent::DeviceStateUpdated {
                    state: DeviceState::Error,
                    ..
                }
            )
        });

        assert!(panicked, "TaskPanicked event must be emitted on crash");
        assert!(state_err, "State must be updated to Error on crash");
    }

    #[tokio::test]
    async fn test_runner_normal_telemetry() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));

        let driver = Box::new(TestDriver {
            name: "HealthyDetector".into(),
            should_panic: false,
        });
        let link = Box::new(MockLink::ble_mock("BLE:HealthyDetector"));

        let handle = DeviceRunner::spawn(driver, link, tx, config);

        // Let it run for 120ms
        tokio::time::sleep(Duration::from_millis(120)).await;
        handle.stop().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }

        let has_realtime = events.iter().any(|ev| matches!(ev, DeviceEvent::CurrentUpdated { .. }));
        let has_status = events.iter().any(|ev| matches!(ev, DeviceEvent::StatusUpdated { .. }));
        let has_spectrum = events.iter().any(|ev| matches!(ev, DeviceEvent::SpectrumUpdated { .. }));

        assert!(has_realtime);
        assert!(has_status);
        assert!(has_spectrum);
    }

    struct ConfigurableDriver {
        name: String,
        realtime_err: std::sync::Mutex<Option<DeviceError>>,
        status_err: std::sync::Mutex<Option<DeviceError>>,
        spectrum_err: std::sync::Mutex<Option<DeviceError>>,
        gps_data: Option<crate::clients::gps::GpsData>,
    }

    #[async_trait]
    impl DeviceDriver for ConfigurableDriver {
        fn info(&self) -> DeviceInfo {
            DeviceInfo {
                name: self.name.clone(),
                model: "ConfigModel".into(),
                channels: 512,
                connection_type: ConnectionType::Ble,
                supported_settings: Vec::new(),
            }
        }

        async fn read_realtime(&self) -> Result<WrappedRealTimePackage, DeviceError> {
            let err = self.realtime_err.lock().unwrap().take();
            if let Some(err) = err {
                return Err(err);
            }
            Ok(WrappedRealTimePackage::default())
        }

        async fn read_status(&self) -> Result<WrappedStatusPackage, DeviceError> {
            let err = self.status_err.lock().unwrap().take();
            if let Some(err) = err {
                return Err(err);
            }
            Ok(WrappedStatusPackage::default())
        }

        async fn read_spectrum(&self) -> Result<WrappedSpectrumPackage, DeviceError> {
            let err = self.spectrum_err.lock().unwrap().take();
            if let Some(err) = err {
                return Err(err);
            }
            Ok(WrappedSpectrumPackage {
                y_axis: Array1::zeros(512),
                live_time: 1.0,
                ..Default::default()
            })
        }

        async fn read_gps(&self) -> Option<crate::clients::gps::GpsData> {
            self.gps_data.clone()
        }
    }

    #[tokio::test]
    async fn test_runner_commands_config_change_and_gps() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let config = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));

        let driver = Box::new(ConfigurableDriver {
            name: "CmdDetector".into(),
            realtime_err: std::sync::Mutex::new(None),
            status_err: std::sync::Mutex::new(None),
            spectrum_err: std::sync::Mutex::new(None),
            gps_data: Some(crate::clients::gps::GpsData::default()),
        });
        let link = Box::new(MockLink::ble_mock("BLE:CmdDetector"));
        let handle = DeviceRunner::spawn(driver, link, tx, config.clone());

        drop(handle.command_tx.send(DeviceCommand::ResetSpectrum).await);
        drop(handle.command_tx.send(DeviceCommand::StartAcquisition).await);
        drop(handle.command_tx.send(DeviceCommand::StopAcquisition).await);
        drop(handle.command_tx.send(DeviceCommand::SetCalibration(vec![0.0, 1.0])).await);
        tokio::time::sleep(Duration::from_millis(80)).await;

        config.write().unwrap().update_loop_delay = 0.06;
        tokio::time::sleep(Duration::from_millis(80)).await;

        drop(handle.command_tx.send(DeviceCommand::Stop).await);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(handle.is_finished());

        let mut events = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            events.push(ev);
        }
        assert!(events.iter().any(|ev| matches!(ev, DeviceEvent::GpsUpdated { .. })));
        assert!(events.iter().any(|ev| matches!(
            ev,
            DeviceEvent::DeviceStateUpdated {
                state: DeviceState::Stopped,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn test_runner_reconnect_success_failure_and_unrecoverable() {
        // 1. Recoverable error -> reconnect succeeds
        let (tx1, mut rx1) = mpsc::unbounded_channel();
        let cfg1 = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));
        let drv1 = Box::new(ConfigurableDriver {
            name: "ReconnOk".into(),
            realtime_err: std::sync::Mutex::new(Some(DeviceError::Link(
                crate::clients::link::LinkError::Disconnected("drop".into()),
            ))),
            status_err: std::sync::Mutex::new(None),
            spectrum_err: std::sync::Mutex::new(None),
            gps_data: None,
        });
        let link1 = Box::new(MockLink::ble_mock("BLE:ReconnOk"));
        let h1 = DeviceRunner::spawn(drv1, link1, tx1, cfg1);
        tokio::time::sleep(Duration::from_millis(120)).await;
        h1.stop().await;
        let mut ev1 = Vec::new();
        while let Ok(ev) = rx1.try_recv() {
            ev1.push(ev);
        }
        assert!(ev1.iter().any(|ev| matches!(ev, DeviceEvent::Reconnected { .. })));

        // 2. Recoverable error -> reconnect fails -> ConnectionLost
        let (tx2, mut rx2) = mpsc::unbounded_channel();
        let cfg2 = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));
        let drv2 = Box::new(ConfigurableDriver {
            name: "ReconnFail".into(),
            realtime_err: std::sync::Mutex::new(None),
            status_err: std::sync::Mutex::new(Some(DeviceError::Link(
                crate::clients::link::LinkError::Disconnected("drop2".into()),
            ))),
            spectrum_err: std::sync::Mutex::new(None),
            gps_data: None,
        });
        let link2 = MockLink::ble_mock("BLE:ReconnFail");
        link2.set_fail_next_reconnect(Some(crate::clients::link::LinkError::Disconnected("nope".into())));
        let h2 = DeviceRunner::spawn(drv2, Box::new(link2), tx2, cfg2);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(h2.is_finished());
        let mut ev2 = Vec::new();
        while let Ok(ev) = rx2.try_recv() {
            ev2.push(ev);
        }
        assert!(ev2.iter().any(|ev| matches!(
            ev,
            DeviceEvent::PollFailed {
                state: DeviceState::ConnectionLost,
                ..
            }
        )));

        // 3. Unrecoverable error in spectrum -> DeviceState::Error + PollFailed
        let (tx3, mut rx3) = mpsc::unbounded_channel();
        let cfg3 = Arc::new(RwLock::new(PollConfig {
            update_loop_delay: 0.05,
            spectrum_update_delay: 0.05,
        }));
        let drv3 = Box::new(ConfigurableDriver {
            name: "Unrecov".into(),
            realtime_err: std::sync::Mutex::new(None),
            status_err: std::sync::Mutex::new(None),
            spectrum_err: std::sync::Mutex::new(Some(DeviceError::Protocol("bad frame".into()))),
            gps_data: None,
        });
        let link3 = Box::new(MockLink::ble_mock("BLE:Unrecov"));
        let h3 = DeviceRunner::spawn(drv3, link3, tx3, cfg3);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(h3.is_finished());
        let mut ev3 = Vec::new();
        while let Ok(ev) = rx3.try_recv() {
            ev3.push(ev);
        }
        assert!(ev3.iter().any(|ev| matches!(
            ev,
            DeviceEvent::PollFailed {
                state: DeviceState::Error,
                ..
            }
        )));
    }
}
