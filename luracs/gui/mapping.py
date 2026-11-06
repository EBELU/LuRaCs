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
from PySide6.QtCore import QObject, Qt, QTimer, QUrl, Signal, Slot
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
from luracs.core import Log, RunManager, Settings, SpectrogramManager, core_utils
from luracs.resources.mapping_resources.local_server import TileServer


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
        css_color: str = "",
    ):
        js = f"add_data_point({id_!r}, {lat}, {lng}, {css_color!r}, {popup_text!r});"
        view.page().runJavaScript(js)

    def set_data_points(self, view, points):
        js = f"set_data_points({json.dumps(points)});"
        view.page().runJavaScript(js)

    def set_data_point_colours(
        self,
        view: QWebEngineView,
        colours: list[str],
    ):
        js = f"set_data_point_colours({json.dumps(colours)});"
        view.page().runJavaScript(js)

    def remove_data_point(self, view: QWebEngineView, id_: int):
        view.page().runJavaScript(f"remove_data_point({id_!r});")

    def remove_all_data_points(self, view: QWebEngineView):
        view.page().runJavaScript("remove_all_data_points();")

    def add_current_location_point(
        self,
        view: QWebEngineView,
        lat: float,
        lng: float,
    ):
        js = f"add_current_location_point({lat}, {lng});"
        view.page().runJavaScript(js)

    def remove_current_location_point(self, view: QWebEngineView):
        if self.web_engine_view is None:
            return
        view.page().runJavaScript("remove_current_location_point();")

    def move_current_location_point(
        self,
        view: QWebEngineView,
        lat: float,
        lng: float,
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

        # ----- Layout -----

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(3, 3, 3, 3)

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
        self.combo_spectrogram.currentIndexChanged.connect(self.on_combo_changed)

        # Data kind combo
        self.combo_shown_data = QComboBox()
        self.combo_shown_data.addItem("Count Rate [/s]", "count_rate")
        self.combo_shown_data.addItem("Dose Rate [uSv/h]", "dose_rate")
        self.combo_shown_data.currentIndexChanged.connect(self.on_combo_changed)

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
        SpectrogramManager.sigSpectrogramStarted.connect(self.catch_spectrogram_added)
        SpectrogramManager.sigSpectrogramClosed.connect(self.catch_spectrogram_closed)
        SpectrogramManager.ROIHandler.sigAddROI.connect(self.catch_roi_added)
        SpectrogramManager.ROIHandler.sigRemoveROI.connect(self.catch_roi_removed)
        SpectrogramManager.sigMappingDataUpdated.connect(self.add_points)
        RunManager.Signals.GPSConnection.connect(self.catch_gps_connected)
        RunManager.Signals.GPSUpdated.connect(self.catch_gps_update)

        self.move_to_current_timer = QTimer(self)
        self.move_to_current_timer.setSingleShot(True)
        self.move_to_current_timer.setInterval(
            int(Settings.Advanced.map_move_to_current_max_rate_s * 1000)
        )

    # ------------------------------------------------------------------
    # Data handling
    # ------------------------------------------------------------------

    @Slot(str)
    def catch_spectrogram_added(self, spectrogram_name: str):
        self.combo_spectrogram.addItem(spectrogram_name)

    @Slot(str)
    def catch_spectrogram_closed(self, spectrogram_name: str):
        for i in range(self.combo_spectrogram.count()):
            if self.combo_spectrogram.itemText(i) == spectrogram_name:
                self.combo_spectrogram.removeItem(i)
                break

    @Slot(object)
    def catch_roi_added(self, roi: SpectrogramROI):
        self.combo_shown_data.addItem(roi.alias, roi.tag)
        for sg in SpectrogramManager.spectrogram_registry.values():
            sg.request_data()

    @Slot(object)
    def catch_roi_removed(self, roi: SpectrogramROI):
        for i in range(self.combo_shown_data.count()):
            if self.combo_shown_data.itemData(i) == roi.tag:
                self.combo_shown_data.removeItem(i)
                break

        # Remove the ROIs data from buffers when the ROI goes
        for buffer in self.map_buffers.values():
            if roi.tag in buffer.buffers:
                del buffer.buffers[roi.tag]

    def on_combo_changed(self, index: int):
        "Update everything when a combobox changes. Clears the whole map and redraws."
        if self.bridge is None:
            return
        spectrogram_key = self.combo_spectrogram.currentText()
        current_data_key = self.combo_shown_data.currentData()

        self.bridge.remove_all_data_points(self.web_engine_view)

        if not spectrogram_key:
            return

        data = SpectrogramManager.MappingHandler.get_complete_data(
            spectrogram_key, current_data_key
        )
        if data is not None:
            self.set_points(*data)

    def set_points(
        self,
        spectrogram_name: str,
        longitude: list,
        latitude: list,
        timestamps: list,
        value_dict: dict[str, list],
    ):
        "Set rendered points in bulk, used for when spectrogram or ROI is changed."
        if (
            spectrogram_name != self.combo_spectrogram.currentText()
            or self.web_engine_view is None
        ):
            return

        values = value_dict.get(self.combo_shown_data.currentData())
        if values is None:
            Log.debug(
                f"No values found for data_type_key={self.combo_shown_data.currentData()} in value_dict={value_dict.keys()}"
            )
            return

        # Generate all points that are to be rendered
        points = []

        for i, (lng, lat, timestamp, p) in enumerate(
            zip(longitude, latitude, timestamps, values)
        ):
            if p is None or np.isnan(p):
                continue

            points.append(
                {
                    "id": i,
                    "lat": float(lat),
                    "lon": float(lng),
                    "colour": rgba_to_css(self.value_to_color(p)),
                    "popup_text": f"TS: {datetime.fromtimestamp(round(timestamp))}, Value {round(p, 2)}",
                }
            )

        if not points:
            return

        # Send over the bulk data and let JS run the update loop to save on bridge overhead
        self.bridge.set_data_points(
            self.web_engine_view,
            points,
        )

        # Update tracked data points and the image
        self.current_datapoints = [
            p for p in values if p is not None and not np.isnan(p)
        ]

        if not self.current_datapoints:
            return

        image = np.asarray(
            self.current_datapoints,
            dtype=np.float32,
        )[None, :]

        self.dummy_image.setImage(image, autoLevels=False)

        low, high = np.percentile(image, [1, 99])
        self.view_slider.setLevels(low, high)
        self.view_slider.vb.setYRange(low, high)

    def add_points(
        self,
        spectrogram_name: str,
        longitude: list,
        latitude: list,
        timestamps: list,
        value_dict: dict[str, list],
    ):
        "Add a number of points to the existing rendered data set. Primarily used for adding single new points during live measurements."
        # Do basic check, dont render points for things that should not be shown
        if (
            spectrogram_name != self.combo_spectrogram.currentText()
            or self.web_engine_view is None
        ):
            return

        values = value_dict.get(self.combo_shown_data.currentData())
        if values is None:
            Log.debug(
                f"No values found for data_type_key={self.combo_shown_data.currentData()} in value_dict={value_dict.keys()}"
            )
            return

        # Determine the current number of points to track point id for the new points
        nr_current_points = len(self.current_datapoints)

        for i, (lng, lat, timestamp, p) in enumerate(
            zip(longitude, latitude, timestamps, values)
        ):
            if p is None or np.isnan(p):
                continue

            # Make new points and add them one by one
            # Inefficient for large data sets so dont use it for that
            pt_data = {
                "id_": i + nr_current_points,
                "lat": float(lat),
                "lng": float(lng),
                "css_color": rgba_to_css(self.value_to_color(p)),
                "popup_text": f"TS: {datetime.fromtimestamp(round(timestamp))}, Value {round(p, 2)}",
            }

            self.bridge.add_data_point(self.web_engine_view, **pt_data)

            self.current_datapoints.append(p)

        # Update the image so the LUT item can keep up
        image = np.asarray(
            self.current_datapoints,
            dtype=np.float32,
        )[None, :]

        self.dummy_image.setImage(image, autoLevels=False)

        low, high = np.percentile(image, [1, 99])
        self.view_slider.setLevels(low, high)
        self.view_slider.vb.setYRange(low, high)

    def change_color(self):
        "Change the color of data points in bulk."
        if not self.current_datapoints or self.web_engine_view is None:
            return

        # Generate a list of new colors and send it over and let JS do the update loop
        colours = [rgba_to_css(self.value_to_color(p)) for p in self.current_datapoints]

        self.bridge.set_data_point_colours(
            self.web_engine_view,
            colours,
        )

    @Slot(bool)
    def catch_gps_connected(self, gps_state: bool):
        if self.bridge:
            if gps_state:
                self.bridge.add_current_location_point(
                    self.web_engine_view, 55.5915924, 12.9980149
                )
            else:
                self.bridge.remove_current_location_point(self.web_engine_view)

    @Slot(GPSData)
    def catch_gps_update(self, data: GPSData):
        self.gps_status_bar.setText(format_gps(data))
        if self.bridge and data.valid:
            self.latest_gps = data
            self.bridge.move_current_location_point(
                self.web_engine_view, data.latitude, data.longitude
            )

            if (
                self.is_tracking_current_location
                and not self.move_to_current_timer.isActive()
            ):
                self.move_to_current()
                self.move_to_current_timer.start()

    # ------------------------------------------------------------------
    # GUI interaction
    # ------------------------------------------------------------------

    def move_to_current(self):
        if self.latest_gps is not None and self.web_engine_view is not None:
            self.bridge.move_view_to(
                self.web_engine_view,
                self.latest_gps.latitude,
                self.latest_gps.longitude,
            )

    def move_to_start(self):
        data = SpectrogramManager.MappingHandler.get_complete_data(
            self.combo_spectrogram.currentText(), self.combo_shown_data.currentData()
        )
        longitude, latitude = data[1], data[2]

        if data is not None and self.web_engine_view is not None:
            for lng, lat in list(zip(longitude, latitude)):
                if lng is not None and lat is not None:
                    self.bridge.move_view_to(self.web_engine_view, lat, lng)
                    return

    def move_to_end(self):
        data = SpectrogramManager.MappingHandler.get_complete_data(
            self.combo_spectrogram.currentText(), self.combo_shown_data.currentData()
        )
        longitude, latitude = data[1], data[2]

        if data is not None and self.web_engine_view is not None:
            for lng, lat in reversed(list(zip(longitude, latitude))):
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

        imported_map_name = (
            SpectrogramManager.MappingHandler.import_simple_mapping_data(Path(file))
        )
        self.combo_spectrogram.addItem(imported_map_name)
        self.on_combo_changed(0)

    def export_to_geojson(self):
        name = self.combo_spectrogram.currentText()
        if name is None:
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

        SpectrogramManager.MappingHandler.export_to_geojson(name, file)

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
        t = np.clip((value - vmin) / max((vmax - vmin), 0.001), 0.0, 1.0)

        # Get the ColorMap
        cmap = self.view_slider.gradient.colorMap()

        # QColor
        qcolor = cmap.mapToQColor(t)
        # Apply alpha channels
        qcolor.setAlpha(Settings.Appearance.map_point_alpha_value)

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
