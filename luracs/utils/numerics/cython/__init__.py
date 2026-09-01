import numpy as np

if __name__ != "__main__":
    from .gaussian import multi_gaussian_cy, multi_gaussian_jacobian_cy
    from .deconvolution import (
        ML_EM_cy,
        richardson_lucy_cy,
    )
    
    from .process_matrix import process_matrix_cy

    from .rebin import rebin_histogram_cy
    
else:
    from gaussian import multi_gaussian_cy, multi_gaussian_jacobian_cy
    from deconvolution import (
        ML_EM_cy,
        richardson_lucy_cy,
    )
    from process_matrix import process_matrix_cy

    from rebin import rebin_histogram_cy


def multi_gaussian(x, params):
    """
    Sum of N Gaussians.

    Parameters
    ----------
    x : array_like
        Independent variable
    params : array_like
        Parameters: [A1, mu1, variance1, A2, mu2, variance2, ...]

    Returns
    -------
    y : ndarray
    """
    return multi_gaussian_cy(x, params)


def multi_gaussian_jacobian(x, params):
    """
    Analytical Jacobian for multi-Gaussian function.

    Returns a matrix J of shape (len(x), len(params))
    """
    return multi_gaussian_jacobian_cy(x, params)

def ml_em(
    y: np.ndarray,
    offsets: np.ndarray,
    indices: np.ndarray,
    values: np.ndarray,
    sensitivity: np.ndarray | None = None,
    *,
    iterations: int = 30,
    reg_tikhonov: bool = False,
    reg_tikhonov_lambda: float = 1e-3,
    use_sensitivity: bool = False,
):
    """
    Perform ML-EM reconstruction.

    Parameters
    ----------
    y : array_like
        Measurement vector.
    offsets : array_like
        CSR row offsets.
    indices : array_like
        CSR column indices.
    values : array_like
        CSR nonzero values.
    sensitivity : array_like, optional
        Sensitivity image. Required if ``use_sensitivity=True``.
    iterations : int, default=30
        Number of ML-EM iterations.
    reg_tikhonov : bool, default=False
        Enable Tikhonov regularization.
    reg_tikhonov_lambda : float, default=1e-3
        Tikhonov regularization weight.
    use_sensitivity : bool, default=False
        Use the supplied sensitivity image.

    Returns
    -------
    numpy.ndarray
        Reconstructed image.
    """

    y = np.asarray(y, dtype=np.float64)
    offsets = np.asarray(offsets, dtype=np.int64)
    indices = np.asarray(indices, dtype=np.int32)
    values = np.asarray(values, dtype=np.float64)

    if sensitivity is None:
        sensitivity = np.ones_like(y, dtype=np.float64)
    else:
        sensitivity = np.asarray(sensitivity, dtype=np.float64)

    return ML_EM_cy(
        y,
        offsets,
        indices,
        values,
        sensitivity,
        iterations=iterations,
        reg_tikhonov=reg_tikhonov,
        reg_tikhonov_lambda=reg_tikhonov_lambda,
        use_sensitivity=use_sensitivity,
    )
    
def process_response(
    matrix: np.ndarray,
    ref_indices: np.ndarray,
    ref_energy_axis: np.ndarray,
    requested_indices: np.ndarray,
    measured_energy_axis: np.ndarray,
):
    """
    Process response matrix by interpolating reference responses onto the
    measured energy axis and constructing a CSR representation for the
    requested indices.

    Parameters
    ----------
    matrix : array_like, shape (n_reference, n_energy)
        Reference response matrix.
    ref_indices : array_like
        Indices corresponding to the rows of ``matrix``.
    ref_energy_axis : array_like
        Energy axis corresponding to the columns of ``matrix``.
    requested_indices : array_like
        Reference indices to generate responses for.
    measured_energy_axis : array_like
        Energy axis onto which the reference responses are interpolated.

    Returns
    -------
    offsets : numpy.ndarray
        CSR row offsets, dtype ``int64``.
    indices : numpy.ndarray
        CSR column indices, dtype ``int32``.
    values : numpy.ndarray
        CSR nonzero values, dtype ``float64``.
    """

    matrix = np.asarray(matrix, dtype=np.float64)
    ref_indices = np.asarray(ref_indices, dtype=np.float64)
    ref_energy_axis = np.asarray(ref_energy_axis, dtype=np.float64)
    requested_indices = np.asarray(requested_indices, dtype=np.float64)
    measured_energy_axis = np.asarray(measured_energy_axis, dtype=np.float64)

    return process_matrix_cy(
        matrix,
        ref_indices,
        ref_energy_axis,
        requested_indices,
        measured_energy_axis,
    )

    
def richardson_lucy(
    x: np.ndarray,
    y: np.ndarray,
    k: float,
    *,
    sensitivity: np.ndarray| None = None,
    iterations: int = 30,
    reg_tikhonov: bool = False,
    reg_tikhonov_lambda: float = 1e-3,
    use_sensitivity: bool = False,
):
    """
    Richardson-Lucy deconvolution.

    Parameters
    ----------
    x : array_like
        Independent variable (e.g., energy axis).
    y : array_like
        Input signal.
    k : float
        Gaussian kernel parameter.
    sensitivity : array_like, optional
        Sensitivity correction vector. If omitted, a vector of ones is used.
    iterations : int, default=30
        Number of Richardson-Lucy iterations.
    reg_tikhonov : bool, default=False
        Enable Tikhonov regularization.
    reg_tikhonov_lambda : float, default=1e-3
        Tikhonov regularization strength.
    use_sensitivity : bool, default=False
        Whether to apply the sensitivity correction.

    Returns
    -------
    numpy.ndarray
        Deconvolved signal.
    """

    y = np.asarray(y, dtype=np.float64)

    if sensitivity is None:
        sensitivity = np.ones_like(y)
    else:
        sensitivity = np.asarray(sensitivity, dtype=np.float64)

    return richardson_lucy_cy(
        x,
        y,
        float(k),
        sensitivity,
        iterations=iterations,
        reg_tikhonov=reg_tikhonov,
        reg_tikhonov_lambda=reg_tikhonov_lambda,
        use_sensitivity=use_sensitivity,
    )
    
