import platform

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import QListWidgetItem, QPushButton

from luracs.clients import ConnectionType, DeviceWrapper
from luracs.core import Log, RunManager

import usb.util
import usb.core

from .ListPopupBase import ListPopupNonBlocking


def _on_usb_device_selected(device: usb.Device, selection_type: str):
    Log.debug("Selected USB device:", device)

    serial = usb.util.get_string(device, device.iSerialNumber)
    product = usb.util.get_string(device, device.iProduct)

    if not serial and platform.system() != "Windows":
        Log.error("Device has no serial number")
        return
    
    if selection_type == "auto":
        for name in DeviceWrapper.get_registry():
            if name.lower() in product.lower():
                RunManager.add_device(serial, name, "USB", {"usb_device": device})
                break
                
    else:
        RunManager.add_device(serial, selection_type, "USB", {"usb_device": device})



class USBListPopup(ListPopupNonBlocking):
    """
    USB device selector popup.
    - Non-blocking
    - Rescan button
    """

    deviceSelected = Signal(object, str)

    def __init__(self, parent=None):
        super().__init__("Select USB Device", parent)

        self.deviceSelected.connect(_on_usb_device_selected)

        self._devices: list[usb.Device] = []
        
        # Auto detection currently does not work on windows
        # Should make one based on vendor and product id
        if platform.system() == "Windows":
            self.alternatives_combo.removeItem(0)
        
        for name, wrapper in DeviceWrapper.get_registry().items():
            if ConnectionType.USB in wrapper.get_connection_types():
                self.alternatives_combo.addItem(name.replace("_", " ").capitalize(), name)

        # ------------------ Rescan button ------------------
        self.rescan_btn = QPushButton("Rescan")
        self.rescan_btn.clicked.connect(self._request_usb_scan)

        btn_layout = self.layout().itemAt(2).layout()
        btn_layout.insertWidget(0, self.rescan_btn)
        
        

        # Base popup hooks
        self.confirmed.connect(self._on_confirmed)
        self.cancelled.connect(self.close)

    # ------------------------------------------------
    # Public API
    # ------------------------------------------------

    def start_popup(self):
        QTimer.singleShot(0, self._start_popup)

    def _start_popup(self):
        if not self.isVisible():
            self.show()
        self._request_usb_scan()

    def set_devices(self, devices: list[usb.Device]):
        self.list_widget.clear()
        self._devices = devices or []

        if not self._devices:
            self.list_widget.addItem("No USB devices found")
            return

        for dev in self._devices:
            if platform.system() == "Windows":
                serial = usb.util.get_string(dev, dev.iSerialNumber)
                product = usb.util.get_string(dev, dev.iProduct)
                port = getattr(dev, "port_number", "None"),
                bus = getattr(dev, "bus", None)
                display_name = f"{product} (serial={serial}, bus={bus}, port={port})"
                
            else:
                product = usb.util.get_string(dev, dev.iProduct)
                serial = usb.util.get_string(dev, dev.iSerialNumber)

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
                self.deviceSelected.emit(dev, self.alternatives_combo.currentData())
        self.close()

    def _on_double_click(self, item):
        dev = item.data(Qt.UserRole)
        if dev:
            self.deviceSelected.emit(dev, self.alternatives_combo.currentData())
        self.close()

    def _request_usb_scan(self):
        devices = RunManager.scan_all_usb()
        self.set_devices(devices)
