from __future__ import annotations

from datetime import datetime

from PySide6.QtWidgets import (
    QButtonGroup,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QDoubleSpinBox,
    QFormLayout,
    QHBoxLayout,
    QLineEdit,
    QRadioButton,
    QVBoxLayout,
)


class StartSpectrogramDialog(QDialog):
    def __init__(self, instruments, parent=None):
        super().__init__(parent)

        self.setWindowTitle("Start Spectrogram")
        self.setMinimumWidth(450)
        self.setMinimumHeight(190)
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(6)

        # Instrument selection
        self.instrument_combo = QComboBox()
        self.instrument_combo.addItems(instruments)
        form.addRow("Instrument:", self.instrument_combo)

        # File name (default = timestamp)
        self.filename_edit = QLineEdit()
        default_name = (
            f"Spectrogram_{datetime.now().replace(microsecond=0).isoformat()}"
        )
        self.filename_edit.setText(default_name)
        form.addRow("File name:", self.filename_edit)

        # Logging interval (float)
        self.interval_spin = QDoubleSpinBox()
        self.interval_spin.setRange(0.001, 1_000_000)
        self.interval_spin.setDecimals(1)
        self.interval_spin.setValue(1.0)
        self.interval_spin.setSingleStep(0.5)
        form.addRow("Interval (s):", self.interval_spin)

        # Channel truncation (radio buttons)
        self.trunc_group = QButtonGroup(self)
        trunc_layout = QHBoxLayout()

        self.trunc_buttons = {}
        for val in [1, 2, 4, 8]:
            btn = QRadioButton(str(val))
            self.trunc_group.addButton(btn, val)
            trunc_layout.addWidget(btn)
            self.trunc_buttons[val] = btn

        # Default = 1
        self.trunc_buttons[1].setChecked(True)

        form.addRow("Channel trunc:", trunc_layout)

        main_layout.addLayout(form)

        # OK / Cancel buttons
        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        main_layout.addWidget(buttons)

        self.adjustSize()

    def get_values(self):
        return {
            "instrument": self.instrument_combo.currentText(),
            "filename": self.filename_edit.text() + ".db",
            "interval": self.interval_spin.value(),
            "channel_truncation": int(self.trunc_group.checkedId()),
        }
