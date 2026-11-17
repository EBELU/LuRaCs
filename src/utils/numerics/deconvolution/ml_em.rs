
use ndarray::Array1;
use rayon::prelude::*;

use super::csr::{CsrMatrix};

pub const EPSILON: f64 = 1e-12;
pub const FWHM_C: f64 = 2.354_820_045_030_949_3;
pub const PARALLEL_ROW_THRESHOLD: usize = 512;

// =============================================================================
// Sparse matrix operations
// =============================================================================

/// Forward projection: out = A * x.
fn forward(
    matrix: &CsrMatrix,
    x: &Array1<f64>,
    out: &mut Array1<f64>,
) {
    assert_eq!(x.len(), matrix.ncols());
    assert_eq!(out.len(), matrix.nrows());

    let row_dot = |row: usize| {
        let mut sum = 0.0;

        for idx in matrix.offsets[row]..matrix.offsets[row + 1] {
            sum = matrix.values[idx].mul_add(x[matrix.indices[idx]], sum);
        }

        sum
    };

    if matrix.nrows() >= PARALLEL_ROW_THRESHOLD {
        out.as_slice_mut()
            .expect("output must be contiguous")
            .par_iter_mut()
            .enumerate()
            .for_each(|(row, value)| *value = row_dot(row));
    } else {
        for row in 0..matrix.nrows() {
            out[row] = row_dot(row);
        }
    }
}

/// Backward projection: out = Aᵀ * y.
fn backward(
    matrix: &CsrMatrix,
    y: &Array1<f64>,
    out: &mut Array1<f64>,
) {
    assert_eq!(y.len(), matrix.nrows());
    assert_eq!(out.len(), matrix.ncols());

    if matrix.nrows() >= PARALLEL_ROW_THRESHOLD {
        let ncols = matrix.ncols();

        let result = (0..matrix.nrows())
            .into_par_iter()
            .fold(
                || vec![0.0; ncols],
                |mut local, row| {
                    let value = y[row];

                    if value != 0.0 {
                        for idx in matrix.offsets[row]..matrix.offsets[row + 1] {
                            let col = matrix.indices[idx];

                            local[col] =
                                matrix.values[idx].mul_add(value, local[col]);
                        }
                    }

                    local
                },
            )
            .reduce(
                || vec![0.0; ncols],
                |mut a, b| {
                    for (x, y) in a.iter_mut().zip(b) {
                        *x += y;
                    }
                    a
                },
            );

        out.as_slice_mut()
            .expect("output must be contiguous")
            .copy_from_slice(&result);
    } else {
        out.fill(0.0);

        for row in 0..matrix.nrows() {
            let value = y[row];

            if value == 0.0 {
                continue;
            }

            for idx in matrix.offsets[row]..matrix.offsets[row + 1] {
                let col = matrix.indices[idx];

                out[col] = matrix.values[idx].mul_add(value, out[col]);
            }
        }
    }
}

// =============================================================================
// ML-EM
// =============================================================================

#[derive(Clone, Debug)]
pub struct MlEmOptions {
    pub iterations: usize,
}

impl Default for MlEmOptions {
    fn default() -> Self {
        Self { iterations: 30 }
    }
}

/// Multiplicative ML-EM update.
///
/// `matrix` is n × n.
/// `sensitivity` has length n.
pub fn ml_em(
    y: &Array1<f64>,
    matrix: &CsrMatrix,
    sensitivity: &Array1<f64>,
    opts: &MlEmOptions,
) -> Array1<f64> {
    let n = y.len();

    assert_eq!(matrix.nrows(), n, "matrix rows must match data length");
    assert_eq!(matrix.ncols(), n, "matrix must be square");
    assert_eq!(sensitivity.len(), n, "sensitivity length mismatch");

    let mut x = Array1::ones(n);
    let mut estimate = Array1::zeros(n);
    let mut ratio = Array1::zeros(n);
    let mut correction = Array1::zeros(n);

    for _ in 0..opts.iterations {
        forward(matrix, &x, &mut estimate);

        for ((r, &yi), &est) in ratio.iter_mut().zip(y.iter()).zip(estimate.iter()) {
            *r = yi / (est + EPSILON);
        }

        backward(matrix, &ratio, &mut correction);

        for ((xi, &ci), &si) in x
            .iter_mut()
            .zip(correction.iter())
            .zip(sensitivity.iter())
        {
            *xi *= ci / si.max(EPSILON);
        }
    }

    x
}

