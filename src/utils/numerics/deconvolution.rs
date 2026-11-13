//! Generalized Richardson–Lucy / ML-EM deconvolution.
//!
//! * 1-D data vectors are `nalgebra::DVector<f64>`.
//! * The dense Gaussian PSF is `ndarray::Array2<f64>`.
//! * The sparse projection operator is a compact hand-rolled CSR.
//! * Row-parallel and element-wise kernels use Rayon, size-gated by the
//!   `PARALLEL_ROW_THRESHOLD` / `PARALLEL_ELEM_THRESHOLD` constants.
//!
//! Behavioural notes vs. the original Cython:
//! * `use_sensitivity` is removed — it was a no-op (sensitivity was applied
//!   unconditionally). It is now always applied.
//! * `reg_tikhonov` is now actually honoured.

use nalgebra::DVector;
use ndarray::Array2;
use rayon::prelude::*;

pub const EPSILON: f64 = 1e-12;
/// `2 * sqrt(2 * ln 2)`. `f64::sqrt` isn't `const` on stable, so it is hard-coded.
pub const FWHM_C: f64 = 2.354_820_045_030_949_3;

/// Row-based kernels below this row count run serially.
pub const PARALLEL_ROW_THRESHOLD: usize = 512;
/// Element-wise kernels below this length run serially.
pub const PARALLEL_ELEM_THRESHOLD: usize = 4096;

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

    /// Build from a dense matrix, skipping exact zeros.
    pub fn from_dense(a: &Array2<f64>) -> Self {
        let (nrows, ncols) = a.dim();
        let mut offsets: Vec<usize> = Vec::with_capacity(nrows + 1);
        let mut indices: Vec<usize> = Vec::new();
        let mut values: Vec<f64> = Vec::new();

        offsets.push(0);
        for i in 0..nrows {
            for j in 0..ncols {
                let v = a[[i, j]];
                if v != 0.0 {
                    indices.push(j);
                    values.push(v);
                }
            }
            offsets.push(values.len());
        }
        Self {
            nrows,
            offsets,
            indices,
            values,
            ncols,
        }
    }

    #[inline]
    pub fn nrows(&self) -> usize {
        self.nrows
    }
    #[inline]
    pub fn ncols(&self) -> usize {
        self.ncols
    }

    /// Sequential `y ← A · x`.
    pub(crate) fn forward_sequential(&self, x: &DVector<f64>, y: &mut DVector<f64>) {
        y.fill(0.0);
        for j in 0..self.nrows {
            let xj = x[j];
            if xj == 0.0 {
                continue;
            }
            for k in self.offsets[j]..self.offsets[j + 1] {
                y[self.indices[k]] += xj * self.values[k];
            }
        }
    }

    /// Row-parallel `y ← A · x` via fold/reduce: each chunk of rows accumulates
    /// into a private dense vector, which are then summed. Cost: `O(chunks × ncols)`
    /// transient memory. Only beneficial for large operators.
    pub(crate) fn forward_parallel(&self, x: &DVector<f64>, y: &mut DVector<f64>) {
        let ncols = self.ncols;
        let offsets = &self.offsets;
        let indices = &self.indices;
        let values = &self.values;
        let xs = x.as_slice();

        let acc = (0..self.nrows)
            .into_par_iter()
            .fold(
                || DVector::zeros(ncols),
                |mut local, j| {
                    let xj = xs[j];
                    if xj != 0.0 {
                        for k in offsets[j]..offsets[j + 1] {
                            local[indices[k]] += xj * values[k];
                        }
                    }
                    local
                },
            )
            .reduce(
                || DVector::zeros(ncols),
                |mut a, b| {
                    a += b;
                    a
                },
            );

        y.copy_from(&acc);
    }

    /// `y ← A · x`.
    #[inline]
    pub fn forward(&self, x: &DVector<f64>, y: &mut DVector<f64>) {
        assert_eq!(x.len(), self.nrows);
        assert_eq!(y.len(), self.ncols);
        if self.nrows >= PARALLEL_ROW_THRESHOLD {
            self.forward_parallel(x, y);
        } else {
            self.forward_sequential(x, y);
        }
    }

    /// `r ← Aᵀ · y`. Each row `j` writes `r[j]` independently — trivially parallel.
    #[inline]
    pub fn backward(&self, y: &DVector<f64>, r: &mut DVector<f64>) {
        assert_eq!(y.len(), self.ncols);
        assert_eq!(r.len(), self.nrows);

        let offsets = &self.offsets;
        let indices = &self.indices;
        let values = &self.values;
        let ys = y.as_slice();

        let work = |j: usize| -> f64 {
            let mut s = 0.0;
            for k in offsets[j]..offsets[j + 1] {
                s += values[k] * ys[indices[k]];
            }
            s
        };

        if self.nrows >= PARALLEL_ROW_THRESHOLD {
            r.as_mut_slice()
                .par_iter_mut()
                .enumerate()
                .for_each(|(j, rj)| *rj = work(j));
        } else {
            for (j, rj) in r.iter_mut().enumerate() {
                *rj = work(j);
            }
        }
    }
}

// =============================================================================
// Element-wise kernels
// =============================================================================

#[inline]
pub fn compute_ratio(y: &DVector<f64>, estimate: &DVector<f64>, ratio: &mut DVector<f64>) {
    let n = y.len();
    assert_eq!(estimate.len(), n);
    assert_eq!(ratio.len(), n);

    if n >= PARALLEL_ELEM_THRESHOLD {
        ratio
            .as_mut_slice()
            .par_iter_mut()
            .zip(y.as_slice().par_iter())
            .zip(estimate.as_slice().par_iter())
            .for_each(|((r, &yi), &ei)| *r = yi / (ei + EPSILON));
    } else {
        for ((r, &yi), &ei) in ratio.iter_mut().zip(y.iter()).zip(estimate.iter()) {
            *r = yi / (ei + EPSILON);
        }
    }
}

