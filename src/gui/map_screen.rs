//! Map screen powered by `walkers` (native egui Slippy Map with raster,
//! MVT vector, and `.pmtiles` support — no WebEngine required).
//!
//! Ported from `luracs/gui/mapping.py`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32, Pos2, Rect};
use rsplot::{Colormap, ColormapName};
use serde_json::{json, Value};
use walkers::sources::{Attribution, TileSource};
use walkers::{lat_lon, HttpTiles, Map, MapMemory, Plugin, PmTiles, Position, Projector, TileId, Tiles};

use crate::app::LuRaCsApp;
use crate::clients::{format_gps, GpsData};
use crate::core::{MappingBuffer, SpectrogramRoi};
use crate::gui::{ConfirmAction, ConfirmModal, InfoModal, Modal, ModalKind};
use crate::settings::SharedSettingsExt;

// ---------------------------------------------------------------------------
// Colormaps
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColormapPreset {
    #[default]
    Thermal,
    Flame,
    Yellowy,
    Bipolar,
    Spectrum,
    Viridis,
    Plasma,
    Inferno,
    Magma,
    Cividis,
    Turbo,
    Grayscale,
}

impl ColormapPreset {
    pub const ALL: [Self; 12] = [
        Self::Thermal,
        Self::Flame,
        Self::Yellowy,
        Self::Bipolar,
        Self::Spectrum,
        Self::Viridis,
        Self::Plasma,
        Self::Inferno,
        Self::Magma,
        Self::Cividis,
        Self::Turbo,
        Self::Grayscale,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Thermal => "Thermal",
            Self::Flame => "Flame",
            Self::Yellowy => "Yellowy",
            Self::Bipolar => "Bipolar",
            Self::Spectrum => "Spectrum",
            Self::Viridis => "Viridis",
            Self::Plasma => "Plasma",
            Self::Inferno => "Inferno",
            Self::Magma => "Magma",
            Self::Cividis => "Cividis",
            Self::Turbo => "Turbo",
            Self::Grayscale => "Grayscale",
        }
    }

    #[must_use]
    pub fn sample(self, t: f64) -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        match self {
            // Exact PyQtGraph GradientPresets stop definitions
            Self::Thermal => sample_stops(
                &[
                    (0.0, [0.0, 0.0, 0.0]),
                    (0.3333, [185.0, 0.0, 0.0]),
                    (0.6666, [255.0, 220.0, 0.0]),
                    (1.0, [255.0, 255.0, 255.0]),
                ],
                t,
            ),
            Self::Flame => sample_stops(
                &[
                    (0.0, [0.0, 0.0, 0.0]),
                    (0.2, [7.0, 0.0, 220.0]),
                    (0.5, [236.0, 0.0, 134.0]),
                    (0.8, [246.0, 246.0, 0.0]),
                    (1.0, [255.0, 255.0, 255.0]),
                ],
                t,
            ),
            Self::Yellowy => sample_stops(
                &[
                    (0.0, [0.0, 0.0, 0.0]),
                    (0.2329, [32.0, 0.0, 129.0]),
                    (0.5258, [115.0, 15.0, 255.0]),
                    (0.8363, [255.0, 255.0, 0.0]),
                    (1.0, [255.0, 255.0, 255.0]),
                ],
                t,
            ),
            Self::Bipolar => sample_stops(
                &[
                    (0.0, [0.0, 255.0, 255.0]),
                    (0.25, [0.0, 0.0, 255.0]),
                    (0.5, [0.0, 0.0, 0.0]),
                    (0.75, [255.0, 0.0, 0.0]),
                    (1.0, [255.0, 255.0, 0.0]),
                ],
                t,
            ),
            Self::Spectrum => hsv_to_rgb(t * 300.0, 1.0, 1.0),
            Self::Viridis => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Viridis, 0.0, 1.0).lut), t)
            }
            Self::Plasma => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Plasma, 0.0, 1.0).lut), t)
            }
            Self::Inferno => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Inferno, 0.0, 1.0).lut), t)
            }
            Self::Magma => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Magma, 0.0, 1.0).lut), t)
            }
            Self::Cividis => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Cividis, 0.0, 1.0).lut), t)
            }
            Self::Turbo => {
                static LUT: OnceLock<[[u8; 4]; 256]> = OnceLock::new();
                sample_lut(LUT.get_or_init(|| Colormap::new(ColormapName::Turbo, 0.0, 1.0).lut), t)
            }
            Self::Grayscale => sample_stops(&[(0.0, [0.0, 0.0, 0.0]), (1.0, [255.0, 255.0, 255.0])], t),
        }
    }
}

fn sample_lut(lut: &[[u8; 4]; 256], t: f64) -> [u8; 3] {
    let idx = (t * 255.0).round().clamp(0.0, 255.0) as usize;
    let [r, g, b, _] = lut[idx];
    [r, g, b]
}

fn sample_stops(stops: &[(f64, [f64; 3])], t: f64) -> [u8; 3] {
    for win in stops.windows(2) {
        let (p0, c0) = win[0];
        let (p1, c1) = win[1];
        if t <= p1 {
            let span = (p1 - p0).max(f64::EPSILON);
            let frac = ((t - p0) / span).clamp(0.0, 1.0);
            return [
                (c1[0] - c0[0]).mul_add(frac, c0[0]).round() as u8,
                (c1[1] - c0[1]).mul_add(frac, c0[1]).round() as u8,
                (c1[2] - c0[2]).mul_add(frac, c0[2]).round() as u8,
            ];
        }
    }
    let last = stops[stops.len() - 1].1;
    [last[0] as u8, last[1] as u8, last[2] as u8]
}

