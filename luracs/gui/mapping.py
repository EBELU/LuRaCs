from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from luracs.containers.roi_classes import SpectrogramROI

import json
import sys
from datetime import datetime
from pathlib import Path

import numpy as np
import pyqtgraph as pg
from PySide6.QtCore import QObject, Qt, QUrl, Signal, Slot
from PySide6.QtGui import QAction
from PySide6.QtWebChannel import QWebChannel
from PySide6.QtWebEngineCore import QWebEngineSettings
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFormLayout,
    QFrame,
    QGroupBox,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMenu,
    QMessageBox,
    QPushButton,
    QStackedLayout,
    QVBoxLayout,
    QWidget,
)

from luracs.clients.gps import GPSData, format_gps
from luracs.core import Log, RunManager, Settings, core_utils
from luracs.resources.mapping_resources.local_server import TileServer
from luracs.utils.file_io import (
    MapFormatParser,
    MapPoint,
    SimpleMappingData,
    export_geojson,
)


class LoadOnlineMapDialog(QDialog):
    def __init__(self, last_url: str = "", parent=None):
        super().__init__(parent=parent)
        self.setWindowTitle("Load Online Map")

        self.resize(600, self.sizeHint().height())

        main_layout = QVBoxLayout(self)
        form = QFormLayout()

        self.line_source_url = QLineEdit()
        self.line_source_url.setText(last_url)
        form.addRow("Source URL (+ API Key):", self.line_source_url)

        self.combo_vector_raster = QComboBox()
        self.combo_vector_raster.addItems(["Vector Tiles", "Raster Tiles"])
        form.addRow("Tile Type:", self.combo_vector_raster)

        self.check_save_url = QCheckBox("Save URL")
        self.check_save_url.setChecked(True)
        form.addRow("", self.check_save_url)

        main_layout.addLayout(form)

        self.button_box = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
        )

        self.button_box.accepted.connect(self.accept)
        self.button_box.rejected.connect(self.reject)

        main_layout.addWidget(self.button_box)

    def get_source_url(self):
        return self.line_source_url.text()

    def get_vector_source(self):
        return self.combo_vector_raster.currentIndex() == 0

    def get_if_save_url(self):
        return self.check_save_url.isChecked()


def rgba_to_css(colour):
    r, g, b, a = colour
    return f"rgba({r},{g},{b},{a / 255.0})"


class Bridge(QObject):
    "Wrapper object to facilitate communication between python and javascript"

    sigMouseMoved = Signal(str, str)
    sigBridgeReady = Signal()

    @Slot(str)
    def from_js(self, msg: str):
        Log.info(f"Map JS: {msg}")

        if msg == "Web channel ready":
            self.sigBridgeReady.emit()

    @Slot(str, str)
    def mouse_move(self, point_json: str, lnglat_json: str):
        self.sigMouseMoved.emit(point_json, lnglat_json)

    def add_data_point(
        self,
        view: QWebEngineView,
        id_: int,
        lat: float,
        lng: float,
        popup_text: str = "",
        color: tuple = (0, 255, 0, 255),
    ):
        css_colour = rgba_to_css(color)
        js = f"add_data_point({id_!r}, {lat}, {lng}, {css_colour!r}, {popup_text!r});"
        view.page().runJavaScript(js)

    def change_data_point_colour(self, view: QWebEngineView, id_: int, color: tuple):
        css_color = rgba_to_css(color)
        js = f"change_data_point_colour({id_!r}, {css_color!r});"
        view.page().runJavaScript(js)

    def remove_data_point(self, view: QWebEngineView, id_: int):
        view.page().runJavaScript(f"remove_data_point({id_!r});")

    def remove_all_data_points(self, view: QWebEngineView):
        view.page().runJavaScript("remove_all_data_points();")
        
    def add_current_location_point(self, view: QWebEngineView, lat: float, lng: float,
    ):
        js = f"add_current_location_point({lat}, {lng});"
        view.page().runJavaScript(js)

    def remove_current_location_point(self, view: QWebEngineView):
        view.page().runJavaScript("remove_current_location_point();")

    def move_current_location_point(self, view: QWebEngineView, lat: float, lng: float,
    ):
        js = f"move_current_location_point({lat}, {lng});"
        view.page().runJavaScript(js)
        
    def move_view_to(
        self,
        view: QWebEngineView,
        lat: float,
        lng: float,
        zoom: float | None = None,
    ):
        if zoom is None:
            js = f"move_view_to({lat}, {lng});"
        else:
            js = f"move_view_to({lat}, {lng}, {zoom});"

        view.page().runJavaScript(js)



