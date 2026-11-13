//! Multi-Gaussian peak models and their analytical Jacobians.
//!
//! Layout matches the Python version exactly: `params` is a flat array of
//! `[A1, mu1, var1, A2, mu2, var2, …]`, `variance` is σ² (not σ).

use ndarray::{Array1, Array2, ArrayView1};

/// Sum of N Gaussians at every point of `x`.
///
/// Matches Python's `multi_gaussian`, including its treatment of a
/// `params` length that isn't a multiple of 3 — the trailing partial
/// group is silently ignored. If you'd rather reject that as an error,
/// call `try_multi_gaussian` (below).
pub fn multi_gaussian(x: ArrayView1<f64>, params: &[f64]) -> Array1<f64> {
    let mut y = Array1::<f64>::zeros(x.len());

    for group in params.chunks_exact(3) {
        let (a, mu, var) = (group[0], group[1], group[2]);
        y += &x.mapv(|xj| {
            let d = xj - mu;
            a * (-0.5 * d * d / var).exp()
        });
    }

    y
}

/// Analytical Jacobian of `multi_gaussian`.
///
/// Returns a matrix of shape `(x.len(), params.len())`. Column order within
/// each Gaussian: `[∂/∂A, ∂/∂μ, ∂/∂var]`.
///
/// Column content, for group i:
/// - `J[:, 3i]     = exp(-(x-μ)² / (2·var))`
/// - `J[:, 3i + 1] = A · exp(...) · (x-μ) / var`
/// - `J[:, 3i + 2] = A · exp(...) · (x-μ)² / (var² · 2)`
pub fn multi_gaussian_jacobian(x: ArrayView1<f64>, params: &[f64]) -> Array2<f64> {
    let m = x.len();
    let n = params.len();
    let mut jac = Array2::<f64>::zeros((m, n));

    for (i, group) in params.chunks_exact(3).enumerate() {
        let (a, mu, var) = (group[0], group[1], group[2]);

        let exp_vals: Array1<f64> = x.mapv(|xj| {
            let d = xj - mu;
            (-0.5 * d * d / var).exp()
        });

        // ∂/∂A: just exp_term
        jac.column_mut(3 * i).assign(&exp_vals);

        // ∂/∂μ: A · exp_term · (x-μ) / var
        jac.column_mut(3 * i + 1)
            .assign(&Array1::from_iter(x.iter().zip(exp_vals.iter()).map(
                |(&xj, &e)| {
                    let d = xj - mu;
                    a * e * d / var
                },
            )));

        // ∂/∂var: A · exp_term · (x-μ)² / (var² · 2)
        jac.column_mut(3 * i + 2)
            .assign(&Array1::from_iter(x.iter().zip(exp_vals.iter()).map(
                |(&xj, &e)| {
                    let d = xj - mu;
                    a * e * d * d / (var * var) / 2.0
                },
            )));
    }

    jac
}

// ---------------------------------------------------------------------------
// Optional validated wrappers
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("params length {len} is not a multiple of 3 (each Gaussian needs A, mu, variance)")]
pub struct ParamLenError {
    pub len: usize,
}

/// Same as `multi_gaussian`, but returns `Err` if `params.len() % 3 != 0`
/// instead of silently dropping the trailing group.
pub fn try_multi_gaussian(
    x: ArrayView1<f64>,
    params: &[f64],
) -> Result<Array1<f64>, ParamLenError> {
    if params.len() % 3 != 0 {
        return Err(ParamLenError { len: params.len() });
    }
    Ok(multi_gaussian(x, params))
}

pub fn try_multi_gaussian_jacobian(
    x: ArrayView1<f64>,
    params: &[f64],
) -> Result<Array2<f64>, ParamLenError> {
    if params.len() % 3 != 0 {
        return Err(ParamLenError { len: params.len() });
    }
    Ok(multi_gaussian_jacobian(x, params))
}
