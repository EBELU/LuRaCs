//! Energy-interpolation of response matrices.
//!
//! Builds a CSR sparse operator by interpolating reference rows of a
//! pre-computed response matrix to a requested set of peak energies.
//! Port of `process_matrix_cy` and its helpers.
//!
//! Behavioural notes vs. the original Cython:
//! * Out-of-range `requested_indices` (≤ first reference energy, or > last)
//!   yield an **empty** output row. The Cython version relied on
//!   `boundscheck=False` and would silently read out-of-bounds memory.
//! * `CsrMatrix` output uses `usize` offsets/indices. Use
//!   [`CsrMatrix::to_i64_i32`] if you need the original `int64_t`/`int32_t`
//!   NumPy layout.
//! * `ref_E_axis` must be sorted ascending — `find_index_from_energy` relies
//!   on this (same as the Cython source).
//! * The unused `requested_E_axis` parameter on `interpolate_point` is
//!   dropped.
//!
//! When dropped into the same crate as the deconvolution module, delete the
//! local `CsrMatrix` definition and `use` the existing one — the API is
//! identical.

use ndarray::Array2;
use rayon::prelude::*;

use crate::utils::numerics::deconvolution::CsrMatrix;

/// Sparsity filter threshold for emitted response-matrix entries.
pub const SPARSITY_THRESHOLD: f64 = 1e-12;

/// Row-parallel `process_matrix` above this many requested rows.
pub const PARALLEL_OUTER_THRESHOLD: usize = 32;

// =============================================================================
// Scalar helpers
// =============================================================================

/// Index of the first element `>= e`, or `None` if every element is `< e`.
#[inline]
pub fn find_index_from_energy(e: f64, array: &[f64]) -> Option<usize> {
    let mut i = 0;
    while i < array.len() && array[i] < e {
        i += 1;
    }
    if i == array.len() { None } else { Some(i) }
}

#[inline]
pub fn linear_interpolation(x: f64, x1: f64, x2: f64, y1: f64, y2: f64) -> f64 {
    y1 + (x - x1) * (y2 - y1) / (x2 - x1)
}

/// Linear interpolation of `y_array` at position `x` on `x_array`.
/// Returns `NaN` if `x` lies outside the interior of `x_array` (matches the
/// Cython source, which returns `NAN` for `i <= 0` or `i >= len`).
#[inline]
pub fn interpolate_point_on_axis(x: f64, x_array: &[f64], y_array: &[f64]) -> f64 {
    match find_index_from_energy(x, x_array) {
        Some(i) if i > 0 && i < x_array.len() => {
            linear_interpolation(x, x_array[i - 1], x_array[i], y_array[i - 1], y_array[i])
        }
        _ => f64::NAN,
    }
}

/// Compton-backscatter energy for a photon of energy `e_p`.
#[inline]
pub fn get_backscatter_e(e_p: f64) -> f64 {
    e_p / (1.0 + 2.0 * e_p / 511.0)
}

/// Region boundaries used by the interpolation.
///
/// With `has_escape_peaks = false`, the result is returned in the fixed order
/// `[0, backscatter, compton, below_peak, above_peak, max_value]` (ascending
/// for physically reasonable inputs). With escape peaks, the same set plus
/// the single- and double-escape energies is **sorted ascending**, matching
/// `sorted([...])` in the Cython source.
pub fn calculate_regions(e_p: f64, max_value: f64, has_escape_peaks: bool) -> Vec<f64> {
    let backscatter = get_backscatter_e(e_p);
    let compton = e_p - backscatter;
    let below_peak = e_p - 10.0;
    let above_peak = e_p + 10.0;

    if !has_escape_peaks {
        vec![0.0, backscatter, compton, below_peak, above_peak, max_value]
    } else {
        let single_escape = e_p - 511.0;
        let double_escape = e_p - 1022.0;
        let mut r = vec![
            0.0,
            backscatter,
            compton,
            below_peak,
            above_peak,
            single_escape,
            double_escape,
            max_value,
        ];
        r.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        r
    }
}

