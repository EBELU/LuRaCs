import numpy as np
from PySide6.QtWidgets import (
    QGroupBox,
    QVBoxLayout,
    QWidget,
)

from luracs.clients import DeviceWrapper, WrappedStatusPackage
from luracs.core import RunManager
from luracs.gui.dialogs.device_settings_dialog import DeviceSettingsDialog
from luracs.gui.misc.table_menu_button import MenuButton

from .roi_info_tab import StrIdxTable


class DevicesInfoTab(QWidget):
    def __init__(self, title="", parent=None):
        super().__init__(parent)

        titles = [
            "Device",
            "Temperature",
            "Battery",
            "Charging",
            "Voltage",
            "Status",
            "Type",
            "Connection",
        ]
        widths = "fit"

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

        RunManager.Signals.newDeviceWrapped.connect(self.add_device)
        RunManager.Signals.statusUpdated.connect(self.update_status)
        RunManager.Signals.deviceStateUpdated.connect(self.update_state)

    def build_menu_button(
        self,
        device_name: str,
    ) -> MenuButton:

        menu_button = MenuButton(parent=self, title="...")
        action_disconnect = menu_button.add_action("Disconnect")
        action_disconnect.triggered.connect(
            lambda: RunManager.remove_device(device_name)
        )
        menu_button.add_separator()
        
        def get_device(): # Helper function
            return RunManager.device_registry.get(device_name)


        action_reset = menu_button.add_action("Reset Spectrum")
        action_reset.triggered.connect(
            lambda: get_device() and get_device().reset_spectrum()
        )

        action_start_acq = menu_button.add_action("Start Acquisition")
        action_start_acq.triggered.connect(
            lambda: get_device() and get_device().start_acquisition()
        )

        action_stop_acq = menu_button.add_action("Stop Acquisition")
        action_stop_acq.triggered.connect(
            lambda: get_device() and get_device().stop_acquisition()
        )

        menu_button.add_separator()
        action_settings = menu_button.add_action("Settings")
        action_settings.triggered.connect(
            lambda: get_device() and DeviceSettingsDialog(device_wrapper=get_device()).exec()
        )
        
        return menu_button

    def add_device(self, name: str, wrapper: DeviceWrapper):
        self.row_regestry[name] = [
            name,
            "None",
            "None",
            "None",
            "None",
            str(wrapper.state.name),
            str(wrapper.type),
            wrapper.connection if isinstance(wrapper.connection, str) else str(wrapper.connection.name),
        ]
        self.table.write_row(
            name, self.row_regestry[name], menu_button=self.build_menu_button(name)
        )
        self.status_ts_buff[name] = 0

    def update_state(self, name: str, new_state: DeviceWrapper.DeviceState):
        self.row_regestry[name][5] = new_state.name
        row_cpy = self.row_regestry[name].copy()
        self.table.write_row(name, row_cpy[0:])

    def update_status(self, name: str, new_status: WrappedStatusPackage):
        if self.status_ts_buff[name] != new_status.timestamp:
            row_cpy = self.row_regestry[name].copy()
            self.row_regestry[name][1:5] = [
                f"{round(new_status.temperature, 1)!s}°C" if not np.isnan(new_status.temperature) else 'nan',
                f"{new_status.battery}%" if not np.isnan(new_status.battery) else 'nan',
                str(new_status.charging),
                f"{str(new_status.voltage)+'V' if not np.isnan(new_status.voltage) else 'nan'}"
            ]
            row_cpy = self.row_regestry[name].copy()
            self.table.write_row(name, row_cpy[0:])
            self.status_ts_buff[name] = new_status.timestamp
