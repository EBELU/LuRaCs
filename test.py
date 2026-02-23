import sys
import numpy as np
from PySide6.QtWidgets import QApplication, QWidget, QVBoxLayout
from PySide6.QtCore import QTimer
import pyqtgraph as pg


class WaterfallDemo(QWidget):
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

        # Add to graphics layout
        self.graphics.addItem(self.hist, row=0, col=1)

        # Hide histogram bars properly
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


if __name__ == "__main__":
    app = QApplication(sys.argv)
    pg.setConfigOptions(imageAxisOrder="row-major")

    win = WaterfallDemo()
    win.resize(1000, 600)
    win.show()

    sys.exit(app.exec())




class DateTimeDemo(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Choose Date and Time")

        layout = QVBoxLayout(self)

        self.datetime_edit = QDateTimeEdit()
        self.datetime_edit.setCalendarPopup(True)  # enables calendar dropdown
        self.datetime_edit.setDateTime(QDateTime.currentDateTime())

        # Optional: display format
        self.datetime_edit.setDisplayFormat("yyyy-MM-dd HH:mm:ss")

        layout.addWidget(self.datetime_edit)


# if __name__ == "__main__":
#     app = QApplication(sys.argv)
#     win = DateTimeDemo()
#     win.resize(300, 100)
#     win.show()
#     sys.exit(app.exec())