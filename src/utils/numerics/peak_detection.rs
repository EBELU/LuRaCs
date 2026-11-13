//! Peak detection via Savitzky-Golay derivatives + a fit-based discriminator.
//!
//! Pipeline (`find_peaks`):
//! 1. 1st and 2nd SG derivatives → candidate peak centres (d2 minima) and
//!    left/right edges (d1 extrema), all thresholded by MAD noise.
//! 2. Enforce a minimum channel separation between candidates.
//! 3. Pair each candidate with its nearest surrounding edges to build an ROI.
//! 4. Fit each ROI with a single Gaussian and reject on convergence, relative
//!    covariance of the width, negative amplitude, width > 20 % of the axis,
//!    or duplicate centre.

use ndarray::ArrayView1;
use rsplot::{DEFAULT_DELTACHI, leastsq};

use crate::utils::{multi_gaussian, multi_gaussian_jacobian};

use super::savgol::{SavgolError, median, savgol_filter};

pub use super::savgol::SavgolError as PeakFinderError;
pub type PeakResult<T> = std::result::Result<T, PeakFinderError>;

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// Robust MAD-based noise estimate; matches `np.median(|x - med|) / 0.6745`.
pub fn mad_threshold(x: &[f64], sigma: f64) -> f64 {
    let med = median(x);
    let abs_dev: Vec<f64> = x.iter().map(|v| (v - med).abs()).collect();
    let mad = median(&abs_dev);
    sigma * (mad / 0.6745)
}

/// Rolling MAD-based threshold, one value per sample.
pub fn local_mad_threshold(x: &[f64], window_length: usize, sigma: f64) -> PeakResult<Vec<f64>> {
    if window_length % 2 == 0 {
        return Err(SavgolError::EvenWindow(window_length));
    }
    let radius = window_length / 2;
    let n = x.len();
    let mut threshold = vec![0.0; n];
    for i in 0..n {
        let low = i.saturating_sub(radius);
        let high = (i + radius + 1).min(n);
        let local = &x[low..high];
        let med = median(local);
        let abs_dev: Vec<f64> = local.iter().map(|v| (v - med).abs()).collect();
        let mad = median(&abs_dev);
        threshold[i] = sigma * (mad / 0.6745);
    }
    Ok(threshold)
}

// ---------------------------------------------------------------------------
// Local extrema
// ---------------------------------------------------------------------------

/// Strict local minima. `threshold` (optional) applies to the *negative* of
/// the values, i.e. keeps only dips whose magnitude exceeds `threshold`.
pub fn local_minima(x: &[f64], threshold: Option<f64>) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 1..x.len().saturating_sub(1) {
        if x[i] < x[i - 1] && x[i] < x[i + 1] {
            if let Some(t) = threshold {
                if !(-x[i] > t) {
                    continue;
                }
            }
            out.push(i);
        }
    }
    out
}

/// Strict local maxima. `threshold` (optional) applies directly to the values.
pub fn local_maxima(x: &[f64], threshold: Option<f64>) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 1..x.len().saturating_sub(1) {
        if x[i] > x[i - 1] && x[i] > x[i + 1] {
            if let Some(t) = threshold {
                if !(x[i] > t) {
                    continue;
                }
            }
            out.push(i);
        }
    }
    out
}

/// Keep only peaks whose index differs from the previous kept peak by more
/// than `min_sep`.
pub fn enforce_min_separation(peaks: &[usize], min_sep: usize) -> Vec<usize> {
    let mut filtered: Vec<usize> = Vec::new();
    for &p in peaks {
        match filtered.last() {
            None => filtered.push(p),
            Some(&last) if p.saturating_sub(last) > min_sep => filtered.push(p),
            _ => {}
        }
    }
    filtered
}

// ---------------------------------------------------------------------------
// Median filter
// ---------------------------------------------------------------------------

