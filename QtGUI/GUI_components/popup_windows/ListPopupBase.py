from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QListWidget, QListWidgetItem, QPushButton, QHBoxLayout
)
from PySide6.QtCore import Qt, Signal

from ...Globals import RunManager

class ListPopupBlocking(QDialog):
    """
    Generic modal popup with a list, Confirm/Cancel buttons, 
    and double-click to confirm selection.
    """
    def __init__(self, title: str, items: list[str], parent=None):
        super().__init__(parent)
        self.setWindowTitle(title)
        self.setModal(True)
        self.resize(350, 450)

        self.selected_item = None

        # ------------------ Main layout ------------------
        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(10)

        # ------------------ List widget ------------------
        self.list_widget = QListWidget()
        self.list_widget.setSelectionMode(QListWidget.SingleSelection)
        self.list_widget.itemDoubleClicked.connect(self._on_double_click)

        # Add items with no editing and centered text
        for text in items:
            item = QListWidgetItem(text)
            item.setTextAlignment(Qt.AlignCenter)
            # Ensure items cannot be edited
            item.setFlags(Qt.ItemIsSelectable | Qt.ItemIsEnabled)
            self.list_widget.addItem(item)

        # Ensure no initial selection
        self.list_widget.clearSelection()
        # Prevent focus rectangle around items
        self.list_widget.setFocusPolicy(Qt.NoFocus)

        # Apply modern stylesheet
        self.list_widget.setStyleSheet("""
            QListWidget {
                border: 1px solid palette(mid);
                border-radius: 8px;
                padding: 4px;
                background: palette(base);
            }

            QListWidget::item {
                padding: 8px;
                margin: 2px;
                border-radius: 6px;
            }

            QListWidget::item:selected {
                background: palette(highlight);
                color: palette(highlighted-text);
            }

            QListWidget::item:hover {
                background: palette(alternate-base);
            }
        """)
        layout.addWidget(self.list_widget)

        # ------------------ Buttons ------------------
        button_layout = QHBoxLayout()
        button_layout.setSpacing(10)
        button_layout.addStretch()

        self.confirm_btn = QPushButton("Confirm ✅")
        self.confirm_btn.clicked.connect(self._on_confirm)

        self.cancel_btn = QPushButton("Cancel ❌")
        self.cancel_btn.clicked.connect(self.reject)

        button_layout.addWidget(self.confirm_btn)
        button_layout.addWidget(self.cancel_btn)
        layout.addLayout(button_layout)

    # ------------------ Internal methods ------------------
    def _on_confirm(self):
        """Called when Confirm button is pressed"""
        item = self.list_widget.currentItem()
        if item:
            self.selected_item = item.text()
            self.accept()

    def _on_double_click(self, item):
        """Called when an item is double-clicked"""
        self.selected_item = item.text()
        self.accept()

    # ------------------ Static helper ------------------
    @staticmethod
    def get_item(title: str, items: list[str], parent=None) -> str | None:
        """
        Show the popup and return the selected item, or None if cancelled.
        """
        dialog = ListPopupBlocking(title, items, parent)
        result = dialog.exec()
        if result == QDialog.Accepted:
            return dialog.selected_item
        return None

class ListPopupNonBlocking(QDialog):
    """
    Non-blocking list popup.
    - No exec()
    - Uses signals
    - Items can be updated dynamically
    """

    confirmed = Signal(str)
    cancelled = Signal()

    def __init__(self, title: str, parent=None):
        super().__init__(parent)

        self.setWindowTitle(title)
        self.setModal(False)
        self.resize(350, 450)

        # ------------------ Layout ------------------
        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(10)

        # ------------------ List ------------------
        self.list_widget = QListWidget()
        self.list_widget.setSelectionMode(QListWidget.SingleSelection)
        self.list_widget.setFocusPolicy(Qt.NoFocus)
        self.list_widget.itemDoubleClicked.connect(self._on_double_click)

        self.list_widget.setStyleSheet("""
            QListWidget {
                border: 1px solid palette(mid);
                border-radius: 8px;
                padding: 4px;
                background: palette(base);
            }

            QListWidget::item {
                padding: 8px;
                margin: 2px;
                border-radius: 6px;
            }

            QListWidget::item:selected {
                background: palette(highlight);
                color: palette(highlighted-text);
            }

            QListWidget::item:hover {
                background: palette(alternate-base);
            }
        """)

        layout.addWidget(self.list_widget)

        # ------------------ Buttons ------------------
        btn_layout = QHBoxLayout()
        btn_layout.addStretch()

        self.confirm_btn = QPushButton("Confirm")
        self.cancel_btn = QPushButton("Cancel")

        self.confirm_btn.clicked.connect(self._confirm)
        self.cancel_btn.clicked.connect(self._cancel)

        btn_layout.addWidget(self.confirm_btn)
        btn_layout.addWidget(self.cancel_btn)

        layout.addLayout(btn_layout)

    # ------------------------------------------------
    # Public API
    # ------------------------------------------------

    def set_items(self, items: list[str]):
        """Replace list contents safely at runtime"""
        self.list_widget.clear()

        for text in items:
            item = QListWidgetItem(text)
            item.setTextAlignment(Qt.AlignCenter)
            item.setFlags(Qt.ItemIsSelectable | Qt.ItemIsEnabled)
            self.list_widget.addItem(item)

        self.list_widget.clearSelection()
        
    def receive_BT_list(self, devices: list):
        """
        Called when a Bluetooth scan completes.
        Updates the list widget with new devices.
        Prioritizes devices containing 'radiacode' or 'raysid'.
        """
        if not self.isVisible():
            # Ignore results if the popup is closed
            return

        # Stop any scan countdown timer
        if hasattr(self, "_timer"):
            self._timer.stop()
            self._scan_start_time = None

        # Clear previous devices
        self.list_widget.clear()
        if hasattr(self, "_devices_by_name"):
            self._devices_by_name.clear()

        if not devices:
            self.list_widget.addItem("No devices found")
            return

        priority = []
        normal = []

        for dev in devices:
            name = dev.name or getattr(dev, "address", "Unknown")
            if hasattr(self, "_devices_by_name"):
                self._devices_by_name[name] = dev

            display_name = name
            if "radiacode" in name.lower() or "raysid" in name.lower():
                display_name = f"{name} ☢️"
                priority.append((display_name, dev))
            else:
                normal.append((display_name, dev))

        # Show priority devices first
        sorted_devices = priority + normal

        for display_name, dev in sorted_devices:
            item = QListWidgetItem(display_name)
            item.setTextAlignment(Qt.AlignCenter)
            item.setFlags(Qt.ItemIsSelectable | Qt.ItemIsEnabled)
            if hasattr(self, "_devices_by_name"):
                item.setData(Qt.UserRole, dev)
            self.list_widget.addItem(item)

        self.list_widget.clearSelection()


    # ------------------------------------------------
    # Internal handlers
    # ------------------------------------------------


    def _confirm(self):
        item = self.list_widget.currentItem()
        if item:
            self.confirmed.emit(item.text())
            self.close()

    def _on_double_click(self, item):
        self.confirmed.emit(item.text())
        self.close()

    def _cancel(self):
        self.cancelled.emit()
        self.close()



