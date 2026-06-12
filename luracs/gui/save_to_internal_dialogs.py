from PySide6.QtWidgets import QMessageBox

from luracs.core import SpectrumManager, Settings, IOManager
from luracs.containers.spectrum_classes import Spectrum
from luracs.gui.popup_windows.save_dialog import SaveNamingDialog


def save_spectrum_to_library_dialog(spectrum: Spectrum):
    save_diag = SaveNamingDialog(spectrum.name)
    save_diag.remark_edit.setText(spectrum.remark)
    res = save_diag.exec()

    spectrum.remark = save_diag.get_remark()

    if res == SaveNamingDialog.Accepted:
        new_file = (Settings.Paths.spectrum_library / save_diag.get_name()).with_suffix(
            ".xml"
        )

        # Check if the file already exists
        if new_file.exists():
            reply = QMessageBox.question(
                None,
                "Overwrite File?",
                f"The file '{new_file.name}' already exists. Do you want to overwrite it?",
                QMessageBox.Yes | QMessageBox.No,
                QMessageBox.No,
            )

            if reply == QMessageBox.No:
                return

            IOManager.FileIndex.spectrum_index.update_file(
                IOManager.FileIndex.spectrum_index.get_key_from_attr(
                    "name", spectrum.name
                ),
                spectrum,
            )

        else:
            if spectrum.connection is None and spectrum.name != save_diag.get_name():
                SpectrumManager.rename_spectrum(spectrum.name, save_diag.get_name())

            IOManager.FileIndex.spectrum_index.save_file(spectrum)

    # Dont rename, it breaks data signalling from device
