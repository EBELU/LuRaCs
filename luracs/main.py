import sys

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (
    QApplication,
    QSplashScreen,
)

from luracs.gui.splash_screen import create_splash_pixmap


def main():
    app = QApplication(sys.argv)

    # Create the splash only if the GUI is used
    if "--headless" not in sys.argv:
        splash = QSplashScreen(create_splash_pixmap())
        splash.setWindowFlag(Qt.WindowType.WindowStaysOnTopHint)
        splash.show()
        splash.showMessage(
            "Initializing application...",
            Qt.AlignmentFlag.AlignBottom | Qt.AlignmentFlag.AlignHCenter,
            QColor("#ffffff"),
        )

    def initialize():
        # --- Build the application ---
        from luracs.application import build_application

        app, win, script_engine = build_application()

        # Dont show the window in headless, since it does not exist
        # Also splash does not exist
        if "--headless" not in sys.argv:
            win.show()
            splash.finish(win)

    # Wait for the qt event loop to start
    QTimer.singleShot(100, initialize)

    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
