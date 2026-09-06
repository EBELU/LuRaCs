from PySide6.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QListWidget, QListWidgetItem
from PySide6.QtCore import Qt

from luracs.core import Settings

class DriverLibraryDialog(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Driver Library")
        self.resize(400, 300)

        layout = QVBoxLayout(self)

        self.driver_list = QListWidget()
        layout.addWidget(self.driver_list)
        
        button_layout = QHBoxLayout()
        self.delete_button = QPushButton("Delete Selected")
        self.delete_button.clicked.connect(self.delete_selected_driver)
        button_layout.addWidget(self.delete_button)

        self.close_button = QPushButton("Close")
        self.close_button.clicked.connect(self.close)
        button_layout.addWidget(self.close_button)
        layout.addLayout(button_layout)

    def load_drivers(self):
        # Load drivers from the third-party drivers library
        drivers_path = Settings.Paths.third_party_drivers_library
        for driver_file in drivers_path.glob("*"):
            print(f"Checking driver file: {driver_file}")
            if driver_file.is_file():
                item = QListWidgetItem(driver_file.name)
                item.setData(Qt.UserRole, driver_file)
                self.driver_list.addItem(item)
    
    def delete_selected_driver(self):
        selected_items = self.driver_list.selectedItems()
        for item in selected_items:
            driver_file = item.data(Qt.UserRole)
            if driver_file.is_file():
                driver_file.unlink()  # Delete the file
                self.driver_list.takeItem(self.driver_list.row(item))  # Remove from list
    
    def show(self):
        super().show()
        self.load_drivers()