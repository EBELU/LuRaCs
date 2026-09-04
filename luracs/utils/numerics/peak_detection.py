import numpy as np
import math
from typing import Callable

from .optimizer import curve_fit
from .cython import multi_gaussian, multi_gaussian_jacobian


# --- Savitzky-Golay filtering ---
def savgol_coeffs(window_length, polyorder, deriv=0):
    """
    Compute Savitzky-Golay convolution coefficients.

    https://scipy-cookbook.readthedocs.io/items/SavitzkyGolay.html


    Savitzky-Golay Smoothing and Differentiation Filter, Neal Gallagher, 2020
    DOI:10.13140/RG.2.2.20339.50725
    """

    if window_length % 2 == 0:
        raise ValueError("window_length must be odd")

    if polyorder >= window_length:
        raise ValueError("polyorder must be < window_length")

    if deriv > polyorder:
        raise ValueError("deriv must be <= polyorder")

    half = window_length // 2

    # Generate window indicies
    x = np.arange(-half, half + 1, dtype=float)

    # Vandermonde matrix
    # https://en.wikipedia.org/wiki/Vandermonde_matrix
    # -------------------------------------------------

    # Given n distinct points:
    # (x0,y0), (x1,y1), ..., (x{n-1}, y{n-1})

    # find the polynomial of degree <= n-1:
    # p(x) = a0 + a1 * x + a2 * x^2 + ... + a{n-1} * x^{n-1}

    # such that:
    # p(x_i) = y_i

    # This produces the linear system:
    # V * a = y

    # where:
    #       / 1   x0   x0^2   ...   x0^{n-1} \
    #       | 1   x1   x1^2   ...   x1^{n-1} |
    # V  =  | .    .      .            .      |
    #       | .    .      .            .      |
    #       \ 1  x{n-1} ...      x{n-1}^{n-1}/

    #       / a0 \
    #       | a1 |
    # a  =  | .  |
    #       | .  |
    #       \a{n-1}/

    #       / y0 \
    #       | y1 |
    # y  =  | .  |
    #       | .  |
    #       \y{n-1}/

    V = np.vander(x, polyorder + 1, increasing=True)

    # det(V) != 0 if and only if x_i are distinct
    # Given det(V) != 0 this matrix invertable
    # Thus, solving inverting V solves for a

    coeffs = np.linalg.pinv(V)[deriv]

    # For a polynomial
    # p(x) = a0 + a1 x + a2 x^2 + ...

    # the m-th derivative at x=0 satisfies
    # p^(m)(0) = m! * a_m
    # The selected row of pinv(V) recovers a_m,
    # so multiply by m! to obtain derivative weights.

    coeffs = coeffs * math.factorial(deriv)

    return coeffs


def savgol_filter(y, window_length, polyorder, deriv=0):
    """
    Apply Savitzky-Golay filter using reflective padding.
    """

    coeffs = savgol_coeffs(window_length, polyorder, deriv)

    half = window_length // 2

    # Pad the array with reflection to avoid edge anomalies
    ypad = np.pad(y, half, mode="reflect")

    return np.convolve(ypad, coeffs[::-1], mode="valid"), coeffs


# --- Helpers ---
def MAD_threshold(x, sigma=5):
    """
    Robust MAD-based threshold estimate.
    https://en.wikipedia.org/wiki/Median_absolute_deviation
    """

    noise = np.median(np.abs(x - np.median(x))) / 0.6745
    return sigma * noise


def local_mad_threshold(
    x: np.ndarray, window_length: int = 101, sigma: float = 5.0
) -> np.ndarray:
    """
    Rolling MAD-based adaptive threshold.
    Returns per-channel threshold.
    """

    if window_length % 2 == 0:
        raise ValueError("window_length must be odd")

    radius = window_length // 2

    threshold = np.zeros_like(x, dtype=float)

    for i in range(len(x)):
        low = max(0, i - radius)
        high = min(len(x), i + radius + 1)

        local = x[low:high]

        med = np.median(local)

        mad = np.median(np.abs(local - med))

        noise = mad / 0.6745

        threshold[i] = sigma * noise

    return threshold


def local_minima(x, threshold=None):
    """
    Detect local minima.
    """

    mask = (x[1:-1] < x[:-2]) & (x[1:-1] < x[2:])

    if threshold is not None:
        mask &= -x[1:-1] > threshold

    return np.where(mask)[0] + 1


def local_maxima(x, threshold=None):
    """
    Detect local maxima.
    """

    mask = (x[1:-1] > x[:-2]) & (x[1:-1] > x[2:])

    if threshold is not None:
        mask &= x[1:-1] > threshold

    return np.where(mask)[0] + 1


def enforce_min_separation(peaks, min_sep):
    """
    Remove peaks closer than min_sep channels.
    """

    filtered = []

    for p in peaks:
        if not filtered or p - filtered[-1] > min_sep:
            filtered.append(p)

    return np.array(filtered)


def median_filter(x: np.ndarray, window_length: int) -> np.ndarray:

    if window_length % 2 == 0:
        raise ValueError("window_length must be odd")

    radius = (window_length - 1) // 2

    filtered = np.zeros_like(x)

    for i in range(len(x)):
        low = max(0, i - radius)
        high = min(len(x), i + radius + 1)

        filtered[i] = np.median(x[low:high])

    return filtered