fn hsv_to_rgb(h_deg: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let h = h_deg / 60.0;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if h < 1.0 {
        (c, x, 0.0)
    } else if h < 2.0 {
        (x, c, 0.0)
    } else if h < 3.0 {
        (0.0, c, x)
    } else if h < 4.0 {
        (0.0, x, c)
    } else if h < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = v - c;
    [
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    ]
}

// ---------------------------------------------------------------------------
// Walkers Custom TileSource (supports any online raster or vector {z}/{x}/{y} URL)
// ---------------------------------------------------------------------------

struct CustomUrlSource {
    template: String,
    is_vector: bool,
}

impl TileSource for CustomUrlSource {
    fn tile_url(&self, tile_id: TileId) -> String {
        self.template
            .replace("{z}", &tile_id.zoom.to_string())
            .replace("{x}", &tile_id.x.to_string())
            .replace("{y}", &tile_id.y.to_string())
    }

    fn attribution(&self) -> Attribution {
        Attribution {
            text: "Custom Map",
            url: "",
            logo_light: None,
            logo_dark: None,
        }
    }

    fn tile_size(&self) -> u32 {
        if self.is_vector { 512 } else { 256 }
    }
}

// ---------------------------------------------------------------------------
// Track Data & GeoJSON / RCTRK Import/Export
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct MapTrack {
    pub timestamps: Vec<f64>,
    pub longitudes: Vec<Option<f64>>,
    pub latitudes: Vec<Option<f64>>,
    pub count_rate: Vec<f64>,
    pub dose_rate: Vec<f64>,
    pub roi_counts: HashMap<String, Vec<f64>>,
}

impl MapTrack {
    #[must_use]
    pub fn from_mapping_buffer(buf: &MappingBuffer) -> Self {
        let longitudes = buf
            .gps
            .iter()
            .map(|g| g.as_ref().filter(|d| d.valid).and_then(|d| d.longitude))
            .collect();
        let latitudes = buf
            .gps
            .iter()
            .map(|g| g.as_ref().filter(|d| d.valid).and_then(|d| d.latitude))
            .collect();
        Self {
            timestamps: buf.timestamps.clone(),
            longitudes,
            latitudes,
            count_rate: buf.count_rate.clone(),
            dose_rate: buf.dose_rate.clone(),
            roi_counts: (*buf.roi_counts).clone(),
        }
    }

    #[must_use]
    pub fn values_for(&self, data_key: &str) -> Option<&[f64]> {
        match data_key {
            "count_rate" => Some(&self.count_rate),
            "dose_rate" => Some(&self.dose_rate),
            tag => self.roi_counts.get(tag).map(Vec::as_slice),
        }
    }

    #[must_use]
    pub fn to_geojson(&self) -> Value {
        let mut features = Vec::new();
        for i in 0..self.timestamps.len() {
            let (Some(lon), Some(lat)) = (
                self.longitudes.get(i).copied().flatten(),
                self.latitudes.get(i).copied().flatten(),
            ) else {
                continue;
            };

            let mut props = serde_json::Map::new();
            props.insert("timestamp".into(), json!(self.timestamps[i]));
            if let Some(&cps) = self.count_rate.get(i) {
                props.insert("count_rate".into(), json!(cps));
            }
            if let Some(&dr) = self.dose_rate.get(i) {
                props.insert("dose_rate".into(), json!(dr));
            }
            for (tag, vals) in &self.roi_counts {
                if let Some(&v) = vals.get(i) {
                    props.insert(tag.clone(), json!(v));
                }
            }

            features.push(json!({
                "type": "Feature",
                "geometry": {
                    "type": "Point",
                    "coordinates": [lon, lat],
                },
                "properties": props,
            }));
        }

        json!({
            "type": "FeatureCollection",
            "features": features,
        })
    }

    pub fn import_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "geojson" || content.trim_start().starts_with('{') {
            Self::parse_geojson(&content)
        } else {
            Self::parse_rctrk(&content)
        }
    }

    fn parse_geojson(content: &str) -> Result<Self, String> {
        let root: Value =
            serde_json::from_str(content).map_err(|e| format!("Invalid GeoJSON: {e}"))?;
        let features = root
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(|| "GeoJSON missing 'features' array".to_string())?;

        let mut track = Self::default();
        for feat in features {
            let coords = feat
                .get("geometry")
                .and_then(|g| g.get("coordinates"))
                .and_then(Value::as_array);
            let (Some(lon), Some(lat)) = (
                coords.and_then(|c| c.first()).and_then(Value::as_f64),
                coords.and_then(|c| c.get(1)).and_then(Value::as_f64),
            ) else {
                continue;
            };

            let props = feat.get("properties").and_then(Value::as_object);
            let ts = props
                .and_then(|p| p.get("timestamp"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let cps = props
                .and_then(|p| p.get("count_rate").or_else(|| p.get("cps")))
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN);
            let dr = props
                .and_then(|p| p.get("dose_rate").or_else(|| p.get("dr")))
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN);

            track.timestamps.push(ts);
            track.longitudes.push(Some(lon));
            track.latitudes.push(Some(lat));
            track.count_rate.push(cps);
            track.dose_rate.push(dr);
        }

        Ok(track)
    }

    fn parse_rctrk(content: &str) -> Result<Self, String> {
        let mut track = Self::default();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("Timestamp") {
                continue;
            }
            let sep = if trimmed.contains('\t') { '\t' } else { ',' };
            let cols: Vec<&str> = trimmed.split(sep).map(str::trim).collect();
            if cols.len() < 4 {
                continue;
            }
            let ts = cols[0].parse::<f64>().unwrap_or(0.0);
            let Ok(lat) = cols[1].parse::<f64>() else {
                continue;
            };
            let Ok(lon) = cols[2].parse::<f64>() else {
                continue;
            };
            let dr = cols[3].parse::<f64>().unwrap_or(f64::NAN);
            let cps = cols
                .get(4)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(f64::NAN);

            track.timestamps.push(ts);
            track.longitudes.push(Some(lon));
            track.latitudes.push(Some(lat));
            track.count_rate.push(cps);
            track.dose_rate.push(dr);
        }
        if track.timestamps.is_empty() {
            return Err("No valid track points found in .rctrk file".into());
        }
        Ok(track)
    }
}

