import numpy as np
from ROIClasses import Fit

def multi_gaussian(x, params):
    """
    Sum of N Gaussians.
    
    Parameters
    ----------
    x : array_like
        Independent variable
    params : array_like
        Parameters: [A1, mu1, sigma1, A2, mu2, sigma2, ...]
    
    Returns
    -------
    y : ndarray
    """
    y = np.zeros_like(x, dtype=float)
    n_gauss = len(params) // 3
    for i in range(n_gauss):
        A, mu, sigma = params[3*i : 3*i+3]
        y += A * np.exp(-0.5 * ((x - mu)**2/sigma))
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
        A, mu, sigma = params[3*i : 3*i+3]
        exp_term = np.exp(-0.5 * ((x - mu)**2/sigma))
        
        # Derivative w.r.t amplitude
        J[:, 3*i] = exp_term
        
        # Derivative w.r.t mean
        J[:, 3*i+1] = A * exp_term * (x - mu) / (sigma)
        
        # Derivative w.r.t sigma
        J[:, 3*i+2] = A * exp_term * ((x - mu)**2) / (sigma**2)
        
    return J

