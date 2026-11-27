//! Connection dialogs for Bluetooth (BLE), USB, Network, and Serial GPS devices.
//!
//! Ported from:
//! - `luracs/gui/windows/connection_window_bluetooth.py`
//! - `luracs/gui/windows/connection_window_usb.py`
//! - `luracs/gui/dialogs/connect_network_device.py`
//! - `luracs/gui/dialogs/connect_serial_gps.py`

use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui;
use nusb::MaybeFuture;
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Connection actions emitted to the App
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BleDeviceKind {
    Auto,
    Radiacode,
    Raysid,
}

impl BleDeviceKind {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Radiacode, Self::Raysid];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Radiacode => "RadiaCode",
            Self::Raysid => "RaySID",
        }
    }

    /// Resolve `Auto` using the advertised BLE device name.
    #[must_use]
    pub fn resolve(self, device_name: &str) -> Option<Self> {
        match self {
            Self::Auto => {
                let lower = device_name.to_ascii_lowercase();
                if lower.contains("radiacode") {
                    Some(Self::Radiacode)
                } else if lower.contains("raysid") {
                    Some(Self::Raysid)
                } else {
                    None
                }
            }
            other => Some(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbDeviceKind {
    Auto,
    Radiacode,
    DigiBase,
    DigiBaseRh,
    DigiDart,
}

impl UsbDeviceKind {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Radiacode,
        Self::DigiBase,
        Self::DigiBaseRh,
        Self::DigiDart,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Radiacode => "RadiaCode",
            Self::DigiBase => "digiBase",
            Self::DigiBaseRh => "digiBase-RH",
            Self::DigiDart => "digiDART",
        }
    }

    /// Resolve `Auto` using USB VID/PID first, then product name fallback.
    #[must_use]
    pub fn resolve(self, vendor_id: u16, product_id: u16, product_name: &str) -> Option<Self> {
        match self {
            Self::Auto => match (vendor_id, product_id) {
                (0x0483, 0xF123) => Some(Self::Radiacode),
                (0x0A2D, 0x001F) => Some(Self::DigiBaseRh),
                (0x0A2D, 0x0005) => Some(Self::DigiDart),
                (0x0A2D, _) => Some(Self::DigiBase),
                _ => {
                    let lower = product_name.to_ascii_lowercase();
                    if lower.contains("radiacode") {
                        Some(Self::Radiacode)
                    } else if lower.contains("digibase") {
                        Some(Self::DigiBase)
                    } else if lower.contains("digidart") {
                        Some(Self::DigiDart)
                    } else {
                        None
                    }
                }
            },
            other => Some(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDeviceKind {
    DetectiveX,
    Identifinder400,
}

impl NetworkDeviceKind {
    pub const ALL: [Self; 2] = [Self::DetectiveX, Self::Identifinder400];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DetectiveX => "Detective X",
            Self::Identifinder400 => "IdentiFINDER R400",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionRequest {
    Ble {
        name: String,
        address: String,
        kind: BleDeviceKind,
    },
    Usb {
        vendor_id: u16,
        product_id: u16,
        serial: Option<String>,
        product: String,
        kind: UsbDeviceKind,
    },
    Network {
        address: String,
        kind: NetworkDeviceKind,
        use_gps: bool,
    },
    SerialGps {
        port: String,
        baud_rate: u32,
    },
    MockGps,
    DisconnectGps,
}

// ---------------------------------------------------------------------------
// Bluetooth (BLE) Connection Window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleScannedDevice {
    pub name: String,
    pub address: String,
    pub rssi: Option<i16>,
    pub is_priority: bool,
}

enum BleScanMessage {
    Finished(Result<Vec<BleScannedDevice>, String>),
}

pub struct BluetoothWindow {
    pub open: bool,
    scan_duration: Duration,
    scanning: bool,
    scan_started_at: Option<Instant>,
    cancel_token: Option<CancellationToken>,
    rx: Option<mpsc::Receiver<BleScanMessage>>,
    devices: Vec<BleScannedDevice>,
    selected_idx: Option<usize>,
    selected_kind: BleDeviceKind,
    error_message: Option<String>,
}

impl Default for BluetoothWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl BluetoothWindow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            scan_duration: Duration::from_secs(5),
            scanning: false,
            scan_started_at: None,
            cancel_token: None,
            rx: None,
            devices: Vec::new(),
            selected_idx: None,
            selected_kind: BleDeviceKind::Auto,
            error_message: None,
        }
    }

    pub fn open_and_scan(&mut self, handle: &Handle, ctx: &egui::Context) {
        self.open = true;
        self.start_scan(handle, ctx);
    }

    pub fn start_scan(&mut self, handle: &Handle, ctx: &egui::Context) {
        self.cancel_scan();
        self.devices.clear();
        self.selected_idx = None;
        self.error_message = None;
        self.scanning = true;
        self.scan_started_at = Some(Instant::now());

        let token = CancellationToken::new();
        self.cancel_token = Some(token.clone());

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let duration = self.scan_duration;
        let ctx_clone = ctx.clone();

        handle.spawn(async move {
            let result = scan_ble_devices(duration, token).await;
            drop(tx.send(BleScanMessage::Finished(result)));
            ctx_clone.request_repaint();
        });
    }

    pub fn cancel_scan(&mut self) {
        if let Some(token) = self.cancel_token.take() {
            token.cancel();
        }
        self.scanning = false;
        self.scan_started_at = None;
        self.rx = None;
    }

    fn poll_scan(&mut self) {
        if let Some(rx) = &self.rx
            && let Ok(BleScanMessage::Finished(res)) = rx.try_recv()
        {
            self.scanning = false;
            self.scan_started_at = None;
            self.cancel_token = None;
            self.rx = None;
            match res {
                Ok(devs) => {
                    self.devices = sort_ble_devices(devs);
                }
                Err(err) => {
                    self.error_message = Some(err);
                }
            }
        }
    }

    pub fn render(
        &mut self,
        ui: &egui::Ui,
        handle: &Handle,
    ) -> Option<ConnectionRequest> {
        if !self.open {
            return None;
        }

        self.poll_scan();
        if self.scanning {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }

        let mut request = None;
        let mut close_requested = false;
        let mut rescan_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("connection_window_bluetooth");
        let builder = egui::ViewportBuilder::default()
            .with_title("Select Bluetooth Device")
            .with_inner_size([440.0, 360.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Device Type:");
                        egui::ComboBox::from_id_salt("ble_device_kind")
                            .selected_text(self.selected_kind.label())
                            .show_ui(ui, |ui| {
                                for kind in BleDeviceKind::ALL {
                                    ui.selectable_value(
                                        &mut self.selected_kind,
                                        kind,
                                        kind.label(),
                                    );
                                }
                            });
                    });

                    ui.separator();

                    if let Some(err) = &self.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, err);
                    }

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(ui.available_height() - 40.0)
                        .show(ui, |ui| {
                            if self.scanning {
                                let remaining = self
                                    .scan_started_at
                                    .map_or(0, |t| {
                                        self.scan_duration
                                            .saturating_sub(t.elapsed())
                                            .as_secs_f64()
                                            .ceil() as u64
                                    });
                                if remaining > 0 {
                                    ui.label(format!("Scanning... {remaining}s"));
                                } else {
                                    ui.label("Finishing scan...");
                                }
                            } else if self.devices.is_empty() {
                                ui.label("No Bluetooth devices found.");
                            } else {
                                for (idx, dev) in self.devices.iter().enumerate() {
                                    let prefix = if dev.is_priority { "☢ " } else { "" };
                                    let rssi_str = dev
                                        .rssi
                                        .map_or_else(String::new, |r| format!(" ({r} dBm)"));
                                    let label = format!(
                                        "{prefix}{} [{}]{rssi_str}",
                                        dev.name, dev.address
                                    );
                                    let resp = ui.selectable_label(
                                        self.selected_idx == Some(idx),
                                        label,
                                    );
                                    if resp.clicked() {
                                        self.selected_idx = Some(idx);
                                    }
                                    if resp.double_clicked() {
                                        self.selected_idx = Some(idx);
                                        request = Some(ConnectionRequest::Ble {
                                            name: dev.name.clone(),
                                            address: dev.address.clone(),
                                            kind: self.selected_kind,
                                        });
                                        close_requested = true;
                                    }
                                }
                            }
                        });

                    ui.separator();

                    ui.horizontal(|ui| {
                        let rescan_btn = ui.add_enabled(!self.scanning, egui::Button::new("Rescan"));
                        if rescan_btn.clicked() {
                            rescan_requested = true;
                        }

                        if ui.button("Cancel").clicked() {
                            close_requested = true;
                        }

                        let can_connect = self
                            .selected_idx
                            .and_then(|i| self.devices.get(i))
                            .is_some();
                        if ui
                            .add_enabled(can_connect, egui::Button::new("Connect"))
                            .clicked()
                            && let Some(dev) = self.selected_idx.and_then(|i| self.devices.get(i))
                        {
                            request = Some(ConnectionRequest::Ble {
                                name: dev.name.clone(),
                                address: dev.address.clone(),
                                kind: self.selected_kind,
                            });
                            close_requested = true;
                        }
                    });
                });
            });

        if rescan_requested {
            self.start_scan(handle, ui.ctx());
        }
        if close_requested {
            self.cancel_scan();
            self.open = false;
        }

        request
    }
}

