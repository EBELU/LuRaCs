import copy
import json
from pathlib import Path
import sys
import numpy as np

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
)
from PySide6.QtGui import QAction
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtWebEngineCore import QWebEngineSettings
from PySide6.QtWebChannel import QWebChannel

import pyqtgraph as pg


class LoadOnlineMapDialog(QDialog):
    def __init__(self, last_url: str = "", parent=None):
        super().__init__(parent=parent)
        self.setWindowTitle("Load Online Map")

        main_layout = QVBoxLayout(self)
        form = QFormLayout()

        self.line_source_url = QLineEdit()
        self.line_source_url.setText(last_url)
        form.addRow("Source URL (+ API Key if needed):", self.line_source_url)

        self.combo_vector_raster = QComboBox()
        self.combo_vector_raster.addItems(["Vector Tiles", "Raster Tiles"])
        form.addRow("Tile Type:", self.combo_vector_raster)

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


def rgba_to_css(colour):
    r, g, b, a = colour
    return f"rgba({r},{g},{b},{a / 255.0})"


class Bridge(QObject):
    mouseMoved = Signal(str, str)
    bridgeReady = Signal()

    @Slot(str)
    def from_js(self, msg):
        print("JS says:", msg)

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

        self.base_style_vector = json.load(
            (Path(__file__).parent / "style_vector.json").open()
        )
        self.base_style_raster = json.load(
            (Path(__file__).parent / "style_raster.json").open()
        )

        self.pending_style = None

        main_layout = QVBoxLayout(self)

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
        tool_bar.addWidget(self.btn_load_map)

        self.combo_spectrogram = QComboBox()
        self.combo_spectrogram.addItem("Spectrogram 1")

        self.combo_shown_data = QComboBox()
        self.combo_shown_data.addItems(["CPS [/s]", "DR   [uSv/h]"])

        tool_bar.addWidget(self.combo_spectrogram)
        tool_bar.addWidget(self.combo_shown_data)

        controls_group.setLayout(tool_bar)
        main_layout.addWidget(controls_group)

        central_layout = QHBoxLayout()

        self.web_engine_view = None
        self.bridge = None
        self.channel = None
        self.web_container = QStackedLayout()

        placeholder = QLabel("Map not loaded")
        placeholder.setAlignment(Qt.AlignCenter)

        self.web_container.addWidget(placeholder)

        web_widget = QWidget()
        web_widget.setStyleSheet("""
            QWidget {
                border: 2px solid #666;
                border-radius: 4px;
            }
        """)
        web_widget.setLayout(self.web_container)

        central_layout.addWidget(web_widget, 10)

        lut_container = pg.GraphicsLayoutWidget()
        self.view_slider = pg.HistogramLUTItem(orientation="vertical")
        self.view_slider.setLevels(0, 0.2)
        self.view_slider.vb.setLimits(yMin=0, yMax=1e6, minXRange=1)
        self.view_slider.region.setBounds([0, 1e6])
        lut_container.addItem(self.view_slider)
        lut_container.setMaximumWidth(125)

        central_layout.addWidget(lut_container, stretch=2)

        main_layout.addLayout(central_layout)

        self.status_label = QLabel(
            "Longitude:           Latitude:           Loaded Map:"
        )
        self.status_label.setFrameShape(QFrame.Shape.Panel)
        self.status_label.setFrameShadow(QFrame.Shadow.Sunken)

        main_layout.addWidget(self.status_label)

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
            f"Longitude: {lnglat['lng']:.6f}    Latitude: {lnglat['lat']:.6f}"
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
            self.pending_style = copy.deepcopy(self.base_style_vector)
            self.pending_style["sources"]["openmaptiles"] = {
                "type": "vector",
                "tiles": source_url,
            }
        else:
            self.pending_style = copy.deepcopy(self.base_style_raster)
            self.pending_style["sources"]["openmaptiles"] = {
                "type": "raster",
                "tiles": [source_url],
                "tileSize": tile_size
                if (
                    tile_size := self.base_style_raster["sources"]["openmaptiles"].get(
                        "tileSize"
                    )
                )
                else 256,
            }

        html_file = Path(__file__).parent / "map.html"
        self.web_engine_view.load(QUrl.fromLocalFile(str(html_file.resolve())))

        self.web_container.addWidget(self.web_engine_view)
        self.web_container.setCurrentWidget(self.web_engine_view)

    def load_online_map(self):
        dialog = LoadOnlineMapDialog(parent=self)
        if dialog.exec() != QDialog.DialogCode.Accepted:
            return

        if not dialog.get_source_url():
            return

        self.start_web_engine(
            dialog.get_source_url(), vector_source=dialog.get_vector_source()
        )

    def load_offline_map(self):
        QFileDialog.getOpenFileName(
            self, "Load Local Map", filter="PM Tiles (*.pmtiles)", options=QFileDialog.Option.DontUseNativeDialog
        )


if __name__ == "__main__":
    app = QApplication.instance() or QApplication(sys.argv)
    window = MapWidget()
    window.resize(800, 500)
    window.show()
    sys.exit(app.exec())
