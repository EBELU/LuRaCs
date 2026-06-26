import json
from pathlib import Path
import sys

from PySide6.QtCore import QObject, Qt, Signal, Slot, QUrl
from PySide6.QtWidgets import (
    QApplication,
    QWidget,
    QDialog,
    QHBoxLayout,
    QVBoxLayout,
    QGroupBox,
    QPushButton,
    QComboBox,
    QLineEdit,
    QLabel,
    QFrame,
    QStackedLayout,
    QFormLayout,
    QMenu,
    QDialogButtonBox,
    QFileDialog,
    QMessageBox,
    QCheckBox,
)
from PySide6.QtGui import QAction
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtWebEngineCore import QWebEngineSettings
from PySide6.QtWebChannel import QWebChannel

import pyqtgraph as pg

from luracs.core import Settings, Log, core_utils
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
    mouseMoved = Signal(str, str)
    bridgeReady = Signal()

    @Slot(str)
    def from_js(self, msg):
        Log.info(f"Map JS: {msg}")

        if msg == "Web channel ready":
            self.bridgeReady.emit()

    @Slot(str, str)
    def mouse_move(self, point_json, lnglat_json):
        self.mouseMoved.emit(point_json, lnglat_json)

    def add_data_point(self, view, id_, lat, lng, colour=(0, 255, 0, 255)):
        css_colour = rgba_to_css(colour)
        js = f"add_data_point({id_!r}, {lat}, {lng}, {css_colour!r});"
        view.page().runJavaScript(js)

    def remove_data_point(self, view, id_):
        view.page().runJavaScript(f"remove_data_point({id_!r});")


class MapWidget(QWidget):
    sigLoadOnlineMapUrl = Signal(str, str)
    sigLoadOfflineMapPath = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent=parent)

        self.pending_style = None
        self.tile_server = None

        main_layout = QVBoxLayout(self)

        # --- Tool bar ---
        controls_group = QGroupBox("")
        tool_bar = QHBoxLayout()

        self.btn_load_map = QPushButton("Load Map")
        menu = QMenu(self.btn_load_map)

        action_offline = QAction("Local", self)
        action_online = QAction("Online", self)

        action_offline.triggered.connect(self.load_offline_map)
        action_online.triggered.connect(self.load_online_map)

        menu.addAction(action_offline)
        menu.addAction(action_online)

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
        self.combo_spectrogram.addItem("Spectrogram 1")

        # Data kind combo
        self.combo_shown_data = QComboBox()
        self.combo_shown_data.addItems(["CPS [/s]", "DR   [uSv/h]"])

        # Add toolbar
        tool_bar.addWidget(self.combo_spectrogram, 2)
        tool_bar.addWidget(self.combo_shown_data, 2)

        controls_group.setLayout(tool_bar)
        main_layout.addWidget(controls_group)

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

        lut_container = pg.GraphicsLayoutWidget()
        self.plot = lut_container.addPlot(row=0, col=0)
        self.plot.hide()

        self.view_slider = pg.HistogramLUTItem(orientation="vertical")
        self.view_slider.setLevels(0, 0.2)
        self.view_slider.vb.setLimits(yMin=0, yMax=1e6, minXRange=1)
        self.view_slider.region.setBounds([0, 1e6])

        self.view_slider.gradient.loadPreset("viridis")

        core_utils.ThemeManager.register_hist_lut(self.view_slider)
        core_utils.ThemeManager.register_plot(self.plot)

        lut_container.addItem(self.view_slider)
        lut_container.setMaximumWidth(125)

        central_layout.addWidget(lut_container, stretch=2)

        main_layout.addLayout(central_layout)

    def stop(self):
        if self.tile_server is not None:
            self.tile_server.stop()

    def createJsMap(self):
        style_json = json.dumps(self.pending_style)

        js = f"""
            createJsMap({style_json});
        """

        self.web_engine_view.page().runJavaScript(js)

    @Slot(str, str)
    def on_mouse_move(self, point_json, lnglat_json):
        lnglat = json.loads(lnglat_json)
        self.status_label.setText(
            f"Longitude: {lnglat['lng']:.6f}    Latitude: {lnglat['lat']:.6f}     Loaded Map: {self.loaded_map_name}"
        )

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

        self.bridge.mouseMoved.connect(self.on_mouse_move)
        self.bridge.bridgeReady.connect(self.createJsMap)

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

        vector_source = dialog.get_vector_source()
        if dialog.get_source_url().endswith(".png") and vector_source:
            vector_source = False
            QMessageBox.information(
                self,
                "Raster Detected",
                "The given URL was identified as raster but vector was chosen. The map will be loaded as raster",
            )

        if dialog.get_if_save_url():
            Settings.State.map_last_online_url = str(dialog.get_source_url())
        else:
            Settings.State.map_last_online_url = ""

        if "{z}/{x}/{y}" not in dialog.get_source_url():
            reply = QMessageBox.question(
                self,
                "Error",
                "The given urls does not match the expected tile coordinate pattern of '{z}/{x}/{y}', continue?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,  # default button
            )
            if reply != QMessageBox.Yes:
                return

        self.start_web_engine(dialog.get_source_url(), vector_source=vector_source)

        self.loaded_map_name = (
            dialog.get_source_url()
            .removeprefix("https://")
            .removesuffix("/{z}/{x}/{y}.png")
        )

    def load_offline_map(self):
        path, _ = QFileDialog.getOpenFileName(
            self,
            "Load Local Map",
            filter="PM Tiles (*.pmtiles)",
            options=QFileDialog.Option.DontUseNativeDialog,
        )

        if not path:
            return

        if self.tile_server:
            self.tile_server.stop()

        self.loaded_map_name = Path(path).name
        self.tile_server = TileServer(Path(path))
        self.tile_server.start()

        Log.info("Tile server warming up, cacheing map...")

        self.start_web_engine(
            self.tile_server.url,
            vector_source=True,
        )


if __name__ == "__main__":
    app = QApplication.instance() or QApplication(sys.argv)
    window = MapWidget()
    window.resize(800, 500)
    window.show()
    sys.exit(app.exec())
