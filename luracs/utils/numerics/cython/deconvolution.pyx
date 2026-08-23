# cython: language_level=3
# cython: boundscheck=False
# cython: wraparound=False
# cython: cdivision=True

import numpy as np
cimport numpy as cnp

from libc.math cimport exp, log, fmax, sqrt

ctypedef cnp.float64_t float64_t
ctypedef cnp.int32_t int32_t
ctypedef cnp.int64_t int64_t

# This file contains a Cython implementation of a generalized Richardson-Lucy deconvolution algorithm and ML-EM algorithm. The implementation is designed to be efficient and can handle large datasets. It includes functions for forward and backward projections, ratio computation, and regularization techniques such as Tikhonov smoothing and entropy-based regularization.

# --------------------------------
# ML-EM
# --------------------------------

cdef void forward(
    const double[:] x,
    const int64_t[:] offsets,
    const int32_t[:] indices,
    const double[:] values,
    double[:] y,
) noexcept nogil:

    cdef:
        Py_ssize_t j, k
        Py_ssize_t start, end

    # clear output
    for j in range(y.shape[0]):
        y[j] = 0.0

    for j in range(x.shape[0]):
        start = offsets[j]
        end = offsets[j + 1]

        for k in range(start, end):
            y[indices[k]] += x[j] * values[k]


cdef double back_project_row(
    Py_ssize_t j,
    const double[:] ratio,
    const int64_t[:] offsets,
    const int32_t[:] indices,
    const double[:] values,
) noexcept nogil:

    cdef:
        Py_ssize_t k
        double total = 0.0

    for k in range(offsets[j], offsets[j + 1]):
        total += values[k] * ratio[indices[k]]

    return total


cdef void backward(
    const double[:] ratio,
    const int64_t[:] offsets,
    const int32_t[:] indices,
    const double[:] values,
    double[:] correction,
) noexcept nogil:

    cdef Py_ssize_t j

    for j in range(
        correction.shape[0],
    ):
        correction[j] = back_project_row(
            j,
            ratio,
            offsets,
            indices,
            values,
        )


cdef void compute_ratio(
    const double[:] y,
    const double[:] estimate,
    double[:] ratio,
) noexcept nogil:

    cdef Py_ssize_t i

    for i in range(
        y.shape[0],
    ):
        ratio[i] = y[i] / (estimate[i] + 1e-12)


cdef void multiply_update(
    double[:] x,
    const double[:] correction,
):

    cdef Py_ssize_t i
    for i in range(
        x.shape[0],
    ):
        x[i] *= correction[i]


cdef void tikhonov_smooth(
    double[:] x,
    double lam,
) noexcept nogil:

    cdef:
        Py_ssize_t i
        double lap

    for i in range(1, x.shape[0]-1):
        lap = x[i-1] - 2.0*x[i] + x[i+1]

        x[i] += lam * lap

cdef void entropy_step(
    double[:] x,
    const double[:] model,
    double lam,
) noexcept nogil:

    cdef:
        Py_ssize_t i
        double ratio

    for i in range(x.shape[0]):

        ratio = x[i] / (model[i] + 1e-12)

        x[i] *= exp(
            -lam * (log(ratio) + 1.0)
        )

        x[i] = fmax(x[i], 1e-12)


cdef void apply_sensitivity(
    double[:] correction,
    const double[:] sensitivity,
) noexcept nogil:

    cdef Py_ssize_t i

    for i in range(correction.shape[0]):
        correction[i] /= fmax(sensitivity[i], 1e-12)

#
# --- ML-EM algorithm implementation ---
#

cpdef cnp.ndarray ML_EM_cy(
    cnp.ndarray[float64_t, ndim=1] y,
    cnp.ndarray[int64_t, ndim=1] offsets,
    cnp.ndarray[int32_t, ndim=1] indices,
    cnp.ndarray[float64_t, ndim=1] values,
    cnp.ndarray[float64_t, ndim=1] sensitivity,
    int iterations=30,
    bool reg_tikhonov=False,
    double reg_tikhonov_lambda=0.001,
    bool use_sensitivity=False,
    # bool reg_entropy=False,
    # double reg_entropy_lambda=0.001,
):

    cdef:
        cnp.ndarray[float64_t, ndim=1] x
        cnp.ndarray[float64_t, ndim=1] estimate
        cnp.ndarray[float64_t, ndim=1] ratio
        cnp.ndarray[float64_t, ndim=1] correction

    # allocate once
    x = np.ones(offsets.shape[0] - 1)

    estimate = np.empty_like(y)
    ratio = np.empty_like(y)
    correction = np.empty_like(y)


    for _ in range(iterations):
        forward(
            x,
            offsets,
            indices,
            values,
            estimate,
        )

        compute_ratio(
            y,
            estimate,
            ratio,
        )

        backward(
            ratio,
            offsets,
            indices,
            values,
            correction,
        )


        apply_sensitivity(
            correction,
            sensitivity,
        )

        multiply_update(
            x,
            correction,
        )

        if reg_tikhonov:
            tikhonov_smooth(x, reg_tikhonov_lambda)
        

    return x