class MappingDataBuffer(QObject):
    sigNewPointsReceived = Signal(str, str, object, object, object)
    sigCompleteData = Signal(str, str, object, object)
    def __init__(self, spectrogram_name: str):
        super().__init__(parent=None)
        self.spectrogram_name: str = spectrogram_name
        self.current_length: int = 0
        self.buffers: dict[np.ndarray] = {}
    
    def process_buffer(self, buffer: dict[np.ndarray]):
        if buffer["gps"].shape[0] == self.current_length:
            return
        
        self.buffers = buffer
        if not np.any(self.buffers["gps"]):
            return
        
        size_diff = buffer["gps"].shape[0] - self.current_length
        self.current_length = buffer["gps"].shape[0]
        
        for key, value in self.buffers.items():
            if key == "gps" or key == "timestamp":
                continue
            lng = [p.longitude for p in self.buffers["gps"][-size_diff:]]
            lat = [p.latitude for p in self.buffers["gps"][-size_diff:]]
            self.sigNewPointsReceived.emit(self.spectrogram_name, key, lng, lat, value[-size_diff:])
            
    def get_all_data(self, key: str):
        return self.spectrogram_name, key, self.buffers["gps"], self.buffers[key]
            
    def clear(self):
        self.buffers.clear()
        self.current_length = 0
            
        

