from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QTableWidget,
    QTableWidgetItem,
    QSizePolicy
)

from ..SpectrumClasses import ROI


from ..Globals import SpectrumManager

def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))

class ROIInfoPane(QWidget):
    
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        # --- Signals ---
        SpectrumManager.Signals.roiUpdated.connect(self.recieve_roi)
        SpectrumManager.Signals.roiRemoved.connect(self.delete_roi)
        
        # ---- Box ----
        self.group_box = QGroupBox(title)

        # ---- Table ----
        titles = ["ROI", "Low", "High", "Peak Center", "FWHM", "Max Height","G", "B", "N",]
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

    def recieve_roi(self, roi_tag):
        roi = list(SpectrumManager.get_spectra_dict().values())[0].ROIs[roi_tag]
        
        if roi.tag not in self.ROIs:
            row_index = self.table.rowCount()
            self.table.insertRow(row_index)
            self.ROIs[roi.tag] = row_index
        else:
            row_index = self.ROIs[roi.tag]

        row = [roi.tag, round(roi.low), round(roi.high), 
                round(roi.gaussian.mu, 4), 
                round(roi.gaussian.FWHM(), 4), 
                round(roi.gaussian.max_height(), 4),
                round(roi.gaussian.G, 4), 
                round(roi.gaussian.B, 4), 
                round(roi.gaussian.N, 4), 
                round(roi.gaussian.A, 4), 

]
        write_row(self.table, row_index, row)

    def delete_roi(self, roi: ROI):
        assert roi in self.ROIs, f"{roi} id not in self.ROIs: {self.ROIs.keys()}"
        roi_idx = self.ROIs[roi]
        self.table.removeRow(roi_idx)
        for key in self.ROIs:
            if self.ROIs[key] > roi_idx:
                self.ROIs[key] -= 1

    