pub fn sort_ble_devices(mut devices: Vec<BleScannedDevice>) -> Vec<BleScannedDevice> {
    devices.sort_by(|a, b| {
        b.is_priority
            .cmp(&a.is_priority)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    devices
}

async fn scan_ble_devices(
    duration: Duration,
    cancel: CancellationToken,
) -> Result<Vec<BleScannedDevice>, String> {
    let peripherals = crate::core::scan_ble_peripherals(duration, cancel).await?;
    Ok(peripherals
        .into_iter()
        .map(|dev| {
            let name = dev.name.unwrap_or_else(|| dev.address.clone());
            let lower = name.to_ascii_lowercase();
            let is_priority = lower.contains("radiacode") || lower.contains("raysid");
            BleScannedDevice {
                name,
                address: dev.address,
                rssi: dev.rssi,
                is_priority,
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// USB Connection Window
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbScannedDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: Option<String>,
    pub product: String,
    pub bus_id: String,
    pub device_address: u8,
    pub is_priority: bool,
}

pub struct UsbWindow {
    pub open: bool,
    scanning: bool,
    rx: Option<mpsc::Receiver<Result<Vec<UsbScannedDevice>, String>>>,
    devices: Vec<UsbScannedDevice>,
    selected_idx: Option<usize>,
    selected_kind: UsbDeviceKind,
    error_message: Option<String>,
}

impl Default for UsbWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbWindow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            scanning: false,
            rx: None,
            devices: Vec::new(),
            selected_idx: None,
            selected_kind: UsbDeviceKind::Auto,
            error_message: None,
        }
    }

    pub fn open_and_scan(&mut self, handle: &Handle, ctx: &egui::Context) {
        self.open = true;
        self.start_scan(handle, ctx);
    }

    pub fn start_scan(&mut self, handle: &Handle, ctx: &egui::Context) {
        self.devices.clear();
        self.selected_idx = None;
        self.error_message = None;
        self.scanning = true;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx_clone = ctx.clone();

        handle.spawn(async move {
            let res = scan_usb_devices().await;
            drop(tx.send(res));
            ctx_clone.request_repaint();
        });
    }

    fn poll_scan(&mut self) {
        if let Some(rx) = &self.rx
            && let Ok(res) = rx.try_recv()
        {
            self.scanning = false;
            self.rx = None;
            match res {
                Ok(devs) => {
                    self.devices = sort_usb_devices(devs);
                }
                Err(err) => {
                    self.error_message = Some(err);
                }
            }
        }
    }

    pub fn render(
        &mut self,
        ui: &egui::Ui,
        handle: &Handle,
    ) -> Option<ConnectionRequest> {
        if !self.open {
            return None;
        }

        self.poll_scan();

        let mut request = None;
        let mut close_requested = false;
        let mut rescan_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("connection_window_usb");
        let builder = egui::ViewportBuilder::default()
            .with_title("Select USB Device")
            .with_inner_size([460.0, 360.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Device Type:");
                        egui::ComboBox::from_id_salt("usb_device_kind")
                            .selected_text(self.selected_kind.label())
                            .show_ui(ui, |ui| {
                                for kind in UsbDeviceKind::ALL {
                                    ui.selectable_value(
                                        &mut self.selected_kind,
                                        kind,
                                        kind.label(),
                                    );
                                }
                            });
                    });

                    ui.separator();

                    if let Some(err) = &self.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, err);
                    }

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(ui.available_height() - 40.0)
                        .show(ui, |ui| {
                            if self.scanning {
                                ui.label("Scanning USB bus...");
                            } else if self.devices.is_empty() {
                                ui.label("No USB devices found.");
                            } else {
                                for (idx, dev) in self.devices.iter().enumerate() {
                                    let prefix = if dev.is_priority { "☢ " } else { "" };
                                    let serial_str = dev
                                        .serial
                                        .as_deref()
                                        .unwrap_or("no serial");
                                    let label = format!(
                                        "{prefix}{} ({:04x}:{:04x}, {serial_str})",
                                        dev.product, dev.vendor_id, dev.product_id
                                    );
                                    let resp = ui.selectable_label(
                                        self.selected_idx == Some(idx),
                                        label,
                                    );
                                    if resp.clicked() {
                                        self.selected_idx = Some(idx);
                                    }
                                    if resp.double_clicked() {
                                        self.selected_idx = Some(idx);
                                        request = Some(ConnectionRequest::Usb {
                                            vendor_id: dev.vendor_id,
                                            product_id: dev.product_id,
                                            serial: dev.serial.clone(),
                                            product: dev.product.clone(),
                                            kind: self.selected_kind,
                                        });
                                        close_requested = true;
                                    }
                                }
                            }
                        });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.scanning, egui::Button::new("Rescan"))
                            .clicked()
                        {
                            rescan_requested = true;
                        }

                        if ui.button("Cancel").clicked() {
                            close_requested = true;
                        }

                        let can_connect = self
                            .selected_idx
                            .and_then(|i| self.devices.get(i))
                            .is_some();
                        if ui
                            .add_enabled(can_connect, egui::Button::new("Connect"))
                            .clicked()
                            && let Some(dev) = self.selected_idx.and_then(|i| self.devices.get(i))
                        {
                            request = Some(ConnectionRequest::Usb {
                                vendor_id: dev.vendor_id,
                                product_id: dev.product_id,
                                serial: dev.serial.clone(),
                                product: dev.product.clone(),
                                kind: self.selected_kind,
                            });
                            close_requested = true;
                        }
                    });
                });
            });

        if rescan_requested {
            self.start_scan(handle, ui.ctx());
        }
        if close_requested {
            self.open = false;
        }

        request
    }
}

