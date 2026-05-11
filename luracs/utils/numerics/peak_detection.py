import numpy as np
import math

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

    return np.convolve(ypad, coeffs[::-1], mode="valid")

# --- Helpers ---
def MAD_threshold(x, sigma=5):
    """
    Robust MAD-based threshold estimate.
    https://en.wikipedia.org/wiki/Median_absolute_deviation
    """

    noise = np.median(np.abs(x)) / 0.6745
    return sigma * noise


def local_minima(x, threshold=None):
    """
    Detect local minima.
    """

    mask = (
        (x[1:-1] < x[:-2]) &
        (x[1:-1] < x[2:])
    )

    if threshold is not None:
        mask &= (-x[1:-1] > threshold)

    return np.where(mask)[0] + 1


def local_maxima(x, threshold=None):
    """
    Detect local maxima.
    """

    mask = (
        (x[1:-1] > x[:-2]) &
        (x[1:-1] > x[2:])
    )

    if threshold is not None:
        mask &= (x[1:-1] > threshold)

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


def pair_peak_edges(peaks, left_edges, right_edges, array_length):
    """
    Pair each peak with surrounding edges.
    return (left_edge_point, p, right_edge_point)
    """

    regions = []

    for p in peaks:

        left = left_edges[left_edges < p]
        right = right_edges[right_edges > p]

        if len(left) == 0 or len(right) == 0:
            continue
        
        span = (right[0] - left[-1])
        left_edge_point = left[-1] - span if left[-1] - span >= 0 else 0
        right_edge_point = right[0] + span if right[0] + span < array_length else array_length - 1
        
        regions.append((left_edge_point, p, right_edge_point))

    return regions


# --- Peak finder ---

def find_peaks(spectrum: np.ndarray, window_length: int = 31) -> list[tuple[float, float, float]]:
    """    
    return (left_edge_point, p, right_edge_point)
    """
    if window_length % 2 == 0:
        raise ValueError("Convolution window should have and off number if elements")
    
    channels = np.arange(len(spectrum))



    # SG derivatives
    d1 = savgol_filter(
        spectrum,
        window_length=window_length, # Must change for high resolution spectra
        polyorder=3,
        deriv=1
    )

    d2 = savgol_filter(
        spectrum,
        window_length=window_length, # --||--
        polyorder=3,
        deriv=2
    )


    # Detection thresholds, set using median absolute deviation
    threshold_d1 = MAD_threshold(d1)
    threshold_d2 = MAD_threshold(d2)


    # Peak centers are derived from 2nd derivative minima
    peaks = local_minima(d2, threshold_d2)


    # Peak edges are derived from 1st derivative extrema
    left_edges =  local_maxima(d1, threshold_d1)
    right_edges = local_minima(d1, threshold_d1)


    # Remove duplicate nearby peaks
    peaks = enforce_min_separation(peaks, min_sep=15)


    # Pair edges to peaks
    peak_regions = pair_peak_edges(
        peaks,
        left_edges,
        right_edges,
        len(channels)
    )
    
    return peak_regions