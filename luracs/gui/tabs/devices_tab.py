from core import RunManager
from clients.DeviceWrappers import DeviceWrapper, WrappedStatusPackage
from .roi_info_tab import StrIdxTable
from PySide6.QtWidgets import (
    QWidget,
    QGroupBox,
    QVBoxLayout,
    QHBoxLayout,
    QTableWidget,
    QTableWidgetItem,
    QSizePolicy,
    QComboBox,
    QRadioButton,
    QButtonGroup,
    QMessageBox,
    QPushButton,
)
from PySide6.QtCore import Signal
from gui.misc.table_menu_button import MenuButton

class DevicesInfoTab(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        titles = [
            "Device",
            "Temperature",
            "Battery",
            "Charging",
            "Status",
            "Type",
            "Connection",
        ]
        widths = [10, 150] + [100] * (len(titles) - 1)

        self.table = StrIdxTable(has_menu_button=True)
        self.table.reset_table(titles, widths)

        self.row_regestry = {}

        # Create group box
        self.group_box = QGroupBox()

        group_layout = QVBoxLayout()
        group_layout.addWidget(self.table.table)
        self.group_box.setLayout(group_layout)

        # Main layout for the tab
        main_layout = QVBoxLayout()
        main_layout.addWidget(self.group_box)

        self.setLayout(main_layout)

        self.status_ts_buff = {}

        RunManager.newDeviceWrapped.connect(self.add_device)
        RunManager.statusUpdated.connect(self.update_status)

    def build_menu_button(self, device_name: str,
    ) -> MenuButton:
        
        menu_button = MenuButton(parent=self, title="...")
        action_disconnect = menu_button.add_action("Disconnect")
        action_disconnect.triggered.connect(lambda: RunManager.remove_device(device_name))
        
        action_settings = menu_button.add_action("Settings")
        
        return menu_button

    def add_device(self, name: str, wrapper: DeviceWrapper):
        wrapper.stateUpdated.connect(self.update_state)
        self.row_regestry[name] = [
            name,
            None,
            None,
            None,
            str(wrapper.state.name),
            str(wrapper.type),
            str(wrapper.connection),
        ]
        self.table.write_row(name, self.row_regestry[name], menu_button=self.build_menu_button(name))
        self.status_ts_buff[name] = 0

    def update_state(self, name, new_state: DeviceWrapper.DeviceState):
        self.row_regestry[name][5] = new_state.name
        self.table.write_row(name, self.row_regestry[name])

    def update_status(self, name, new_status: WrappedStatusPackage):
        if self.status_ts_buff[name] != new_status.timestamp:
            self.row_regestry[name][2:5] = [
                f"{str(round(new_status.temperature, 1))}°C",
                f"{new_status.battery}%",
                str(new_status.charging),
            ]
            self.table.write_row(name, self.row_regestry[name])
            self.status_ts_buff[name] = new_status.timestamp
