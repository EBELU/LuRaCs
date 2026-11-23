//! Shared numerical and array helper functions.

use ndarray::Array1;

/// Horner evaluation of a polynomial: `coeffs[0] * x^n + … + coeffs[n]`.
///
/// Coefficients are in descending degree order (matching `np.polyval`).
#[inline]
pub fn polyval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().fold(0.0, |acc, &c| f64::mul_add(acc, x, c))
}

/// Linear interpolation of `(xp, fp)` at query point `x`.
///
/// Flat-extrapolates at the boundaries (`fp[0]` for `x <= xp[0]`,
/// `fp[last]` for `x >= xp[last]`). Assumes `xp` is sorted ascending.
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    debug_assert_eq!(xp.len(), fp.len());
    if xp.is_empty() {
        return f64::NAN;
    }
    if x <= xp[0] {
        return fp[0];
    }
    if x >= xp[xp.len() - 1] {
        return fp[fp.len() - 1];
    }
    let idx = xp.partition_point(|&val| val < x);
    let x0 = xp[idx - 1];
    let x1 = xp[idx];
    let y0 = fp[idx - 1];
    let y1 = fp[idx];
    if (x1 - x0).abs() < 1e-12 {
        y0
    } else {
        y0 + (x - x0) * (y1 - y0) / (x1 - x0)
    }
}

/// Find insertion index maintaining sorted order (matching `numpy.searchsorted(arr, v, side='left')`).
#[inline]
pub fn searchsorted_left(arr: &[f64], v: f64) -> usize {
    arr.partition_point(|&x| x < v)
}

/// Find insertion index for `Array1<f64>`.
#[inline]
pub fn searchsorted_left_arr(arr: &Array1<f64>, v: f64) -> usize {
    arr.as_slice()
        .map(|s| s.partition_point(|&x| x < v))
        .unwrap_or_else(|| {
            let mut i = 0;
            while i < arr.len() && arr[i] < v {
                i += 1;
            }
            i
        })
}

/// Returns true if slice elements are monotonically non-decreasing.
#[inline]
pub fn is_monotonic_increasing(xp: &[f64]) -> bool {
    xp.windows(2).all(|w| w[0] <= w[1])
}

/// Median of a slice (mean of two middle elements for even length, `NaN` if empty).
pub fn median(x: &[f64]) -> f64 {
    if x.is_empty() {
        return f64::NAN;
    }
    let mut sorted = x.to_vec();
    let n = sorted.len();
    let mid = n / 2;
    let (_, upper_mid, _) = sorted.select_nth_unstable_by(mid, f64::total_cmp);
    let upper_val = *upper_mid;
    if n % 2 == 1 {
        upper_val
    } else {
        // Even length: mean of the two central values
        let lower_val = sorted[..mid]
            .iter()
            .copied()
            .max_by(f64::total_cmp)
            .unwrap_or(upper_val);
        0.5 * (lower_val + upper_val)
    }
}
