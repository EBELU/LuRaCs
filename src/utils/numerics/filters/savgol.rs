//! Savitzky-Golay filtering (coefficients + zero-copy interior correlation).
//!
//! Reproduces `scipy.signal.savgol_filter(..., mode="reflect")` exactly:
//! left pad is `y[half], y[half-1], …, y[1]`, and right pad is
//! `y[n-2], y[n-3], …, y[n-1-half]`.
//!
//! Because `np.convolve(ypad, coeffs[::-1], mode="valid")` reverses `coeffs`
//! twice, the operation reduces directly to correlation:
//! `out[i] = Σⱼ ypad[i + j] · coeffs[j]`.

use nalgebra::DMatrix;
use ndarray::ArrayView1;
use thiserror::Error;

pub use crate::utils::numerics::helpers::median;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SavgolError {
    #[error("window_length must be odd (got {0})")]
    EvenWindow(usize),
    #[error("polyorder ({polyorder}) must be < window_length ({window})")]
    PolyorderTooLarge { polyorder: usize, window: usize },
    #[error("deriv ({deriv}) must be <= polyorder ({polyorder})")]
    DerivTooLarge { deriv: usize, polyorder: usize },
    #[error("signal length ({signal}) must be > half window_length ({half})")]
    SignalTooShort { signal: usize, half: usize },
    #[error("pseudo-inverse of Vandermonde matrix failed (SVD did not converge)")]
    PinvFailed,
}

pub type SavgolResult<T> = std::result::Result<T, SavgolError>;

/// Compute Savitzky-Golay correlation coefficients.
///
/// `coeffs[j]` are the weights applied to samples `[i..i + window_length)`
/// to produce the filtered value (or `deriv`-th derivative) at the window center.
pub fn savgol_coeffs(
    window_length: usize,
    polyorder: usize,
    deriv: usize,
) -> SavgolResult<Vec<f64>> {
    if window_length == 0 || window_length.is_multiple_of(2) {
        return Err(SavgolError::EvenWindow(window_length));
    }
    if polyorder >= window_length {
        return Err(SavgolError::PolyorderTooLarge {
            polyorder,
            window: window_length,
        });
    }
    if deriv > polyorder {
        return Err(SavgolError::DerivTooLarge { deriv, polyorder });
    }

    let half = (window_length / 2) as isize;
    let cols = polyorder + 1;

    // Vandermonde matrix V[i, j] = x[i]^j, j in 0..=polyorder.
    let v = DMatrix::<f64>::from_fn(window_length, cols, |i, j| {
        let x = (i as isize - half) as f64;
        x.powi(j as i32)
    });

    let svd = v.svd(true, true);
    let u = svd.u.ok_or(SavgolError::PinvFailed)?;
    let vt = svd.v_t.ok_or(SavgolError::PinvFailed)?;
    let s = svd.singular_values;

    let max_s = s.iter().copied().fold(0.0_f64, f64::max);
    let tol = max_s * 1e-10;
    let fact = factorial(deriv) as f64;

    // Extract only row `deriv` of V⁺ = V · Σ⁻¹ · Uᵀ:
    // (V⁺)[deriv, i] = Σₖ V[deriv, k] · U[i, k] / s[k] = Σₖ Vt[k, deriv] · U[i, k] / s[k]
    let mut coeffs = vec![0.0; window_length];
    for k in 0..s.len() {
        if s[k] <= tol {
            continue;
        }
        let scale = (vt[(k, deriv)] / s[k]) * fact;
        for i in 0..window_length {
            coeffs[i] = scale.mul_add(u[(i, k)], coeffs[i]);
        }
    }

    Ok(coeffs)
}

#[inline]
fn factorial(n: usize) -> u64 {
    (1..=n as u64).product()
}

/// Apply a Savitzky-Golay filter using `mode="reflect"` boundary padding.
///
/// Returns `(filtered, coeffs)`.
pub fn savgol_filter(
    y: &[f64],
    window_length: usize,
    polyorder: usize,
    deriv: usize,
) -> SavgolResult<(Vec<f64>, Vec<f64>)> {
    let coeffs = savgol_coeffs(window_length, polyorder, deriv)?;
    let out = savgol_filter_with_coeffs(y, &coeffs)?;
    Ok((out, coeffs))
}

/// Apply a Savitzky-Golay filter using precomputed `coeffs` (from [`savgol_coeffs`]).
///
/// Uses small `3 * half` edge buffers so the interior `half..(n - half)` is
/// correlated zero-copy directly from `y`.
pub fn savgol_filter_with_coeffs(y: &[f64], coeffs: &[f64]) -> SavgolResult<Vec<f64>> {
    let window_length = coeffs.len();
    if window_length == 0 || window_length.is_multiple_of(2) {
        return Err(SavgolError::EvenWindow(window_length));
    }

    let half = window_length / 2;
    let n = y.len();
    if n <= half {
        return Err(SavgolError::SignalTooShort { signal: n, half });
    }

    let mut out = Vec::with_capacity(n);
    let coeffs_view = ArrayView1::from(coeffs);

    // Handle short signals where left and right reflection zones overlap
    // (half < n < window_length).
    if n < window_length {
        let mut ypad = Vec::with_capacity(n + 2 * half);
        for i in (1..=half).rev() {
            ypad.push(y[i]);
        }
        ypad.extend_from_slice(y);
        for k in 0..half {
            ypad.push(y[n - 2 - k]);
        }
        for window in ypad.windows(window_length).take(n) {
            out.push(ArrayView1::from(window).dot(&coeffs_view));
        }
        return Ok(out);
    }

    // 1. Left boundary (`half` output samples): requires `3 * half` samples.
    let mut left_buf = Vec::with_capacity(3 * half);
    for i in (1..=half).rev() {
        left_buf.push(y[i]);
    }
    left_buf.extend_from_slice(&y[..2 * half]);
    for window in left_buf.windows(window_length).take(half) {
        out.push(ArrayView1::from(window).dot(&coeffs_view));
    }

    // 2. Interior (`n - 2 * half` output samples): zero-copy directly on `y`.
    for window in y.windows(window_length) {
        out.push(ArrayView1::from(window).dot(&coeffs_view));
    }

    // 3. Right boundary (`half` output samples): requires `3 * half` samples.
    let mut right_buf = Vec::with_capacity(3 * half);
    right_buf.extend_from_slice(&y[(n - 2 * half)..]);
    for k in 0..half {
        right_buf.push(y[n - 2 - k]);
    }
    for window in right_buf.windows(window_length).take(half) {
        out.push(ArrayView1::from(window).dot(&coeffs_view));
    }

    Ok(out)
}
