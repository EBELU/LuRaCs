import sys
import time
from datetime import datetime
import numpy as np
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout
from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QListWidget, QListWidgetItem, QPushButton, QHBoxLayout, QComboBox, QLabel, QLineEdit, QSpinBox, QDoubleSpinBox, QDialogButtonBox, QSizePolicy, QFormLayout, QFrame
)
from PySide6.QtCore import QTimer, Signal, QCoreApplication, Qt
import pyqtgraph as pg
pg.setConfigOptions(antialias=True)
from textwrap import dedent

from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QPushButton, QHBoxLayout, QMessageBox
)
from PySide6.QtCore import Qt
from datetime import datetime, timedelta
import sqlite3 as sql
from pathlib import Path

from PySide6.QtWidgets import QAbstractItemView


from ..core import RunManager, Settings
from ..utils.DataLogging import WrappedSpectrogramData, SpectrumLogger, start_logger

class StartLoggerDialog(QDialog):
    def __init__(self, instruments, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Start Data Logger")
        self.setMinimumWidth(350)
        self.setMinimumHeight(190)
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(6)

        # Instrument selection
        self.instrument_combo = QComboBox()
        self.instrument_combo.addItems(instruments)
        form.addRow("Instrument:", self.instrument_combo)

        # File name (default = timestamp)
        self.filename_edit = QLineEdit()
        default_name = f"SpectrumLog-{datetime.now().strftime("%Y%m%d_%H%M%S")}"
        self.filename_edit.setText(default_name)
        form.addRow("File name:", self.filename_edit)

        # Logging interval (float)
        self.interval_spin = QDoubleSpinBox()
        self.interval_spin.setRange(0.001, 1_000_000)
        self.interval_spin.setDecimals(3)
        self.interval_spin.setValue(1.0)
        self.interval_spin.setSingleStep(0.1)
        form.addRow("Interval (s):", self.interval_spin)

        # Channel truncation (int)
        self.trunc_spin = QSpinBox()
        self.trunc_spin.setRange(0, 1_000_000)
        self.trunc_spin.setValue(0)
        form.addRow("Channel trunc:", self.trunc_spin)

        main_layout.addLayout(form)

        # OK / Cancel buttons
        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

        self.adjustSize()

    def get_values(self):
        return {
            "instrument": self.instrument_combo.currentText(),
            "filename": self.filename_edit.text() + ".db",
            "interval": self.interval_spin.value(),
            "channel_truncation": self.trunc_spin.value(),
        }
        
class SpectrogramLoadDialog(QDialog):

    def __init__(self, db_directory: Path, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Load Spectrogram")
        self.resize(700, 400)

        self.db_directory = db_directory
        self.selected_db = None

        layout = QVBoxLayout(self)

        self.table = QTableWidget()
        self.table.setColumnCount(5)
        self.table.setHorizontalHeaderLabels([
            "Database",
            "Device",
            "Start Date",
            "Last Update",
            "Duration"
        ])

        self.table.setSelectionBehavior(QAbstractItemView.SelectRows)
        self.table.setSelectionMode(QAbstractItemView.SingleSelection)
        self.table.setEditTriggers(QAbstractItemView.NoEditTriggers)

        layout.addWidget(self.table)

        btn_layout = QHBoxLayout()

        self.load_btn = QPushButton("Load")
        self.cancel_btn = QPushButton("Cancel")

        btn_layout.addStretch()
        btn_layout.addWidget(self.load_btn)
        btn_layout.addWidget(self.cancel_btn)

        layout.addLayout(btn_layout)

        self.load_btn.clicked.connect(self.load_selected)
        self.cancel_btn.clicked.connect(self.reject)

        self.populate_table()

    def populate_table(self):
        db_files = sorted(self.db_directory.glob("*.db"))

        self.table.setRowCount(len(db_files))

        for row, db_file in enumerate(db_files):

            info = self.read_db_info(db_file)

            self.table.setItem(row, 0, QTableWidgetItem(db_file.name))
            self.table.setItem(row, 1, QTableWidgetItem(info["device"]))
            self.table.setItem(row, 2, QTableWidgetItem(info["start"]))
            self.table.setItem(row, 3, QTableWidgetItem(info["last_update"]))
            self.table.setItem(row, 4, QTableWidgetItem(info["duration"]))

        self.table.resizeColumnsToContents()

    def read_db_info(self, db_path: Path):

        try:
            conn = sql.connect(db_path)
            cursor = conn.cursor()

            cursor.execute("""
                SELECT created, device_id
                FROM header
                WHERE id = 1
            """)
            header = cursor.fetchone()

            cursor.execute("""
                SELECT total_duration, last_update
                FROM summary
                WHERE id = 1
            """)
            summary = cursor.fetchone()

            conn.close()

            created, device = header if header else (None, "Unknown")
            duration, last_update = summary if summary else (0, None)

            start_str = "-"
            last_str = "-"
            duration_str = "-"

            if created:
                start = datetime.fromtimestamp(created)
                start_str = start.strftime("%Y-%m-%d %H:%M:%S")

            if last_update:
                last = datetime.fromtimestamp(last_update)
                last_str = last.strftime("%Y-%m-%d %H:%M:%S")

            if duration:
                duration_str = str(timedelta(seconds=int(duration)))

            return dict(
                device=device,
                start=start_str,
                last_update=last_str,
                duration=duration_str
            )

        except Exception:
            return dict(
                device="Invalid DB",
                start="-",
                last_update="-",
                duration="-"
            )

    def load_selected(self):

        row = self.table.currentRow()

        if row < 0:
            QMessageBox.warning(self, "Select database", "Please select a database to load.")
            return

        db_name = self.table.item(row, 0).text()
        self.selected_db = self.db_directory / db_name

        self.accept()

class SpectrogramWidget(QWidget):
    startLogger = Signal(str, str, int, int, bool)
    
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Waterfall")
        
        self.startLogger.connect(start_logger)
        RunManager.loggerStarted.connect(self.fill_combo_box)
        RunManager.loggerStarted.connect(self.connect_logger)
        
        self.y_len = 256
        self.x_len = 1024
        
        self.selected_log = None
        
        main_layout = QHBoxLayout(self)
        
        right_layout = QVBoxLayout()
        right_layout.setSpacing(0)
        
        left_layout = QVBoxLayout()
        
        self.options_bar = QHBoxLayout()
        
        self.btn_start_log = QPushButton("Start New")
        
        self.btn_load_log = QPushButton("Load")
        
        self.btn_import_log = QPushButton("Import")
        
        self.options_bar.addWidget(self.btn_start_log)
        self.options_bar.addWidget(self.btn_load_log)
        self.options_bar.addWidget(self.btn_import_log)
        
        self.btn_start_log.clicked.connect(self.start_logger)
        self.btn_load_log.clicked.connect(self.load_logger)
        
        
        
        left_layout.addLayout(self.options_bar)
        
        self.spectrogram_selection = QComboBox()
        
        left_layout.addWidget(self.spectrogram_selection)
        
        self.stop_resume = QHBoxLayout()
        self.btn_stop = QPushButton("Stop")
        self.btn_stop.setEnabled(False)
        self.btn_resume = QPushButton("Resume")
        self.btn_resume.setEnabled(False)
        
        self.stop_resume.addWidget(self.btn_stop)
        self.stop_resume.addWidget(self.btn_resume)
        
        
        left_layout.addLayout(self.stop_resume)
        
        self.info_label = QLabel()
        self.info_label.setText("")

        self.info_label.setFrameStyle(QFrame.Box | QFrame.Plain)
        self.info_label.setLineWidth(1)

        left_layout.addWidget(self.info_label)
        
        
        self.top_spectrum_plot = pg.PlotWidget()
        self.top_spectrum_plot.getPlotItem().layout.setContentsMargins(14, 13, 13, 0)

        self.x = np.arange(self.x_len)
        y = np.zeros(self.x_len)

        self.bar = pg.BarGraphItem(
            x=self.x,
            height=y,
            width=1.0,
            brush='y'
        )
        
        self.top_spectrum_plot.setLimits(
            xMin=0,
            xMax=self.x_len,
            yMin=0,
            yMax=512,
            minXRange=10,
            minYRange=4
        )

        self.top_spectrum_plot.addItem(self.bar)
        
        
        right_layout.addWidget(self.top_spectrum_plot, 1)

        # Graphics layout
        self.spectrogram_plot = pg.GraphicsLayoutWidget()
        right_layout.addWidget(self.spectrogram_plot, 4)

        # Plot
        self.plot = self.spectrogram_plot.addPlot(row=0, col=0)
        self.img = pg.ImageItem()
        self.plot.addItem(self.img)
        
        self.vLine = pg.InfiniteLine(angle=90, movable=False)
        self.hLine = pg.InfiniteLine(angle=0, movable=False)

        self.plot.addItem(self.vLine, ignoreBounds=True)
        self.plot.addItem(self.hLine, ignoreBounds=True)
        
        self.plot.scene().sigMouseClicked.connect(self.mouse_clicked)


        # HistogramLUT (used only for gradient + dual slider)
        self.hist = pg.HistogramLUTItem(orientation='horizontal')
        self.hist.setMinimumHeight(15)
        self.hist.setImageItem(self.img)
        self.hist.vb.enableAutoRange(axis = 'x')
        self.hist.vb.enableAutoRange(axis='y')
        self.hist.vb.setLimits(
            xMin=-0.2,
            xMax=1e3,
            minXRange=1
        )

        self.spectrogram_plot.addItem(self.hist, row=1, col=0)
        self.top_spectrum_plot.setXLink(self.plot)
        self.hist.plot.setVisible(True)

        
        main_layout.addLayout(left_layout)
        main_layout.addLayout(right_layout)
        

        # Stop auto histogram range recalculation (removes jitter)

        # Load colormap preset
        self.hist.gradient.loadPreset("viridis")
        
        self.plot.invertY(True)
        # self.top_spectrum_plot.getPlotItem().layout.setContentsMargins(7, 10, 0, 0)
        self.plot.layout.setContentsMargins(0, 0, 0, 0)
        self.top_spectrum_plot.getPlotItem().getAxis('left').setWidth(30)
        self.plot.getAxis('left').setWidth(35)
        # Data buffer

        self.data = np.zeros((self.y_len, self.x_len), dtype=np.float32)
        self.data.fill(np.nan)
        self.plot.setLimits(
            xMin=0,
            xMax=self.x_len,
            yMin=0,
            yMax=self.y_len,
            minXRange=32,
            minYRange=32
        )
        self.img.setLevels((0, 5))

        
    def start_logger(self, *_):
        devices = list(RunManager.devices.keys())
        dialog = StartLoggerDialog(devices)
        if dialog.exec():

            if dialog.get_values()["instrument"]:
                "db_name, device, save_interval, "
                print(dialog.get_values())
                chosen_values = dialog.get_values()
                print(chosen_values)
                self.startLogger.emit(
                    chosen_values["filename"],
                    chosen_values["instrument"],
                    chosen_values["interval"],
                    chosen_values["channel_truncation"],
                    False)
                
    def load_logger(self, *_):
        dialog = SpectrogramLoadDialog(Settings.Paths.spect_logs)
        
        if dialog.exec():
            db_path = dialog.selected_db
            print("Selected:", db_path)
    
    def fill_combo_box(self, name):
        self.spectrogram_selection.addItem("[Running] " + name) 
        
    def update_x_len(self, new_len: int):
        if new_len == self.x_len:
            return
        self.x_len = new_len
        self.data = np.zeros((self.y_len, self.x_len), dtype=np.float32)
        self.data.fill(np.nan)
        self.top_spectrum_plot.setLimits(xMax=self.x_len)
        self.plot.setLimits(xMax=self.x_len)
        
        self.x = np.arange(self.x_len)
        y = np.zeros(self.x_len)

        self.bar = pg.BarGraphItem(
            x=self.x,
            height=y,
            width=1.0,
            brush='y'
        )
        self.top_spectrum_plot.addItem(self.bar)
        
        
    def connect_logger(self):
        last_logger = list(RunManager.dataloggers.values())[0]
        last_logger.dataUpdated.connect(self.recieve_data)
        
    def recieve_data(self, device_name: str, data_packet: WrappedSpectrogramData):
        if data_packet.estimated_dose < 5e-1:
            dose = data_packet.estimated_dose * 1e3
            dose_unit = "nSv"
        elif data_packet.estimated_dose > 5e2:
            dose = data_packet.estimated_dose * 1e-3
            dose_unit = "mSv"
        else:
            dose = data_packet.estimated_dose
            dose_unit = "uSv"
        
        self.info_label.setText(dedent(f"""
            Database:
                {data_packet.db_name}
            Instrument: {data_packet.instrument}
            Start Date: {data_packet.start_date}
            Duration: {data_packet.duration}
            Estimated Dose: {round(dose, 3)} {dose_unit}
            Time Interval: {data_packet.save_interval}s
            Spectrum Concat Factor: {data_packet.concat}
            Spectrum Channels: {data_packet.spect_channels}
        """).strip())
        
        
        self.update_x_len(data_packet.spect_channels)
        self.bar.setOpts(height=data_packet.latest_spectrum)
        self.update_spectrogram_img(data_packet.spectrogram)
        
        
    def update_spectrogram_img(self, view_buf):
        # self.data[:-1] = self.data[1:]
        # self.data[-1] = view_buf
        
        # self.img.setImage(self.data[::-1, :].T, autoLevels=False)
        new_image = np.array(view_buf)[::-1]
        self.img.setImage(new_image.T, autoLevels=False)
    
    def mouse_clicked(self, evt):

        if evt.button() != Qt.LeftButton:
            return

        pos = evt.scenePos()

        if self.plot.sceneBoundingRect().contains(pos):

            mousePoint = self.plot.vb.mapSceneToView(pos)

            x = int(mousePoint.x())
            y = int(mousePoint.y())

            image = self.img.image
            if image is None:
                return

            if 0 <= x < image.shape[1] and 0 <= y < image.shape[0]:

                value = image[y, x]

                self.vLine.setPos(x)
                self.hLine.setPos(y)

                print(f"x={x}, y={y}, value={value}")
                
if __name__ == "__main__":
    pass