#[inline]
pub fn multiply_update(x: &mut DVector<f64>, correction: &DVector<f64>) {
    assert_eq!(x.len(), correction.len());
    if x.len() >= PARALLEL_ELEM_THRESHOLD {
        x.as_mut_slice()
            .par_iter_mut()
            .zip(correction.as_slice().par_iter())
            .for_each(|(xi, &ci)| *xi *= ci);
    } else {
        x.component_mul_assign(correction);
    }
}

#[inline]
pub fn apply_sensitivity(correction: &mut DVector<f64>, sensitivity: &DVector<f64>) {
    assert_eq!(correction.len(), sensitivity.len());
    if correction.len() >= PARALLEL_ELEM_THRESHOLD {
        correction
            .as_mut_slice()
            .par_iter_mut()
            .zip(sensitivity.as_slice().par_iter())
            .for_each(|(c, &s)| *c /= s.max(EPSILON));
    } else {
        for (c, s) in correction.iter_mut().zip(sensitivity.iter()) {
            *c /= s.max(EPSILON);
        }
    }
}

/// Sequential — stencil reads neighbouring cells, so naive parallelisation races.
pub fn tikhonov_smooth(x: &mut DVector<f64>, lam: f64) {
    let n = x.len();
    if n < 3 {
        return;
    }
    for i in 1..n - 1 {
        let lap = x[i - 1] - 2.0 * x[i] + x[i + 1];
        x[i] += lam * lap;
    }
}

/// Not wired into the pipeline — kept for parity with the Cython source.
#[allow(dead_code)]
pub fn entropy_step(x: &mut DVector<f64>, model: &DVector<f64>, lam: f64) {
    for i in 0..x.len() {
        let ratio = x[i] / (model[i] + EPSILON);
        x[i] *= (-lam * (ratio.ln() + 1.0)).exp();
        x[i] = x[i].max(EPSILON);
    }
}

// =============================================================================
// ML-EM
// =============================================================================

#[derive(Clone, Debug)]
pub struct MlEmOptions {
    pub iterations: usize,
    pub reg_tikhonov: bool,
    pub reg_tikhonov_lambda: f64,
}

impl Default for MlEmOptions {
    fn default() -> Self {
        Self {
            iterations: 30,
            reg_tikhonov: false,
            reg_tikhonov_lambda: 1e-3,
        }
    }
}

/// Multiplicative ML-EM update. `matrix` is `n × n`, `sensitivity` has length `n`.
pub fn ml_em(
    y: &DVector<f64>,
    matrix: &CsrMatrix,
    sensitivity: &DVector<f64>,
    opts: &MlEmOptions,
) -> DVector<f64> {
    let n = y.len();
    assert_eq!(matrix.nrows(), n, "matrix rows must equal data length");
    assert_eq!(matrix.ncols(), n, "matrix must be square for ML-EM");
    assert_eq!(sensitivity.len(), n, "sensitivity length must match data");

    let mut x = DVector::from_element(n, 1.0);
    let mut estimate = DVector::zeros(n);
    let mut ratio = DVector::zeros(n);
    let mut correction = DVector::zeros(n);

    for _ in 0..opts.iterations {
        matrix.forward(&x, &mut estimate);
        compute_ratio(y, &estimate, &mut ratio);
        matrix.backward(&ratio, &mut correction);
        apply_sensitivity(&mut correction, sensitivity);
        multiply_update(&mut x, &correction);

        if opts.reg_tikhonov {
            tikhonov_smooth(&mut x, opts.reg_tikhonov_lambda);
        }
    }

    x
}

// =============================================================================
// Richardson–Lucy
// =============================================================================

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
        let xmin = e - nsigma * s;
        let xmax = e + nsigma * s;

        // np.searchsorted equivalents (assumes `axis` is sorted ascending)
        let low = axis.partition_point(|&v| v < xmin);
        let high = axis.partition_point(|&v| v <= xmax);

        let mut norm = 0.0;
        for j in low..high {
            let t = (axis[j] - e) / s;
            norm += (-0.5 * t * t).exp();
        }
        for j in low..high {
            let t = (axis[j] - e) / s;
            row[j] = (-0.5 * t * t).exp() / norm;
        }
    };

    if n >= PARALLEL_ROW_THRESHOLD {
        let flat = a.as_slice_mut().expect("Array2 is contiguous");
        flat.par_chunks_mut(n)
            .enumerate()
            .for_each(|(i, row)| fill_row(i, row));
    } else {
        let flat = a.as_slice_mut().expect("Array2 is contiguous");
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

/// RL = ML-EM with a Gaussian PSF whose per-row σ depends on the source
/// energy `x_axis[i]`. `x_axis.len()` must be `>= y.len()`.
pub fn richardson_lucy(
    x_axis: &DVector<f64>,
    y: &DVector<f64>,
    k: f64,
    sensitivity: &DVector<f64>,
    opts: &MlEmOptions,
) -> DVector<f64> {
    let n = y.len();
    assert!(x_axis.len() >= n, "x_axis must be at least as long as y");

    let sigma = DVector::from_iterator(n, (0..n).map(|i| get_sigma(k, x_axis[i])));
    let matrix = build_gaussian_response(y.as_slice(), sigma.as_slice(), 5.0);
    ml_em(y, &matrix, sensitivity, opts)
}