def rebin_histogram(
    energy_axis: np.ndarray,
    count_axis: np.ndarray,
    new_energy_axis: np.ndarray
) -> np.ndarray[np.float64]:
    """
    Rebin a histogram onto a new energy axis while conserving total counts.
    
    Counts are redistributed according to the fractional overlap between the
    original and new histogram bins. This treats the input as histogram data
    (bin integrals) rather than point samples, preserving the total number of
    counts up to floating-point precision.
    
    Parameters
    ----------
    energy_axis : ndarray of float
        Original histogram bin edges. Must have length one greater than
        ``count_axis``. Each bin is defined by consecutive values in this
        array.
    
    count_axis : ndarray of float
        Counts in each original histogram bin. Must have length one less than
        ``energy_axis``.
    
    new_energy_axis : ndarray of float
        Target histogram bin edges. The output histogram will contain one
        count value per bin defined by this axis.
    
    Returns
    -------
    new_counts : ndarray
        Rebinned histogram counts on ``new_energy_axis``. The sum of the
        returned counts matches the sum of ``count_axis`` within numerical
        precision.
    
    Notes
    -----
    This method performs conservative redistribution rather than interpolation.
    It is suitable for spectra and other measurements where each bin value
    represents an integrated number of events over a finite energy interval.
    """
    energy_axis = np.asarray(energy_axis, dtype=np.float64)
    count_axis = np.asarray(count_axis, dtype=np.float64)
    new_energy_axis = np.asarray(new_energy_axis, dtype=np.float64)

    return rebin_histogram_cy(
        energy_axis,
        count_axis,
        new_energy_axis,
    )

if __name__ == "__main__":
    import pandas as pd
    import matplotlib.pyplot as plt
    # data = pd.read_csv("~/Desktop/Cyklotron_Ba.csv").to_numpy()
    # clipped_data = data[(data[:, 0] >= 30)]
    # # print(richardson_lucy(clipped_data[:, 1], 2/100))
    # plt.plot(clipped_data[:, 0], clipped_data[:, 1], label="Original Data")
    # plt.plot(clipped_data[:, 0], richardson_lucy(clipped_data[:, 0], clipped_data[:, 1], 2, iterations=25, reg_tikhonov=False, reg_tikhonov_lambda=0.001), label="Deconvolved Data")
    # plt.xlabel("Energy (keV)")
    # plt.ylabel("Counts")
    # plt.show()
    
    # data = pd.read_csv("~/Desktop/Cyklotron_Ba.csv").to_numpy()
    # response_matrix = np.load("dev/Rc103_response.npz", allow_pickle=False)
    

    
    # processed_response = process_response_cy(
    #     response_matrix["response_matrix"],
    #     response_matrix["indices"].astype(np.float64),
    #     response_matrix["bin_centres"],
    #     data[:, 0],
    #     data[:, 0],
    # )
    
    
    # ml_em_result = ml_em(
    #     data[:, 1],
    #     processed_response[0],
    #     processed_response[1].astype(np.int32),
    #     processed_response[2],
    #     sensitivity=None,
    #     iterations=1500,
    #     use_sensitivity=False,
    # )
    
    # plt.plot(data[:, 0], data[:, 1])
    # plt.plot(data[:, 0], ml_em_result)
    # plt.show()
    
    # from luracs.utils.file_io.xml_writer import xml_writer
    # from luracs.containers.spectrum_classes import Spectrum, SpectrumData
    
    # spectrum = Spectrum(len(ml_em_result), "deconvolved")
    # sd = SpectrumData(
    #     ml_em_result,
    #     len(ml_em_result),
    #     np.sum(ml_em_result),
    #     1
    # )
    
    # sdb = SpectrumData(
    #     data[:, 1],
    #     len(data[:, 1]),
    #     np.sum(data[:, 1]),
    #     1
    # )

    # cal = np.polyfit(np.arange(len(data)), data[:, 0], 2)
    
    # spectrum.apply_calibration(cal)
    
    # spectrum.set_foreground(sd)
    # spectrum.set_background(sdb)
    # xml_writer(spectrum, "dev/debug/deconv")
    
    
    import pandas as pd
    from time import time
    
    path = "/home/eewa/Documents/git/MySpect/dev/Rc103_response.npz"
    response_data = dict(np.load(path))
    
    data = pd.read_csv("/home/eewa/Desktop/Cyklotron_Ba.csv").to_numpy().T    
    data_region = data[0] > 30
    
    start = time()
    matrix = process_matrix_cy(response_data["response_matrix"],
                   response_data["indices"].astype(np.float64),
                   response_data["bin_centres"], 
                   np.linspace(50, 3000, 2**14), 
                   data[0][data_region])
    end = time()
    print(f"Processing took {round(end - start, 2)}s")
    
    print(matrix)