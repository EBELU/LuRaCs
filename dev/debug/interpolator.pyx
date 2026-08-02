import numpy as np

def rebin_histogram(
        double[:] energy_axis,
        double[:] count_axis,
        double[:] new_energy_axis):

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
    
    cdef:
        Py_ssize_t old_n = count_axis.shape[0]
        Py_ssize_t new_n = new_energy_axis.shape[0] - 1

        double[:] new_counts = np.zeros(new_n, dtype=np.float64)

        Py_ssize_t i, j, k
        double old_left, old_right
        double new_left, new_right
        double overlap, old_width

    j = 0

    for i in range(old_n):
        old_left = energy_axis[i]
        old_right = energy_axis[i + 1]
        old_width = old_right - old_left

        # Skip invalid bins
        if old_width <= 0:
            continue

        # Move forward until new bin overlaps old bin
        while j < new_n and new_energy_axis[j + 1] <= old_left:
            j += 1

        k = j

        while k < new_n and new_energy_axis[k] < old_right:
            new_left = new_energy_axis[k]
            new_right = new_energy_axis[k + 1]

            overlap = min(old_right, new_right) - max(old_left, new_left)

            if overlap > 0:
                new_counts[k] += count_axis[i] * (overlap / old_width)

            k += 1

    return np.asarray(new_counts)