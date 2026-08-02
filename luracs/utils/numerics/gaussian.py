import numpy as np


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
    y = np.zeros_like(x, dtype=float)
    n_gauss = len(params) // 3
    for i in range(n_gauss):
        A, mu, variance = params[3 * i : 3 * i + 3]
        y += A * np.exp(-0.5 * ((x - mu) ** 2 / variance))
    return y


def multi_gaussian_jacobian(x, params):
    """
    Analytical Jacobian for multi-Gaussian function.

    Returns a matrix J of shape (len(x), len(params))
    """
    x = np.asarray(x)
    m = len(x)
    n = len(params)
    J = np.zeros((m, n))
    n_gauss = n // 3

    for i in range(n_gauss):
        A, mu, variance = params[3 * i : 3 * i + 3]
        exp_term = np.exp(-0.5 * ((x - mu) ** 2 / variance))

        # Derivative w.r.t amplitude
        J[:, 3 * i] = exp_term

        # Derivative w.r.t mean
        J[:, 3 * i + 1] = A * exp_term * (x - mu) / (variance)

        # Derivative w.r.t variance
        J[:, 3 * i + 2] = A * exp_term * ((x - mu) ** 2) / (variance**2) / 2

    return J
