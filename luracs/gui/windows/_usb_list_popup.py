from PySide6.QtWidgets import (
    QDialog,
    QVBoxLayout,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QHBoxLayout,
)
from PySide6.QtCore import Qt, Signal

from luracs.core import RunManager, Log
import asyncio

def _on_usb_device_selected(device: dict):
    Log.debug("Selected USB device:", device)

    serial = device.get("serial_number")
    product = (device.get("product") or "").lower()

    if not serial:
        print("Device has no serial number")
        return

    # Adjust if your RunManager function name differs
    asyncio.create_task(RunManager.add_device(serial, product, True))

class USBListPopup(QDialog):
    sigSelected = Signal(object)
    sigCancel = Signal()
    sigRescan = Signal()
    
    def __init__(self, parent = None):
        super().__init__(parent=parent)
        self.setWindowTitle("Connect device")
        self.resize(350, 450)
        
        self._devices: list[dict] = []
        
        # --- Main layout ---
        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(10)

        # --- List widget ---
        self.list_widget = QListWidget()
        self.list_widget.setSelectionMode(QListWidget.SingleSelection)
        self.list_widget.itemDoubleClicked.connect(self._on_double_click)
        
        layout.addWidget(self.list_widget)
        
        # --- Buttons ---
        button_layout = QHBoxLayout()
        button_layout.setSpacing(10)
        self.rescan_btn = QPushButton("Rescan")
        self.rescan_btn.clicked.connect(self._request_usb_scan)
        button_layout.addWidget(self.rescan_btn)
        button_layout.addStretch()

        self.confirm_btn = QPushButton("Confirm")
        self.confirm_btn.clicked.connect(self._on_confirmed)

        self.cancel_btn = QPushButton("Cancel")
        self.cancel_btn.clicked.connect(self.reject)

        button_layout.addWidget(self.confirm_btn)
        button_layout.addWidget(self.cancel_btn)
        layout.addLayout(button_layout)
        
        self.list_widget.itemDoubleClicked.connect(self._on_double_click)
        


    def _on_confirmed(self):
        item = self.list_widget.currentItem()
        if item:
            dev = item.data(Qt.UserRole)
            if dev:
                self.sigSelected.emit(dev)
        self.close()

    def _on_double_click(self, item):
        dev = item.data(Qt.UserRole)
        if dev:
            self.sigSelected.emit(dev)
        self.close()

    def _request_usb_scan(self):
        devices = RunManager.scan_all_usb()
        self.set_devices(devices)
        
    def set_devices(self, devices: list):
        self.list_widget.clear()
        self._devices = devices or []

        if not self._devices:
            self.list_widget.addItem("No USB devices found")
            return

        for dev in self._devices:
            product = dev.get("product") or "Unknown"
            serial = dev.get("serial_number") or "No SN"

            display_name = f"{product} ({serial})"

            item = QListWidgetItem(display_name)
            item.setTextAlignment(Qt.AlignCenter)
            item.setFlags(Qt.ItemIsSelectable | Qt.ItemIsEnabled)
            item.setData(Qt.UserRole, dev)

            self.list_widget.addItem(item)

        self.list_widget.clearSelection()
        
if __name__ == "__main__":
    import sys
    from PySide6.QtWidgets import QApplication
    app = QApplication.instance() or QApplication(sys.argv)

    window = USBListPopup()
    window.resize(200, 300)
    window.show()


    sys.exit(app.exec())