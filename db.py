import sys
from pathlib import Path
from PySide6.QtWidgets import QApplication

from QtGUI.GUI_components.Spectrogram import SpectrogramLoadDialog
from QtGUI.core import Settings


def main():
    app = QApplication(sys.argv)

    dialog = SpectrogramLoadDialog(Settings.Paths.spect_logs)

    if dialog.exec():
        db_path = dialog.selected_db
        print("Selected:", db_path)
    else:
        print("Dialog cancelled")

    sys.exit(0)


if __name__ == "__main__":
    main()