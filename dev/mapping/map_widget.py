import sys
from PySide6.QtWidgets import (
    QApplication,
    QWidget,
    QDialog,
    QHBoxLayout,
    QVBoxLayout,
    QGroupBox,
    QPushButton,
    QComboBox,
    QCheckBox,
    QLineEdit,
    QLabel,
    QFrame,
    QStackedLayout,
    QFormLayout, 
    QMenu, 
    QDialogButtonBox,
    QFileDialog
)
from PySide6.QtWebEngineWidgets import QWebEngineView
from PySide6.QtCore import Qt, Signal, Slot, QUrl
from PySide6.QtGui import QAction

import pyqtgraph as pg
import numpy as np

class LoadOnlineMapDialog(QDialog):
    def __init__(self, last_url: str = "", parent=None):
        super().__init__(parent=parent)
        self.setWindowTitle("Load Online Map")
        
        main_layout = QVBoxLayout(self)
        form = QFormLayout()
        
        # URL
        self.line_url = QLineEdit()
        self.line_url.setText(last_url)
        form.addRow("URL:", self.line_url)
        
        # API Key
        self.line_api_key = QLineEdit()
        form.addRow("API Key:", self.line_api_key)
        
        main_layout.addLayout(form)
        
        # Ok or Cancel buttons
        self.button_box = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok |
            QDialogButtonBox.StandardButton.Cancel
        )

        self.button_box.accepted.connect(self.accept)
        self.button_box.rejected.connect(self.reject)

        main_layout.addWidget(self.button_box)
    
    # --- Getters ---
    def get_url(self):
        return self.line_url.text()
    
    def get_api_key(self):
        return self.line_api_key.text()

class MapWidget(QWidget):
    sigLoadOnlineMapUrl = Signal(str, str)
    sigLoadOfflineMapPath = Signal(str)
    def __init__(self, parent=None):
        super().__init__(parent=parent)
        main_layout = QVBoxLayout(self)
        
        # --- Top Tool bar ---
        
        # Layout and box
        controls_group = QGroupBox("")

        tool_bar = QHBoxLayout()
        tool_bar.setContentsMargins(2, 2, 2, 2)
        tool_bar.setSpacing(4)
        
        # Menu button
        self.btn_load_map = QPushButton("Load Map")
        menu = QMenu(self.btn_load_map)
        
        action_offline = QAction("Offline Map", self)
        action_offline.triggered.connect(self.load_offline_map)
        
        action_online = QAction("Online Map", self)
        action_online.triggered.connect(self.load_online_map)
        
        menu.addAction(action_offline)
        menu.addAction(action_online)

        self.btn_load_map.setMenu(menu)
        tool_bar.addWidget(self.btn_load_map)

        # Combos
        self.combo_spectrogram = QComboBox()
        self.combo_spectrogram.addItem("Spectrogram 1")

        self.combo_shown_data = QComboBox()
        self.combo_shown_data.addItems(["CPS [/s]", "DR   [uSv/h]"])

        tool_bar.addWidget(self.combo_spectrogram)
        tool_bar.addWidget(self.combo_shown_data)

        # Apply layout to group box
        controls_group.setLayout(tool_bar)
        
        main_layout.addWidget(controls_group)
        
        # --- Central Layout ---
        central_layout = QHBoxLayout()
        
        # WebEngine placeholder, to save RAM use if no map is used
        self.web_engine_view = None
        self.web_container = QStackedLayout()

        placeholder = QLabel("Map not loaded")
        placeholder.setAlignment(Qt.AlignCenter)

        self.web_container.addWidget(placeholder)

        # Wrap layout in a widget
        web_widget = QWidget()
        web_widget.setStyleSheet("""
            QWidget {
                border: 2px solid #666;
                border-radius: 4px;
            }
        """)
        web_widget.setLayout(self.web_container)

        central_layout.addWidget(web_widget, 10)
        
        # Colormap slider
        lut_container = pg.GraphicsLayoutWidget()
        self.view_slider = pg.HistogramLUTItem(orientation="vertical")
        self.view_slider.setLevels(0, 0.2)
        self.view_slider.vb.setLimits(yMin=0, yMax=1e6, minXRange=1)
        self.view_slider.region.setBounds([0, 1e6])
        lut_container.addItem(self.view_slider)
        
        lut_container.setMaximumWidth(125)

        central_layout.addWidget(lut_container, stretch=2)
        
        main_layout.addLayout(central_layout)
        
        # --- Bottom status bar ---
        lon = lat = loaded_map = ""
        self.status_label = QLabel(f"Longitude: {lon:<20} Latitude: {lat:<20}\t Loaded Map: {loaded_map}")
        self.status_label.setFrameShape(QFrame.Shape.Panel)
        self.status_label.setFrameShadow(QFrame.Shadow.Sunken)

        main_layout.addWidget(self.status_label)
        
    def start_webengine(self):
        if self.web_engine_view is not None:
            return

        # Create WebEngine view
        self.web_engine_view = QWebEngineView()

        self.web_engine_view.load(QUrl("https://your-map-url-here.com"))

        # Add to stacked layout
        self.web_container.addWidget(self.web_engine_view)
        self.web_container.setCurrentWidget(self.web_engine_view)
        
    def values_to_colors(self, values: np.ndarray):
        """Match an array of values to the current settings of the colormap. Returns an array of colours."""
        cmap = self.view_slider.gradient.colorMap()
        low, high = self.view_slider.getLevels()

        values = np.asarray(values)

        # normalize to 0–1
        norm = (values - low) / (high - low)
        norm = np.clip(norm, 0, 1)

        # map to QColor objects
        colors = cmap.map(norm, mode='qcolor')

        return colors
    
    def load_online_map(self):
        dialog = LoadOnlineMapDialog(parent=self)
        res = dialog.exec()
        
        if res != LoadOnlineMapDialog.Accepted or not dialog.get_url():
            return
        
        
        
    def load_offline_map(self):
        chosen_path, _ = QFileDialog.getOpenFileName(
            self, 
            "Load Offline Map",
            filter="PM Tiles (.*pmtiles)")
        
        
if __name__ == "__main__":
    app = QApplication.instance() or QApplication(sys.argv)


    window = MapWidget()
    window.resize(800, 500)
    window.show()


    sys.exit(app.exec())
