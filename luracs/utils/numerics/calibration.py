from __future__ import annotations
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from luracs.containers.spectrum_classes import Spectrum
    from luracs.containers.roi_classes import ROI, Fit

import numpy as np


def calibrate_x_axis(ref_channel_points: list, ref_energy_points: list, polynomial_degree:int, axis_len: int, current_x_axis: np.ndarray = None):
    """Calibrate the energy axis of a spectrum.

    Args:
        ref_channel_points (list): Measured centroids
        ref_energy_points (list): Reference emission energies for the centroids
        polynomial_degree (int): Degree of fitted polynomial
        axis_len (int): Length of returned x-axis
        current_x_axis (np.ndarray, optional): If the ref_channel_points are measured on an already calibrated x-axis this must be provided to interpolate the points to channel values. Defaults to None.

    Returns:
        np.ndarray: New x-axix
        np.ndarray: Calibration coefficients
    """
    
    if current_x_axis is not None:
        ref_channel_points = np.interp(ref_channel_points, current_x_axis, np.arange(axis_len))
    
    coefficients = np.polyfit(ref_channel_points, ref_energy_points,  polynomial_degree)
    new_x = np.polyval(coefficients, np.arange(axis_len))
    
    return new_x, coefficients
        
if __name__ == "__main__":
    channels = 1024
    current_x = np.polyval([0.0003705, 2.3694975, 4.2583089], np.arange(channels))
    
    ref_channel_points = [655, 1165, 1323]
    ref_energy_points = [662, 1173, 1332]
    
    new_x, coeff = calibrate_x_axis(ref_channel_points, ref_energy_points, 2, channels, current_x)

    print(coeff)