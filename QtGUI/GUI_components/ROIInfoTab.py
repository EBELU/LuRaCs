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
        self.table = QTableWidget(0, len(titles))
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
        if roi.tag not in self.ROIs:
            row_index = self.table.rowCount()
            self.table.insertRow(row_index)
            self.ROIs[roi.tag] = row_index
        else:
            row_index = self.ROIs[roi.tag]

        row = [roi.tag, roi.low, roi.high, 
               round(roi.gaussian.FWHM(), 4), 
               round(roi.gaussian.G, 4), 
               round(roi.gaussian.B, 4), 
               round(roi.gaussian.N, 4), 
               round(roi.gaussian.A, 4), 
               round(roi.gaussian.mu, 4), 
               round(roi.gaussian.sigma, 4), ]
        write_row(self.table, row_index, row)

    def delete_roi(self, roi: ROI):
        assert roi.tag in self.ROIs, f"{roi.tag} id not in self.ROIs: {self.ROIs.keys()}"
        self.table.removeRow(self.ROIs[roi.tag])

    