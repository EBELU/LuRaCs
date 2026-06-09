from datetime import datetime, timedelta
import numpy as np
from PySide6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QDialog,
    QPushButton,
    QHBoxLayout,
    QComboBox,
    QLabel,
    QTextEdit,
    QLineEdit,
    QDoubleSpinBox,
    QDialogButtonBox,
    QFormLayout,
    QFrame,
    QRadioButton,
    QButtonGroup,
    QMessageBox,
    QMenu,
)
from PySide6.QtCore import Signal, QRectF, Qt
from PySide6.QtGui import QAction, QFont
import pyqtgraph as pg


from textwrap import dedent

from core import RunManager, Settings, IOManager, Log
from containers.spectrogram import (
    WrappedSpectrogramData,
    start_spectrogram,
    restart_spectrogram,
)
from utils import file_io
from gui.popup_windows.save_dialog import SaveNamingDialog

pg.setConfigOptions(antialias=True)


def combobox_has_data(combobox, target):
    return any(combobox.itemData(i) == target for i in range(combobox.count()))


def find_index_by_data(combobox, target):
    for i in range(combobox.count()):
        if combobox.itemData(i) == target:
            return i
    return -1


class StartSpectrogramDialog(QDialog):
    def __init__(self, instruments, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Start Spectrogram")
        self.setMinimumWidth(450)
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
        default_name = (
            f"Spectrogram_{datetime.now().replace(microsecond=0).isoformat()}"
        )
        self.filename_edit.setText(default_name)
        form.addRow("File name:", self.filename_edit)

        # Logging interval (float)
        self.interval_spin = QDoubleSpinBox()
        self.interval_spin.setRange(0.001, 1_000_000)
        self.interval_spin.setDecimals(1)
        self.interval_spin.setValue(1.0)
        self.interval_spin.setSingleStep(0.5)
        form.addRow("Interval (s):", self.interval_spin)

        # Channel truncation (radio buttons)
        self.trunc_group = QButtonGroup(self)
        trunc_layout = QHBoxLayout()

        self.trunc_buttons = {}
        for val in [1, 2, 4, 8]:
            btn = QRadioButton(str(val))
            self.trunc_group.addButton(btn, val)
            trunc_layout.addWidget(btn)
            self.trunc_buttons[val] = btn

        # Default = 1
        self.trunc_buttons[1].setChecked(True)

        form.addRow("Channel trunc:", trunc_layout)

        main_layout.addLayout(form)

        # OK / Cancel buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

        self.adjustSize()

    def get_values(self):
        return {
            "instrument": self.instrument_combo.currentText(),
            "filename": self.filename_edit.text() + ".db",
            "interval": self.interval_spin.value(),
            "channel_truncation": int(self.trunc_group.checkedId()),
        }


class SpectrogramWidget(QWidget):
    sigStartSpectrogram = Signal(
        str, str, int, int, bool
    )  # Signal to the start logger function
    sigLoadSpectrogram = Signal(str)
    sigShowDataStore = Signal(int)

    def __init__(self, parent):
        super().__init__(parent=parent)
        self.setWindowTitle("Waterfall")

        self.sigStartSpectrogram.connect(start_spectrogram)
        self.sigLoadSpectrogram.connect(restart_spectrogram)
        RunManager.spectrogramStarted.connect(self.connect_logger)
        RunManager.spectrogramDequeResized.connect(self.update_on_spectrogram_selection)

        # Default values
        self.y_len = Settings.Advanced.spectrogram_deque_length
        self.x_len = 1024
        self.y_axis = np.zeros(self.y_len)
        self.show_date_on_y: bool = False
        self.break_lines: list = []
        self.current_packet_buffer = None
        self.tracked_spectrogram = None
        self.current_calibration: list = []
        self.current_concat_factor: int = np.nan
        self.info_text_total = ""
        self.info_text_selector = ""

        main_layout = QHBoxLayout(self)

        # Right layout contains buttons and infobox
        right_layout = QVBoxLayout()
        right_layout.setSpacing(0)

        left_layout = QVBoxLayout()

        self.options_bar = QHBoxLayout()

        # First row of buttons
        btn_start_log = QPushButton("Start New")
        btn_start_log.clicked.connect(self.start_logger)
        btn_unload_log = QPushButton("Unload")
        btn_unload_log.clicked.connect(self.unload_logger)
        btn_load_log = QPushButton("Load")
        btn_load_log.clicked.connect(self.show_data_store)

        self.options_bar.addWidget(btn_start_log)
        self.options_bar.addWidget(btn_unload_log)
        self.options_bar.addWidget(btn_load_log)

        left_layout.addLayout(self.options_bar)

        # Combo box for selecting loaded spectrogram
        self.spectrogram_selection = QComboBox()
        self.spectrogram_selection.currentIndexChanged.connect(
            self.update_on_spectrogram_selection
        )

        left_layout.addWidget(self.spectrogram_selection)

        # Second row of buttons
        self.stop_resume = QHBoxLayout()
        self.btn_stop = QPushButton("Stop")
        self.btn_stop.clicked.connect(self.pause_logger)
        self.btn_stop.setEnabled(False)
        self.btn_resume = QPushButton("Resume")
        self.btn_resume.clicked.connect(self.restart_logger)

        btn_menu = QPushButton("...")
        btn_menu.setMaximumWidth(40)

        # Dropdown menu
        self.menu = QMenu(btn_menu)

        self.action_pause_visual_updates = QAction(
            "Pause Visual Updates", self, checkable=True
        )
        self.action_time_selector = QAction("Time Selector", self, checkable=True)
        self.action_time_selector.toggled.connect(self.set_time_selector)
        action_export_spectrum_selection = QAction("Selection to Spectrum", self)
        action_export_spectrum_selection.triggered.connect(
            self.export_time_selection_to_spectrum
        )
        action_export_spectrum_total = QAction("Accumulated to Spectrum", self)
        action_export_spectrum_total.triggered.connect(self.export_to_spectrum)

        self.menu.addAction(self.action_pause_visual_updates)
        self.menu.addAction(self.action_time_selector)
        self.menu.addAction(action_export_spectrum_selection)
        self.menu.addAction(action_export_spectrum_total)

        self.menu.addSeparator()
        self.action_show_break_lines = QAction("Show Break Lines", self, checkable=True)
        self.action_show_break_lines.setChecked(True)
        self.action_show_break_lines.toggled.connect(
            self.update_on_spectrogram_selection
        )
        self.menu.addAction(self.action_show_break_lines)

        btn_menu.setMenu(self.menu)

        self.stop_resume.addWidget(self.btn_stop)
        self.stop_resume.addWidget(self.btn_resume)
        self.stop_resume.addWidget(btn_menu)

        left_layout.addLayout(self.stop_resume)

        # Info box with text
        self.info_label = QTextEdit()
        self.info_label.setText("")
        self.info_label.setReadOnly(True)
        self.info_label.setLineWrapMode(QTextEdit.NoWrap)
        self.info_label.setMaximumWidth(275)

        self.info_label.setFrameStyle(QFrame.Box | QFrame.Plain)
        self.info_label.setLineWidth(1)

        left_layout.addWidget(self.info_label)

        # --- Plots ---
        self.top_spectrum_plot = pg.PlotWidget()
        self.top_spectrum_plot.getPlotItem().layout.setContentsMargins(0, 13, 13, 0)

        # Defaults
        self.x = np.arange(self.x_len)
        y = np.zeros(self.x_len)

        # Call bar for changes
        self.bar = pg.BarGraphItem(x=self.x, height=y, width=1.0, brush="y")

        self.top_spectrum_plot.setLimits(
            xMin=0, xMax=self.x_len, yMin=0, minXRange=10, minYRange=4
        )

        self.top_spectrum_plot.addItem(self.bar)

        right_layout.addWidget(self.top_spectrum_plot, 1)

        # Graphics layout
        self.spectrogram_plot = pg.GraphicsLayoutWidget()
        right_layout.addWidget(self.spectrogram_plot, 4)

        # Waterfall plot
        self.plot = self.spectrogram_plot.addPlot(row=0, col=0)
        self.plot.getViewBox().setAspectLocked(True)
        self.img = pg.ImageItem()
        self.plot.addItem(self.img)

        # Connect x-change signal
        self.plot.getViewBox().sigXRangeChanged.connect(self.on_x_range_changed)

        self.plot.getViewBox().sigYRangeChanged.connect(self.on_y_range_changed)

        self.plot.getAxis("left").setStyle(tickFont=QFont("Arial", 9))

        # HistogramLUT (used only for gradient + dual slider)
        self.hist = pg.HistogramLUTItem(orientation="horizontal")
        self.hist.setMinimumHeight(15)
        self.hist.setImageItem(self.img)
        self.hist.vb.enableAutoRange(axis="x")
        self.hist.vb.enableAutoRange(axis="y")
        self.hist.vb.setLimits(xMin=-0.2, xMax=1e3, minXRange=1)
        self.hist.region.setBounds([0, 100000])

        self.spectrogram_plot.addItem(
            self.hist, row=1, col=0
        )  # Sets the slider at the bottoms
        self.top_spectrum_plot.setXLink(
            self.plot
        )  # Connects x-axis of bar with x-axis of waterfall
        self.hist.plot.setVisible(True)

        main_layout.addLayout(left_layout)
        main_layout.addLayout(right_layout)

        # Load colormap preset
        self.hist.gradient.loadPreset("viridis")

        self.plot.invertY(True)
        self.plot.layout.setContentsMargins(0, 0, 0, 0)
        self.top_spectrum_plot.getPlotItem().getAxis("left").setWidth(70)
        self.plot.getAxis("left").setWidth(60)
        # Data buffer

        self.plot.setLimits(
            xMin=0, xMax=self.x_len, yMin=0, yMax=self.y_len, minXRange=8, minYRange=8
        )
        self.img.setLevels((0, 5))

        self.time_selector = pg.LinearRegionItem([1, 9], orientation="horizontal")
        self.time_selector.sigRegionChanged.connect(self.time_selector_changed)

    def start_logger(self, *_):
        devices = list(RunManager.device_registry.keys())
        dialog = StartSpectrogramDialog(devices)
        if dialog.exec():
            if dialog.get_values()["instrument"]:
                "db_name, device, save_interval, "
                chosen_values = dialog.get_values()
                self.sigStartSpectrogram.emit(
                    chosen_values["filename"],
                    chosen_values["instrument"],
                    chosen_values["interval"],
                    chosen_values["channel_truncation"],
                    False,
                )

                self.btn_stop.setEnabled(True)
                self.btn_resume.setEnabled(False)

                # remove old lines
                for line in self.break_lines:
                    self.plot.removeItem(line)
                self.break_lines.clear()

    def restart_logger(self):
        current_spectrogram_name = self.spectrogram_selection.currentData()

        current_spectrogram = RunManager.loaded_spectrogram.get(
            current_spectrogram_name
        )
        if (
            current_spectrogram is not None
            and current_spectrogram.device_id not in RunManager.device_registry
        ):
            QMessageBox.warning(
                self,
                "Device not found",
                f"Spectrogram was measured with '{current_spectrogram.device_id}'. This device was not detected, spectrogram restart failed",
            )
            return

        if current_spectrogram is not None:
            current_spectrogram.pause_unpause()
            self.btn_resume.setEnabled(False)
            self.btn_stop.setEnabled(True)
            current_spectrogram.request_data()

    def pause_logger(self):
        current_spectrogram_name = self.spectrogram_selection.currentData()
        current_spectrogram = RunManager.loaded_spectrogram.get(
            current_spectrogram_name
        )

        if current_spectrogram is not None:
            current_spectrogram.pause_unpause()
            self.btn_resume.setEnabled(True)
            self.btn_stop.setEnabled(False)
            current_spectrogram.request_data()

    def show_data_store(self):
        self.sigShowDataStore.emit(1)

    def update_x_len(self, new_len: int, calib_coeff: list, concat_factor: int):
        if (
            new_len != self.x_len
            or len(calib_coeff) != len(self.current_calibration)
            or any(
                cc != nc
                for cc, nc in zip(self.current_calibration, calib_coeff)
                or self.current_concat_factor != concat_factor
            )
        ):
            self.x_len = new_len
            self.current_calibration = calib_coeff
            self.current_concat_factor = concat_factor

            self.x = np.arange(self.x_len)
            y = np.zeros(self.x_len)
            bar_width = np.mean(np.diff(self.x))

            self.top_spectrum_plot.removeItem(self.bar)
            self.bar = pg.BarGraphItem(x=self.x, height=y, width=bar_width, brush="y")
            self.top_spectrum_plot.addItem(self.bar)

            self.top_spectrum_plot.setLimits(xMax=np.max(self.x))
            self.plot.setLimits(xMin=self.x[0], xMax=self.x[-1])
            self.plot.setRange(xRange=[self.x[0], self.x[-1]])

            x_calib = np.polyval(calib_coeff, np.arange(self.x_len) * concat_factor)

            step = self.x_len // 15
            ticks = [(i * step, f"{x_c:.0f}") for i, x_c in enumerate(x_calib[::step])]

            self.top_spectrum_plot.getAxis("bottom").setTicks([ticks])
            self.plot.getAxis("bottom").setTicks([ticks])

    def update_spectrogram_y(self, timestamp_queue):
        self.y_axis[: len(timestamp_queue)] = np.asarray(timestamp_queue)[::-1]

        ymin, ymax = self.plot.getViewBox().viewRange()[1]

        n_ticks = 10
        positions = np.linspace(ymin, ymax, n_ticks)

        time_str = "%H:%M:%S\n%m-%d" if self.show_date_on_y else "%H:%M:%S"

        ticks = []
        for pos in positions:
            idx = int(np.clip(round(pos), 0, len(self.y_axis) - 1))
            ts = self.y_axis[idx]

            ticks.append(
                (pos, datetime.fromtimestamp(ts).strftime(time_str) if ts else "")
            )

        self.plot.getAxis("left").setTicks([ticks])

    def draw_y_break_lines(self, expected_dt: float):
        if not self.action_show_break_lines.isChecked():
            return
        dt = np.abs(np.diff(self.y_axis[self.y_axis > 0]))

        break_indices = np.where(dt > expected_dt * 1.5 + 1)[0]

        if len(break_indices) != self.break_lines:
            # remove old lines
            for line in self.break_lines:
                self.plot.removeItem(line)
            self.break_lines.clear()

            # add new lines
            for y in break_indices:
                line = pg.InfiniteLine(
                    pos=y,
                    angle=0,
                    pen=pg.mkPen((255, 0, 0, 180), width=2, style=Qt.DashLine),
                )
                self.plot.addItem(line)
                self.break_lines.append(line)

    def on_x_range_changed(self, viewbox, x_range):
        xmin, xmax = x_range

        n_ticks = 10
        positions = np.linspace(xmin, xmax, n_ticks)

        ticks = []
        for pos in positions:
            idx = int(np.clip(pos, 0, self.x_len - 1))
            calib_val = np.polyval(
                self.current_calibration, idx * self.current_concat_factor
            )
            ticks.append((pos, f"{calib_val:.0f}"))

        self.plot.getAxis("bottom").setTicks([ticks])
        self.top_spectrum_plot.getAxis("bottom").setTicks([ticks])

    def on_y_range_changed(self, viewbox, y_range):
        ymin, ymax = y_range

        n_ticks = 10
        positions = np.linspace(ymin, ymax, n_ticks)

        ticks = []
        for pos in positions:
            idx = int(np.clip(round(pos), 0, len(self.y_axis) - 1))

            ts = self.y_axis[idx]
            time_str = "%H:%M:%S\n%m-%d" if self.show_date_on_y else "%H:%M:%S"
            ticks.append(
                (pos, datetime.fromtimestamp(ts).strftime(time_str) if ts else "")
            )

        self.plot.getAxis("left").setTicks([ticks])

    def update_on_spectrogram_selection(self):
        db_name = self.spectrogram_selection.currentData()
        logger = RunManager.loaded_spectrogram.get(db_name)
        self.y_axis = np.zeros_like(self.y_axis)  # Reset the y-axis

        # Ensure old breaklines are removed
        for line in self.break_lines:
            self.plot.removeItem(line)
        self.break_lines.clear()

        if logger:
            if logger.paused:
                self.btn_resume.setEnabled(True)
                self.btn_stop.setEnabled(False)
            else:
                self.btn_resume.setEnabled(False)
                self.btn_stop.setEnabled(True)
            logger.request_data()

    def connect_logger(self):
        for logger in list(RunManager.loaded_spectrogram.values()):
            logger.sigDataUpdated.connect(self.receive_data)

    def unload_logger(self):
        index = self.spectrogram_selection.currentIndex()
        if index != -1:
            self.spectrogram_selection.removeItem(index)

            if self.spectrogram_selection.count() == 0:
                self.img.clear()
                self.bar.setOpts(height=np.zeros(self.x_len))
                self.info_label.setText("")

    def receive_data(self, logger_name: str, data_packet: WrappedSpectrogramData):
        if self.action_pause_visual_updates.isChecked():
            return
        index = find_index_by_data(self.spectrogram_selection, logger_name)

        state_text = data_packet.status.name

        if index == -1:
            # Add new item
            self.spectrogram_selection.addItem(
                f"[{state_text}] {logger_name}", logger_name
            )
            self.spectrogram_selection.setCurrentText(f"[{state_text}] {logger_name}")
        else:
            # Update existing item text
            self.spectrogram_selection.setItemText(
                index, f"[{state_text}] {logger_name}"
            )
        if data_packet.db_name != self.spectrogram_selection.currentData():
            return

        self.current_packet_buffer = data_packet

        if (
            max(min(data_packet.spectrogram.maxlen, len(data_packet.spectrogram)), 256)
            != self.y_len
        ):
            self.y_len = max(
                min(data_packet.spectrogram.maxlen, len(data_packet.spectrogram)), 256
            )
            self.plot.setLimits(yMax=self.y_len)

        # Change unit for readability
        if data_packet.estimated_dose < 5e-1:
            dose = data_packet.estimated_dose * 1e3
            dose_unit = "nSv"
        elif data_packet.estimated_dose > 5e2:
            dose = data_packet.estimated_dose * 1e-3
            dose_unit = "mSv"
        else:
            dose = data_packet.estimated_dose
            dose_unit = "uSv"

        self.info_text_total = dedent(f"""
            Database:
                {data_packet.db_name}
            Instrument: {data_packet.instrument}
            Start Date: {datetime.fromtimestamp(round(data_packet.start_date))}
            Duration: {timedelta(seconds=np.floor(data_packet.duration)) if data_packet.duration else 0}
            Estimated Dose: {round(dose, 3)} {dose_unit}
            Time Interval: {data_packet.save_interval}s
            Spectrum Concat Factor: {data_packet.concat}
            Spectrum Channels: {data_packet.spect_channels}
        """).strip()
        self.set_info_text()

        self.update_x_len(
            data_packet.spect_channels,
            data_packet.calibration_coefficients,
            data_packet.concat,
        )  # Update x-axis if changed
        self.update_spectrogram_y(data_packet.timestamp_deque)
        self.draw_y_break_lines(data_packet.save_interval)
        if data_packet.latest_spectrum is None:
            return
        if not self.action_time_selector.isChecked():
            self.bar.setOpts(
                height=data_packet.latest_spectrum
            )  # Update bar plot above the waterfall
        self.update_spectrogram_img(data_packet.spectrogram)

    def update_spectrogram_img(self, view_buf):
        # Flip the view buffer so the earlist is at the top
        current_spectrogram = np.array(view_buf)[::-1]
        self.img.setImage(
            current_spectrogram.T,
            autoLevels=False,
            rect=QRectF(
                self.x[0], 0, self.x[-1] - self.x[0], current_spectrogram.shape[0]
            ),
        )
        self.time_selector_changed()

    def set_info_text(self):
        self.info_label.setText(self.info_text_total + "\n\n" + self.info_text_selector)

    def time_selector_changed(self):
        if (
            self.current_packet_buffer is None
            or not self.action_time_selector.isChecked()
        ):
            return
        ymin, ymax = self.time_selector.getRegion()
        ymin, ymax = round(ymin), round(ymax)

        self.bar.setOpts(
            height=np.sum(
                np.array(self.current_packet_buffer.spectrogram)[::-1][
                    max(ymin, 0) : min(ymax, self.y_len - 1)
                ],
                axis=0,
            )
        )
        timestamps = np.array(self.current_packet_buffer.timestamp_deque)[::-1]
        start_time, stop_time = timestamps[ymax - 1], timestamps[ymin]
        time_str = "%H:%M:%S\n%m-%d" if self.show_date_on_y else "%H:%M:%S"

        self.info_text_selector = (
            "= Time Selector =\n"
            f"Duration: {timedelta(seconds=self.current_packet_buffer.save_interval * (min(ymax, len(self.current_packet_buffer.spectrogram)) - ymin))}\n"
            f"{datetime.fromtimestamp(start_time).strftime(time_str)} -> {datetime.fromtimestamp(stop_time).strftime(time_str)}\n"
        )
        self.set_info_text()

    def set_time_selector(self, state: bool):
        if state:
            self.plot.addItem(self.time_selector)
            self.time_selector_changed()
        else:
            self.plot.removeItem(self.time_selector)
            if len(self.current_packet_buffer.spectrogram) > 0:
                self.bar.setOpts(height=self.current_packet_buffer.spectrogram[0])

    def export_to_spectrum(self):
        "Export the total accumulated spectrum in a spectrogram to a spectrum"

        current_spectrogram_name = self.spectrogram_selection.currentData()
        current_spectrogram = RunManager.loaded_spectrogram.get(
            current_spectrogram_name
        )

        if current_spectrogram is not None:
            parser = file_io.db_parser(connection=current_spectrogram.connection)
            dialog = SaveNamingDialog(name=current_spectrogram.db_name)

            res = dialog.exec()
            if res != SaveNamingDialog.Accepted:
                return

            if not dialog.get_name:
                QMessageBox.warning(self, "Error", "Invalid name")
                return

            new_name = Settings.Paths.spectrum_library / dialog.get_name()

            new_spectrum = file_io.db_writer.build_spectrum_from_db(parser, new_name)

            new_spectrum.remark = dialog.get_remark()
            new_name = Settings.Paths.spectrum_library / current_spectrogram.db_name

            new_spectrum = file_io.db_writer.build_spectrum_from_db(parser, new_name)
            new_spectrum.remark = dialog.get_remark()

            IOManager.FileIndex.spectrum_index.save_file(new_spectrum)
            Log.info(
                f"Spectrum Exported from spectrogram: spectrogram={current_spectrogram.db_name}, spectrum={new_spectrum.name}"
            )
        
        else:
            QMessageBox.warning(self, "Error", "No spectrogram to export")

    def export_time_selection_to_spectrum(self):
        "Export the spectrum created within the time selector to  to a spectrum"

        if not self.action_time_selector.isChecked():
            QMessageBox.warning(
                self, "Error", "No spectrogram section is selected"
            )
            return

        current_spectrogram_name = self.spectrogram_selection.currentData()
        current_spectrogram = RunManager.loaded_spectrogram.get(
            current_spectrogram_name
        )

        ymin, ymax = self.time_selector.getRegion()
        ymin, ymax = round(ymin), round(ymax)
        timestamps = np.array(self.current_packet_buffer.timestamp_deque)[::-1]
        start_time, stop_time = (
            datetime.fromtimestamp(timestamps[ymax - 1]),
            datetime.fromtimestamp(timestamps[ymin]),
        )

        if current_spectrogram is not None:
            parser = file_io.db_parser(connection=current_spectrogram.connection)
            dialog = SaveNamingDialog(name=current_spectrogram.db_name)
            res = dialog.exec()
            if res != SaveNamingDialog.Accepted:
                return
            if not dialog.get_name:
                QMessageBox.warning(self, "Error", "Invalid name")
                return

            new_name = Settings.Paths.spectrum_library / dialog.get_name()

            new_spectrum = file_io.db_writer.build_spectrum_from_db(parser, new_name)

            new_spectrum.remark = dialog.get_remark()
            new_name = Settings.Paths.spectrum_library / current_spectrogram.db_name
            new_spectrum = file_io.db_writer.build_spectrum_from_db(
                parser, new_name, start_time, stop_time
            )
            print(new_spectrum.foreground)
            IOManager.FileIndex.spectrum_index.save_file(new_spectrum)
            Log.info(
                f"Spectrum Exported from spectrogram: spectrogram={current_spectrogram.db_name}, spectrum={new_spectrum.name}"
            )
        else:
            QMessageBox.warning(self, "Error", "No spectrogram to export")

if __name__ == "__main__":
    pass