pub fn sort_usb_devices(mut devices: Vec<UsbScannedDevice>) -> Vec<UsbScannedDevice> {
    devices.sort_by(|a, b| {
        b.is_priority
            .cmp(&a.is_priority)
            .then_with(|| a.product.to_ascii_lowercase().cmp(&b.product.to_ascii_lowercase()))
    });
    devices
}

async fn scan_usb_devices() -> Result<Vec<UsbScannedDevice>, String> {
    let list = nusb::list_devices()
        .await
        .map_err(|e| format!("USB enumeration failed: {e}"))?;

    let mut out = Vec::new();
    for dev in list {
        let vid = dev.vendor_id();
        let pid = dev.product_id();
        let is_priority = matches!(vid, 0x0483 | 0x0A2D);
        let product = dev
            .product_string()
            .map_or_else(|| format!("USB {vid:04x}:{pid:04x}"), str::to_string);
        let serial = dev.serial_number().map(str::to_string);
        out.push(UsbScannedDevice {
            vendor_id: vid,
            product_id: pid,
            serial,
            product,
            bus_id: dev.bus_id().to_string(),
            device_address: dev.device_address(),
            is_priority,
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Network Connection Window
// ---------------------------------------------------------------------------

pub struct NetworkWindow {
    pub open: bool,
    pub address: String,
    pub kind: NetworkDeviceKind,
    pub use_gps: bool,
}

impl Default for NetworkWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkWindow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            address: String::new(),
            kind: NetworkDeviceKind::DetectiveX,
            use_gps: false,
        }
    }

    pub fn open_with_last_ip(&mut self, last_ip: &str) {
        if self.address.is_empty() && !last_ip.is_empty() {
            self.address = last_ip.to_string();
        }
        self.open = true;
    }

    pub fn render(&mut self, ui: &egui::Ui) -> Option<ConnectionRequest> {
        if !self.open {
            return None;
        }

        let mut request = None;
        let mut close_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("connection_window_network");
        let builder = egui::ViewportBuilder::default()
            .with_title("Connect Network Device")
            .with_inner_size([380.0, 190.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::Grid::new("network_connect_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Device IP:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.address)
                                    .desired_width(240.0)
                                    .hint_text("192.168.1.100"),
                            );
                            ui.end_row();

                            ui.label("Device Type:");
                            egui::ComboBox::from_id_salt("network_device_kind")
                                .selected_text(self.kind.label())
                                .show_ui(ui, |ui| {
                                    for kind in NetworkDeviceKind::ALL {
                                        ui.selectable_value(&mut self.kind, kind, kind.label());
                                    }
                                });
                            ui.end_row();

                            ui.label("");
                            ui.checkbox(&mut self.use_gps, "Use for GPS");
                            ui.end_row();
                        });

                    ui.add_space(12.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close_requested = true;
                        }
                        let valid = !self.address.trim().is_empty();
                        if ui
                            .add_enabled(valid, egui::Button::new("Connect"))
                            .clicked()
                        {
                            request = Some(ConnectionRequest::Network {
                                address: self.address.trim().to_string(),
                                kind: self.kind,
                                use_gps: self.use_gps,
                            });
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            self.open = false;
        }

        request
    }
}

// ---------------------------------------------------------------------------
// Serial GPS Connection Window
// ---------------------------------------------------------------------------

const BAUD_RATES: [u32; 8] = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];

