from PySide6.QtWidgets import QMessageBox

from luracs.containers.roi_classes import ROI
from luracs.containers.spectrum_classes import Spectrum
from luracs.core import IOManager, Log, Settings, SpectrumManager
from luracs.gui.dialogs.save_dialog import SaveNamingDialog
from luracs.utils.file_io.xml_writer import xml_writer


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


def save_roi_references():
    "Export reference rois for the library to be loaded on any spectrum"
    save_diag = SaveNamingDialog()
    # Check if there are any rois
    if len(SpectrumManager.ROIManager.roi_registry) == 0:
        QMessageBox.warning(save_diag, "Warning Message", "No ROIs set")
        return

    res = save_diag.exec()

    if res == SaveNamingDialog.Accepted:
        new_file = Settings.Paths.roi_library / str(save_diag.get_name())

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
    else:
        return

    # I cant be bothered to change the functional writer, just hack it
    # Create a dummy spectrum, only give it rois and the export it using the normal xml_writer
    dummy_spectrum = Spectrum(1, "Dummy")

    for r in SpectrumManager.ROIManager.roi_registry.values():
        # Same as for the spectrum, make a dummy ROI
        dummy_roi = ROI(
            tag=r.tag,
            alias=r.alias,
            spectrum="None",
            roi_bound=r.getRegion(),
            region_bound=(None, None),
            fit_type=r.fit_type,
            bkg_type=r.bkg_type,
            fit=None,
            roi_counts=0,
            live_time=1,
            emission=r.emission,
            meta={
                "movable": r.movable,
                "poisson_weights": r.poisson_weights,
                "merge": r.merge,
            },
        )

        dummy_spectrum.set_roi(dummy_roi)

    xml_writer(dummy_spectrum, new_file, export_spectrum=False, export_instrument=False)
    Log.debug(f"ROI References saved to library: {new_file}")
    return new_file.with_suffix(".xml")