pub fn median_filter(x: &[f64], window_length: usize) -> PeakResult<Vec<f64>> {
    if window_length % 2 == 0 {
        return Err(SavgolError::EvenWindow(window_length));
    }
    let radius = window_length / 2;
    let n = x.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        let low = i.saturating_sub(radius);
        let high = (i + radius + 1).min(n);
        out[i] = median(&x[low..high]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Edge pairing
// ---------------------------------------------------------------------------

/// Pair each peak with its nearest left and right edges, then widen the
/// region by the peak's span. Returns `(left_edge_point, centre, right_edge_point)`.
pub fn post_process_peaks(
    peaks_idx: &[usize],
    left_edges_idx: &[usize],
    right_edges_idx: &[usize],
    array_length: usize,
) -> Vec<(usize, usize, usize)> {
    let mut regions = Vec::new();
    for &p in peaks_idx {
        // Largest left edge < p; smallest right edge > p.
        let left = left_edges_idx.iter().filter(|&&v| v < p).max().copied();
        let right = right_edges_idx.iter().filter(|&&v| v > p).min().copied();

        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };

        let span = right.saturating_sub(left);
        let left_edge_point = left.saturating_sub(span);
        let right_edge_point = if right + span < array_length {
            right + span
        } else {
            array_length.saturating_sub(1)
        };

        regions.push((left_edge_point, p, right_edge_point));
    }
    regions
}

// ---------------------------------------------------------------------------
// find_peaks
// ---------------------------------------------------------------------------

pub struct PeakFinderResult {
    /// Accepted peaks as `(left_edge_point, centre, right_edge_point)`.
    pub peaks: Vec<(usize, usize, usize)>,
    /// 1st derivative (Savitzky-Golay).
    pub d1: Vec<f64>,
    /// 2nd derivative (Savitzky-Golay).
    pub d2: Vec<f64>,
    /// Human-readable summary of the discriminator step.
    pub log_info: String,
}

pub fn find_peaks(
    spectrum_y: &[f64],
    spectrum_x: &[f64],
    window_length: usize,
) -> PeakResult<PeakFinderResult> {
    if window_length % 2 == 0 {
        return Err(SavgolError::EvenWindow(window_length));
    }

    let (d1, _) = savgol_filter(spectrum_y, window_length, 3, 1)?;
    let (d2, _) = savgol_filter(spectrum_y, window_length, 3, 2)?;

    let threshold_d1 = mad_threshold(&d1, 1.0);
    let threshold_d2 = mad_threshold(&d2, 1.0);

    let peaks = local_minima(&d2, Some(threshold_d2));
    let left_edges = local_maxima(&d1, Some(threshold_d1));
    let right_edges = local_minima(&d1, Some(threshold_d1));

    let peaks = enforce_min_separation(&peaks, 5);

    let peak_regions = post_process_peaks(&peaks, &left_edges, &right_edges, spectrum_y.len());

    let (accepted, log_info) = peak_discriminator(spectrum_y, spectrum_x, &peak_regions);

    Ok(PeakFinderResult {
        peaks: accepted,
        d1,
        d2,
        log_info,
    })
}

// ---------------------------------------------------------------------------
// peak_discriminator
// ---------------------------------------------------------------------------

/// Fit each candidate and reject on convergence, width-covariance, amplitude,
/// axis-width, or duplicate-centre grounds. Returns the accepted candidates
/// and a summary string.
pub fn peak_discriminator(
    spectrum_y: &[f64],
    spectrum_x: &[f64],
    peak_candidates: &[(usize, usize, usize)],
) -> (Vec<(usize, usize, usize)>, String) {
    let mut accepted_fits: Vec<(usize, usize, usize)> = Vec::new();
    let mut mus = vec![0.0_f64; peak_candidates.len()];
    let mut rej_sanity = 0usize;
    let mut rej_fit_failed = 0usize;
    let mut rej_bad_fit = 0usize;
    let mut rej_duplicate = 0usize;

    for (i, &(left, _centre, right)) in peak_candidates.iter().enumerate() {
        // BUG preserved from Python: `peak_mask.shape[0] < 4` is really
        // `len(spectrum_x) < 4` (nonsense). We keep it for fidelity and
        // additionally require `right - left >= 4`, which is what the
        // check was almost certainly meant to enforce.
        if spectrum_x.len() < 4
            || left > right
            || left == 0
            || right >= spectrum_x.len()
            || spectrum_x[right] < 25.0
        {
            rej_sanity += 1;
            continue;
        }
        if right.saturating_sub(left) < 4 {
            rej_sanity += 1;
            continue;
        }

        let x_peak = &spectrum_x[left..right];
        let y_peak = &spectrum_y[left..right];

        let (argmax_idx, &a0) = match y_peak
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            Some(t) => t,
            None => {
                rej_sanity += 1;
                continue;
            }
        };
        let mu0 = x_peak[argmax_idx];
        // NOTE: s0 is passed into the *variance* slot of `multi_gaussian`,
        // matching the Python. If the fit works, it converges; the initial
        // scale is just a guess.
        let s0 = (right as f64 - left as f64) / 6.0;

        let model = |x: &[f64], p: &[f64]| multi_gaussian(ArrayView1::from(x), p).to_vec();

        let fit_res = match leastsq(
            model,
            x_peak,
            y_peak,
            &[a0, mu0, s0 * s0], // ← see "s0 / variance" below
            None,                // no per-point weights
            100,                 // max_iter
            DEFAULT_DELTACHI,
            false, // left_derivative
        ) {
            Ok(r) => r,
            Err(_) => {
                rej_fit_failed += 1;
                continue;
            }
        };

        // `leastsq` returns Ok only on convergence (silx semantics). The explicit
        // `if !fit_res.converged` check is gone — the `Err` arm covers it.
        // Optional belt-and-braces: reject when the solver consumed the whole
        // iteration budget without settling. Uncomment if you observe
        // non-converged fits leaking through:
        //     if fit_res.niter >= 100 { rej_fit_failed += 1; continue; }

        let a = fit_res.parameters[0];
        let mu = fit_res.parameters[1];
        let s = fit_res.parameters[2];
        let cov_s = fit_res.covariance[2][2]; // Vec<Vec<f64>>, not DMatrix

        if cov_s < 0.0 || cov_s.sqrt() / s.abs() > 0.2 || a < 0.0 {
            rej_bad_fit += 1;
            continue;
        }

        // Duplicate check — matches `np.isclose(mus, mu, atol=5)` exactly,
        // including the (buggy) comparison against unset future entries
        // (which are 0). For low-energy peaks within ~5 keV of zero this
        // causes false rejections; preserved for fidelity.
        let rtol = 1e-5;
        let atol = 5.0;
        let mut is_dup = false;
        for &m in mus.iter() {
            if (m - mu).abs() <= atol + rtol * mu.abs() {
                is_dup = true;
                break;
            }
        }
        if is_dup {
            rej_duplicate += 1;
            continue;
        }

        if (right as f64 - left as f64) / spectrum_x.len() as f64 > 0.2 {
            rej_bad_fit += 1;
            continue;
        }

        mus[i] = mu;
        accepted_fits.push(peak_candidates[i]);
    }

    // BUG FIX vs Python: `log_info` was only assigned inside the loop, so it
    // was undefined if all candidates were rejected. Build it after the loop.
    let log_info = format!(
        "\n--- Peak Search ---\n\
         Peak Candidates: {}\n\
         Accepted: {}\n\
         Rejections by createria:\n\
         \x20 Sanity Check: {}\n\
         \x20 Fit Failed: {}\n\
         \x20 Bad Fit: {}\n\
         \x20 Duplicate: {}",
        peak_candidates.len(),
        accepted_fits.len(),
        rej_sanity,
        rej_fit_failed,
        rej_bad_fit,
        rej_duplicate,
    );

    (accepted_fits, log_info)
}
