#!/home/eewa/anaconda3/bin/python3
# -*- coding: utf-8 -*-
"""
Created on Mon Mar 23 00:14:05 2026

@author: Erik Ewald
"""

import os
os.chdir(os.path.dirname(os.path.realpath(__file__)))
import numpy as np
import matplotlib.pyplot as plt
import pandas as pd

def numerical_jacobian(f, x, eps=1e-8):
    """
    Compute the Jacobian of f at x numerically using finite differences.
    
    Parameters
    ----------
    f : function
        f(x) should return a 1D array of length m
    x : array_like
        Current parameter vector (length n)
    eps : float
        Step size for finite differences
    
    Returns
    -------
    J : ndarray, shape (m, n)
        Jacobian matrix
    """
    f0 = f(x)
    m = len(f0)
    n = len(x)
    J = np.zeros((m, n))
    
    for j in range(n):
        dx = np.zeros_like(x)
        dx[j] = eps
        f1 = f(x + dx)
        J[:, j] = (f1 - f0) / eps
    return J

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
        y += A * np.exp(-0.5 * ((x - mu)/sigma)**2)
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
        exp_term = np.exp(-0.5 * ((x - mu)/sigma)**2)
        
        # Derivative w.r.t amplitude
        J[:, 3*i] = exp_term
        
        # Derivative w.r.t mean
        J[:, 3*i+1] = A * exp_term * (x - mu) / (sigma**2)
        
        # Derivative w.r.t sigma
        J[:, 3*i+2] = A * exp_term * ((x - mu)**2) / (sigma**3)
        
    return J


def huber_weights(r, yfit, params, delta=1.0):
    abs_r = np.abs(r)
    return np.where(abs_r < delta, 1.0, delta / abs_r)

def poisson_weights(residuals, yfit, params, eps=1e-10):
    return 1.0 / np.maximum(yfit, eps)


def curve_fit(
    f,
    xdata,
    ydata,
    p0,
    jac=None,
    weight_fn=None,
    weight_cov_chi2=True,
    max_iter=100,
    tol=1e-8,
    lam=1e-3
):
    """
    Levenberg–Marquardt optimizer with flexible weighting.

    Parameters
    ----------
    f : callable
        Model function f(xdata, params) -> y
    xdata : array_like
        Independent variable
    ydata : array_like
        Observed data
    p0 : array_like
        Initial parameter guess
    jac : callable, optional
        Jacobian J(xdata, params) -> shape (N, M)
    weight_fn : callable, optional
        weight_fn(residuals, yfit, params) -> weights (N,)
    """

    p = np.array(p0, dtype=float)
    xdata = np.array(xdata, dtype=float)
    ydata = np.array(ydata, dtype=float)

    converged = False

    for _ in range(max_iter):
        yfit = f(xdata, p)
        r = ydata - yfit

        # Jacobian
        if jac is not None:
            J = jac(xdata, p)
        else:
            J = numerical_jacobian(lambda params: f(xdata, params), p)

        # Apply weights
        if weight_fn is not None:
            w = np.asarray(weight_fn(r, yfit, p))
            W = np.sqrt(w)
            r_w = W * r
            J_w = W[:, None] * J
        else:
            r_w = r
            J_w = J

        # Normal equations
        H = J_w.T @ J_w
        g = J_w.T @ r_w

        # Damped system
        A = H + lam * np.eye(len(p))

        try:
            dp = np.linalg.solve(A, g)
        except np.linalg.LinAlgError:
            dp = np.linalg.lstsq(A, g, rcond=None)[0]

        # Trial step
        p_new = p + dp
        yfit_new = f(xdata, p_new)
        r_new = ydata - yfit_new

        if weight_fn is not None:
            w_new = np.asarray(weight_fn(r_new, yfit_new, p_new))
            r_new_w = np.sqrt(w_new) * r_new
        else:
            r_new_w = r_new

        # Accept/reject
        if np.linalg.norm(r_new_w) < np.linalg.norm(r_w):
            p = p_new
            lam *= 0.5
        else:
            lam *= 2.0

        # Convergence checks
        if np.linalg.norm(dp) < tol * (np.linalg.norm(p) + tol):
            converged = True
            break

        if np.linalg.norm(g, ord=np.inf) < tol:
            converged = True
            break

    # Final Jacobian
    if jac is not None:
        J = jac(xdata, p)
    else:
        J = numerical_jacobian(lambda params: f(xdata, params), p)

    if weight_fn is not None:
        w = np.asarray(weight_fn(ydata - f(xdata, p), f(xdata, p), p))
        J = np.sqrt(w)[:, None] * J

    H = J.T @ J

    # Covariance
    try:
        cov = np.linalg.inv(H)
    except np.linalg.LinAlgError:
        cov = np.linalg.pinv(H)

    # Chi^2 scaling
    if weight_cov_chi2:
        residuals = ydata - f(xdata, p)
        dof = max(len(ydata) - len(p), 1)
        chi2 = np.sum(residuals**2) / dof
        cov *= chi2

    return p, cov, converged