#[derive(Debug, Clone)]
pub struct SerialPortEntry {
    pub port_name: String,
    pub description: String,
}

pub struct SerialGpsWindow {
    pub open: bool,
    pub port: String,
    pub baud_rate: u32,
    ports: Vec<SerialPortEntry>,
    error_message: Option<String>,
}

impl Default for SerialGpsWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialGpsWindow {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            port: String::new(),
            baud_rate: 4800,
            ports: Vec::new(),
            error_message: None,
        }
    }

    pub fn open_and_scan(&mut self) {
        self.open = true;
        self.refresh_ports();
    }

    pub fn refresh_ports(&mut self) {
        self.error_message = None;
        match serialport::available_ports() {
            Ok(list) => {
                self.ports = list
                    .into_iter()
                    .map(|p| {
                        let desc = match p.port_type {
                            serialport::SerialPortType::UsbPort(info) => info
                                .product
                                .unwrap_or_else(|| format!("USB {:04x}:{:04x}", info.vid, info.pid)),
                            serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                            serialport::SerialPortType::PciPort => "PCI".into(),
                            serialport::SerialPortType::Unknown => "Serial".into(),
                        };
                        SerialPortEntry {
                            port_name: p.port_name,
                            description: desc,
                        }
                    })
                    .collect();
            }
            Err(e) => {
                self.ports.clear();
                self.error_message = Some(format!("Failed to list serial ports: {e}"));
            }
        }
    }

    pub fn render(&mut self, ui: &egui::Ui, gps_connected: bool) -> Option<ConnectionRequest> {
        if !self.open {
            return None;
        }

        let mut request = None;
        let mut close_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("connection_window_serial_gps");
        let builder = egui::ViewportBuilder::default()
            .with_title("Connect USB Serial GPS")
            .with_inner_size([440.0, 320.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::Grid::new("serial_gps_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Port:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.port)
                                    .desired_width(260.0)
                                    .hint_text("/dev/ttyUSB0 or COM3"),
                            );
                            ui.end_row();

                            ui.label("Baud Rate:");
                            egui::ComboBox::from_id_salt("serial_gps_baud")
                                .selected_text(self.baud_rate.to_string())
                                .show_ui(ui, |ui| {
                                    for baud in BAUD_RATES {
                                        ui.selectable_value(
                                            &mut self.baud_rate,
                                            baud,
                                            baud.to_string(),
                                        );
                                    }
                                });
                            ui.end_row();
                        });

                    ui.add_space(6.0);
                    ui.label("Detected Ports:");

                    if let Some(err) = &self.error_message {
                        ui.colored_label(ui.visuals().error_fg_color, err);
                    }

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(ui.available_height() - 40.0)
                        .show(ui, |ui| {
                            if self.ports.is_empty() {
                                ui.label("No serial ports detected.");
                            } else {
                                for entry in &self.ports {
                                    let label =
                                        format!("{} ({})", entry.port_name, entry.description);
                                    let resp = ui.selectable_label(
                                        self.port == entry.port_name,
                                        label,
                                    );
                                    if resp.clicked() {
                                        self.port = entry.port_name.clone();
                                    }
                                    if resp.double_clicked() {
                                        self.port = entry.port_name.clone();
                                        request = Some(ConnectionRequest::SerialGps {
                                            port: entry.port_name.clone(),
                                            baud_rate: self.baud_rate,
                                        });
                                        close_requested = true;
                                    }
                                }
                            }
                        });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Rescan").clicked() {
                            self.refresh_ports();
                        }
                        if ui.button("Mock GPS").clicked() {
                            request = Some(ConnectionRequest::MockGps);
                            close_requested = true;
                        }
                        if ui
                            .add_enabled(gps_connected, egui::Button::new("Disconnect"))
                            .clicked()
                        {
                            request = Some(ConnectionRequest::DisconnectGps);
                            close_requested = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close_requested = true;
                        }
                        let valid = !self.port.trim().is_empty();
                        if ui
                            .add_enabled(valid, egui::Button::new("Connect"))
                            .clicked()
                        {
                            request = Some(ConnectionRequest::SerialGps {
                                port: self.port.trim().to_string(),
                                baud_rate: self.baud_rate,
                            });
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            self.open = false;
        }

        request
    }
}

