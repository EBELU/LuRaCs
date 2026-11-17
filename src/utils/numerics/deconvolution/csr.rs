use nalgebra::DVector;
use ndarray::Array2;
use rayon::prelude::*;

use super::solver::{EPSILON, FWHM_C, PARALLEL_ELEM_THRESHOLD, PARALLEL_ROW_THRESHOLD};

// =============================================================================
// CSR sparse operator
// =============================================================================

#[derive(Clone, Debug)]
pub struct CsrMatrix {
    offsets: Vec<usize>, // len = nrows + 1
    indices: Vec<usize>, // column indices
    values: Vec<f64>,
    nrows: usize,
    ncols: usize,
}

impl CsrMatrix {
    /// Construct from raw CSR parts. Panics on inconsistent input.
    pub fn from_parts(
        offsets: Vec<usize>,
        indices: Vec<usize>,
        values: Vec<f64>,
        ncols: usize,
    ) -> Self {
        assert!(offsets.len() >= 2, "offsets must have >= 2 entries");
        assert_eq!(
            indices.len(),
            values.len(),
            "indices/values length mismatch"
        );
        assert_eq!(
            *offsets.last().unwrap(),
            indices.len(),
            "offsets must terminate at indices.len()"
        );
        Self {
            nrows: offsets.len() - 1,
            offsets,
            indices,
            values,
            ncols,
        }
    }

    pub const fn nrows(&self) -> usize {
        self.nrows
    }

    pub const fn ncols(&self) -> usize {
        self.ncols
    }

    pub const fn nnz(&self) -> usize {
        self.values.len()
    }

    /// Convert to NumPy-compatible int64/int32 arrays if needed for serialization.
    pub fn to_i64_i32(&self) -> (Vec<i64>, Vec<i32>, Vec<f64>) {
        (
            self.offsets.iter().map(|&x| x as i64).collect(),
            self.indices.iter().map(|&x| x as i32).collect(),
            self.values.clone(),
        )
    }

    /// Construct from raw CSR buffers. Validates shapes and monotonicity.
    pub fn new(
        offsets: Vec<usize>,
        indices: Vec<usize>,
        values: Vec<f64>,
        nrows: usize,
        ncols: usize,
    ) -> Result<Self, &'static str> {
        if offsets.len() != nrows + 1 {
            return Err("offsets length must be nrows + 1");
        }
        if indices.len() != values.len() {
            return Err("indices and values must have the same length");
        }
        if offsets[0] != 0 || offsets[nrows] != values.len() {
            return Err("offsets must start at 0 and end at nnz");
        }
        for w in offsets.windows(2) {
            if w[0] > w[1] {
                return Err("offsets must be non-decreasing");
            }
        }
        for &col in &indices {
            if col >= ncols {
                return Err("column index out of bounds");
            }
        }
        Ok(Self {
            offsets,
            indices,
            values,
            nrows,
            ncols,
        })
    }

    /// Construct from a dense `ndarray::Array2`. Drops entries below `1e-12`.
    pub fn from_dense(dense: &Array2<f64>) -> Self {
        let (nrows, ncols) = (dense.nrows(), dense.ncols());
        let mut offsets = Vec::with_capacity(nrows + 1);
        let mut indices = Vec::new();
        let mut values = Vec::new();

        offsets.push(0);
        for row in dense.rows() {
            for (col, &val) in row.iter().enumerate() {
                if val.abs() > 1e-12 {
                    indices.push(col);
                    values.push(val);
                }
            }
            offsets.push(values.len());
        }

        Self {
            offsets,
            indices,
            values,
            nrows,
            ncols,
        }
    }

    /// Forward projection: `out = A · x`.
    ///
    /// Row-parallel via Rayon when `nrows >= PARALLEL_ROW_THRESHOLD`.
    pub fn forward(&self, x: &DVector<f64>, out: &mut DVector<f64>) {
        assert_eq!(x.len(), self.ncols);
        assert_eq!(out.len(), self.nrows);

        let row_dot = |r: usize| -> f64 {
            let start = self.offsets[r];
            let end = self.offsets[r + 1];
            let mut acc = 0.0;
            for idx in start..end {
                acc = self.values[idx].mul_add(x[self.indices[idx]], acc);
            }
            acc
        };

        if self.nrows >= PARALLEL_ROW_THRESHOLD {
            out.as_mut_slice()
                .par_iter_mut()
                .enumerate()
                .for_each(|(r, slot)| *slot = row_dot(r));
        } else {
            for r in 0..self.nrows {
                out[r] = row_dot(r);
            }
        }
    }

    /// Backward / transpose projection: `out = Aᵀ · y`.
    ///
    /// The CSR row-parallel transpose uses thread-local accumulators,
    /// which is efficient up to moderate `ncols` (~8192). Above that the
    /// allocation overhead dominates; for spectral deconvolution `ncols`
    /// is 1024–4096 so this is optimal.
    pub fn backward(&self, y: &DVector<f64>, out: &mut DVector<f64>) {
        assert_eq!(y.len(), self.nrows);
        assert_eq!(out.len(), self.ncols);

        if self.nrows >= PARALLEL_ROW_THRESHOLD {
            let ncols = self.ncols;
            // Parallel reduction of thread-local dense accumulators
            let combined = (0..self.nrows)
                .into_par_iter()
                .fold(
                    || vec![0.0; ncols],
                    |mut local, r| {
                        let yr = y[r];
                        if yr != 0.0 {
                            let start = self.offsets[r];
                            let end = self.offsets[r + 1];
                            for idx in start..end {
                                local[self.indices[idx]] =
                                    self.values[idx].mul_add(yr, local[self.indices[idx]]);
                            }
                        }
                        local
                    },
                )
                .reduce(
                    || vec![0.0; ncols],
                    |mut a, b| {
                        for (ai, bi) in a.iter_mut().zip(b.iter()) {
                            *ai += bi;
                        }
                        a
                    },
                );

            out.as_mut_slice().copy_from_slice(&combined);
        } else {
            out.fill(0.0);
            for r in 0..self.nrows {
                let yr = y[r];
                if yr == 0.0 {
                    continue;
                }
                let start = self.offsets[r];
                let end = self.offsets[r + 1];
                for idx in start..end {
                    out[self.indices[idx]] = self.values[idx].mul_add(yr, out[self.indices[idx]]);
                }
            }
        }
    }

    /// Column-sum vector: `c = Aᵀ · 1`. Used for pre-computing sensitivities.
    pub fn col_sums(&self) -> DVector<f64> {
        let ones = DVector::from_element(self.nrows, 1.0);
        let mut sums = DVector::zeros(self.ncols);
        self.backward(&ones, &mut sums);
        sums
    }
}

