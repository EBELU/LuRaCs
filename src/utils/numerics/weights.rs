//! Robust and statistical weight functions for least-squares fitting.
//!
//! These match the NumPy semantics of the originals exactly, including the
//! NaN-handling of `np.maximum` (which differs from `f64::max` in Rust).

use ndarray::{Array1, ArrayView1};

// ---------------------------------------------------------------------------
// Huber
// ---------------------------------------------------------------------------

/// Huber weights: `1.0` for small residuals, `delta / |r|` for large ones.
///
/// Matches `np.where(np.abs(r) < delta, 1.0, delta / np.abs(r))` for every
/// input, including `r == 0` (weight `1`) and `r == ±delta` (weight `1`).
/// For `r = NaN`, `NaN < delta` is false so the formula evaluates to
/// `delta / NaN = NaN` — identical to the Python.
pub fn huber_weights(r: ArrayView1<f64>, delta: f64) -> Array1<f64> {
    r.mapv(|ri| {
        let abs_r = ri.abs();
        if abs_r < delta { 1.0 } else { delta / abs_r }
    })
}

// ---------------------------------------------------------------------------
// Poisson
// ---------------------------------------------------------------------------

/// Poisson weights: `1 / max(yfit, eps)`.
///
/// Mimics `np.maximum(yfit, eps)`, which propagates `NaN` (unlike Rust's
/// `f64::max`, which would substitute `eps`). Matches the Python exactly on
/// all inputs, including `NaN` and `±∞`.
pub fn poisson_weights(yfit: ArrayView1<f64>, eps: f64) -> Array1<f64> {
    yfit.mapv(|y| {
        let clamped = if y.is_nan() {
            f64::NAN
        } else if y > eps {
            y
        } else {
            eps
        };
        1.0 / clamped
    })
}

// ---------------------------------------------------------------------------
// Callback adapter (only if you need the Python-shaped signature)
// ---------------------------------------------------------------------------

/// A weight provider matching the Python callback shape:
/// `(residuals, yfit, params) -> weights`.
///
/// Implement this trait if you have code (e.g. an optimizer port) that
/// expects every weight function to accept all three arguments regardless of
/// which it actually uses. Otherwise call `huber_weights` / `poisson_weights`
/// directly.
pub trait WeightProvider {
    fn weights(
        &self,
        residuals: ArrayView1<f64>,
        yfit: ArrayView1<f64>,
        params: &[f64],
    ) -> Array1<f64>;
}

pub struct Huber {
    pub delta: f64,
}

impl Default for Huber {
    fn default() -> Self {
        Self { delta: 1.0 }
    }
}

impl WeightProvider for Huber {
    fn weights(
        &self,
        residuals: ArrayView1<f64>,
        _yfit: ArrayView1<f64>,
        _params: &[f64],
    ) -> Array1<f64> {
        huber_weights(residuals, self.delta)
    }
}

pub struct Poisson {
    pub eps: f64,
}

impl Default for Poisson {
    fn default() -> Self {
        Self { eps: 1e-10 }
    }
}

impl WeightProvider for Poisson {
    fn weights(
        &self,
        _residuals: ArrayView1<f64>,
        yfit: ArrayView1<f64>,
        _params: &[f64],
    ) -> Array1<f64> {
        poisson_weights(yfit, self.eps)
    }
}
