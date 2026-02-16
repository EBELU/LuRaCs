from PySide6.QtWidgets import QApplication, QWidget
from PySide6.QtWidgets import (
    QWidget, QGroupBox, QVBoxLayout, QTableWidget, QTableWidgetItem,
    QSizePolicy, QColorDialog, QFrame, QHBoxLayout
)
from PySide6.QtCore import Qt, Signal
import sys

def write_row(table, row_index, values):
    for col_index, value in enumerate(values):
        table.setItem(row_index, col_index, QTableWidgetItem(str(value)))


class DeviceInfoPane(QWidget):

    def __init__(self, title="", parent=None):
        super().__init__(parent)


        self.group_box = QGroupBox(title)

        titles = ["", "Device", "Status", "Battery", "Charging", "Temperature\n [°C]"]
        self.table = QTableWidget(0, len(titles))
        self.table.setColumnCount(len(titles))
        self.table.setHorizontalHeaderLabels(titles)
        
                # Layout inside the box
        box_layout = QVBoxLayout(self.group_box)
        box_layout.addWidget(self.table)

        # Main layout of this widget
        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group_box)
        
        
app = QApplication.instance() or QApplication(sys.argv)


w = DeviceInfoPane()
w.resize(400, 300)
w.show()

app.exec()