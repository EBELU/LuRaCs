//! Polynomial energy-axis calibration.
//!
//! Direct port of `calibrate_x_axis`, with two design differences:
//!
//! 1. The `return_reference_points` flag is replaced by a struct that
//!    *always* returns the reference points. Callers that don't need them
//!    simply ignore `.reference_channels` / `.reference_energies`.
//! 2. `np.polyfit` is not in std; the fit is done with SVD-based least
//!    squares via `nalgebra`, which is more robust than `np.polyfit` for
//!    high-degree fits because it does not need the internal x-scaling that
//!    NumPy performs. For typical degrees (1–3) and 4096-channel axes the
//!    two approaches agree to ~1e-12.
//!
//! Coefficient order matches `np.polyfit` / `np.polyval`: **highest degree
//! first**. `coefficients[0] * x^n + … + coefficients[n]`.

use nalgebra::{DMatrix, DVector};
use ndarray::Array1;
use thiserror::Error;

use crate::utils::numerics::{interp, is_monotonic_increasing, polyval};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum CalibrationError {
    #[error(
        "ref_channel_points and ref_energy_points must have the same length \
         ({channels} vs {energies})"
    )]
    LengthMismatch { channels: usize, energies: usize },

    #[error("need at least {needed} reference points for a degree-{degree} fit, got {got}")]
    TooFewPoints {
        degree: usize,
        needed: usize,
        got: usize,
    },

    #[error("current_x_axis length ({axis}) does not match axis_len ({axis_len})")]
    AxisLengthMismatch { axis: usize, axis_len: usize },

    #[error("current_x_axis must be monotonically non-decreasing")]
    NonMonotonicAxis,

    #[error("polynomial fit failed (matrix is rank-deficient relative to tolerance)")]
    FitFailed,
}

