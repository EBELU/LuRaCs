//! Weighted polynomial fit with covariance, matching
//! `np.polyfit(x, y, deg, w=w, cov="unscaled")`.
//!
//! `w` is treated as `1/σ` (NumPy convention). Internally multiplies the
//! Vandermonde matrix and target by `w`, so the normal-matrix inverse is
//! the weighted least-squares covariance.
//!
//! Coefficient order is highest-degree first (`[a_n, …, a_0]`), matching
//! `np.polyval` and the rest of this codebase.

use nalgebra::{DMatrix, DVector};
use thiserror::Error;

use super::pseudo_inverse;

#[derive(Debug, Error)]
pub enum PolyfitError {
    #[error("x and y length mismatch: {nx} vs {ny}")]
    SizeMismatch { nx: usize, ny: usize },
    #[error("not enough points for degree {degree}: {got} < {needed}")]
    TooFewPoints {
        degree: usize,
        got: usize,
        needed: usize,
    },
    #[error("SVD failed")]
    SvdFailed,
}

pub struct PolyfitResult {
    pub coefficients: Vec<f64>,
    pub covariance: DMatrix<f64>,
}

pub fn polyfit_weighted(
    x: &[f64],
    y: &[f64],
    w: Option<&[f64]>,
    degree: usize,
    cov_scaled: bool,
) -> Result<PolyfitResult, PolyfitError> {
    let n = x.len();
    let p = degree + 1;
    if n != y.len() {
        return Err(PolyfitError::SizeMismatch { nx: n, ny: y.len() });
    }
    if n < p {
        return Err(PolyfitError::TooFewPoints {
            degree,
            got: n,
            needed: p,
        });
    }
    if let Some(w) = w {
        if w.len() != n {
            return Err(PolyfitError::SizeMismatch { nx: n, ny: w.len() });
        }
    }

    // A = vander(x) .* w, b = y .* w
    let mut a = DMatrix::zeros(n, p);
    let mut b = DVector::zeros(n);
    for i in 0..n {
        let wi = w.map(|wv| wv[i]).unwrap_or(1.0);
        for k in 0..p {
            a[(i, k)] = x[i].powi((degree - k) as i32) * wi;
        }
        b[i] = y[i] * wi;
    }

    let ata = a.transpose() * &a;
    let atb = a.transpose() * &b;
    let svd = ata.clone().svd(true, true);
    let coeffs = svd
        .solve(&atb, 1e-12)
        .map_err(|_| PolyfitError::SvdFailed)?;

    let inv = pseudo_inverse(&ata).ok_or(PolyfitError::SvdFailed)?;
    let cov = if cov_scaled {
        let pred = &a * &coeffs;
        let resid = &b - &pred;
        let chi2 = resid.dot(&resid) / (n - p) as f64;
        inv * chi2
    } else {
        inv
    };

    Ok(PolyfitResult {
        coefficients: coeffs.iter().copied().collect(),
        covariance: cov,
    })
}