// ---------------------------------------------------------------------------
// DriverLibraryDialog (port of luracs/gui/dialogs/driver_library_dialog.py)
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct DriverLibraryDialog {
    pub open: bool,
    drivers: Vec<std::path::PathBuf>,
    selected: Option<usize>,
    last_dir: Option<std::path::PathBuf>,
}

impl DriverLibraryDialog {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            open: false,
            drivers: Vec::new(),
            selected: None,
            last_dir: None,
        }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.selected = None;
        self.last_dir = None;
    }

    fn reload(&mut self, dir: &std::path::Path) {
        self.drivers.clear();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    self.drivers.push(p);
                }
            }
        }
        self.drivers.sort();
        self.last_dir = Some(dir.to_path_buf());
    }

    pub fn render(&mut self, ui: &egui::Ui, drivers_dir: &std::path::Path) {
        if !self.open {
            return;
        }
        if self.last_dir.as_deref() != Some(drivers_dir) {
            self.reload(drivers_dir);
        }

        let viewport_id = egui::ViewportId::from_hash_of("driver_library_dialog");
        let builder = egui::ViewportBuilder::default()
            .with_title("Driver Library")
            .with_inner_size([400.0, 300.0]);

        let mut close_requested = false;
        let mut delete_selected = false;

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(ui.available_height() - 36.0)
                        .show(ui, |ui| {
                            if self.drivers.is_empty() {
                                ui.label("No third-party drivers installed.");
                            } else {
                                for (idx, path) in self.drivers.iter().enumerate() {
                                    let name = path
                                        .file_name()
                                        .and_then(|s| s.to_str())
                                        .unwrap_or("<unknown>");
                                    if ui
                                        .selectable_label(self.selected == Some(idx), name)
                                        .clicked()
                                    {
                                        self.selected = Some(idx);
                                    }
                                }
                            }
                        });

                    ui.separator();
                    ui.horizontal(|ui| {
                        let has_sel = self.selected.is_some();
                        if ui
                            .add_enabled(has_sel, egui::Button::new("Delete Selected"))
                            .clicked()
                        {
                            delete_selected = true;
                        }
                        if ui.button("Close").clicked() {
                            close_requested = true;
                        }
                    });
                });
            });

        if delete_selected && let Some(idx) = self.selected.take()
            && let Some(path) = self.drivers.get(idx).cloned()
        {
            if path.is_file() {
                drop(std::fs::remove_file(&path));
            }
            self.reload(drivers_dir);
        }
        if close_requested {
            self.open = false;
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
    fn test_ble_device_kind_resolve() {
        assert_eq!(
            BleDeviceKind::Auto.resolve("RadiaCode-102 #1234"),
            Some(BleDeviceKind::Radiacode)
        );
        assert_eq!(
            BleDeviceKind::Auto.resolve("RaySID_55"),
            Some(BleDeviceKind::Raysid)
        );
        assert_eq!(BleDeviceKind::Auto.resolve("Unknown BLE"), None);
        assert_eq!(
            BleDeviceKind::Raysid.resolve("CustomName"),
            Some(BleDeviceKind::Raysid)
        );
    }

    #[test]
    fn test_usb_device_kind_resolve() {
        assert_eq!(
            UsbDeviceKind::Auto.resolve(0x0483, 0xF123, "Any"),
            Some(UsbDeviceKind::Radiacode)
        );
        assert_eq!(
            UsbDeviceKind::Auto.resolve(0x0A2D, 0x001F, "Any"),
            Some(UsbDeviceKind::DigiBaseRh)
        );
        assert_eq!(
            UsbDeviceKind::Auto.resolve(0x0A2D, 0x0005, "Any"),
            Some(UsbDeviceKind::DigiDart)
        );
        assert_eq!(
            UsbDeviceKind::Auto.resolve(0x0A2D, 0x000F, "Any"),
            Some(UsbDeviceKind::DigiBase)
        );
        assert_eq!(
            UsbDeviceKind::Auto.resolve(0x1234, 0x5678, "RadiaCode 103"),
            Some(UsbDeviceKind::Radiacode)
        );
        assert_eq!(UsbDeviceKind::Auto.resolve(0x1234, 0x5678, "Mouse"), None);
    }

    #[test]
    fn test_ble_priority_sorting() {
        let devs = vec![
            BleScannedDevice {
                name: "Alpha".into(),
                address: "00:01".into(),
                rssi: Some(-70),
                is_priority: false,
            },
            BleScannedDevice {
                name: "RaySID".into(),
                address: "00:02".into(),
                rssi: Some(-60),
                is_priority: true,
            },
            BleScannedDevice {
                name: "RadiaCode-102".into(),
                address: "00:03".into(),
                rssi: Some(-50),
                is_priority: true,
            },
        ];

        let sorted = sort_ble_devices(devs);
        assert_eq!(sorted[0].name, "RadiaCode-102");
        assert_eq!(sorted[1].name, "RaySID");
        assert_eq!(sorted[2].name, "Alpha");
    }

    #[test]
    fn test_usb_priority_sorting() {
        let devs = vec![
            UsbScannedDevice {
                vendor_id: 0x1111,
                product_id: 0x2222,
                serial: None,
                product: "Keyboard".into(),
                bus_id: "1".into(),
                device_address: 1,
                is_priority: false,
            },
            UsbScannedDevice {
                vendor_id: 0x0483,
                product_id: 0xF123,
                serial: Some("RC-1".into()),
                product: "RadiaCode".into(),
                bus_id: "1".into(),
                device_address: 2,
                is_priority: true,
            },
        ];

        let sorted = sort_usb_devices(devs);
        assert_eq!(sorted[0].product, "RadiaCode");
        assert_eq!(sorted[1].product, "Keyboard");
    }

    #[test]
    fn test_driver_library_dialog_reload() {
        let mut dialog = DriverLibraryDialog::new();
        assert!(!dialog.open);
        assert!(dialog.drivers.is_empty());

        dialog.open();
        assert!(dialog.open);

        let dir = std::env::temp_dir().join("luracs_test_driver_library");
        let _res = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("zebra.so"), b"").unwrap();
        std::fs::write(dir.join("alpha.py"), b"").unwrap();
        std::fs::create_dir_all(dir.join("subdir")).unwrap();

        dialog.reload(&dir);
        assert_eq!(dialog.drivers.len(), 2);
        assert_eq!(dialog.drivers[0].file_name().unwrap(), "alpha.py");
        assert_eq!(dialog.drivers[1].file_name().unwrap(), "zebra.so");
        assert_eq!(dialog.last_dir.as_deref(), Some(dir.as_path()));

        let missing = dir.join("missing");
        dialog.reload(&missing);
        assert!(dialog.drivers.is_empty());
        assert_eq!(dialog.last_dir.as_deref(), Some(missing.as_path()));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
