from PySide6.QtWidgets import (
    QDialog, QVBoxLayout, QFormLayout,
    QLineEdit, QDialogButtonBox, QPushButton
)


class SaveNamingDialog(QDialog):
    def __init__(self, name="", title="Name Editor", parent=None):
        super().__init__(parent=parent)

        self.setWindowTitle(title)
        self.setMinimumWidth(200)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(8, 8, 8, 8)
        main_layout.setSpacing(6)

        form = QFormLayout()
        form.setSpacing(9)

        # --- Name field ---
        self.name_edit = QLineEdit()
        self.name_edit.setText(name)
        form.addRow("Name:", self.name_edit)

        main_layout.addLayout(form)

        # --- Buttons ---
        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel
        )

        self.ok_button = buttons.button(QDialogButtonBox.Ok)
        self.ok_button.setEnabled(bool(name))  # disable if empty initially

        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)

        # Enable OK only when text is not empty
        self.name_edit.textChanged.connect(
            lambda text: self.ok_button.setEnabled(bool(text.strip()))
        )

        main_layout.addWidget(buttons)

    def get_name(self):
        return self.name_edit.text().strip()