from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QTableWidget,
    QTableWidgetItem,
    QSizePolicy
)

from ..SpectrumClasses import ROI

def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))

class ROIInfoPane(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        # ---- Box ----
        self.group_box = QGroupBox(title)

        # ---- Table ----
        titles = ["ROI", "Low", "High", "FWHM", "G", "B", "N", "A", r"μ", r"σ", ]
        self.table = QTableWidget(1, len(titles))
        self.table.setColumnCount(len(titles))
        self.table.setHorizontalHeaderLabels(titles)


        self.table.setMaximumWidth(
            self.table.verticalHeader().width()
            + self.table.horizontalHeader().length()
            + self.table.frameWidth() * 2
        )
        self.table.setSizePolicy(
            QSizePolicy.Expanding,      # vertical
            QSizePolicy.MinimumExpanding,  # horizontal
        )

        # Layout inside the box
        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table)

        # Main layout of this widget
        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group_box)

        self.ROIs = {}

    def recieve_roi(self, roi: ROI):
        # roi = ROI()
        # if roi.tag not in self.rois:
        #     row_index = self.table.rowCount()  # current number of rows
        #     self.table.insertRow(row_index)
        # self.ROIs[roi.tag] = roi

        row = [roi.tag, roi.low, roi.high, 
               round(roi.gaussian.FWHM(), 4), 
               round(roi.gaussian.G, 4), 
               round(roi.gaussian.B, 4), 
               round(roi.gaussian.N, 4), 
               round(roi.gaussian.A, 4), 
               round(roi.gaussian.mu, 4), 
               round(roi.gaussian.sigma, 4), ]
        write_row(self.table, 0, row)


    def set_roi_info(self, roi):
        pass


    