
import numpy as np
cimport numpy as cnp

from libc.math cimport sqrt

def mariscotti_peak_search_cy(
    const double[:] y_d2,
):
    cdef:
        Py_ssize_t i
        cnp.ndarray[double, ndim=1] S = np.zeros_like(y_d2, dtype=np.float64)
        cnp.ndarray[double, ndim=1] F = np.zeros_like(y_d2, dtype=np.float64)

    for i in range(1, y_d2.shape[0] - 1):
        S[i] = y_d2[i-1] - 2 * y_d2[i] + y_d2[i+1]
        F[i] = sqrt(y_d2[i-1] + 4 * y_d2[i] + y_d2[i+1])

    return S, F