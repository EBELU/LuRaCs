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

use super::optimizer::pseudo_inverse;

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
    if let Some(w) = w
        && w.len() != n
    {
        return Err(PolyfitError::SizeMismatch { nx: n, ny: w.len() });
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
        coefficients: coeffs.as_slice().to_vec(),
        covariance: cov,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polyfit_weighted_and_errors() {
        let x = [0.0, 1.0, 2.0, 3.0];
        let y = [1.0, 3.0, 5.0, 7.0]; // y = 2x + 1
        let w = [1.0, 1.0, 1.0, 1.0];

        let res_unscaled = polyfit_weighted(&x, &y, None, 1, false).unwrap();
        assert!((res_unscaled.coefficients[0] - 2.0).abs() < 1e-9);
        assert!((res_unscaled.coefficients[1] - 1.0).abs() < 1e-9);

        let res_scaled = polyfit_weighted(&x, &y, Some(&w), 1, true).unwrap();
        assert!((res_scaled.coefficients[0] - 2.0).abs() < 1e-9);
        assert_eq!(res_scaled.covariance.nrows(), 2);

        assert!(matches!(
            polyfit_weighted(&[1.0], &[1.0, 2.0], None, 1, false),
            Err(PolyfitError::SizeMismatch { .. })
        ));
        assert!(matches!(
            polyfit_weighted(&[1.0], &[1.0], None, 1, false),
            Err(PolyfitError::TooFewPoints { .. })
        ));
        assert!(matches!(
            polyfit_weighted(&x, &y, Some(&[1.0]), 1, false),
            Err(PolyfitError::SizeMismatch { .. })
        ));
    }
}
