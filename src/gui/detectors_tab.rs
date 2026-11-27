//! Detectors bottom tab: connected detectors table with live status and
//! per-device acquisition / disconnect actions.
//!
//! Ported from `luracs/gui/tabs/devices_tab.py`.

use std::collections::HashMap;

use eframe::egui;

use crate::clients::{DeviceState, WrappedStatusPackage};
use crate::core::RunManager;

#[derive(Debug, Clone, Default)]
struct DetectorRowState {
    state: Option<DeviceState>,
    status: Option<WrappedStatusPackage>,
}

pub struct DetectorsTab {
    rows: HashMap<String, DetectorRowState>,
}

impl Default for DetectorsTab {
    fn default() -> Self {
        Self::new()
    }
}

impl DetectorsTab {
    pub fn new() -> Self {
        Self {
            rows: HashMap::new(),
        }
    }

    pub fn on_device_wrapped(&mut self, name: &str) {
        self.rows.entry(name.to_string()).or_default();
    }

    pub fn on_state_updated(&mut self, name: &str, state: DeviceState) {
        self.rows.entry(name.to_string()).or_default().state = Some(state);
    }

    pub fn on_device_connected(&mut self, name: &str) {
        self.rows.entry(name.to_string()).or_default().state = Some(DeviceState::Connected);
    }

    pub fn on_device_removed(&mut self, name: &str) {
        self.rows.remove(name);
    }

    pub fn on_status_updated(&mut self, name: &str, status: &WrappedStatusPackage) {
        let entry = self.rows.entry(name.to_string()).or_default();
        if let Some(st) = status.device_state {
            entry.state = Some(st);
        }
        entry.status = Some(status.clone());
    }

