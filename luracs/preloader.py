from PySide6.QtWidgets import QWidget, QVBoxLayout, QLabel, QProgressBar


class StartupWidget(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Starting...")
        self.setFixedSize(400, 100)

        layout = QVBoxLayout(self)
        self.label = QLabel("Starting...")
        self.progress = QProgressBar()

        layout.addWidget(self.label)
        layout.addWidget(self.progress)

    def update_progress(self, value, text):
        self.progress.setValue(value)
        self.label.setText(text)


def preloader(splash):
    pass
