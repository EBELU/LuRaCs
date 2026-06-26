from luracs.utils.file_io import xml_parser, xml_writer, io_dispatcher
from luracs.containers.spectrum_classes import Spectrum
from luracs.containers.instrument_classes import GenericInstrument, UniqueInstrument
from dataclasses import dataclass
from datetime import datetime
import numpy as np
import copy
import logging

logging.basicConfig(level=logging.DEBUG)

from luracs.core import SpectrumManager, IOManager

# --- Example GenericInstrument ---
generic = GenericInstrument(
    model="XR-2000",
    manufacturer="SpectraTech Instruments",
    detector_type="Scintillation",
    detector_material="NaI(Tl)",
    detector_shape="cylindrical",
    detector_dimensions_cm=[5.0, 5.0],  # diameter, length
    # Resolution (example: sqrt(a + bE))
    resolution_fn="sqrt(a + bE)",
    resolution_params=[0.8, 0.02],
    resolution_E_points=[59.5, 122.0, 662.0, 1332.0],
    resolution_FWHM_points=[7.5, 8.2, 12.0, 18.5],
    # Efficiency (example: exponential decay fit)
    int_efficiency_fn="a * exp(-bE)",
    int_efficiency_params=[0.9, 0.0015],
    int_efficiency_created=datetime(2025, 6, 15),
    int_efficiency_eff_points=[0.4, 0.2, 0.1, 0.1],
    int_efficiency_uncert_points=[0.01, 0.01, 0.01, 0.01],
    int_efficiency_E_points=[356, 662, 1173, 1132],
    int_efficiency_description="Estimated from calibration sources (Cs-137, Co-60)",
)

# --- Example UniqueInstrument ---
unique = UniqueInstrument(
    name="Detector_A1",
    **generic.get_copy().__dict__,
    calibration_poly_order=2,
    calibration_coefficients=[0.5, 2.1, 0.0003],  # E = a + bC + cC^2
    calibration_energy_points=[59.5, 122.0, 662.0, 1332.0],
    calibration_channel_points=[120, 250, 1350, 2750],
    calibration_date=datetime(2026, 3, 10),
)


def test_xml_io():
    outpt = io_dispatcher("dev/debug/xml/Raysid-GRF-Ba133.xml", meta_parsing=True)

    new_spect = Spectrum(
        len(outpt.get_background_spectrum().y_axis), outpt.data["name"]
    )
    new_spect.set_foreground(outpt.get_background_spectrum())
    new_spect.instrument = unique

    for r in outpt.get_rois():
        new_spect.set_roi(r)

    xml_writer(new_spect, "/home/eewa/Documents/git/MySpect/debug")
    print(
        io_dispatcher(
            "/home/eewa/Documents/git/MySpect/debug.xml", meta_parsing=False
        ).__dict__
    )


if __name__ == "__main__":
    # import sys
    # from PySide6.QtWidgets import QApplication
    # app = QApplication.instance() or QApplication(sys.argv)

    print(IOManager.FileIndex.spectrum_index.get_item_from_attr("name", "Cyklotron_Co"))
    print(SpectrumManager.UniqueInstrumentLibrary.instrument_registry)

    # sys.exit(app.exec())
