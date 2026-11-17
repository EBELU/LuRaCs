use ndarray::{Array1, Array2};
use rayon::prelude::*;

use super::csr::{CsrMatrix, PARALLEL_ROW_THRESHOLD};
use super::ml_em::{ml_em, MlEmOptions, FWHM_C};

// =============================================================================
// Gaussian response
// =============================================================================

#[inline]
fn get_sigma(k: f64, energy: f64) -> f64 {
    k * energy.sqrt() * energy / FWHM_C
}

/// Build a row-normalized Gaussian response matrix.
///
/// Each row is centered on its corresponding axis value and truncated
/// at `nsigma * sigma[i]`.
fn build_gaussian_response_dense(
    axis: &[f64],
    sigma: &[f64],
    nsigma: f64,
) -> Array2<f64> {
    let n = axis.len();
    assert_eq!(sigma.len(), n);

    let mut matrix = Array2::<f64>::zeros((n, n));

    if n == 0 {
        return matrix;
    }

    let fill_row = |i: usize, row: &mut [f64]| {
        let energy = axis[i];
        let s = sigma[i];

        // A zero-width Gaussian becomes a discrete identity contribution.
        if s <= 0.0 {
            row[i] = 1.0;
            return;
        }

        let min = nsigma.mul_add(-s, energy);
        let max = nsigma.mul_add(s, energy);

        // Find the Gaussian support in the sorted axis.
        let low = axis.partition_point(|&v| v < min);
        let high = axis.partition_point(|&v| v <= max);

        let mut norm = 0.0;

        for &value in &axis[low..high] {
            let t = (value - energy) / s;
            norm += (-0.5 * t * t).exp();
        }

        if norm <= 0.0 {
            row[i] = 1.0;
            return;
        }

        for j in low..high {
            let t = (axis[j] - energy) / s;
            row[j] = (-0.5 * t * t).exp() / norm;
        }
    };

    let flat = matrix.as_slice_mut().expect("matrix must be contiguous");

    if n >= PARALLEL_ROW_THRESHOLD {
        flat.par_chunks_mut(n)
            .enumerate()
            .for_each(|(i, row)| fill_row(i, row));
    } else {
        for i in 0..n {
            fill_row(i, &mut flat[i * n..(i + 1) * n]);
        }
    }

    matrix
}

/// Build a sparse Gaussian response matrix.
fn build_gaussian_response(
    axis: &[f64],
    sigma: &[f64],
    nsigma: f64,
) -> CsrMatrix {
    CsrMatrix::from_dense(
        &build_gaussian_response_dense(axis, sigma, nsigma),
    )
}

// =============================================================================
// Richardson–Lucy
// =============================================================================

/// Richardson–Lucy with a Gaussian PSF whose per-row sigma
/// depends on the source energy.
/// 
pub fn richardson_lucy(
    x_axis: &Array1<f64>,
    y: &Array1<f64>,
    k: f64,
    sensitivity: &Array1<f64>,
    opts: &MlEmOptions,
) -> Array1<f64> {
    let n = y.len();

    assert!(x_axis.len() >= n, "x_axis must be at least as long as y");
    assert_eq!(sensitivity.len(), n, "sensitivity length mismatch");

    let axis = x_axis.slice(ndarray::s![..n]);

    let sigma = Array1::from_iter(
        axis.iter().map(|&energy| get_sigma(k, energy)),
    );

    let matrix = build_gaussian_response(
        axis.as_slice().expect("axis must be contiguous"),
        sigma.as_slice().expect("sigma must be contiguous"),
        5.0,
    );

    ml_em(y, &matrix, sensitivity, opts)
}