// =============================================================================
// Per-element / per-row interpolation
// =============================================================================

/// Interpolate the response value for a single (peak-energy, column-energy) pair.
///
/// `j_k` lies on the "peak-energy axis" of regions `e_i_regions`; `e_1`, `e_2`
/// are the bracketing reference peak energies; `y_1`, `y_2` are the
/// normalised reference rows; `e_i` is the peak energy being interpolated to.
#[allow(clippy::too_many_arguments)]
#[inline]
fn interpolate_point(
    j_k: f64,
    e_i: f64,
    e_1: f64,
    e_2: f64,
    e_1_regions: &[f64],
    e_2_regions: &[f64],
    e_i_regions: &[f64],
    ref_e_axis: &[f64],
    y_1: &[f64],
    y_2: &[f64],
) -> f64 {
    // Guard on `k` — the Cython source reads `E_i_regions[k-1]` and
    // `E_i_regions[k]` without bounds checks, so out-of-range `j_k` would
    // read arbitrary memory. We return NaN, which the sparsity filter drops.
    let k = match find_index_from_energy(j_k, e_i_regions) {
        Some(k) if k > 0 && k < e_i_regions.len() => k,
        _ => return f64::NAN,
    };

    let denom = e_i_regions[k] - e_i_regions[k - 1];
    let t = (j_k - e_i_regions[k - 1]) / denom;

    let e1_jk = t * (e_1_regions[k] - e_1_regions[k - 1]) + e_1_regions[k - 1];
    let e2_jk = t * (e_2_regions[k] - e_2_regions[k - 1]) + e_2_regions[k - 1];

    let y1_jk = interpolate_point_on_axis(e1_jk, ref_e_axis, y_1);
    let y2_jk = interpolate_point_on_axis(e2_jk, ref_e_axis, y_2);

    // Interpolate response at the target peak energy between the two reference rows.
    linear_interpolation(e_i, e_2, e_1, y2_jk, y1_jk)
}

/// Fill `row_buffer` with the interpolated response row at peak energy `e_p`,
/// sandwiched between reference rows `low_row` (energy `low_e`) and
/// `high_row` (energy `high_e`). Both reference rows must already be normalised.
#[allow(clippy::too_many_arguments)]
fn interpolate_response_row(
    e_p: f64,
    req_e_axis: &[f64],
    ref_e_axis: &[f64],
    low_e: f64,
    low_row: &[f64],
    high_e: f64,
    high_row: &[f64],
    row_buffer: &mut [f64],
) {
    debug_assert_eq!(row_buffer.len(), req_e_axis.len());

    let has_escape_peaks = low_e > 1022.0;
    let max_req = req_e_axis.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let low_regions = calculate_regions(low_e, max_req, has_escape_peaks);
    let high_regions = calculate_regions(high_e, max_req, has_escape_peaks);

    let n_regions = low_regions.len();
    let mut e_i_regions = Vec::with_capacity(n_regions);
    let t = (e_p - low_e) / (high_e - low_e);
    for k in 0..n_regions {
        e_i_regions.push(t * (high_regions[k] - low_regions[k]) + low_regions[k]);
    }

    for i in 0..req_e_axis.len() {
        row_buffer[i] = interpolate_point(
            req_e_axis[i],
            e_p,
            high_e,
            low_e,
            &high_regions,
            &low_regions,
            &e_i_regions,
            ref_e_axis,
            high_row,
            low_row,
        );
    }
}

// =============================================================================
// Per-row driver
// =============================================================================

#[inline]
fn normalize_row(row: ndarray::ArrayView1<'_, f64>) -> Vec<f64> {
    let sum: f64 = row.iter().sum();
    row.iter().map(|&v| v / sum).collect()
}

