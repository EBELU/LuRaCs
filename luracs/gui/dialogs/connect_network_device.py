from PySide6.QtCore import Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QLineEdit,
    QVBoxLayout,
)

from luracs.core import Settings, RunManager


class ConnectNetworkDeviceDialog(QDialog):
    sigAccepted = Signal(str, str, object)
    sigUseNetworkGPS = Signal(bool)

    def __init__(self, parent=None):
        super().__init__(parent=parent)
        self.setWindowTitle("Connect network device")
        
        self.sigAccepted.connect(RunManager.add_device)

        main_layout = QVBoxLayout(self)

        form = QFormLayout()

        self.ip_line = QLineEdit()
        self.ip_line.setMinimumWidth(250)
        self.ip_line.setText(Settings.State.last_network_connection_ip)
        form.addRow("Device IP", self.ip_line)

        self.device_combo = QComboBox()
        form.addRow("Device Type", self.device_combo)
        
        self.device_combo.addItem("Detective X", userData="detective_x")

        self.capture_gps_check = QCheckBox(text="Use for GPS")
        form.addRow("", self.capture_gps_check)

        main_layout.addLayout(form)

        # OK / Cancel buttons at the bottom
        buttons = QDialogButtonBox(
            QDialogButtonBox.StandardButton.Ok
            | QDialogButtonBox.StandardButton.Cancel
        )

        buttons.accepted.connect(self.emit_connection)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

    def emit_connection(self):
        self.sigAccepted.emit(self.ip_line.text(), self.device_combo.currentData(), "NETWORK")
        self.sigUseNetworkGPS.emit(self.capture_gps_check.isChecked())
        Settings.State.last_network_connection_ip = self.ip_line.text()
        