// ---------------------------------------------------------------------------
// Load Online Map Dialog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LoadOnlineMapDialog {
    pub open: bool,
    pub url: String,
    pub is_vector: bool,
    pub save_url: bool,
}

impl Default for LoadOnlineMapDialog {
    fn default() -> Self {
        Self {
            open: false,
            url: String::new(),
            is_vector: false,
            save_url: true,
        }
    }
}

impl LoadOnlineMapDialog {
    pub fn open_with_url(&mut self, last_url: &str) {
        if self.url.is_empty() {
            if last_url.is_empty() {
                self.url = "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into();
            } else {
                self.url = last_url.to_string();
            }
        }
        self.open = true;
    }

    fn render(&mut self, ui: &egui::Ui) -> Option<(String, bool, bool)> {
        if !self.open {
            return None;
        }

        let mut submitted = None;
        let mut close_requested = false;

        let viewport_id = egui::ViewportId::from_hash_of("map_load_online_dialog");
        let builder = egui::ViewportBuilder::default()
            .with_title("Load Online Map")
            .with_inner_size([560.0, 175.0])
            .with_resizable(false);

        ui.ctx()
            .show_viewport_immediate(viewport_id, builder, |ui, _class| {
                if ui.input(|i| i.viewport().close_requested()) {
                    close_requested = true;
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    egui::Grid::new("load_online_map_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Source URL (+ API Key):");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.url)
                                    .desired_width(360.0)
                                    .hint_text("https://tile.openstreetmap.org/{z}/{x}/{y}.png"),
                            );
                            ui.end_row();

                            ui.label("Tile Type:");
                            egui::ComboBox::from_id_salt("online_map_tile_type")
                                .selected_text(if self.is_vector {
                                    "Vector Tiles"
                                } else {
                                    "Raster Tiles"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.is_vector, false, "Raster Tiles");
                                    ui.selectable_value(&mut self.is_vector, true, "Vector Tiles");
                                });
                            ui.end_row();

                            ui.label("");
                            ui.checkbox(&mut self.save_url, "Save URL");
                            ui.end_row();
                        });

                    ui.add_space(10.0);
                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            close_requested = true;
                        }
                        if ui
                            .add_enabled(!self.url.trim().is_empty(), egui::Button::new("OK"))
                            .clicked()
                        {
                            submitted =
                                Some((self.url.trim().to_string(), self.is_vector, self.save_url));
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested {
            self.open = false;
        }

        submitted
    }
}

// ---------------------------------------------------------------------------
// Walkers Plugin for Track Points, Hover Coordinates & GPS Marker
// ---------------------------------------------------------------------------

struct TrackOverlayPlugin<'a> {
    track: Option<&'a MapTrack>,
    data_key: &'a str,
    colormap: ColormapPreset,
    level_min: f64,
    level_max: f64,
    alpha: u8,
    gps_pos: Option<Position>,
    hover_out: &'a mut Option<(f64, f64)>,
}

