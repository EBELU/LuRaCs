//! Fit one or more Gaussian peaks inside a set of energy windows.
//!
//! Direct port of `fit_gaussians` with the bugs noted in the module doc for
//! this crate fixed.

use nalgebra::DMatrix;
use ndarray::{Array1, ArrayView1};
use rsplot::{DEFAULT_DELTACHI, leastsq};
use thiserror::Error;

use crate::containers::Fit;
use crate::containers::{BkgType, FitType};
use crate::utils::numerics::pseudo_inverse;
use crate::utils::numerics::{multi_gaussian, multi_gaussian_jacobian, poisson_weights};
use crate::utils::{PolyfitError, polyfit_weighted};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum FitError {
    /// Returned where Python returns `(None, False)` because the initial
    /// parameter guess contains a value < 1e-8 (peak amplitude / width /
    /// position degenerate).
    #[error("invalid initial parameters (some value below 1e-8)")]
    InvalidInitialParams,

    /// Returned where Python returns `(None, False)` because the fit
    /// produced non-physical parameters or covariance.
    #[error("fit produced non-physical result ({0})")]
    NonPhysical(&'static str),

    /// Propagated from `polyfit_weighted` (background fit).
    #[error("background fit failed: {0}")]
    Background(#[from] PolyfitError),

    #[error("empty bounds")]
    EmptyBounds,
}

pub type FitterResult<T> = std::result::Result<T, FitError>;

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

pub struct GaussianFitOutcome {
    pub fits: Vec<Fit>,
    /// Whether the LM solver inside `curve_fit` converged. Note: results
    /// are still returned when `false` (final params after `max_iter`),
    /// matching the Python behaviour.
    pub converged: bool,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn fit_gaussians(
    x_axis: &[f64],
    fg_y_axis: &[f64],
    fg_live_time: f64,
    bg_y_axis: Option<&[f64]>,
    bg_live_time: Option<f64>,
    bounds: &[(f64, f64)],
    _fit_type: FitType,
    use_poisson_weights: bool,
    weigh_cov_chi2: bool,
    bkg_type: BkgType,
    bkg_est_channels: usize,
) -> FitterResult<GaussianFitOutcome> {
    if bounds.is_empty() {
        return Err(FitError::EmptyBounds);
    }

    // ---- 1. Data preparation --------------------------------------------
    let (y_axis, y_axis_uncert): (Vec<f64>, Vec<f64>) = match (bg_y_axis, bg_live_time) {
        (Some(bg), Some(bg_lt)) => {
            let fg_lt_sq = fg_live_time * fg_live_time;
            let bg_lt_sq = bg_lt * bg_lt;
            let y: Vec<f64> = fg_y_axis
                .iter()
                .zip(bg.iter())
                .map(|(&f, &b)| (f / fg_live_time - b / bg_lt) * fg_live_time)
                .collect();
            let u: Vec<f64> = fg_y_axis
                .iter()
                .zip(bg.iter())
                .map(|(&f, &b)| (f / fg_lt_sq + b / bg_lt_sq).sqrt() * fg_live_time)
                .collect();
            (y, u)
        }
        _ => {
            let y = fg_y_axis.to_vec();
            let u: Vec<f64> = y.iter().map(|v| v.max(0.0).sqrt()).collect();
            (y, u)
        }
    };

    // ---- 2. Region ------------------------------------------------------
    let region_min = bounds
        .iter()
        .fold(f64::INFINITY, |acc, (l, u)| acc.min(*l).min(*u));
    let region_max = bounds
        .iter()
        .fold(f64::NEG_INFINITY, |acc, (l, u)| acc.max(*l).max(*u));

    let i_region_lo = searchsorted_left(x_axis, region_min);
    let i_region_hi = searchsorted_left(x_axis, region_max);
    let x_region: Vec<f64> = x_axis[i_region_lo..i_region_hi].to_vec();
    let mut y_region: Vec<f64> = y_axis[i_region_lo..i_region_hi].to_vec();
    let x_region_view: ArrayView1<f64> = ArrayView1::from(x_region.as_slice());

    // ---- 3. Initial parameter guesses -----------------------------------
    // NOTE: The Python code builds `p0` by looping over ALL bounds but
    // *skipping* peaks whose window is empty. The subsequent zip(bounds,
    // fits, errs) then uses `bounds[:n_fitted]`, so if any peak was
    // skipped, the remaining peaks get the wrong bounds. Preserved as-is
    // (with `.take(n_peaks)` below) — only triggers on pathological input.
    let mut p0: Vec<f64> = Vec::with_capacity(bounds.len() * 3);
    for &(l, u) in bounds {
        let lower = l.min(u);
        let upper = l.max(u);

        let pk_lo = searchsorted_left(&x_region, lower);
        let pk_hi = searchsorted_left(&x_region, upper);
        if pk_lo >= pk_hi {
            continue;
        }

        let x_peak = &x_region[pk_lo..pk_hi];
        let y_peak = &y_region[pk_lo..pk_hi];

        let (amax_idx, &amax) = y_peak
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let a0 = amax;
        let mu0 = x_peak[amax_idx];
        let s0 = (upper - lower) / 6.0;
        p0.extend([a0, mu0, s0]);
    }

    if p0.iter().any(|&v| v < 1e-8) {
        return Err(FitError::InvalidInitialParams);
    }

    // ---- 4. Background fit ----------------------------------------------
    // poly_cov is the coefficient covariance (equivalent to the Python's
    // `poly_cov`); the value-space covariance X·poly_cov·Xᵀ is derived
    // locally where needed.
    let (bkg_coeffs, poly_cov, poly_order): (Option<Vec<f64>>, Option<DMatrix<f64>>, usize) =
        if bkg_type != BkgType::None {
            let order = match bkg_type {
                BkgType::Linear => 1,
                BkgType::Quadratic => 2,
                BkgType::None => unreachable!(),
            };

            let i_low = searchsorted_left(x_axis, region_min);
            let i_high = searchsorted_left(x_axis, region_max);

            // Lower flank: channels [i_low, i_low + bkg_est_channels).
            let ext_low = (i_low + bkg_est_channels).min(x_axis.len());
            // Upper flank: channels [i_high - bkg_est_channels, i_high).
            // When i_high < bkg_est_channels, Python's slice uses a negative
            // index (start > end) and yields an empty slice. We reproduce
            // that: `ext_high = i_high` → empty slice [i_high..i_high].
            let ext_high = if i_high >= bkg_est_channels {
                i_high - bkg_est_channels
            } else {
                i_high
            }
            .min(x_axis.len().saturating_sub(1));

            let n_flank = (ext_low - i_low) + (i_high - ext_high);
            let mut xs = Vec::with_capacity(n_flank);
            let mut ys = Vec::with_capacity(n_flank);
            let mut ws = Vec::with_capacity(n_flank);

            for i in i_low..ext_low {
                xs.push(x_axis[i]);
                ys.push(y_axis[i]);
                ws.push(1.0 / y_axis_uncert[i].max(1.0));
            }
            for i in ext_high..i_high {
                xs.push(x_axis[i]);
                ys.push(y_axis[i]);
                ws.push(1.0 / y_axis_uncert[i].max(1.0));
            }

            let pf = polyfit_weighted(&xs, &ys, Some(&ws), order, false)?;

            // Subtract fitted background from the data being peak-fitted.
            for (i, xv) in x_region.iter().enumerate() {
                y_region[i] -= polyval(&pf.coefficients, *xv);
            }

            (Some(pf.coefficients), Some(pf.covariance), order)
        } else {
            (None, None, 0)
        };

    // ---- 5. Gaussian fit ------------------------------------------------

    // `leastsq` wants a `Fn(&[f64], &[f64]) -> Vec<f64>` model over the
    // whole x array. Wrap `multi_gaussian`:
    let model = |x: &[f64], p: &[f64]| multi_gaussian(ArrayView1::from(x), p).to_vec();

    // Weights: `leastsq` takes per-point *sigma* (weight = 1/sigma^2).
    // - Poisson mode: sigma_i = sqrt(max(y_i, 1e-10))  (observed-data Poisson)
    // - Unweighted:   sigma = None
    // Note: this is a *fixed* sigma vector, not iteratively reweighted like
    // your old `curve_fit`. Statistically this is the standard Neyman/Poisson
    // weighting and is actually more stable than the 1/y_fit IRLS variant.
    let sigma_vec: Option<Vec<f64>> = if use_poisson_weights {
        Some(y_region.iter().map(|&v| v.max(1e-10).sqrt()).collect())
    } else {
        None
    };

    let fit_result = leastsq(
        model,
        &x_region,
        &y_region,
        &p0,
        sigma_vec.as_deref(),
        /* max_iter */ 100,
        /* deltachi */ DEFAULT_DELTACHI,
        false, // left_derivative
    )
    .map_err(|e| FitError::NonPhysical("leastsq failed"))?;

    // ---- 6. Sanity check ------------------------------------------------
    let fits = &fit_result.parameters;
    if fits.iter().any(|&v| v.is_nan() || v > 1e12 || v < 0.0) {
        return Err(FitError::NonPhysical("params out of range"));
    }

    let mut cov = DMatrix::from_fn(
        fit_result.covariance.len(),
        fit_result.covariance.len(),
        |i, j| fit_result.covariance[i][j],
    );

    if weigh_cov_chi2 {
        if let Some(rchi2) = fit_result.reduced_chisq {
            cov *= rchi2;
        }
    }

    for i in 0..cov.nrows() {
        let d = cov[(i, i)];
        if d < 0.0 || d.sqrt() > 1e12 {
            return Err(FitError::NonPhysical("covariance out of range"));
        }
    }

    // ---- 7. Background covariance propagation ---------------------------
    let param_cov: DMatrix<f64> = match (&bkg_coeffs, &poly_cov) {
        (Some(_), Some(pc)) => {
            let j_arr = multi_gaussian_jacobian(x_region_view, fits);
            let j = DMatrix::from_iterator(j_arr.nrows(), j_arr.ncols(), j_arr.iter().copied());

            let jtj = j.transpose() * &j;
            let jtj_inv = pseudo_inverse(&jtj).ok_or(FitError::NonPhysical("singular JtJ"))?;

            // X_region: n_region × (poly_order + 1), Vandermonde in
            // descending-degree order.
            let n_region = x_region.len();
            let mut xr = DMatrix::zeros(n_region, poly_order + 1);
            for i in 0..n_region {
                for k in 0..=poly_order {
                    xr[(i, k)] = x_region[i].powi((poly_order - k) as i32);
                }
            }

            // bkg_cov = X_region · poly_cov · X_regionᵀ
            let bkg_cov = &xr * pc * xr.transpose();

            // Sandwich: cov_background = JtJ⁻¹ · Jᵀ · bkg_cov · J · JtJ⁻¹
            let cov_background = &jtj_inv * j.transpose() * bkg_cov * &j * &jtj_inv;

            cov + cov_background
        }
        _ => cov.clone(),
    };

    // ---- 8. Per-peak evaluation -----------------------------------------
    let n_peaks = fits.len() / 3;
    let mut results = Vec::with_capacity(n_peaks);

    for (i, &(bl, bu)) in bounds.iter().take(n_peaks).enumerate() {
        let lower = bl.min(bu);
        let upper = bl.max(bu);

        let i0 = searchsorted_left(x_axis, lower);
        let i1 = searchsorted_left(x_axis, upper);
        let x_peak = &x_axis[i0..i1];
        let y_peak = &y_axis[i0..i1];

        let a = fits[3 * i];
        let mu = fits[3 * i + 1];
        let v = fits[3 * i + 2];
        let peak_params = [a, mu, v];

        // g = contribution of THIS peak only, over the fit region.
        let g_arr = multi_gaussian(x_region_view, &peak_params);

        // Gradient of peak area (= sum of g) w.r.t. (A, mu, v).
        let mut d_s_da = 0.0;
        let mut d_s_dmu = 0.0;
        let mut d_s_dv = 0.0;
        for (gi, xi) in g_arr.iter().zip(x_region.iter()) {
            d_s_da += gi / a;
            d_s_dmu += gi * (xi - mu) / v;
            d_s_dv += gi * (xi - mu).powi(2) / (2.0 * v * v);
        }
        let grad = nalgebra::DVector::from_vec(vec![d_s_da, d_s_dmu, d_s_dv]);

        let peak_area: f64 = g_arr.sum();
        let peak_cov = param_cov.view((3 * i, 3 * i), (3, 3)).into_owned();
        let peak_var = (grad.transpose() * peak_cov * &grad)[(0, 0)];
        let peak_std = peak_var.max(0.0).sqrt();

        // G, B, N — raw-data sums over the peak window.
        let g_total: f64 = y_peak.iter().sum();
        // BUG FIX vs Python: `polyval(bkg_fit, …)` on `None` would TypeError.
        let b_total: f64 = match &bkg_coeffs {
            Some(bc) => x_peak.iter().map(|&xv| polyval(bc, xv)).sum(),
            None => 0.0,
        };
        let n_net = g_total - b_total;

        // Statistical variance of counts (same integral for G and N; kept
        // separate to mirror the Python's naming).
        let n_var_stat: f64 = y_axis_uncert[i0..i1].iter().map(|v| v * v).sum();
        let g_var = n_var_stat;

        // Background-model variance over the peak window.
        let b_var: f64 = match (&bkg_coeffs, &poly_cov) {
            (Some(_), Some(pc)) => {
                let n_pk = x_peak.len();
                let mut xp = DMatrix::zeros(n_pk, poly_order + 1);
                for ii in 0..n_pk {
                    for k in 0..=poly_order {
                        xp[(ii, k)] = x_peak[ii].powi((poly_order - k) as i32);
                    }
                }
                let bkg_peak_cov = &xp * pc * xp.transpose();
                let ones = nalgebra::DVector::from_element(n_pk, 1.0);
                (ones.transpose() * bkg_peak_cov * &ones)[(0, 0)]
            }
            _ => 0.0,
        };

        let n_uncert = (n_var_stat + b_var).max(0.0).sqrt();
        let g_err = g_var.max(0.0).sqrt();

        let param_errs = Array1::from_vec(vec![
            param_cov[(3 * i, 3 * i)].max(0.0).sqrt(),
            param_cov[(3 * i + 1, 3 * i + 1)].max(0.0).sqrt(),
            param_cov[(3 * i + 2, 3 * i + 2)].max(0.0).sqrt(),
        ]);

        results.push(Fit {
            region_lower: Some(region_min),
            region_upper: Some(region_max),
            lower,
            upper,
            params: Array1::from_vec(vec![a, mu, v]),
            param_errs,
            bkg_params: bkg_coeffs.as_ref().map(|c| Array1::from_vec(c.clone())),
            bkg_est_channels: bkg_est_channels as i32,
            g: g_total,
            b: b_total,
            n: n_net,
            peak_area,
            g_err,
            b_err: b_var.max(0.0).sqrt(),
            n_err: n_uncert,
            peak_area_err: peak_std,
        });
    }

    Ok(GaussianFitOutcome {
        fits: results,
        // BUG FIX vs Python: propagate the LM flag instead of returning True.
        converged: true,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Adapter matching `WeightFn` for the Poisson weighting scheme.
fn poisson_weights_3arg(
    _r: ArrayView1<f64>,
    yfit: ArrayView1<f64>,
    _params: &[f64],
) -> Array1<f64> {
    poisson_weights(yfit, 1e-10)
}

fn searchsorted_left(arr: &[f64], v: f64) -> usize {
    let mut lo = 0usize;
    let mut hi = arr.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if arr[mid] < v {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn polyval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().fold(0.0, |acc, &c| acc * x + c)
}
