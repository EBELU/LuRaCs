import sys
import time
from datetime import datetime
import numpy as np
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout
from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QListWidget, QListWidgetItem, QPushButton, QHBoxLayout, QComboBox, QLabel, QLineEdit, QSpinBox, QDoubleSpinBox, QDialogButtonBox, QSizePolicy, QFormLayout
)
from PySide6.QtCore import QTimer
import pyqtgraph as pg
pg.setConfigOptions(antialias=True)

class StartLoggerDialog(QDialog):
    def __init__(self, instruments, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Start Data Logger")
        self.setMinimumWidth(300)
        self.setMinimumHeight(170)
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
            "filename": self.filename_edit.text(),
            "interval": self.interval_spin.value(),
            "channel_truncation": self.trunc_spin.value(),
        }
        
        
        
        

class SpectrogramWidget(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Waterfall - Clean LUT (No Histogram Bars)")

        layout = QVBoxLayout(self)

        # Graphics layout
        self.graphics = pg.GraphicsLayoutWidget()
        layout.addWidget(self.graphics)

        # Plot
        self.plot = self.graphics.addPlot(row=0, col=0)
        self.img = pg.ImageItem()
        self.plot.addItem(self.img)


        # HistogramLUT (used only for gradient + dual slider)
        self.hist = pg.HistogramLUTItem()
        self.hist.setImageItem(self.img)

        self.graphics.addItem(self.hist, row=0, col=1)
        self.hist.plot.setVisible(True)

        # Stop auto histogram range recalculation (removes jitter)
        self.hist.vb.disableAutoRange()

        # Load colormap preset
        self.hist.gradient.loadPreset("viridis")

        # Data buffer
        self.history = 256
        self.freq_bins = 1024
        self.data = np.zeros((self.history, self.freq_bins), dtype=np.float32)
        self.plot.setLimits(
            xMin=0,
            xMax=self.freq_bins,
            yMin=0,
            yMax=self.history
        )
        self.img.setLevels((-60, 0))

        # Timer
        self.timer = QTimer()
        self.timer.timeout.connect(self.update_data)
        self.timer.start(500)

    def update_data(self):
        noise = np.random.normal(0, 1, self.freq_bins)
        tone_center = np.random.randint(40, 220)
        tone = np.exp(-((np.arange(self.freq_bins) - tone_center) ** 2) / 150)

        new_row = 20 * np.log10(np.abs(noise + 5 * tone) + 1e-6)

        self.data[:-1] = self.data[1:]
        self.data[-1] = new_row

        # Critical: never enable autoLevels
        self.img.setImage(self.data, autoLevels=False)


# if __name__ == "__main__":

#     app = QApplication(sys.argv)
#     pg.setConfigOptions(imageAxisOrder="row-major")

#     win = SpectrogramWidget()
#     win.resize(1000, 600)
#     win.show()

#     sys.exit(app.exec())
    
    
if __name__ == "__main__":
    app = QApplication(sys.argv)

    connected_instruments = ["Instrument A", "Instrument B", "Instrument C"]

    dialog = StartLoggerDialog(connected_instruments)
    if dialog.exec():
        print("User pressed OK")
        print(dialog.get_values())
    else:
        print("User cancelled")

    sys.exit()




# class DateTimeDemo(QWidget):
#     def __init__(self):
#         super().__init__()
#         self.setWindowTitle("Choose Date and Time")

#         layout = QVBoxLayout(self)

#         self.datetime_edit = QDateTimeEdit()
#         self.datetime_edit.setCalendarPopup(True)  # enables calendar dropdown
#         self.datetime_edit.setDateTime(QDateTime.currentDateTime())

#         # Optional: display format
#         self.datetime_edit.setDisplayFormat("yyyy-MM-dd HH:mm:ss")

#         layout.addWidget(self.datetime_edit)


# if __name__ == "__main__":
#     app = QApplication(sys.argv)
#     win = DateTimeDemo()
#     win.resize(300, 100)
#     win.show()
#     sys.exit(app.exec())