class MapWidget(QWidget):
    sigLoadOnlineMapUrl = Signal(str, str)
    sigLoadOfflineMapPath = Signal(str)
    sigMoveToCurrent = Signal()

    def __init__(self, parent=None):
        super().__init__(parent=parent)

        self.pending_style = None
        self.tile_server = None
        
        # --- Data Containers ---
        self.selected_content = None
        self.latest_gps: GPSData | None = None
        self.is_tracking_current_location: bool = False
        self.current_datapoints: list = []
        self.map_buffers: dict[str, MappingDataBuffer] = {}
        self.simple_buffers: dict[str, SimpleMappingData] = {}
        
        # ----- Layout -----

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(3,3,3,3)

        # --- Tool bar ---
        controls_group = QGroupBox("")
        tool_bar = QHBoxLayout()

        self.btn_load_map = QPushButton("Map Options")
        menu = QMenu(self.btn_load_map)

        # Load actions
        action_offline = QAction("Load Offline Map", self)
        action_offline.setToolTip(
            "Load a map from a locally stored .pmtiles-file and run on the embedded tile server."
        )
        action_online = QAction("Load Online Map", self)
        action_online.setToolTip(
            "Load a map from a web URL matching the format /{z}/{x}/{y}.*"
        )

        action_import = QAction("Import Simple Track", self)
        action_export_to_geojson = QAction("To .geojson", self)

        # Connections
        action_offline.triggered.connect(self.load_map_from_file)
        action_online.triggered.connect(self.load_online_map)
        action_import.triggered.connect(self.import_data)
        action_export_to_geojson.triggered.connect(self.export_to_geojson)

        menu.addAction(action_offline)
        menu.addAction(action_online)
        menu.addSeparator()
        menu.addAction(action_import)
        menu.addAction(action_export_to_geojson)

        menu.setToolTipsVisible(True)

        self.btn_load_map.setMenu(menu)
        tool_bar.addWidget(self.btn_load_map, 1)

        # Info label
        self.loaded_map_name = ""
        self.status_label = QLabel(
            "Longitude:           Latitude:           Loaded Map:"
        )
        self.status_label.setFrameShape(QFrame.Shape.Panel)
        self.status_label.setFrameShadow(QFrame.Shadow.Sunken)
        tool_bar.addWidget(self.status_label, 5)

        # Data source combo
        self.combo_spectrogram = QComboBox()
        self.combo_spectrogram.currentIndexChanged.connect(
            self.combo_changed
        )

        # Data kind combo
        self.combo_shown_data = QComboBox()
        self.combo_shown_data.addItem("Count Rate [/s]", "count_rate")
        self.combo_shown_data.addItem("Dose Rate [uSv/h]", "dose_rate")
        self.combo_shown_data.currentIndexChanged.connect(self.combo_changed)

        # Add toolbar
        tool_bar.addWidget(self.combo_spectrogram, 2)
        tool_bar.addWidget(self.combo_shown_data, 1)

        controls_group.setLayout(tool_bar)
        main_layout.addWidget(controls_group)
        
        # --- GPS status bar ---
        self.gps_status_bar = QLabel()
        

        # --- Map ---
        central_layout = QHBoxLayout()

        self.web_engine_view = None
        self.bridge = None
        self.channel = None
        self.web_container = QStackedLayout()

        placeholder = QLabel("Map not loaded")
        placeholder.setAlignment(Qt.AlignCenter)

        self.web_container.addWidget(placeholder)

        web_widget = QWidget()

        web_widget.setLayout(self.web_container)

        central_layout.addWidget(web_widget, 10)

        # Cheat the LUTItem to both follow the theme and show a histogram
        # The image is there for the histogram and the plot for the theming
        # None are shown and the overhead is negligible compared with the WebEngine
        lut_container = pg.GraphicsLayoutWidget()
        self.plot = lut_container.addPlot(row=0, col=0)
        self.plot.hide()

        # Create an ImageItem with a dummy 1x1 image
        self.dummy_image = pg.ImageItem(np.zeros((1, 1), dtype=float))
        self.dummy_image.hide()

        self.view_slider = pg.HistogramLUTItem(orientation="vertical")
        self.view_slider.setImageItem(self.dummy_image)

        self.view_slider.setLevels(0, 0.2)
        self.view_slider.vb.setLimits(
            yMin=0,
            yMax=1e6,
            minXRange=1,
            minYRange=0.05,
        )
        self.view_slider.region.setBounds([0, 1e6])
        self.view_slider.vb.enableAutoRange(axis="y", enable=False)

        # Proxy to not drown the thread in updates
        self.color_proxy = pg.SignalProxy(
            self.view_slider.sigLevelsChanged, rateLimit=5, slot=self.change_color
        )
        
        self.view_slider.sigLookupTableChanged.connect(self.change_color)

        self.view_slider.gradient.loadPreset("viridis")

        core_utils.ThemeManager.register_hist_lut(self.view_slider)
        core_utils.ThemeManager.register_plot(self.plot)

        lut_container.addItem(self.view_slider)
        lut_container.setMaximumWidth(125)

        central_layout.addWidget(lut_container, stretch=2)

        main_layout.addLayout(central_layout)
        main_layout.addWidget(self.gps_status_bar)

        
        # --- Connect Signals ---
        RunManager.Signals.spectrogramStarted.connect(self.catch_spectrogram_added)
        RunManager.Signals.spectrogramClosed.connect(self.catch_spectrogram_closed)
        RunManager.SpectrogramManager.sigAddROI.connect(self.catch_roi_added)
        RunManager.SpectrogramManager.sigRemoveROI.connect(self.catch_roi_removed)
        RunManager.SpectrogramManager.sigMapBufferUpdated.connect(self.catch_map_buffer)
        RunManager.Signals.GPSConnection.connect(self.catch_gps_connected)
        RunManager.Signals.GPSUpdated.connect(self.catch_gps_update)
        self.move_to_current_proxy = pg.SignalProxy(
            self.sigMoveToCurrent,
            delay=Settings.Advanced.map_move_to_current_max_rate_s,
            slot=self.move_to_current
        )



    # ------------------------------------------------------------------
    # Data handling
    # ------------------------------------------------------------------
    
    def catch_spectrogram_added(self, spectrogram_name: str):
        self.combo_spectrogram.addItem(spectrogram_name)
        new_buffer = MappingDataBuffer(spectrogram_name)
        self.map_buffers[spectrogram_name] = new_buffer
        new_buffer.sigNewPointsReceived.connect(self.add_points)
    
    def catch_spectrogram_closed(self, spectrogram_name: str):
        for i in range(self.combo_spectrogram.count()):
            if self.combo_spectrogram.itemText(i) == spectrogram_name:
                self.combo_spectrogram.removeItem(i)
                break
        
        old_buffer =  self.map_buffers.pop(spectrogram_name)
        old_buffer.sigNewPointsReceived.disconnect(self.add_points)
            
    def catch_roi_added(self, roi: SpectrogramROI):
        self.combo_shown_data.addItem(roi.alias, roi.tag)
        for buff in self.map_buffers.values():
            buff.clear()
        for sg in RunManager.SpectrogramManager.spectrogram_registry.values():
            sg.request_data()
    
    def catch_roi_removed(self, roi: SpectrogramROI):
        for i in range(self.combo_shown_data.count()):
            if self.combo_shown_data.itemData(i) == roi.tag:
                self.combo_shown_data.removeItem(i)
                break
    
    def add_simple_data(self, data: SimpleMappingData):
        self.simple_buffers[data.title] = data
        for i in range(self.combo_spectrogram.count()):
            if self.combo_spectrogram.itemText(i) == data.title:
                return
        
        self.combo_spectrogram.addItem(data.title)
        self.combo_changed(0)
        
    def get_data(self)->tuple[list, list, list]:
        "Returns [longitude, latitude, value]"
        spectrogram_key = self.combo_spectrogram.currentText()
        current_data_key = self.combo_shown_data.currentData()
        
        if not spectrogram_key:
            return
        
        if spectrogram_key in self.simple_buffers:
            values = []
            lng = []
            lat = []
            for p in self.simple_buffers[spectrogram_key].data_points:
                lng.append(p.lng)
                lat.append(p.lat)
                if current_data_key == "count_rate":
                    values.append(p.count_rate)
                elif current_data_key == "dose_rate":
                    values.append(p.dose_rate)
                elif p.other_data_point is not None and current_data_key in p.other_data_point:
                    values.append(p.other_data_point.get(current_data_key))
                else:
                    return
                    
            return lng, lat, values
                
        elif spectrogram_key in self.map_buffers:
            _, _, gps, values = self.map_buffers[spectrogram_key].get_all_data(current_data_key)
            
            lng = [p.longitude for p in gps]
            lat = [p.latitude for p in gps]
            
            return lng, lat, list(values)

        else:
            Log.debug(f"No data found! \n sg_key={spectrogram_key}, data_key={current_data_key}\n map_buffer={self.map_buffers.keys()}, simple_buffers={self.simple_buffers.keys()}")
        

    def combo_changed(self, index: int):
        if self.bridge is None:
            return
        spectrogram_key = self.combo_spectrogram.currentText()
        current_data_key = self.combo_shown_data.currentData()

        self.bridge.remove_all_data_points(self.web_engine_view)
        
        data = self.get_data()
        if data is not None:
            self.set_points(spectrogram_key, current_data_key, *data)
        

    def set_points(self, spectrogram_name: str, data_type_key: str, longitude: list, latitude: list, values: list):
        if self.web_engine_view is None:
            return
        for i, (lng, lat, p) in enumerate(zip(longitude, latitude, values)):
            if p is None or np.isnan(p):
                return
            self.bridge.add_data_point(
                self.web_engine_view,
                i,
                lat,
                lng,
                f"Value {round(p, 2)}",
                self.value_to_color(p),
            )
            
        self.current_datapoints = list(values)
        image = np.asarray(values, dtype=np.float32)[None, :]  # (1, N)
        self.dummy_image.setImage(image, autoLevels=False)
        low, high = np.percentile(image, [1, 99])
        self.view_slider.setLevels(low, high)
        self.view_slider.vb.setYRange(low, high)
        
    def add_points(self, spectrogram_name: str, data_type_key: str, longitude: list, latitude: list, values: list):
        if spectrogram_name != self.combo_spectrogram.currentText() or data_type_key != self.combo_shown_data.currentData() or self.web_engine_view is None:
            # print("returned", f"{spectrogram_name} = {self.combo_spectrogram.currentText()}, {data_type_key} = {self.combo_shown_data.currentData()}")
            return
        
        nr_current_points = len(self.current_datapoints)
        for i, (lng, lat, p) in enumerate(zip(longitude, latitude, values)):
            self.bridge.add_data_point(
                self.web_engine_view,
                i + nr_current_points,
                lat,
                lng,
                f"Value {round(p, 2)}",
                self.value_to_color(p),
            )
            
        self.current_datapoints.extend(values)
        image = np.asarray(self.current_datapoints, dtype=np.float32)[None, :]  # (1, N)
        self.dummy_image.setImage(image, autoLevels=False)
        if i == len(self.current_datapoints) - 1:
            low, high = np.percentile(image, [1, 99])
            self.view_slider.setLevels(low, high)
            self.view_slider.vb.setYRange(low, high)
        
        
            
    def change_color(self):
        if not len(self.current_datapoints):
            return
        
        for i, p in enumerate(self.current_datapoints):
            self.bridge.change_data_point_colour(
                self.web_engine_view, i, self.value_to_color(p)
            )
    
    @Slot(bool)
    def catch_gps_connected(self, gps_state: bool):
        if self.bridge:
            if gps_state:
                self.bridge.add_current_location_point(self.web_engine_view, 55.5915924, 12.9980149)
            else:
                self.bridge.remove_current_location_point(self.web_engine_view)
    
    @Slot(GPSData)
    def catch_gps_update(self, data: GPSData):
        self.gps_status_bar.setText(format_gps(data))
        if self.bridge and data.valid:
            self.latest_gps = data
            self.bridge.move_current_location_point(self.web_engine_view, data.latitude, data.longitude)
            if self.is_tracking_current_location:
                self.sigMoveToCurrent.emit()

    
    @Slot(str, dict)
    def catch_map_buffer(self, spectrogram_name: str, buffers: dict):
        self.map_buffers[spectrogram_name].process_buffer(buffers)
        
    # ------------------------------------------------------------------
    # GUI interaction
    # ------------------------------------------------------------------
    
    def move_to_current(self):
        if self.latest_gps is not None and self.web_engine_view is not None:
            self.bridge.move_view_to(self.web_engine_view, self.latest_gps.latitude, self.latest_gps.longitude)
            
    def move_to_start(self):
        data = self.get_data()
        if data is not None and self.web_engine_view is not None:
            for lng, lat, _ in zip(*data):
                if lng is not None and lat is not None:
                    self.bridge.move_view_to(self.web_engine_view, lat, lng)
                    return
                
    def move_to_end(self):
        data = self.get_data()
        if data is not None and self.web_engine_view is not None:
            for lng, lat, _ in reversed(list(zip(*data))):
                if lng is not None and lat is not None:
                    self.bridge.move_view_to(self.web_engine_view, lat, lng)
                    return
                
    def track_current_location(self, state: bool):
        self.is_tracking_current_location = state

    # ------------------------------------------------------------------
    # Internal runners and callbacks
    # ------------------------------------------------------------------

    def createJsMap(self):
        style_json = json.dumps(self.pending_style)

        js = f"""
            createJsMap({style_json});
        """

        self.web_engine_view.page().runJavaScript(js)

        self.combo_changed(0)

    @Slot(str, str)
    def on_mouse_move(self, point_json, lnglat_json):
        "Callback for receiving coordinates from the map"
        lnglat = json.loads(lnglat_json)
        self.status_label.setText(
            f"Longitude: {lnglat['lng']:.6f}    Latitude: {lnglat['lat']:.6f}     Loaded Map: {self.loaded_map_name}"
        )

    def stop(self):
        # Ensure the local tile server is closed when the program shuts down
        if self.tile_server is not None:
            self.tile_server.stop()

    def start_web_engine(self, source_url: str, vector_source: bool):
        if self.web_engine_view is not None:
            self.web_engine_view.deleteLater()
            self.web_engine_view = None

        self.web_engine_view = QWebEngineView()

        self.web_engine_view.settings().setAttribute(
            QWebEngineSettings.WebAttribute.JavascriptEnabled, True
        )

        self.web_engine_view.settings().setAttribute(
            QWebEngineSettings.WebAttribute.LocalContentCanAccessRemoteUrls, True
        )

        self.web_engine_view.settings().setAttribute(
            QWebEngineSettings.WebAttribute.LocalContentCanAccessFileUrls, True
        )

        self.bridge = Bridge()
        self.channel = QWebChannel()

        self.channel.registerObject("bridge", self.bridge)
        self.web_engine_view.page().setWebChannel(self.channel)

        self.bridge.sigMouseMoved.connect(self.on_mouse_move)
        self.bridge.sigBridgeReady.connect(self.createJsMap)

        if vector_source:
            self.pending_style = json.load(
                (
                    Settings.Paths.resources / "mapping_resources" / "style_vector.json"
                ).open()
            )
            self.pending_style["sources"]["openmaptiles"] = {
                "type": "vector",
                "tiles": [source_url],
                "maxzoom": 14,
            }
        else:
            self.pending_style = json.load(
                (
                    Settings.Paths.resources / "mapping_resources" / "style_raster.json"
                ).open()
            )

            self.pending_style["sources"]["openmaptiles"] = {
                "type": "raster",
                "tiles": [source_url],
                "tileSize": tile_size
                if (
                    tile_size := self.pending_style["sources"]["openmaptiles"].get(
                        "tileSize"
                    )
                )
                else 256,
            }

        html_file = Settings.Paths.resources / "mapping_resources" / "map.html"
        self.web_engine_view.load(QUrl.fromLocalFile(str(html_file.resolve())))

        self.web_container.addWidget(self.web_engine_view)
        self.web_container.setCurrentWidget(self.web_engine_view)

    def load_online_map(self):
        dialog = LoadOnlineMapDialog(
            parent=self, last_url=Settings.State.map_last_online_url
        )
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return

        if not dialog.get_source_url():
            return

        self.load_map_from_url(
            dialog.get_source_url(), dialog.get_vector_source(), from_gui=True
        )

    def load_map_from_url(
        self, url: str, vector_source: bool = True, from_gui: bool = False
    ):
        if url.endswith(".png") and vector_source:
            vector_source = False
            if from_gui:
                QMessageBox.information(
                    self,
                    "Raster Detected",
                    "The given URL was identified as raster but vector was chosen. The map will be loaded as raster",
                )
            else:
                Log.info(
                    "The given URL was identified as raster but vector was chosen. The map will be loaded as raster"
                )

        if url:
            Settings.State.map_last_online_url = str(url)
        else:
            Settings.State.map_last_online_url = ""

        if "{z}/{x}/{y}" not in url and from_gui:
            reply = QMessageBox.question(
                self,
                "Error",
                "The given urls does not match the expected tile coordinate pattern of '{z}/{x}/{y}', continue?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,  # default button
            )

            if reply != QMessageBox.Yes:
                return

        self.start_web_engine(url, vector_source=vector_source)

        self.loaded_map_name = url.removeprefix("https://").removesuffix(
            "/{z}/{x}/{y}.png"
        )

    def load_map_from_file(self):
        path, _ = QFileDialog.getOpenFileName(
            self,
            "Load Local Map",
            filter="PM Tiles (*.pmtiles)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )
        if not path:
            return
        self.load_offline_map(path)

    def load_offline_map(self, path: str):
        if not path:
            return

        if self.tile_server:
            self.tile_server.stop()

        self.loaded_map_name = Path(path).name
        if not Path(path).is_file():
            Log.error(f"{path} is not a file!")
            return

        self.tile_server = TileServer(Path(path))
        self.tile_server.start()

        Log.info("Tile server warming up, cacheing map...")

        self.start_web_engine(
            self.tile_server.url,
            vector_source=True,
        )

    # ------------------------------------------------------------------
    # IO
    # ------------------------------------------------------------------
    
    def import_data(self):
        if self.web_engine_view is None:
            QMessageBox.warning(self, "Error", "Map must be loaded before a track")
            return
        file, _ = QFileDialog.getOpenFileName(
            self,
            "Import Simple Track",
            str(Settings.Paths.last_opened_dir),
            "Map Files (*.geojson *.rctrk)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )
        if not file:
            return
        parser = MapFormatParser(file)
        self.add_simple_data(parser.data)
        Settings.Paths.last_opened_dir = Path(file).parent

    def export_to_geojson(self):
        name = self.combo_spectrogram.currentText()
        if name in self.simple_buffers:
            smd = self.simple_buffers[name]
        elif name in self.map_buffers:
            data = self.map_buffers[name]
            sg = RunManager.SpectrogramManager.spectrogram_registry[name]

            map_points = []
            for i in range(data.current_length):
                mp = MapPoint(
                    timestamp=datetime.fromtimestamp(data.buffers["timestamp"][i]),
                    count_rate=data.buffers["count_rate"][i],
                    dose_rate=data.buffers["count_rate"][i],
                    lng=data.buffers["gps"][i].longitude,
                    lat=data.buffers["gps"][i].latitude,
                    other_data_point={
                        RunManager.SpectrogramManager.get_roi_alias_from_tag(tag): v[i] for tag, v in data.buffers.items() if tag not in ["gps", "timestamp", "count_rate", "dose_rate"]
                    }
                )
                map_points.append(mp)
            
            smd = SimpleMappingData(
                title = name,
                start=datetime.fromtimestamp(sg.start_date),
                end=datetime.fromtimestamp(sg.buffers.latest_timestamp),
                device_id=sg.device_id,
                data_points=map_points,
                meta_data={}
            )
            
        else:
            QMessageBox.warning(self, "Error", "No track to export")
            return

        file, _ = QFileDialog.getSaveFileName(
            self,
            "Export to .geojson",
            str(Settings.Paths.last_opened_dir),
            "GeoJSON GIS File (*.geojson)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )
        if not file:
            return

        export_geojson(smd, file)

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------
    
    def value_to_color(self, value, alpha=True):
        """
        Returns the RGBA color corresponding to a scalar value.

        Parameters
        ----------
        hist_lut : pg.HistogramLUTItem
        value : float
            Data value.
        alpha : bool
            If True returns (R, G, B, A), otherwise (R, G, B).

        Returns
        -------
        tuple
        """
        # Get current levels (min/max)
        vmin, vmax = self.view_slider.getLevels()

        # Normalize to [0, 1]
        t = np.clip((value - vmin) / max((vmax - vmin), 1), 0.0, 1.0)

        # Get the ColorMap
        cmap = self.view_slider.gradient.colorMap()

        # QColor
        qcolor = cmap.mapToQColor(t)

        if alpha:
            return qcolor.red(), qcolor.green(), qcolor.blue(), qcolor.alpha()
        else:
            return qcolor.red(), qcolor.green(), qcolor.blue()


if __name__ == "__main__":
    app = QApplication.instance() or QApplication(sys.argv)
    window = MapWidget()
    window.resize(800, 500)
    window.show()
    sys.exit(app.exec())