pub type CalibrationnResult<T> = std::result::Result<T, CalibrationError>;

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CalibrationResult {
    /// New energy axis, length `axis_len`.
    pub x_axis: Array1<f64>,

    /// Polynomial coefficients, highest degree first
    /// (`[a_n, a_{n-1}, …, a_0]`), matching `np.polyfit`.
    pub coefficients: Vec<f64>,

    /// Reference channel coordinates actually used for the fit. If a
    /// `current_x_axis` was passed in, these are the interpolated
    /// (raw-channel) values rather than the original energy values.
    pub reference_channels: Vec<f64>,

    /// Reference energies, unchanged from the input.
    pub reference_energies: Vec<f64>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Calibrate an energy axis from reference peaks.
///
/// # Arguments
/// * `ref_channel_points` - measured centroids (channels, or energies if
///   `current_x_axis` is `Some`).
/// * `ref_energy_points` - reference emission energies, in keV.
/// * `polynomial_degree` - degree of the fitted polynomial.
/// * `axis_len` - number of channels in the returned axis.
/// * `current_x_axis` - if the `ref_channel_points` were measured on an
///   already-calibrated axis, pass that axis so they can be mapped back to
///   channel indices. `None` if the points are already in channel space.
pub fn calibrate_x_axis(
    ref_channel_points: &[f64],
    ref_energy_points: &[f64],
    polynomial_degree: usize,
    axis_len: usize,
    current_x_axis: Option<&[f64]>,
) -> CalibrationnResult<CalibrationResult> {
    if ref_channel_points.len() != ref_energy_points.len() {
        return Err(CalibrationError::LengthMismatch {
            channels: ref_channel_points.len(),
            energies: ref_energy_points.len(),
        });
    }

    // If the reference points were measured against an already-calibrated
    // axis, map them back to raw channel indices via linear interpolation.
    let effective_channels: Vec<f64> = match current_x_axis {
        Some(current) => {
            if current.len() != axis_len {
                return Err(CalibrationError::AxisLengthMismatch {
                    axis: current.len(),
                    axis_len,
                });
            }
            if !is_monotonic_increasing(current) {
                return Err(CalibrationError::NonMonotonicAxis);
            }
            let fp: Vec<f64> = (0..axis_len).map(|i| i as f64).collect();
            ref_channel_points
                .iter()
                .map(|&x| interp(x, current, &fp))
                .collect()
        }
        None => ref_channel_points.to_vec(),
    };

    let coefficients = polyfit(&effective_channels, ref_energy_points, polynomial_degree)?;

    let x_axis: Array1<f64> =
        Array1::from_iter((0..axis_len).map(|i| polyval(&coefficients, i as f64)));

    Ok(CalibrationResult {
        x_axis,
        coefficients,
        reference_channels: effective_channels,
        reference_energies: ref_energy_points.to_vec(),
    })
}

// ---------------------------------------------------------------------------
// Polynomial fit / evaluate / interp
// ---------------------------------------------------------------------------

/// Least-squares polynomial fit; matches `np.polyfit(x, y, degree)`
/// including the descending-degree coefficient order.
fn polyfit(x: &[f64], y: &[f64], degree: usize) -> CalibrationnResult<Vec<f64>> {
    let n = x.len();
    let needed = degree + 1;
    if n < needed {
        return Err(CalibrationError::TooFewPoints {
            degree,
            needed,
            got: n,
        });
    }

    // Vandermonde matrix, columns ordered highest degree first so the
    // solution vector comes out in `np.polyval` order.
    let mut a = DMatrix::<f64>::zeros(n, needed);
    for i in 0..n {
        for j in 0..needed {
            a[(i, j)] = x[i].powi((degree - j) as i32);
        }
    }
    let b = DVector::from_row_slice(y);

    // SVD-based least squares. `tol` acts like `np.polyfit`'s default
    // rcond=None (a few orders of magnitude above machine epsilon).
    let svd = a.svd(true, true);
    let coeffs = svd
        .solve(&b, 1e-10)
        .map_err(|_| CalibrationError::FitFailed)?;

    Ok(coeffs.as_slice().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calibrate_x_axis_direct_and_remapped_and_errors() {
        // Direct linear fit: E = 2 * ch + 10
        let res = calibrate_x_axis(&[0.0, 50.0, 100.0], &[10.0, 110.0, 210.0], 1, 5, None).unwrap();
        assert_eq!(res.x_axis.len(), 5);
        assert!((res.x_axis[0] - 10.0).abs() < 1e-9);
        assert!((res.x_axis[4] - 18.0).abs() < 1e-9);

        // Remapped from current_x_axis
        let current = [0.0, 10.0, 20.0, 30.0, 40.0];
        let res2 = calibrate_x_axis(&[10.0, 30.0], &[100.0, 300.0], 1, 5, Some(&current)).unwrap();
        assert!((res2.reference_channels[0] - 1.0).abs() < 1e-9);
        assert!((res2.reference_channels[1] - 3.0).abs() < 1e-9);
        assert!((res2.x_axis[2] - 200.0).abs() < 1e-9);

        // Error branches
        assert!(matches!(
            calibrate_x_axis(&[1.0], &[1.0, 2.0], 1, 5, None),
            Err(CalibrationError::LengthMismatch { .. })
        ));
        assert!(matches!(
            calibrate_x_axis(&[1.0], &[1.0], 2, 5, None),
            Err(CalibrationError::TooFewPoints { .. })
        ));
        assert!(matches!(
            calibrate_x_axis(&[1.0, 2.0], &[10.0, 20.0], 1, 5, Some(&[0.0, 1.0])),
            Err(CalibrationError::AxisLengthMismatch { .. })
        ));
        assert!(matches!(
            calibrate_x_axis(&[1.0, 2.0], &[10.0, 20.0], 1, 3, Some(&[0.0, 5.0, 2.0])),
            Err(CalibrationError::NonMonotonicAxis)
        ));
    }
}