impl Plugin for TrackOverlayPlugin<'_> {
    fn run(
        self: Box<Self>,
        ui: &mut egui::Ui,
        response: &egui::Response,
        projector: &Projector<'_>,
        _map_memory: &MapMemory,
    ) {
        let rect = response.rect;
        let painter = ui.painter_at(rect);

        if let Some(hp) = response.hover_pos() {
            let pos = projector.unproject(hp.to_vec2());
            *self.hover_out = Some((pos.y(), pos.x()));
        }

        let mut hovered_info: Option<(f64, f64)> = None;
        let mut best_dist_sq = 100.0_f32;

        if let Some(track) = self.track
            && let Some(values) = track.values_for(self.data_key)
        {
            let span = (self.level_max - self.level_min).max(1e-4);
            for (i, &val) in values.iter().enumerate() {
                if !val.is_finite() {
                    continue;
                }
                let (Some(lon), Some(lat)) = (
                    track.longitudes.get(i).copied().flatten(),
                    track.latitudes.get(i).copied().flatten(),
                ) else {
                    continue;
                };

                let screen_pos = projector.project(lat_lon(lat, lon)).to_pos2();
                if !rect.contains(screen_pos) {
                    continue;
                }

                let t = ((val - self.level_min) / span).clamp(0.0, 1.0);
                let [r, g, b] = self.colormap.sample(t);
                let color = Color32::from_rgba_unmultiplied(r, g, b, self.alpha);
                painter.circle_filled(screen_pos, 5.0, color);

                if let Some(hp) = response.hover_pos() {
                    let d2 = screen_pos.distance_sq(hp);
                    if d2 < best_dist_sq {
                        best_dist_sq = d2;
                        let ts = track.timestamps.get(i).copied().unwrap_or(0.0);
                        hovered_info = Some((ts, val));
                    }
                }
            }
        }

        if let Some((ts, val)) = hovered_info {
            let dt_str = DateTime::<Utc>::from_timestamp(ts.round() as i64, 0).map_or_else(
                || format!("{ts:.0}"),
                |dt| {
                    dt.with_timezone(&Local)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                },
            );
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                ui.make_persistent_id("map_point_tooltip"),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.label(format!("TS: {dt_str}, Value: {val:.2}"));
            });
        }

        if let Some(gps) = self.gps_pos {
            let pos = projector.project(gps).to_pos2();
            if rect.contains(pos) {
                painter.circle_filled(pos, 7.0, Color32::from_rgb(30, 136, 229));
                painter.circle_stroke(pos, 7.0, egui::Stroke::new(2.0, Color32::WHITE));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MapScreen
// ---------------------------------------------------------------------------

enum ActiveTiles {
    Http(HttpTiles),
    PmTiles(PmTiles),
}

impl ActiveTiles {
    fn as_tiles_mut(&mut self) -> &mut dyn Tiles {
        match self {
            Self::Http(t) => t,
            Self::PmTiles(t) => t,
        }
    }
}

pub struct MapScreen {
    map_memory: MapMemory,
    tiles: Option<ActiveTiles>,
    loaded_map_name: String,
    hover_lat_lon: Option<(f64, f64)>,

    tracks: HashMap<String, MapTrack>,
    track_names: Vec<String>,
    selected_track: String,
    data_kinds: Vec<(String, String)>,
    selected_data_key: String,

    colormap: ColormapPreset,
    level_min: f64,
    level_max: f64,
    auto_levels: bool,

    gps_connected: bool,
    latest_gps: Option<GpsData>,
    pub track_current_location: bool,
    last_move_to_current: Option<Instant>,

    online_dialog: LoadOnlineMapDialog,
}

impl Default for MapScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl MapScreen {
    #[must_use]
    pub fn new() -> Self {
        let mut map_memory = MapMemory::default();
        drop(map_memory.set_zoom(13.0));
        map_memory.center_at(lat_lon(55.591_592_4, 12.998_014_9));

        Self {
            map_memory,
            tiles: None,
            loaded_map_name: String::new(),
            hover_lat_lon: None,
            tracks: HashMap::new(),
            track_names: Vec::new(),
            selected_track: String::new(),
            data_kinds: vec![
                ("Count Rate [/s]".into(), "count_rate".into()),
                ("Dose Rate [µSv/h]".into(), "dose_rate".into()),
            ],
            selected_data_key: "count_rate".into(),
            colormap: ColormapPreset::default(),
            level_min: 0.0,
            level_max: 0.2,
            auto_levels: true,
            gps_connected: false,
            latest_gps: None,
            track_current_location: false,
            last_move_to_current: None,
            online_dialog: LoadOnlineMapDialog::default(),
        }
    }

    // -----------------------------------------------------------------------
    // Event hooks (called from LuRaCsApp::dispatch_all_events)
    // -----------------------------------------------------------------------

    pub fn on_spectrogram_added(&mut self, name: &str) {
        if !self.track_names.iter().any(|n| n == name) {
            self.track_names.push(name.to_string());
        }
        self.tracks.entry(name.to_string()).or_default();
        if self.selected_track.is_empty() {
            self.selected_track = name.to_string();
        }
    }

    pub fn on_spectrogram_closed(&mut self, name: &str) {
        self.track_names.retain(|n| n != name);
        self.tracks.remove(name);
        if self.selected_track == name {
            self.selected_track = self.track_names.first().cloned().unwrap_or_default();
        }
    }

    pub fn on_roi_added(&mut self, roi: &SpectrogramRoi) {
        if !self.data_kinds.iter().any(|(_, k)| k == &roi.tag) {
            self.data_kinds.push((roi.alias.clone(), roi.tag.clone()));
        }
    }

    pub fn on_roi_removed(&mut self, tag: &str) {
        self.data_kinds.retain(|(_, k)| k != tag);
        if self.selected_data_key == tag {
            self.selected_data_key = "count_rate".into();
        }
        for track in self.tracks.values_mut() {
            track.roi_counts.remove(tag);
        }
    }

    pub fn on_map_buffer_updated(&mut self, logger_name: &str, buffer: &MappingBuffer) {
        self.on_spectrogram_added(logger_name);
        let track = MapTrack::from_mapping_buffer(buffer);
        self.tracks.insert(logger_name.to_string(), track);
        if self.auto_levels && logger_name == self.selected_track {
            self.recompute_auto_levels();
        }
    }

    pub fn on_gps_connection(&mut self, connected: bool) {
        self.gps_connected = connected;
        if !connected {
            self.latest_gps = None;
        }
    }

    pub fn on_gps_updated(&mut self, data: &GpsData, max_rate_s: f64) {
        self.latest_gps = Some(data.clone());
        if self.track_current_location
            && data.valid
            && let (Some(lat), Some(lon)) = (data.latitude, data.longitude)
        {
            let ready = self
                .last_move_to_current
                .is_none_or(|t| t.elapsed().as_secs_f64() >= max_rate_s);
            if ready {
                self.map_memory.center_at(lat_lon(lat, lon));
                self.last_move_to_current = Some(Instant::now());
            }
        }
    }

    // -----------------------------------------------------------------------
    // View -> Map menu actions
    // -----------------------------------------------------------------------

    pub fn move_to_current(&mut self) {
        if let Some(gps) = &self.latest_gps
            && gps.valid
            && let (Some(lat), Some(lon)) = (gps.latitude, gps.longitude)
        {
            self.map_memory.center_at(lat_lon(lat, lon));
        }
    }

    pub fn move_to_start(&mut self) {
        let Some(track) = self.tracks.get(&self.selected_track) else {
            return;
        };
        for (lon_opt, lat_opt) in track.longitudes.iter().zip(track.latitudes.iter()) {
            if let (Some(lon), Some(lat)) = (lon_opt, lat_opt) {
                self.map_memory.center_at(lat_lon(*lat, *lon));
                return;
            }
        }
    }

    pub fn move_to_end(&mut self) {
        let Some(track) = self.tracks.get(&self.selected_track) else {
            return;
        };
        for (lon_opt, lat_opt) in track.longitudes.iter().zip(track.latitudes.iter()).rev() {
            if let (Some(lon), Some(lat)) = (lon_opt, lat_opt) {
                self.map_memory.center_at(lat_lon(*lat, *lon));
                return;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Map loading & LUT helpers
    // -----------------------------------------------------------------------

    fn data_range(&self) -> (f64, f64) {
        let mut dmin = f64::INFINITY;
        let mut dmax = f64::NEG_INFINITY;
        if let Some(vals) = self
            .tracks
            .get(&self.selected_track)
            .and_then(|t| t.values_for(&self.selected_data_key))
        {
            for &v in vals {
                if v.is_finite() {
                    dmin = dmin.min(v);
                    dmax = dmax.max(v);
                }
            }
        }
        if !dmin.is_finite() || !dmax.is_finite() {
            dmin = self.level_min.min(0.0);
            dmax = self.level_max.max(1.0);
        }
        let lo = dmin.min(self.level_min);
        let hi = dmax.max(self.level_max).max(lo + 1e-4);
        (lo, hi)
    }

    fn recompute_auto_levels(&mut self) {
        let Some(track) = self.tracks.get(&self.selected_track) else {
            return;
        };
        let Some(vals) = track.values_for(&self.selected_data_key) else {
            return;
        };
        let mut valid: Vec<f64> = vals.iter().copied().filter(|v| v.is_finite()).collect();
        if valid.is_empty() {
            return;
        }
        valid.sort_by(f64::total_cmp);
        let lo_idx = ((valid.len() - 1) as f64 * 0.01).round() as usize;
        let hi_idx = ((valid.len() - 1) as f64 * 0.99).round() as usize;
        self.level_min = valid[lo_idx];
        self.level_max = valid[hi_idx].max(self.level_min + 0.01);
    }

    fn set_online_source(&mut self, url: &str, is_vector: bool, ctx: &egui::Context) {
        let source = CustomUrlSource {
            template: url.to_string(),
            is_vector,
        };
        self.tiles = Some(ActiveTiles::Http(HttpTiles::new(source, ctx.clone())));
        self.loaded_map_name = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches("/{z}/{x}/{y}.png")
            .trim_end_matches("/{z}/{x}/{y}.pbf")
            .to_string();
    }

    fn set_offline_pmtiles(&mut self, path: &Path, ctx: &egui::Context) {
        self.tiles = Some(ActiveTiles::PmTiles(PmTiles::new(path, ctx.clone())));
        self.loaded_map_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("offline.pmtiles")
            .to_string();
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    pub fn render(app: &mut LuRaCsApp, ui: &mut egui::Ui) {
        let point_alpha = app.settings.read_settings().gps.point_alpha;

        let mut action_load_offline = false;
        let mut action_load_online = false;
        let mut action_import_track = false;
        let mut action_export_geojson = false;

        // --- 1. Top Toolbar ---
        ui.horizontal(|ui| {
            ui.menu_button("Map Options", |ui| {
                if ui.button("Load Offline Map (.pmtiles)").clicked() {
                    action_load_offline = true;
                    ui.close();
                }
                if ui.button("Load Online Map").clicked() {
                    action_load_online = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Import Simple Track").clicked() {
                    action_import_track = true;
                    ui.close();
                }
                if ui.button("To .geojson").clicked() {
                    action_export_geojson = true;
                    ui.close();
                }
            });

            let (lon_str, lat_str) = app.map_screen.hover_lat_lon.map_or_else(
                || ("---".to_string(), "---".to_string()),
                |(lat, lon)| (format!("{lon:.6}"), format!("{lat:.6}")),
            );
            ui.label(format!(
                "Longitude: {lon_str}   Latitude: {lat_str}   Loaded Map: {}",
                app.map_screen.loaded_map_name
            ));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let curr_label = app
                    .map_screen
                    .data_kinds
                    .iter()
                    .find(|(_, k)| k == &app.map_screen.selected_data_key)
                    .map_or("Count Rate [/s]", |(l, _)| l.as_str())
                    .to_string();
                let prev_key = app.map_screen.selected_data_key.clone();
                egui::ComboBox::from_id_salt("map_data_kind_combo")
                    .selected_text(curr_label)
                    .show_ui(ui, |ui| {
                        for (label, key) in &app.map_screen.data_kinds {
                            ui.selectable_value(
                                &mut app.map_screen.selected_data_key,
                                key.clone(),
                                label,
                            );
                        }
                    });
                if app.map_screen.selected_data_key != prev_key && app.map_screen.auto_levels {
                    app.map_screen.recompute_auto_levels();
                }

                let prev_track = app.map_screen.selected_track.clone();
                egui::ComboBox::from_id_salt("map_track_combo")
                    .width(160.0)
                    .selected_text(if prev_track.is_empty() {
                        "(no track)"
                    } else {
                        &prev_track
                    })
                    .show_ui(ui, |ui| {
                        for name in &app.map_screen.track_names {
                            ui.selectable_value(
                                &mut app.map_screen.selected_track,
                                name.clone(),
                                name,
                            );
                        }
                    });
                if app.map_screen.selected_track != prev_track && app.map_screen.auto_levels {
                    app.map_screen.recompute_auto_levels();
                }
            });
        });

        ui.separator();

        // --- 2. Bottom GPS status bar, Right LUT Bar, Central Walkers Map ---
        let gps_text = app
            .map_screen
            .latest_gps
            .as_ref()
            .map_or_else(|| "GPS: Disconnected".to_string(), format_gps);
        egui::Panel::bottom("map_gps_bar").show(ui, |ui| {
            ui.label(gps_text);
        });

        let ms = &mut app.map_screen;
        egui::Panel::right("map_lut_bar")
            .exact_size(115.0)
            .resizable(false)
            .show(ui, |ui| {
                let h = ui.available_height();
                draw_lut_sidebar(ms, h, ui);
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let gps_pos = if ms.gps_connected {
                ms.latest_gps
                    .as_ref()
                    .filter(|g| g.valid)
                    .and_then(|g| Some(lat_lon(g.latitude?, g.longitude?)))
            } else {
                None
            };
            let my_position = gps_pos.unwrap_or_else(|| lat_lon(55.591_592_4, 12.998_014_9));
            let mut tiles = ms.tiles.take();

            let active_track = ms.tracks.get(&ms.selected_track);
            let plugin = TrackOverlayPlugin {
                track: active_track,
                data_key: &ms.selected_data_key,
                colormap: ms.colormap,
                level_min: ms.level_min,
                level_max: ms.level_max,
                alpha: point_alpha,
                gps_pos,
                hover_out: &mut ms.hover_lat_lon,
            };

            let tiles_ref = tiles.as_mut().map(ActiveTiles::as_tiles_mut);
            let map_widget =
                Map::new(tiles_ref, &mut ms.map_memory, my_position).with_plugin(plugin);
            ui.add(map_widget);
            ms.tiles = tiles;
        });

        // --- 4. Actions & Modals ---
        if action_load_online {
            let last_url = app.settings.read_settings().session.last_map_url.clone();
            app.map_screen.online_dialog.open_with_url(&last_url);
        }

        if let Some((url, mut is_vector, save_url)) = app.map_screen.online_dialog.render(ui) {
            if save_url {
                app.settings.write_settings().session.last_map_url = url.clone();
            }
            if is_vector && (url.ends_with(".png") || url.ends_with(".jpg") || url.ends_with(".jpeg")) {
                is_vector = false;
                app.modal_stack.push(Modal::root(ModalKind::Info(InfoModal::new(
                    "Raster Detected",
                    "The given URL was identified as raster but vector was chosen. The map will be loaded as raster.",
                ))));
            }
            if !url.contains("{z}/{x}/{y}") {
                let url_clone = url;
                let ctx_clone = ui.ctx().clone();
                app.modal_stack.push(Modal::root(ModalKind::Confirm(
                    ConfirmModal::new(
                        "Invalid Tile Pattern",
                        "The given URL does not contain '{z}/{x}/{y}'. Continue anyway?",
                        ConfirmAction::Custom(Box::new(move |app| {
                            app.map_screen
                                .set_online_source(&url_clone, is_vector, &ctx_clone);
                        })),
                    ),
                )));
            } else {
                app.map_screen.set_online_source(&url, is_vector, ui.ctx());
            }
        }

        if action_load_offline
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PMTiles", &["pmtiles"])
                .pick_file()
        {
            app.map_screen.set_offline_pmtiles(&path, ui.ctx());
        }

        if action_import_track {
            let last_dir = app.settings.read_settings().paths.last_opened_dir.clone();
            let mut dialog =
                rfd::FileDialog::new().add_filter("Track Files", &["geojson", "rctrk"]);
            if last_dir.is_dir() {
                dialog = dialog.set_directory(&last_dir);
            }
            if let Some(path) = dialog.pick_file() {
                match MapTrack::import_from_file(&path) {
                    Ok(track) => {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Imported Track")
                            .to_string();
                        app.map_screen.on_spectrogram_added(&name);
                        app.map_screen.tracks.insert(name.clone(), track);
                        app.map_screen.selected_track = name;
                        app.map_screen.recompute_auto_levels();
                    }
                    Err(e) => {
                        app.modal_stack.push(Modal::root(ModalKind::Info(
                            InfoModal::new("Import Track Failed", e),
                        )));
                    }
                }
            }
        }

        if action_export_geojson {
            let track_name = app.map_screen.selected_track.clone();
            if let Some(track) = app.map_screen.tracks.get(&track_name) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("GeoJSON", &["geojson"])
                    .set_file_name(format!("{track_name}.geojson"))
                    .save_file()
                {
                    let geojson = track.to_geojson();
                    match serde_json::to_string_pretty(&geojson)
                        .map_err(|e| e.to_string())
                        .and_then(|s| std::fs::write(&path, s).map_err(|e| e.to_string()))
                    {
                        Ok(()) => {
                            app.modal_stack.push(Modal::root(ModalKind::Info(
                                InfoModal::new(
                                    "GeoJSON Exported",
                                    format!("Saved track to {}", path.display()),
                                ),
                            )));
                        }
                        Err(e) => {
                            app.modal_stack.push(Modal::root(ModalKind::Info(
                                InfoModal::new("Export Failed", e),
                            )));
                        }
                    }
                }
            } else {
                app.modal_stack.push(Modal::root(ModalKind::Info(
                    InfoModal::new("No Track", "No track selected to export."),
                )));
            }
        }
    }
}

fn draw_lut_sidebar(screen: &mut MapScreen, height: f32, ui: &mut egui::Ui) {
    egui::ComboBox::from_id_salt("map_colormap_preset")
        .width(98.0)
        .selected_text(screen.colormap.label())
        .show_ui(ui, |ui| {
            for preset in ColormapPreset::ALL {
                ui.selectable_value(&mut screen.colormap, preset, preset.label());
            }
        });

    ui.add_space(4.0);
    ui.label("Max:");
    if ui
        .add(
            egui::DragValue::new(&mut screen.level_max)
                .speed(0.1)
                .range(screen.level_min + 0.001..=1e6),
        )
        .changed()
    {
        screen.auto_levels = false;
    }

    let (axis_min, axis_max) = screen.data_range();
    let axis_span = (axis_max - axis_min).max(1e-4);
    let min_span = (axis_span * 0.005).max(1e-4);

    let bar_h = (height - 150.0).max(80.0);
    let total_w = 88.0;
    let (widget_rect, _) =
        ui.allocate_exact_size(egui::vec2(total_w, bar_h), egui::Sense::hover());
    let bar_rect = Rect::from_min_size(widget_rect.min, egui::vec2(24.0, bar_h));
    let hist_rect = Rect::from_min_max(
        Pos2::new(bar_rect.right() + 4.0, widget_rect.top()),
        widget_rect.max,
    );

    let y_of = |v: f64| -> f32 {
        let norm = ((v - axis_min) / axis_span).clamp(0.0, 1.0) as f32;
        norm.mul_add(-bar_rect.height(), bar_rect.bottom())
    };
    let val_of = |y: f32| -> f64 {
        let norm = ((bar_rect.bottom() - y) / bar_rect.height()).clamp(0.0, 1.0) as f64;
        norm.mul_add(axis_span, axis_min)
    };

    let painter = ui.painter_at(widget_rect);
    let level_span = (screen.level_max - screen.level_min).max(1e-4);

    // 1. Colorbar gradient (clamped outside [level_min, level_max], matching PyQtGraph)
    let steps = 64usize;
    let step_h = bar_rect.height() / (steps as f32);
    for i in 0..steps {
        let y_mid = (i as f32 + 0.5).mul_add(step_h, bar_rect.top());
        let v = val_of(y_mid);
        let t = ((v - screen.level_min) / level_span).clamp(0.0, 1.0);
        let [r, g, b] = screen.colormap.sample(t);
        let y0 = (i as f32).mul_add(step_h, bar_rect.top());
        let strip = Rect::from_min_size(
            Pos2::new(bar_rect.left(), y0),
            egui::vec2(bar_rect.width(), step_h + 0.5),
        );
        painter.rect_filled(strip, 0.0, Color32::from_rgb(r, g, b));
    }
    painter.rect_stroke(
        bar_rect,
        0.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.fg_stroke.color),
        egui::StrokeKind::Outside,
    );

    // 2. Mini-histogram alongside the colorbar
    const BINS: usize = 32;
    let mut counts = [0usize; BINS];
    if let Some(vals) = screen
        .tracks
        .get(&screen.selected_track)
        .and_then(|t| t.values_for(&screen.selected_data_key))
    {
        for &v in vals {
            if v.is_finite() {
                let norm = ((v - axis_min) / axis_span).clamp(0.0, 0.999_999);
                let bin = (norm * (BINS as f64)) as usize;
                counts[bin] += 1;
            }
        }
    }
    let max_count = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bin_h = hist_rect.height() / (BINS as f32);
    let hist_color = Color32::from_rgba_unmultiplied(180, 180, 195, 130);
    for (bin, &cnt) in counts.iter().enumerate() {
        if cnt == 0 {
            continue;
        }
        let w = (cnt as f32 / max_count) * hist_rect.width();
        let y_bot = (bin as f32).mul_add(-bin_h, hist_rect.bottom());
        let r = Rect::from_min_size(
            Pos2::new(hist_rect.left(), y_bot - bin_h),
            egui::vec2(w, bin_h),
        );
        painter.rect_filled(r, 0.0, hist_color);
    }

    // 3. LinearRegionItem-style shaded window + draggable handles
    let y_max = y_of(screen.level_max);
    let y_min = y_of(screen.level_min);
    let region_rect = Rect::from_min_max(
        Pos2::new(hist_rect.left(), y_max),
        Pos2::new(hist_rect.right(), y_min),
    );
    painter.rect_filled(
        region_rect,
        0.0,
        Color32::from_rgba_unmultiplied(80, 150, 255, 45),
    );

    // Middle region drag (moves both min & max together)
    let inner_top = (y_max + 6.0).min(y_min);
    let inner_bot = (y_min - 6.0).max(y_max);
    if inner_bot - inner_top > 4.0 {
        let inner_rect = Rect::from_min_max(
            Pos2::new(hist_rect.left(), inner_top),
            Pos2::new(hist_rect.right(), inner_bot),
        );
        let region_resp = ui.interact(
            inner_rect,
            ui.make_persistent_id("lut_region_drag"),
            egui::Sense::click_and_drag(),
        );
        if region_resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let dy = region_resp.drag_delta().y;
            if dy.abs() > f32::EPSILON {
                let dv = -(dy as f64) / (bar_rect.height() as f64) * axis_span;
                let width = screen.level_max - screen.level_min;
                let new_min = (screen.level_min + dv).clamp(axis_min, axis_max - width);
                screen.level_min = new_min;
                screen.level_max = new_min + width;
                screen.auto_levels = false;
            }
        } else if region_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }

    // Top (Max) and Bottom (Min) handle interactions
    let max_handle_rect = Rect::from_center_size(
        Pos2::new(widget_rect.center().x, y_max),
        egui::vec2(total_w, 12.0),
    );
    let max_resp = ui.interact(
        max_handle_rect,
        ui.make_persistent_id("lut_handle_max"),
        egui::Sense::click_and_drag(),
    );
    if max_resp.dragged()
        && let Some(pos) = max_resp.interact_pointer_pos()
    {
        screen.level_max = val_of(pos.y).max(screen.level_min + min_span);
        screen.auto_levels = false;
    }

    let min_handle_rect = Rect::from_center_size(
        Pos2::new(widget_rect.center().x, y_min),
        egui::vec2(total_w, 12.0),
    );
    let min_resp = ui.interact(
        min_handle_rect,
        ui.make_persistent_id("lut_handle_min"),
        egui::Sense::click_and_drag(),
    );
    if min_resp.dragged()
        && let Some(pos) = min_resp.interact_pointer_pos()
    {
        screen.level_min = val_of(pos.y).min(screen.level_max - min_span);
        screen.auto_levels = false;
    }

    if max_resp.hovered() || max_resp.dragged() || min_resp.hovered() || min_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }

    // Draw handle lines + triangular grip markers
    let draw_handle = |y: f32, active: bool| {
        let color = if active {
            Color32::from_rgb(255, 220, 90)
        } else {
            Color32::from_rgb(120, 185, 255)
        };
        painter.line_segment(
            [
                Pos2::new(bar_rect.left(), y),
                Pos2::new(hist_rect.right(), y),
            ],
            egui::Stroke::new(2.0, color),
        );
        let tip = Pos2::new(bar_rect.right() - 1.0, y);
        let tri = vec![
            tip,
            Pos2::new(bar_rect.right() + 7.0, y - 5.0),
            Pos2::new(bar_rect.right() + 7.0, y + 5.0),
        ];
        painter.add(egui::Shape::convex_polygon(tri, color, egui::Stroke::NONE));
    };
    draw_handle(y_of(screen.level_max), max_resp.hovered() || max_resp.dragged());
    draw_handle(y_of(screen.level_min), min_resp.hovered() || min_resp.dragged());

    ui.add_space(4.0);
    ui.label("Min:");
    if ui
        .add(
            egui::DragValue::new(&mut screen.level_min)
                .speed(0.1)
                .range(0.0..=(screen.level_max - 0.001).max(0.0)),
        )
        .changed()
    {
        screen.auto_levels = false;
    }

    if ui.checkbox(&mut screen.auto_levels, "Auto").changed() && screen.auto_levels {
        screen.recompute_auto_levels();
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geojson_roundtrip_and_rctrk_parse() {
        let buf = MappingBuffer {
            timestamps: vec![1_700_000_000.0, 1_700_000_005.0],
            gps: vec![
                Some(GpsData {
                    latitude: Some(55.6),
                    longitude: Some(13.0),
                    valid: true,
                    ..Default::default()
                }),
                None,
            ],
            count_rate: vec![42.0, 99.0],
            dose_rate: vec![0.12, 0.25],
            roi_counts: std::sync::Arc::new(HashMap::new()),
        };

        let track = MapTrack::from_mapping_buffer(&buf);
        let geo = track.to_geojson();
        let features = geo["features"].as_array().unwrap();
        assert_eq!(features.len(), 1);
        assert_eq!(features[0]["geometry"]["coordinates"][0], 13.0);
        assert_eq!(features[0]["geometry"]["coordinates"][1], 55.6);

        let parsed = MapTrack::parse_geojson(&geo.to_string()).unwrap();
        assert_eq!(parsed.timestamps.len(), 1);
        assert!((parsed.count_rate[0] - 42.0).abs() < 1e-6);

        let rctrk = "Timestamp,Lat,Lon,DoseRate,CPS\n1700000010,55.61,13.01,0.18,55.5\n";
        let parsed_rc = MapTrack::parse_rctrk(rctrk).unwrap();
        assert_eq!(parsed_rc.timestamps.len(), 1);
        assert!((parsed_rc.dose_rate[0] - 0.18).abs() < 1e-6);
        assert!((parsed_rc.count_rate[0] - 55.5).abs() < 1e-6);
    }

    #[test]
    fn test_custom_url_source() {
        let raster = CustomUrlSource {
            template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),
            is_vector: false,
        };
        assert_eq!(
            raster.tile_url(TileId { zoom: 13, x: 4391, y: 2568 }),
            "https://tile.openstreetmap.org/13/4391/2568.png"
        );
        assert_eq!(raster.tile_size(), 256);

        let vector = CustomUrlSource {
            template: "http://127.0.0.1:8000/tiles/{z}/{x}/{y}.pbf".into(),
            is_vector: true,
        };
        assert_eq!(vector.tile_size(), 512);
    }
}
