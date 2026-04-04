import time
import asyncio
from PySide6.QtCore import Signal, QTimer, Qt
from PySide6.QtWidgets import QListWidgetItem, QPushButton

from .ListPopupBase import ListPopupNonBlocking

from core import RunManager

def _on_bt_device_selected(device):
    print("Selected device:", device)
    
    if not device.name:
        return

    if "radiacode" in device.name.lower():
        device_type = "radiacode"
    elif "raysid" in device.name.lower():
        device_type = "raysid"
    else:
        print(f"Invalid device type {device.name}")
        return
    asyncio.create_task(
        RunManager.add_device(device, device_type)
    )
    

class BluetoothListPopup(ListPopupNonBlocking):
    """
    Bluetooth device selector popup.
    - Non-blocking
    - Rescan button
    - Scan countdown
    - Emits selected Bleak device
    """

    deviceSelected = Signal(object)
    rescanRequested = Signal()
    cancelScan = Signal()

    def __init__(self, scan_duration: int = 5, parent=None):
        super().__init__("Select Bluetooth Device", parent)

        self.deviceSelected.connect(_on_bt_device_selected)
        self.rescanRequested.connect(self._request_bt_scan)
        self.cancelScan.connect(RunManager.cancel_scan_task)
        RunManager.bluetoothFound.connect(self.receive_BT_list)

        self.scan_task = None
        
        self._scan_duration = scan_duration
        self._scan_start_time = None
        self._devices_by_name: dict[str, object] = {}

        # ------------------ Rescan button ------------------
        self.rescan_btn = QPushButton("Rescan")
        self.rescan_btn.clicked.connect(self._on_rescan)

        # Insert before Confirm
        btn_layout = self.layout().itemAt(1).layout()
        btn_layout.insertWidget(0, self.rescan_btn)

        # ------------------ Countdown timer ------------------
        self._timer = QTimer(self)
        self._timer.setInterval(1000)
        self._timer.timeout.connect(self._update_countdown)

        # ------------------ Hook base confirmation ------------------
        self.confirmed.connect(self._on_confirmed)
        self.cancelled.connect(self._on_cancelled)

    # ------------------------------------------------
    # Public Bluetooth API
    # ------------------------------------------------


    def start_popup(self):
        QTimer.singleShot(0, lambda: self._start_popup())
        
    def _start_popup(self):
        self.start_scan_ui()
        if not self.isVisible():
            self.show()
        self._request_bt_scan()
    
    def start_scan_ui(self):
        """Call when scan begins"""
        self.rescan_btn.setEnabled(False)
        self._scan_start_time = time.monotonic()

        self.list_widget.clear()
        self._update_countdown()
        self._timer.start()

    def set_devices(self, devices: list):
        """Called when scan results arrive"""
        self._timer.stop()
        self.rescan_btn.setEnabled(True)
        self._scan_start_time = None

        self.list_widget.clear()
        self._devices_by_name.clear()

        if not devices:
            self.list_widget.addItem("No devices found")
            return

        # Prepare list with priority devices on top
        priority = []
        normal = []

        for dev in devices:
            name = dev.name or dev.address
            self._devices_by_name[name] = dev

            # Check if name contains radiacode or raysid (case-insensitive)
            if name.lower().find("radiacode") != -1 or name.lower().find("raysid") != -1:
                display_name = f"{name} ☢️"
                priority.append((display_name, dev))
            else:
                normal.append((name, dev))

        # Combine priority first, then normal devices
        sorted_devices = priority + normal

        for display_name, dev in sorted_devices:
            item = QListWidgetItem(display_name)
            item.setTextAlignment(Qt.AlignCenter)
            item.setFlags(Qt.ItemIsSelectable | Qt.ItemIsEnabled)
            item.setData(Qt.UserRole, dev)
            self.list_widget.addItem(item)

        self.list_widget.clearSelection()

    # ------------------------------------------------
    # Internal
    # ------------------------------------------------

    def _on_rescan(self):
        self.start_scan_ui()
        self.rescanRequested.emit()

    def _update_countdown(self):
        if self._scan_start_time is None:
            return

        elapsed = int(time.monotonic() - self._scan_start_time)
        remaining = max(0, self._scan_duration - elapsed)

        text = f"🔍 Scanning… {remaining}s"

        if self.list_widget.count() == 0:
            self.list_widget.addItem(text)
        else:
            self.list_widget.item(0).setText(text)

        if remaining <= 0:
            self._timer.stop()
            self.list_widget.item(0).setText("🔍 Finishing scan…")

    def _on_confirmed(self):
        item = self.list_widget.currentItem()
        if item:
            # Remove emoji (everything after first space)
            name = item.text().split(" ☢️")[0]
            dev = self._devices_by_name.get(name)
            if dev:
                self.deviceSelected.emit(dev)
            self.close()

    def _on_double_click(self, item):
        name = item.text().split(" ☢️")[0]
        dev = self._devices_by_name.get(name)
        if dev:
            self.deviceSelected.emit(dev)
        self.close()

    def _on_cancelled(self):
        self.cancelScan.emit()
        if self.scan_task:
            self.scan_task.cancel()

    def closeEvent(self, event):
        self._timer.stop()
        super().closeEvent(event)

    def _request_bt_scan(self):
        self.start_scan_ui()
        self.scan_task = asyncio.create_task(RunManager.find_bluetooth())
        
    def receive_BT_list(self, device_list):
        self.set_devices(device_list)