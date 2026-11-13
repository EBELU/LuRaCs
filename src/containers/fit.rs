use ndarray::Array1;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub region_lower: Option<f64>,
    pub region_upper: Option<f64>,
    pub lower: f64,
    pub upper: f64,

    pub params: Array1<f64>,
    pub param_errs: Array1<f64>,

    pub bkg_params: Option<Array1<f64>>,
    pub bkg_est_channels: i32,

    // --- Assumed Gaussian ---
    pub g: f64,
    pub b: f64,
    pub n: f64,
    pub peak_area: f64,
    pub g_err: f64,
    pub b_err: f64,
    pub n_err: f64,
    pub peak_area_err: f64,
}

impl Fit {
    // Python `A` (amplitude) = params[0]
    pub fn a(&self) -> f64 {
        self.params[0]
    }
    pub fn mu(&self) -> f64 {
        self.params[1]
    }
    pub fn sigma(&self) -> f64 {
        self.params[2].sqrt()
    }
    pub fn fwhm(&self) -> f64 {
        2.354_820_045 * self.params[2].sqrt()
    }

    pub fn a_err(&self) -> f64 {
        self.param_errs[0]
    }
    pub fn mu_err(&self) -> f64 {
        self.param_errs[1]
    }
    pub fn sigma_err(&self) -> f64 {
        self.param_errs[2] / (2.0 * self.sigma())
    }
    pub fn fwhm_err(&self) -> f64 {
        2.354_820_045 * self.sigma_err()
    }
}
