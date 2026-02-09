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

def format_duration(seconds):
    seconds = int(seconds)
    days, seconds = divmod(seconds, 86400)
    hours, seconds = divmod(seconds, 3600)
    minutes, seconds = divmod(seconds, 60)
    return f"{days:02d} {hours:02d}:{minutes:02d}:{seconds:02d}"


class SpectrumInfoPane(QWidget):
    
    def __init__(self, title="", parent=None):
        super().__init__(parent)
        # --- Signals ---
        SpectrumManager.Signals.spectrumUpdated.connect(self.recieve_update)
        
        # ---- Box ----
        self.group_box = QGroupBox(title)

        # ---- Table ----
        titles = ["", "Spectrum", "Type", "Counts", "Uptime"]
        self.table = QTableWidget(0, len(titles))
        self.table.setColumnCount(len(titles))
        self.table.setHorizontalHeaderLabels(titles)

        self.table.setColumnWidth(3, 100)
        self.table.setColumnWidth(4, 150)


        self.table.setMaximumWidth(
            self.table.verticalHeader().width()
            + self.table.horizontalHeader().length()
            + self.table.frameWidth() * 2 + 12
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

        self.saved_rows = {}
        self.row_counter = 0


    def recieve_update(self, name):
        new_spect = SpectrumManager.get_spectrum(name)

        if name not in self.saved_rows:
            row_indicies = []
            self.table.insertRow(self.row_counter)
            row_indicies.append(self.row_counter)
            self.row_counter += 1
            if new_spect.bkg_y_data is not None:
                self.table.insertRow(self.row_counter)
                row_indicies.append(self.row_counter)
                self.row_counter += 1

            self.saved_rows[name] = row_indicies


        indicies = self.saved_rows[name]

        if len(indicies) == 1 and new_spect.bkg_y_data is not None:
            new_index = indicies[0] + 1
            
            for key, spect_rows in self.saved_rows.items():
                new_idxs = [i + 1 for i in spect_rows if i >= new_index]
                self.saved_rows[key] = new_idxs


            self.table.insertRow(new_index)
            indicies.append(new_index)
            self.saved_rows[name] = indicies
            assert len(self.saved_rows[name]) <= 2




        indicies = self.saved_rows[name]
        if len(indicies) == 1:
            write_row(self.table, indicies[0], ["Blue", new_spect.name, "Foregorund", new_spect.total_counts, new_spect.primary_uptime])
        else:
            write_row(self.table, indicies[0], ["Blue", new_spect.name, "Foregorund", f"{new_spect.total_counts:,}".replace(",", " "), format_duration(new_spect.primary_uptime)])
            write_row(self.table, indicies[1], ["Red", new_spect.name, "Background", f"{sum(new_spect.bkg_y_data):,}".replace(",", " "), format_duration(new_spect.bkg_uptime)])