# -------------------------------
# Process Response
# -------------------------------

cpdef tuple process_response_cy(
    cnp.ndarray[float64_t, ndim=2] matrix,
    cnp.ndarray[float64_t, ndim=1] ref_indicies,
    cnp.ndarray[float64_t, ndim=1] ref_energy_axis,
    cnp.ndarray[float64_t, ndim=1] requested_indices,
    cnp.ndarray[float64_t, ndim=1] measured_energy_axis,
):
    cdef:
        cnp.ndarray[float64_t, ndim=2] interp_matrix

        Py_ssize_t i, j
        Py_ssize_t low_idx
        Py_ssize_t n_ref = matrix.shape[0]
        Py_ssize_t n_energy = matrix.shape[1]
        Py_ssize_t n_requested = requested_indices.shape[0]

        double x
        double x0, x1
        double w
        double value

    # Empty matrix to hold interpolated values
    interp_matrix = np.empty(
        (n_ref, n_energy),
        dtype=np.float64,
    )

    # ------------------------------------------------------------
    # 1. Interpolate each reference response onto measured energy
    # ------------------------------------------------------------

    for i in range(n_ref):
        interp_matrix[i] = np.interp(
            measured_energy_axis,
            ref_energy_axis,
            matrix[i],
            left=0.0,
            right=0.0,
        )

    # ------------------------------------------------------------
    # 2. Build CSR
    # ------------------------------------------------------------

    cdef list offsets = [0]
    cdef list indices = []
    cdef list values = []

    low_idx = 0

    for i in range(n_requested):

        x = requested_indicies[i]

        # Move to the lower bracketing reference index
        while (
            low_idx < n_ref - 2
            and ref_indicies[low_idx + 1] <= x
        ):
            low_idx += 1

        # Outside range -> empty CSR row
        if x < ref_indicies[0] or x > ref_indicies[n_ref - 1]:
            offsets.append(len(values))
            continue

        x0 = ref_indicies[low_idx]
        x1 = ref_indicies[low_idx + 1]

        w = (x - x0) / (x1 - x0)

        for j in range(n_energy):

            value = (
                interp_matrix[low_idx, j]
                + w * (
                    interp_matrix[low_idx + 1, j]
                    - interp_matrix[low_idx, j]
                )
            )

            # Store only nonzero values
            if value > 1e-12:
                indices.append(j)
                values.append(value)

        offsets.append(len(values))

    return (
        np.asarray(offsets, dtype=np.int64),
        np.asarray(indices, dtype=np.int32),
        np.asarray(values, dtype=np.float64),
    )

# -------------------------------
# Richardson-Lucy
# -------------------------------

cdef double FWHM_C = 2.0 * sqrt(2.0 * log(2.0))

cdef inline double get_sigma(double k, double E) noexcept nogil:
    cdef double resolution = k * sqrt(E)
    return resolution * E / FWHM_C


cpdef tuple build_gaussian_response(
    double[:] axis,
    double[:] sigma,
    double nsigma=5.0,
):
    cdef:
        Py_ssize_t n = axis.shape[0]
        Py_ssize_t i, j
        Py_ssize_t low, high
        double E, s
        double xmin, xmax
        double x, w
        double norm

    cdef list offsets = [0]
    cdef list indices = []
    cdef list values = []

    for i in range(n):
        E = axis[i]
        s = sigma[i]

        xmin = E - nsigma * s
        xmax = E + nsigma * s

        low = np.searchsorted(axis, xmin)
        high = np.searchsorted(axis, xmax, side="right")

        norm = 0.0
        for j in range(low, high):
            x = (axis[j] - E) / s
            norm += exp(-0.5 * x * x)

        for j in range(low, high):
            x = (axis[j] - E) / s
            w = exp(-0.5 * x * x) / norm
            indices.append(j)
            values.append(w)

        offsets.append(len(values))

    return (
        np.asarray(offsets, dtype=np.int64),
        np.asarray(indices, dtype=np.int32),
        np.asarray(values, dtype=np.float64),
    )

cpdef cnp.ndarray richardson_lucy_cy(
    cnp.ndarray[float64_t, ndim=1] x,
    cnp.ndarray[float64_t, ndim=1] y,
    double k,
    cnp.ndarray[float64_t, ndim=1] sensitivity,
    int iterations=30,
    bool reg_tikhonov=False,
    double reg_tikhonov_lambda=0.001,
    bool use_sensitivity=False,
):
    cdef:
        cnp.ndarray[int64_t, ndim=1] offsets
        cnp.ndarray[int32_t, ndim=1] indices
        cnp.ndarray[float64_t, ndim=1] values
        cnp.ndarray[float64_t, ndim=1] sigmas = np.empty_like(y)
        Py_ssize_t i

    for i in range(y.shape[0]):
        sigmas[i] = get_sigma(k, x[i])

    offsets, indices, values = build_gaussian_response(
        y,
        sigmas,
    )
    
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
