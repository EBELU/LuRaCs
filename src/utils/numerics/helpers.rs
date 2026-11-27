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

pub fn nanmean(v: &[f64]) -> Option<f64> {
    let mut sum = 0.0_f64;
    let mut count = 0usize;
    for &x in v {
        if x.is_finite() {
            sum += x;
            count += 1;
        }
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polyval_interp_searchsorted_monotonic_median_nanmean() {
        // polyval: 2x^2 + 3x + 1 at x = 2 => 8 + 6 + 1 = 15
        assert!((polyval(&[2.0, 3.0, 1.0], 2.0) - 15.0).abs() < 1e-12);

        // interp: empty, below, above, flat segment, linear interior
        assert!(interp(1.0, &[], &[]).is_nan());
        let xp = [0.0, 10.0, 10.0, 20.0];
        let fp = [1.0, 11.0, 11.0, 31.0];
        assert!((interp(-5.0, &xp, &fp) - 1.0).abs() < 1e-12);
        assert!((interp(25.0, &xp, &fp) - 31.0).abs() < 1e-12);
        assert!((interp(10.0, &xp, &fp) - 11.0).abs() < 1e-12);
        assert!((interp(15.0, &xp, &fp) - 21.0).abs() < 1e-12);

        // searchsorted_left & searchsorted_left_arr (contiguous & non-contiguous)
        assert_eq!(searchsorted_left(&xp, 10.0), 1);
        let arr = Array1::from_vec(vec![1.0, 3.0, 5.0, 7.0]);
        assert_eq!(searchsorted_left_arr(&arr, 4.0), 2);
        let strided = arr.slice(ndarray::s![..;2]).to_owned();
        assert_eq!(searchsorted_left_arr(&strided, 4.0), 1);

        // is_monotonic_increasing
        assert!(is_monotonic_increasing(&[1.0, 1.0, 2.0, 5.0]));
        assert!(!is_monotonic_increasing(&[1.0, 3.0, 2.0]));

        // median
        assert!(median(&[]).is_nan());
        assert!((median(&[5.0, 1.0, 3.0]) - 3.0).abs() < 1e-12);
        assert!((median(&[4.0, 1.0, 3.0, 2.0]) - 2.5).abs() < 1e-12);

        // nanmean
        assert_eq!(nanmean(&[f64::NAN, f64::INFINITY]), None);
        assert_eq!(nanmean(&[2.0, f64::NAN, 4.0]), Some(3.0));
    }
}