import asyncio
from PySide6.QtCore import Signal, QTimer, Qt
from PySide6.QtWidgets import QListWidgetItem, QPushButton

from .ListPopupBase import ListPopupNonBlocking
from luracs.core import RunManager, Log


def _on_usb_device_selected(device: dict):
    Log.debug("Selected USB device:", device)

    serial = device.get("serial_number")
    product = (device.get("product") or "").lower()

    if not serial:
        print("Device has no serial number")
        return

    RunManager.add_device(serial, product, True)


class USBListPopup(ListPopupNonBlocking):
    """
    USB device selector popup.
    - Non-blocking
    - Rescan button
    """

    deviceSelected = Signal(object)

    def __init__(self, parent=None):
        super().__init__("Select USB Device", parent)

        self.deviceSelected.connect(_on_usb_device_selected)

        self._devices: list[dict] = []

        # ------------------ Rescan button ------------------
        self.rescan_btn = QPushButton("Rescan")
        self.rescan_btn.clicked.connect(self._request_usb_scan)

        btn_layout = self.layout().itemAt(1).layout()
        btn_layout.insertWidget(0, self.rescan_btn)

        # Base popup hooks
        self.confirmed.connect(self._on_confirmed)
        self.cancelled.connect(self.close)
        self.list_widget.itemDoubleClicked.connect(self._on_double_click)

    # ------------------------------------------------
    # Public API
    # ------------------------------------------------

    def start_popup(self):
        QTimer.singleShot(0, self._start_popup)

    def _start_popup(self):
        if not self.isVisible():
            self.show()
        self._request_usb_scan()

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

    # ------------------------------------------------
    # Internal
    # ------------------------------------------------

    def _on_confirmed(self):
        item = self.list_widget.currentItem()
        if item:
            dev = item.data(Qt.UserRole)
            if dev:
                self.deviceSelected.emit(dev)
        self.close()

    def _on_double_click(self, item):
        dev = item.data(Qt.UserRole)
        if dev:
            self.deviceSelected.emit(dev)
        self.close()

    def _request_usb_scan(self):
        devices = RunManager.scan_all_usb()
        self.set_devices(devices)
