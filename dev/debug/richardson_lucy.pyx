# cython: language_level=3
# cython: boundscheck=False
# cython: wraparound=False
# cython: cdivision=True

import numpy as np
cimport numpy as cnp

from cython.parallel import prange
from libc.math cimport fmax


ctypedef cnp.float64_t float64_t
ctypedef cnp.int32_t int32_t
ctypedef cnp.int64_t int64_t


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
        ratio[i] = y[i] / fmax(estimate[i], 1e-12)


cdef void multiply_update(
    double[:] x,
    const double[:] correction,
) noexcept nogil:

    cdef Py_ssize_t i
    
    for i in range(
        x.shape[0],
    ):
        x[i] *= correction[i]


cpdef cnp.ndarray richardson_lucy(
    cnp.ndarray[float64_t, ndim=1] y,
    cnp.ndarray[int64_t, ndim=1] offsets,
    cnp.ndarray[int32_t, ndim=1] indices,
    cnp.ndarray[float64_t, ndim=1] values,
    int iterations=30,
):

    cdef:
        cnp.ndarray[float64_t, ndim=1] x
        cnp.ndarray[float64_t, ndim=1] estimate
        cnp.ndarray[float64_t, ndim=1] ratio
        cnp.ndarray[float64_t, ndim=1] correction

    # allocate once
    x = y.copy()

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

        multiply_update(
            x,
            correction,
        )

    return x