# gaussian.pyx
# cython: language_level=3
# cython: boundscheck=False
# cython: wraparound=False
# cython: cdivision=True

import numpy as np
cimport numpy as cnp

from libc.math cimport exp

def multi_gaussian_cy(
    cnp.ndarray[double, ndim=1] x,
    cnp.ndarray[double, ndim=1] params
): 
    cdef:
        int m = x.shape[0]
        int n_gauss = params.shape[0] // 3
        cnp.ndarray[double, ndim=1] y = np.zeros(m, dtype=np.float64)
        int i, j
        double A, mu, variance
        double dx, exponent

    for i in range(n_gauss):

        A = params[3*i]
        mu = params[3*i+1]
        variance = params[3*i+2]

        for j in range(m):

            dx = x[j] - mu
            exponent = -0.5 * dx * dx / variance

            y[j] += A * exp(exponent)

    return y


def multi_gaussian_jacobian_cy(
    cnp.ndarray[double, ndim=1] x,
    cnp.ndarray[double, ndim=1] params
):

    cdef:
        int m = x.shape[0]
        int n = params.shape[0]
        int n_gauss = n // 3
        cnp.ndarray[double, ndim=2] J = np.zeros((m,n))
        int i,j
        double A, mu, variance
        double dx, e

    for i in range(n_gauss):

        A = params[3*i]
        mu = params[3*i+1]
        variance = params[3*i+2]

        for j in range(m):

            dx = x[j] - mu
            e = exp(-0.5 * dx * dx / variance)

            # dA
            J[j,3*i] = e

            # dmu
            J[j,3*i+1] = A * e * dx / variance

            # dvariance
            J[j,3*i+2] = (
                A * e * dx * dx /
                (2.0 * variance * variance)
            )

    return J