#[inline]
pub fn get_sigma(k: f64, e: f64) -> f64 {
    let resolution = k * e.sqrt();
    resolution * e / FWHM_C
}

/// Row-wise-normalised Gaussian PSF centred on each `axis[i]`, truncated at
/// `nsigma · sigma[i]`. Returns a dense `(n, n)` matrix.
pub fn build_gaussian_response_dense(axis: &[f64], sigma: &[f64], nsigma: f64) -> Array2<f64> {
    let n = axis.len();
    assert_eq!(sigma.len(), n);

    let mut a = Array2::<f64>::zeros((n, n));

    let fill_row = |i: usize, row: &mut [f64]| {
        let e = axis[i];
        let s = sigma[i];
        let xmin = nsigma.mul_add(-s, e);
        let xmax = nsigma.mul_add(s, e);

        // np.searchsorted equivalents (assumes `axis` is sorted ascending)
        let low = axis.partition_point(|&v| v < xmin);
        let high = axis.partition_point(|&v| v <= xmax);

        let mut norm = 0.0;
        for &axis_j in &axis[low..high] {
            let t = (axis_j - e) / s;
            norm += (-0.5 * t * t).exp();
        }
        for j in low..high {
            let t = (axis[j] - e) / s;
            row[j] = (-0.5 * t * t).exp() / norm;
        }
    };

    let flat = a.as_slice_mut().expect("Array2 is contiguous");
    if n >= PARALLEL_ROW_THRESHOLD {
        flat.par_chunks_mut(n)
            .enumerate()
            .for_each(|(i, row)| fill_row(i, row));
    } else {
        for i in 0..n {
            fill_row(i, &mut flat[i * n..(i + 1) * n]);
        }
    }
    a
}

/// Sparse version of [`build_gaussian_response_dense`].
pub fn build_gaussian_response(axis: &[f64], sigma: &[f64], nsigma: f64) -> CsrMatrix {
    CsrMatrix::from_dense(&build_gaussian_response_dense(axis, sigma, nsigma))
}
