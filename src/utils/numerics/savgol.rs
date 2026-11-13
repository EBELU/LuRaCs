//! Savitzky-Golay filtering (coeffs + convolution).
//!
//! `np.pad(y, half, mode="reflect")` is reproduced exactly: left pad is
//! `y[half], y[half-1], …, y[1]`, right pad is `y[n-2], y[n-3], …, y[n-1-half]`.
//!
//! The convolution `np.convolve(ypad, coeffs[::-1], mode="valid")` reduces
//! to standard correlation `out[i] = Σⱼ ypad[i+j]·coeffs[j]` after the two
//! reversals cancel — no `coeffs` reversal needed.

use nalgebra::DMatrix;
use thiserror::Error;

use super::pseudo_inverse;

#[derive(Debug, Error)]
pub enum SavgolError {
    #[error("window_length must be odd (got {0})")]
    EvenWindow(usize),
    #[error("polyorder ({polyorder}) must be < window_length ({window})")]
    PolyorderTooLarge { polyorder: usize, window: usize },
    #[error("deriv ({deriv}) must be <= polyorder ({polyorder})")]
    DerivTooLarge { deriv: usize, polyorder: usize },
    #[error("signal length {signal} is not greater than window_length {window}")]
    SignalTooShort { signal: usize, window: usize },
    #[error("pseudo-inverse of Vandermonde matrix failed (SVD did not converge)")]
    PinvFailed,
}

pub type SavgolResult<T> = std::result::Result<T, SavgolError>;

/// Compute Savitzky-Golay convolution coefficients.
///
/// `coeffs[j]` are the weights applied to samples `[i..i+window_length)`
/// to produce the filtered value at `i`.
pub fn savgol_coeffs(
    window_length: usize,
    polyorder: usize,
    deriv: usize,
) -> SavgolResult<Vec<f64>> {
    if window_length % 2 == 0 {
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

    let half = (window_length / 2) as i64;
    let x: Vec<f64> = (0..window_length as i64)
        .map(|i| (i - half) as f64)
        .collect();

    // Vandermonde V[i, j] = x[i]^j, j in 0..=polyorder.
    let mut v = DMatrix::<f64>::zeros(window_length, polyorder + 1);
    for i in 0..window_length {
        for j in 0..=polyorder {
            v[(i, j)] = x[i].powi(j as i32);
        }
    }

    let pinv = pseudo_inverse(&v).ok_or(SavgolError::PinvFailed)?;

    // Row `deriv` of pinv; multiply by deriv! (the m-th derivative at x=0
    // picks out m! · a_m from the polynomial coefficients).
    let fact: f64 = factorial(deriv) as f64;
    Ok((0..window_length)
        .map(|i| pinv[(deriv, i)] * fact)
        .collect())
}

fn factorial(n: usize) -> u64 {
    // Empty product for n=0 is 1 by Rust convention.
    (1..=n as u64).product::<u64>()
}

/// Apply a Savitzky-Golay filter. Returns `(filtered, coeffs)`.
pub fn savgol_filter(
    y: &[f64],
    window_length: usize,
    polyorder: usize,
    deriv: usize,
) -> SavgolResult<(Vec<f64>, Vec<f64>)> {
    let coeffs = savgol_coeffs(window_length, polyorder, deriv)?;
    let half = window_length / 2;

    if y.len() <= half {
        return Err(SavgolError::SignalTooShort {
            signal: y.len(),
            window: window_length,
        });
    }

    // Reflect pad: left = y[half..=1] in reverse, right = y[n-2 ..= n-1-half]
    // in forward order.
    let n = y.len();
    let mut ypad = Vec::with_capacity(n + 2 * half);
    for i in (1..=half).rev() {
        ypad.push(y[i]);
    }
    ypad.extend_from_slice(y);
    for k in 0..half {
        ypad.push(y[n - 2 - k]);
    }

    // Correlation: out[i] = Σⱼ ypad[i+j] · coeffs[j]
    let out_len = ypad.len() - window_length + 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let mut acc = 0.0;
        for j in 0..window_length {
            acc += ypad[i + j] * coeffs[j];
        }
        out.push(acc);
    }

    Ok((out, coeffs))
}

/// Median of a slice (mean of two middle elements for even length, `NaN` if empty).
pub(crate) fn median(x: &[f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    let mut sorted = x.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}
