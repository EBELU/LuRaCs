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

from luracs.utils.numerics.cython.gaussian import (
    multi_gaussian,
    multi_gaussian_jacobian,
)

def test_gaussian():
    def gaussian_numpy(x, params):
        """
        Reference implementation in pure NumPy.
        params = [A1, mu1, var1, A2, mu2, var2, ...]
        """
        y = np.zeros_like(x)

        n_gauss = len(params) // 3

        for i in range(n_gauss):
            A = params[3*i]
            mu = params[3*i + 1]
            variance = params[3*i + 2]

            y += A * np.exp(
                -0.5 * (x - mu)**2 / variance
            )

        return y


    def jacobian_finite_difference(x, params, eps=1e-6):
        """
        Numerical Jacobian using central finite differences.
        """
        n = len(params)
        m = len(x)

        J = np.zeros((m, n))

        for i in range(n):
            p1 = params.copy()
            p2 = params.copy()

            p1[i] += eps
            p2[i] -= eps

            J[:, i] = (
                gaussian_numpy(x, p1)
                - gaussian_numpy(x, p2)
            ) / (2 * eps)

        return J


    def test_multi_gaussian():
        x = np.linspace(-5, 5, 100)

        # Two Gaussian components:
        # A, mu, variance
        params = np.array([
            2.0, -1.0, 0.5,
            1.5,  2.0, 1.2,
        ], dtype=np.float64)

        y_cython = multi_gaussian(x, params)
        y_numpy = gaussian_numpy(x, params)

        np.testing.assert_allclose(
            y_cython,
            y_numpy,
            rtol=1e-12,
            atol=1e-12,
        )

        print("multi_gaussian passed")


    def test_gaussian_jacobian():
        x = np.linspace(-5, 5, 50)

        params = np.array([
            2.0, -1.0, 0.5,
            1.5,  2.0, 1.2,
        ], dtype=np.float64)

        J_cython = multi_gaussian_jacobian(x, params)

        J_numeric = jacobian_finite_difference(
            x,
            params,
            eps=1e-6,
        )

        np.testing.assert_allclose(
            J_cython,
            J_numeric,
            rtol=1e-5,
            atol=1e-7,
        )

        print("multi_gaussian_jacobian passed")


    if __name__ == "__main__":
        test_multi_gaussian()
        test_gaussian_jacobian()

        print("All tests passed!")

if __name__ == "__main__":
    # import sys
    # from PySide6.QtWidgets import QApplication
    # app = QApplication.instance() or QApplication(sys.argv)
    test_gaussian()

    # print(IOManager.FileIndex.spectrum_index.get_item_from_attr("name", "Cyklotron_Co"))
    # print(SpectrumManager.UniqueInstrumentLibrary.instrument_registry)

    # sys.exit(app.exec())
