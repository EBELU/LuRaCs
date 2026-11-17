use nalgebra::DVector;
use rayon::prelude::*;

use super::csr::{CsrMatrix, build_gaussian_response, get_sigma};

pub const EPSILON: f64 = 1e-12;
/// `2 * sqrt(2 * ln 2)`. `f64::sqrt` isn't `const` on stable, so it is hard-coded.
pub const FWHM_C: f64 = 2.354_820_045_030_949_3;

/// Row-based kernels below this row count run serially.
pub const PARALLEL_ROW_THRESHOLD: usize = 512;
/// Element-wise kernels below this length run serially.
pub const PARALLEL_ELEM_THRESHOLD: usize = 4096;

#[inline]
pub fn compute_ratio(y: &DVector<f64>, estimate: &DVector<f64>, ratio: &mut DVector<f64>) {
    assert_eq!(y.len(), estimate.len());
    assert_eq!(y.len(), ratio.len());

    if y.len() >= PARALLEL_ELEM_THRESHOLD {
        ratio
            .as_mut_slice()
            .par_iter_mut()
            .zip(y.as_slice().par_iter())
            .zip(estimate.as_slice().par_iter())
            .for_each(|((r, &yi), &est)| {
                *r = yi / (est + EPSILON);
            });
    } else {
        for i in 0..y.len() {
            ratio[i] = y[i] / (estimate[i] + EPSILON);
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
        let lap = 2.0f64.mul_add(-x[i], x[i - 1]) + x[i + 1];
        x[i] = lam.mul_add(lap, x[i]);
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
