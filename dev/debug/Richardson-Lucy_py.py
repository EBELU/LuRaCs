#!/home/eewa/anaconda3/bin/python3
# -*- coding: utf-8 -*-
"""
Created on Sun Jul 26 23:42:56 2026

@author: Erik Ewald
"""

import os
os.chdir(os.path.dirname(os.path.realpath(__file__)))
import numpy as np
import matplotlib.pyplot as plt
import pandas as pd
import math

from richardson_lucy import richardson_lucy

_fwhm_c = 2 * math.sqrt(2 * math.log(2))

def get_sigma(k: float, E: float):
    resolution = k * np.sqrt(E)  # fractional FWHM/E
    fwhm = resolution * E          # FWHM in energy units
    return fwhm / _fwhm_c          # Gaussian sigma
    

def build_gaussian_response(axis, sigma, nsigma=5):
    """
    Construct a sparse Gaussian detector response matrix.

    For each energy bin in ``axis``, a normalized Gaussian kernel is generated
    with standard deviation given by ``sigma``. Only bins within
    ``±nsigma * sigma`` are stored, producing a compressed sparse row (CSR-like)
    representation suitable for efficient forward and back projection in
    Richardson–Lucy deconvolution.

    Parameters
    ----------
    axis : ndarray of float
        Energy (or channel) values defining both the true and measured spectrum.
    sigma : ndarray of float
        Gaussian standard deviation for each energy bin. Must have the same
        length as ``axis``.
    nsigma : float, optional
        Number of standard deviations on either side of the Gaussian mean to
        retain. Default is 5.

    Returns
    -------
    offsets : ndarray
        Start/end positions for each true-energy kernel.
    indices : ndarray
        Measured-axis indices for each kernel value.
    values : ndarray
        Kernel probabilities.
    """

    offsets = [0]
    indices = []
    values = []

    for E, s in zip(axis, sigma):

        # Find affected measured bins
        low = np.searchsorted(axis, E - nsigma * s)
        high = np.searchsorted(axis, E + nsigma * s, side="right")

        idx = np.arange(low, high)

        x = axis[idx] - E

        kernel = np.exp(-0.5 * (x / s) ** 2)

        # Normalize to preserve counts
        kernel /= kernel.sum()

        indices.extend(idx)
        values.extend(kernel)

        offsets.append(len(values))

    return (
        np.asarray(offsets, dtype=np.int64),
        np.asarray(indices, dtype=np.int32),
        np.asarray(values, dtype=np.float64),
    )

data = pd.read_csv("~/Desktop/Raysid-GRF-Eu152 Recalib-.csv").to_numpy()
data = pd.read_csv("~/Desktop/Th.csv").to_numpy()
# data = pd.read_csv("~/Desktop/Raysid-GRF-Co60.csv").to_numpy()


clipped_data = data[(data[:, 0] >= 30)]

csi = 2
detx = 0.0857
sigmas = get_sigma(detx / 100, clipped_data[:, 0])

offsets, indices, values = build_gaussian_response(clipped_data[:, 0], sigmas)

# res = richardson_lucy(data, offsets, indices, values)

def forward_project(x, offsets, indices, values):
    y = np.zeros_like(x)

    for j in range(len(x)):
        start = offsets[j]
        end = offsets[j + 1]

        idx = indices[start:end]
        val = values[start:end]

        y[idx] += x[j] * val

    return y

def back_project(ratio, offsets, indices, values):
    correction = np.empty(len(offsets) - 1)

    for j in range(len(correction)):
        start = offsets[j]
        end = offsets[j + 1]

        idx = indices[start:end]
        val = values[start:end]

        correction[j] = np.sum(val * ratio[idx])

    return correction


def richardson_lucy_py(y, offsets, indices, values,
                    iterations=30):

    x = y.astype(np.float64).copy()

    for _ in range(iterations):

        estimate = forward_project(x, offsets, indices, values)

        ratio = y / np.maximum(estimate, 1e-12)

        correction = back_project(ratio, offsets, indices, values)

        x *= correction

    return x

from time import perf_counter

# Warm up (important for fair comparison)
# richardson_lucy_py(clipped_data[:, 1], offsets, indices, values, iterations=10)
richardson_lucy(clipped_data[:, 1], offsets, indices, values, iterations=10)

N = 1

# t0 = perf_counter()
# for _ in range(N):
#     y_py = richardson_lucy_py(
#         clipped_data[:, 1],
#         offsets,
#         indices,
#         values,
#         iterations=10,
#     )
# t1 = perf_counter()

t2 = perf_counter()
for _ in range(N):
    y_cy = richardson_lucy(
        clipped_data[:, 1],
        offsets,
        indices,
        values,
        iterations=5,
    )
t3 = perf_counter()

# print(f"Python : {(t1 - t0)/N:.6f} s/run")
# print(f"Cython : {(t3 - t2)/N:.6f} s/run")
# print(f"Speedup: {(t1 - t0)/(t3 - t2):.2f}×")

plt.plot(clipped_data[:, 1])
plt.plot(y_cy)
#plt.plot(y_py)
plt.show()