    pub fn render(&self, ui: &mut egui::Ui, run_manager: &RunManager) {
        let mut names = run_manager.list_devices();
        names.sort();

        if names.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No detectors connected.");
            });
            return;
        }

        let mut disconnect_target: Option<String> = None;

        egui::ScrollArea::vertical()
            .id_salt("detectors_tab_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("detectors_tab_grid")
                    .num_columns(9)
                    .spacing([12.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for h in [
                            "",
                            "Detector",
                            "Temperature",
                            "Battery",
                            "Charging",
                            "Voltage",
                            "Status",
                            "Type",
                            "Connection",
                        ] {
                            ui.strong(h);
                        }
                        ui.end_row();

                        for name in &names {
                            let Some(dev) = run_manager.get_device(name) else {
                                continue;
                            };
                            let row = self.rows.get(name);
                            let status = row.and_then(|r| r.status.as_ref());

                            ui.menu_button("...", |ui| {
                                if ui.button("Disconnect").clicked() {
                                    disconnect_target = Some(name.clone());
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Reset Spectrum").clicked() {
                                    dev.reset_spectrum();
                                    ui.close();
                                }
                                if ui.button("Start Acquisition").clicked() {
                                    dev.start_acquisition();
                                    ui.close();
                                }
                                if ui.button("Stop Acquisition").clicked() {
                                    dev.stop_acquisition();
                                    ui.close();
                                }
                            });

                            ui.label(name);

                            let temp = status.map_or_else(
                                || "None".to_string(),
                                |s| {
                                    if s.temperature.is_finite() {
                                        format!("{:.1}°C", s.temperature)
                                    } else {
                                        "nan".to_string()
                                    }
                                },
                            );
                            let battery = status.map_or_else(
                                || "None".to_string(),
                                |s| s.battery.map_or_else(|| "nan".to_string(), |b| format!("{b}%")),
                            );
                            let charging = status.map_or_else(
                                || "None".to_string(),
                                |s| s.charging.to_string(),
                            );
                            let voltage = status.map_or_else(
                                || "None".to_string(),
                                |s| {
                                    if s.voltage.is_finite() {
                                        format!("{:.1}V", s.voltage)
                                    } else {
                                        "nan".to_string()
                                    }
                                },
                            );
                            let state = row
                                .and_then(|r| r.state)
                                .unwrap_or_else(|| dev.state());

                            ui.label(temp);
                            ui.label(battery);
                            ui.label(charging);
                            ui.label(voltage);
                            ui.label(format!("{state:?}"));
                            ui.label(dev.type_key());
                            ui.label(format!("{:?}", dev.connection()));
                            ui.end_row();
                        }
                    });
            });

        if let Some(name) = disconnect_target {
            run_manager.remove_device(name, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::{
        register_device, ConnectionType, DeviceAddress, DeviceWrapper, DummyDevice, PollConfig,
    };
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    #[test]
    fn test_detectors_tab_empty_populated_status_and_menu_actions() {
        struct RunningDummy(DummyDevice);
        #[async_trait::async_trait]
        impl DeviceWrapper for RunningDummy {
            fn type_key(&self) -> &'static str { self.0.type_key() }
            fn name(&self) -> &str { self.0.name() }
            fn connection(&self) -> ConnectionType { self.0.connection() }
            fn state(&self) -> DeviceState { self.0.state() }
            fn set_state(&self, s: DeviceState) { self.0.set_state(s); }
            fn channels(&self) -> Option<usize> { self.0.channels() }
            fn is_running(&self) -> bool { true }
            fn is_stopped(&self) -> bool { false }
            fn connected_timestamp(&self) -> f64 { self.0.connected_timestamp() }
            async fn start(&self) -> Result<(), crate::clients::DeviceError> { Ok(()) }
            async fn stop(&self) -> Result<(), crate::clients::DeviceError> { Ok(()) }
            async fn get_realtime_data(&self) -> Result<crate::clients::WrappedRealTimePackage, crate::clients::DeviceError> {
                self.0.get_realtime_data().await
            }
            async fn get_status(&self) -> Result<WrappedStatusPackage, crate::clients::DeviceError> {
                self.0.get_status().await
            }
            async fn get_spectrum(&self) -> Result<crate::clients::WrappedSpectrumPackage, crate::clients::DeviceError> {
                self.0.get_spectrum().await
            }
        }
        register_device("DetTabDummy", |addr, conn| {
            Ok(Box::new(RunningDummy(DummyDevice::new(addr, conn))))
        });

        let rm = RunManager::new(8, PollConfig::default()).unwrap();
        let mut tab = DetectorsTab::default();

        // 1. Empty state
        let mut harness = Harness::new_ui_state(
            |ui, (tab, rm): &mut (DetectorsTab, RunManager)| {
                tab.render(ui, rm);
            },
            (tab, rm),
        );
        harness.run();
        assert!(harness.query_by_label("No detectors connected.").is_some());

        // 2. Add device and update status
        {
            let rm = &harness.state().1;
            let jh = rm.add_device(
                DeviceAddress::Other("det_1".into()),
                "DetTabDummy".into(),
                ConnectionType::Network,
            );
            rm.runtime_handle().block_on(jh).unwrap();
        }
        let dev_name = harness.state().1.list_devices().into_iter().next().unwrap();
        {
            let tab = &mut harness.state_mut().0;
            tab.on_device_wrapped(&dev_name);
            tab.on_state_updated(&dev_name, DeviceState::Connecting);
            tab.on_device_connected(&dev_name);
            tab.on_status_updated(
                &dev_name,
                &WrappedStatusPackage {
                    temperature: 25.5,
                    battery: Some(88),
                    charging: true,
                    voltage: 3.7,
                    device_state: Some(DeviceState::Connected),
                    ..Default::default()
                },
            );
        }
        harness.run();
        assert!(harness.query_by_label(dev_name.as_str()).is_some());
        assert!(harness.query_by_label("25.5°C").is_some());
        assert!(harness.query_by_label("88%").is_some());
        assert!(harness.query_by_label("3.7V").is_some());

        // Non-finite temperature/voltage and None battery
        harness.state_mut().0.on_status_updated(
            &dev_name,
            &WrappedStatusPackage {
                temperature: f64::NAN,
                battery: None,
                charging: false,
                voltage: f64::NAN,
                device_state: None,
                ..Default::default()
            },
        );
        harness.run();
        assert!(harness.query_all_by_label("nan").next().is_some());

        // Open "..." menu and trigger acquisition + disconnect actions
        for action in [
            "Reset Spectrum",
            "Start Acquisition",
            "Stop Acquisition",
            "Disconnect",
        ] {
            harness.get_by_label("...").click();
            harness.run();
            harness.get_by_label(action).click();
            harness.run();
        }

        harness.state_mut().0.on_device_removed(&dev_name);
        assert!(harness.state().0.rows.is_empty());
    }
}