/// Interpolate a single output row, returning its `(indices, values)`.
/// An empty result means the requested peak energy was outside the reference range.
fn compute_row(
    req_e: f64,
    resp: &Array2<f64>,
    ref_indices: &[f64],
    ref_e_axis: &[f64],
    req_e_axis: &[f64],
) -> (Vec<usize>, Vec<f64>) {
    // Bracket the requested peak energy in `ref_indices`. `i == 0` would make
    // `ref_indices[i-1]` underflow; `None` means `req_e` exceeds every
    // reference energy. Both are handled as "empty output" here.
    let high_row_i = match find_index_from_energy(req_e, ref_indices) {
        Some(i) if i > 0 && i < ref_indices.len() => i,
        _ => return (Vec::new(), Vec::new()),
    };
    let high_e = ref_indices[high_row_i];
    let low_e = ref_indices[high_row_i - 1];

    let high_row = normalize_row(resp.row(high_row_i));
    let low_row = normalize_row(resp.row(high_row_i - 1));

    let mut row_buffer = vec![0.0_f64; req_e_axis.len()];
    interpolate_response_row(
        req_e,
        req_e_axis,
        ref_e_axis,
        low_e,
        &low_row,
        high_e,
        &high_row,
        &mut row_buffer,
    );

    // NaN-aware normalisation: `np.nansum` treats NaN as 0. Any NaN entries
    // stay NaN (NaN / x = NaN) and are dropped by the sparsity filter below.
    let nansum: f64 = row_buffer.iter().filter(|v| !v.is_nan()).sum();
    for v in row_buffer.iter_mut() {
        *v /= nansum;
    }

    let mut indices = Vec::new();
    let mut values = Vec::new();
    for (j, &v) in row_buffer.iter().enumerate() {
        if v > SPARSITY_THRESHOLD {
            indices.push(j);
            values.push(v);
        }
    }
    (indices, values)
}

// =============================================================================
// Public entry point
// =============================================================================

/// Interpolate `response_matrix` (rows indexed by `ref_indices`, columns by
/// `ref_e_axis`) to the requested peak energies `requested_indices` on the
/// energy axis `requested_e_axis`. The result is a `n_req × n_req_cols` CSR
/// operator; each row is normalised to sum to 1.
pub fn process_matrix(
    response_matrix: &Array2<f64>,
    ref_indices: &[f64],
    ref_e_axis: &[f64],
    requested_indices: &[f64],
    requested_e_axis: &[f64],
) -> CsrMatrix {
    assert_eq!(
        response_matrix.nrows(),
        ref_indices.len(),
        "response rows must match ref_indices"
    );
    assert_eq!(
        response_matrix.ncols(),
        ref_e_axis.len(),
        "response columns must match ref_E_axis"
    );

    let n_req = requested_indices.len();
    let n_col = requested_e_axis.len();

    let compute = |i: usize| -> (Vec<usize>, Vec<f64>) {
        compute_row(
            requested_indices[i],
            response_matrix,
            ref_indices,
            ref_e_axis,
            requested_e_axis,
        )
    };

    // Parallel per-row construction. Rows are fully independent; each writes
    // its own buffers, then the results are merged sequentially to build the
    // CSR offsets (order-preserving `collect`).
    let per_row: Vec<(Vec<usize>, Vec<f64>)> = if n_req >= PARALLEL_OUTER_THRESHOLD {
        (0..n_req).into_par_iter().map(compute).collect()
    } else {
        (0..n_req).map(compute).collect()
    };

    let nnz: usize = per_row.iter().map(|(idx, _)| idx.len()).sum();
    let mut offsets = Vec::with_capacity(n_req + 1);
    let mut indices = Vec::with_capacity(nnz);
    let mut values = Vec::with_capacity(nnz);
    offsets.push(0);
    for (idx_row, val_row) in per_row {
        indices.extend(idx_row);
        values.extend(val_row);
        offsets.push(indices.len());
    }

    CsrMatrix::from_parts(offsets, indices, values, n_col)
}
