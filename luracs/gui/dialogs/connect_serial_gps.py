from PySide6.QtCore import Qt, Signal
from PySide6.QtSerialPort import QSerialPortInfo, QSerialPort
from PySide6.QtWidgets import (
    QDialog,
    QDialogButtonBox,
    QFormLayout,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QVBoxLayout,
    QComboBox
)

from luracs.core import RunManager


class ConnectSerialGPSDialog(QDialog):
    sigAccepted = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent=parent)
        self.setWindowTitle("Connect USB Serial GPS")
        
        self.sigAccepted.connect(RunManager.add_device)

        main_layout = QVBoxLayout(self)

        form = QFormLayout()

        self.ip_line = QLineEdit()
        self.ip_line.setMinimumWidth(250)
        form.addRow("Port", self.ip_line)

        self.ports_list = QListWidget()
        self.ports_list.itemDoubleClicked.connect(self.port_double_clicked)
        form.addRow("Detected Ports", self.ports_list)
        
        baud_rates = [
            QSerialPort.BaudRate.Baud1200,
            QSerialPort.BaudRate.Baud2400,
            QSerialPort.BaudRate.Baud4800,
            QSerialPort.BaudRate.Baud9600,
            QSerialPort.BaudRate.Baud19200,
            QSerialPort.BaudRate.Baud38400,
            QSerialPort.BaudRate.Baud57600,
            QSerialPort.BaudRate.Baud115200,
        ]

        self.baud_combo = QComboBox()

        for baud in baud_rates:
            self.baud_combo.addItem(str(baud.value), baud)

        self.baud_combo.setCurrentIndex(2)
        form.addRow("Baud Rate", self.baud_combo)

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
        
    def exec(self):
        self.ports_list.clear()
        for port in QSerialPortInfo.availablePorts():
            item = QListWidgetItem(f"{port.portName()} | {port.description()} | {port.systemLocation()}")
            item.setData(Qt.UserRole, port)
            self.ports_list.addItem(item)
        return super().exec()

    def port_double_clicked(self, item):
        port = item.data(Qt.UserRole)
        
        self.ip_line.setText(port.systemLocation())
        
    def emit_connection(self):
        port_name = self.ip_line.text()
        if port_name:
            RunManager.connect_serial_gps(port_name, self.baud_combo.currentData())

        