from dataclasses import dataclass
@dataclass
class Fit:
    region_lower: float
    region_upper: float
    
    lower: float
    upper: float

    fit_type: str
    params: np.array
    param_errs: np.array
    
    bkg_type: str
    bkg_params: str
    
    G: float
    B: float
    N: float
    
    @property
    def A(self):
        return self.params[0]
    @property
    def mu(self):
        return self.params[1]
    @property
    def sigma(self):
        return self.params[2]
    
    @property
    def A_err(self):
        return self.params_errs[0]
    @property
    def mu_err(self):
        return self.params_errs[1]
    @property
    def sigma_err(self):
        return self.params_errs[2]
    
    
    
def fit_gaussians(x_axis, y_axis, bounds, 
                  fit_type, use_poisson_weights, weigh_cov_chi2,
                  bkg_type, bkg_fit_extension,):
    region_min, region_max = np.min(bounds), np.max(bounds)
    
    region = (region_min <= x_axis) & (x_axis <= region_max)
    x_region = x_axis[region].copy().astype(float)
    y_region = y_axis[region].copy().astype(float)
    
    p0 = []
    for b in bounds:
        lower, upper = np.min(b), np.max(b)
    
        # mask for this peak window
        peak_mask = (lower <= x_region) & (x_region <= upper)
    
        x_peak = x_region[peak_mask]
        y_peak = y_region[peak_mask]
    
        if len(x_peak) == 0:
            continue  # skip empty regions
    
        # Initial guesses
        A0 = np.max(y_peak)                 
        mu0 = x_peak[np.argmax(y_peak)]      
        s0 = (upper - lower) / 6.0           
    
        p0.extend([A0, mu0, s0])
        
    p0s = np.array(p0)
        
    
    if bkg_type != "None":
        i_low = np.searchsorted(x_axis, region_min)
        i_high = np.searchsorted(x_axis, region_max)
        
        bkg_extention_lower = i_low - bkg_fit_extension
        if bkg_extention_lower < 0: bkg_extention_lower = 0
        
        lower_bkg_points_x = x_axis[bkg_extention_lower:i_low]
        lower_bkg_points_y = y_axis[bkg_extention_lower:i_low]
        
        bkg_extention_upper = i_high + bkg_fit_extension
        if bkg_extention_upper > len(x_axis): bkg_extention_lower = len(x_axis)
        
        upper_bkg_points_x = x_axis[i_high:bkg_extention_upper]
        upper_bkg_points_y = y_axis[i_high:bkg_extention_upper]
        
        if bkg_type == "Linear":
            poly_order = 1
        elif bkg_type == "Quadratic":
            poly_order = 2
        else:
            raise ValueError(f"Invald background type {bkg_type}")
        
        bkg_fit = np.polyfit(np.concatenate((lower_bkg_points_x, upper_bkg_points_x)), 
                             np.concatenate((lower_bkg_points_y, upper_bkg_points_y)), 
                             poly_order)
        
        y_region -= np.polyval(bkg_fit, x_region)
    
    else:
        bkg_fit = None
            
    p0 = p0s.flatten()
    
    weight = None
    if use_poisson_weights:
        weight = poisson_weights
    
    fits, cov, converged = curve_fit(multi_gaussian, x_region, y_region, p0, jac=multi_gaussian_jacobian, weight_fn=weight,
                                    weight_cov_chi2=weigh_cov_chi2)
    
    if np.any(fits > 1e12) or np.any(np.sqrt(np.abs(np.diag(cov))) > 1e12):
        return None, None, None, None, False
    
    fits = fits.reshape(-1, 3)
    errs = np.sqrt(np.diag(cov)).reshape(-1, 3)
    
    results = []
    for b, fit, err in zip(bounds, fits, errs):
        lower, upper = np.min(b), np.max(b)
    
        peak_mask = (lower <= x_axis) & (x_axis<= upper)
    
        x_peak = x_axis[peak_mask]
        y_peak = y_axis[peak_mask]
        
        G = np.sum(y_peak)
        B = np.sum(np.polyval(bkg_fit, x_peak))
        plt.plot(y_peak)
        plt.plot(np.polyval(bkg_fit, x_peak))
        N = G - B
        
        fit_data = Fit(
            region_min, region_max,
            lower, upper,
            fit_type, fit, err,
            bkg_type, bkg_fit,
            G, B, N)
        results.append(fit_data)
    
    return results, converged



@dataclass
class ROIResults:
    tag: str
    roi_bound: tuple
    region_bound: tuple
    fit: object | None
    counts: float
    meta: dict
    
def eval_rois(spectrum, roi_bound, bkg_parameters, fit_parameteres):
    pass
    
data = pd.read_csv("/home/eewa/Documents/SommarProjekt/RadiaCode/Spectroscopy/RadiacodeSpectra/Cyklotron_Co.csv").to_numpy().T[1]
data = data[390: 600]
#plt.plot(data)
fits, converged = fit_gaussians(np.arange(len(data)), data, [[20, 100], [100, 200]], 
              **{"bkg_type": "Quadratic", "bkg_fit_extension": 5} | 
              {"use_poisson_weights": False, "weigh_cov_chi2": True, "fit_type": "Gaussian"})


print(*fits)
# x = np.arange(len(data))

# plt.plot(np.polyval(bkg_fit, x))
# plt.plot(np.polyval(bkg_fit, x) + multi_gaussian(x, fits.flatten()))
    