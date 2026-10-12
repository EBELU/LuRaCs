import sys
import time

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QPixmap, QColor, QPainter, QFont
from PySide6.QtWidgets import (
    QApplication,
    QMainWindow,
    QSplashScreen,
    QLabel,
    QVBoxLayout,
    QWidget,
    QProgressBar,
)


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("My Application")
        self.resize(800, 500)

        label = QLabel("Welcome to My Application!")
        label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.setCentralWidget(label)


def create_splash_pixmap():
    """Create a splash screen image programmatically."""
    pixmap = QPixmap(500, 280)
    pixmap.fill(QColor("#202938"))
    
    pixmap = QPixmap("luracs/resources/themes/icons/main_icon_green.png")

    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)

    painter.setPen(QColor("#61dafb"))
    painter.setFont(QFont("Arial", 28, QFont.Weight.Bold))
    painter.drawText(0, 80, pixmap.width(), 60,
                     Qt.AlignmentFlag.AlignCenter,
                     "LuRaCs")

    # painter.setPen(QColor("#ffffff"))
    # painter.setFont(QFont("Arial", 12))
    # painter.drawText(0, 145, 500, 35,
    #                  Qt.AlignmentFlag.AlignCenter,
    #                  "Lund Radiation analysis Computer software")

    painter.end()
    return pixmap


def main():
    app = QApplication(sys.argv)

    # Create and display the splash screen.
    splash = QSplashScreen(create_splash_pixmap())
    splash.setWindowFlag(Qt.WindowType.WindowStaysOnTopHint)
    splash.show()
    splash.showMessage(
        "Initializing application...",
        Qt.AlignmentFlag.AlignBottom | Qt.AlignmentFlag.AlignHCenter,
        QColor("#ffffff"),
    )

    app.processEvents()

    # Initialize the main window.
    window = MainWindow()

    # Keep the splash screen visible briefly for demonstration.
    QTimer.singleShot(1500, lambda: (
        splash.finish(window),
        window.show()
    ))

    sys.exit(app.exec())


if __name__ == "__main__":
    main()