def post_process_peaks(
    peaks_idx: np.ndarray,
    left_edges_idx: np.ndarray,
    right_edges_idx: np.ndarray,
    array_length: int,
):
    """
    Pair each peak with surrounding edges.
    return (left_edge_point, p, right_edge_point)
    """

    regions = []

    for p in peaks_idx:
        left = left_edges_idx[left_edges_idx < p]
        right = right_edges_idx[right_edges_idx > p]

        if len(left) == 0 or len(right) == 0:
            continue

        span = right[0] - left[-1]
        left_edge_point = max(left[-1] - span, 0)
        right_edge_point = (
            right[0] + span if right[0] + span < array_length else array_length - 1
        )

        regions.append((left_edge_point, p, right_edge_point))

    return regions


# --- Peak finder ---


def find_peaks(
    spectrum_y: np.ndarray, 
    spectrum_x: np.ndarray,
    window_length: int = 31
) -> tuple[list[tuple[float, float, float]], np.ndarray, np.ndarray, str]:
    """
    return (left_edge_point, p, right_edge_point)
    """
    if window_length % 2 == 0:
        raise ValueError("Convolution window should have and off number if elements")

    channels = np.arange(len(spectrum_y))

    # noise_gain = np.sqrt(np.sum(coeffs**2))

    # SG derivatives
    d1, d1_coeffs = savgol_filter(
        spectrum_y,
        window_length=window_length,  # Must change for high resolution spectra
        polyorder=3,
        deriv=1,
    )

    d2, d2_coeffs = savgol_filter(
        spectrum_y,
        window_length=window_length,  # --||--
        polyorder=3,
        deriv=2,
    )

    # Detection thresholds, set using median absolute deviation
    threshold_d1 = MAD_threshold(d1, 1)
    threshold_d2 = MAD_threshold(d2, 1)

    # Peak centers are derived from 2nd derivative minima
    peaks = local_minima(d2, threshold_d2)

    # Peak edges are derived from 1st derivative extrema
    left_edges = local_maxima(d1, threshold_d1)
    right_edges = local_minima(d1, threshold_d1)

    # Remove duplicate nearby peaks
    peaks = enforce_min_separation(peaks, min_sep=5)

    # Pair edges to peaks
    peak_regions = post_process_peaks(
        peaks,
        left_edges,
        right_edges,
        len(channels),
    )
    peaks, log_info = peak_discriminator(spectrum_y, spectrum_x, peak_regions, True, 15)
    return peaks, d1, d2, log_info

def peak_discriminator(
    spectrum_y: np.ndarray,
    spectrum_x: np.ndarray,
    peak_candidates: list[tuple[float, float, float]],
    require_fit: bool,
    min_separation: int,
    resolution_k: float | None = None,
    resolution_limit: float = 1.5,
    status: Callable | None = None
    
) -> list[tuple[float, float, float]]:
    """
    Filter peaks based on their height relative to the spectrum.
    """

    accepted_fits = []
    mus = np.zeros(len(peak_candidates))
    rejected_sanity_check = rejected_fit_failed = rejected_bad_fit = rejected_duplicate = 0
    for i, (left, centre, right) in enumerate(peak_candidates):
        
        peak_mask = np.zeros_like(spectrum_x, dtype=bool)
        peak_mask[int(left):int(right)] = True

        if peak_mask.shape[0] < 4 or left > right or left == 0 or spectrum_x[right] < 25:
            rejected_sanity_check += 1
            continue
        
        x_peak = spectrum_x[peak_mask]
        y_peak = spectrum_y[peak_mask]
            
        A0 = np.max(y_peak)
        mu0 = x_peak[np.argmax(y_peak)]
        s0 = (right - left) / 6.0
        
        fit, cov, converged = curve_fit(
            multi_gaussian,
            x_peak,
            y_peak,
            [A0, mu0, s0],
            jac=multi_gaussian_jacobian,
        )
        if not converged:
            rejected_fit_failed += 1
            continue
        
        A, mu, s = fit
        
        if math.sqrt(np.diag(cov)[2]) / s > 0.2 or A < 0:
            rejected_bad_fit += 1
            continue

        if np.any(np.isclose(mus, mu, atol=1)):
            rejected_duplicate += 1
            continue
        
        mus[i] = mu
        
        if converged:
            accepted_fits.append(peak_candidates[i])
    

        log_info = (
            "\n".join([
            "\n--- Peak Search ---",
            f"Peak Candidates: {len(peak_candidates)}",
            f"Accepted: {len(accepted_fits)}",
            "Rejections by createria:",
            f"  Sanity Check: {rejected_sanity_check}",
            f"  Fit Failed: {rejected_fit_failed}",
            f"  Bad Fit: {rejected_bad_fit}",
            f"  Duplicate: {rejected_duplicate}",
            ]
            ))
    return accepted_fits, log_info



if __name__ == "__main__":
    import pandas as pd
    import matplotlib.pyplot as plt
    from luracs.utils.numerics.cython.mariscotti import mariscotti_peak_search_cy
    
    data = pd.read_csv("~/Desktop/Th.csv").to_numpy().T
    
    regions, d1, d2 = find_peaks(data[1])
    peak_discriminator(data[0], data[1], regions, True, 15)
    
    plt.plot(data[1] / np.max(d1))
    #plt.plot(d1)
    plt.plot(np.abs(d1))
    plt.plot(local_mad_threshold(np.abs(d1), 501, sigma=2))
    plt.show()