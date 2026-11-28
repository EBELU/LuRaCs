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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fit_accessors_and_serde() {
        let fit = Fit {
            region_lower: Some(600.0),
            region_upper: Some(720.0),
            lower: 640.0,
            upper: 680.0,
            params: Array1::from_vec(vec![100.0, 661.7, 4.0]),
            param_errs: Array1::from_vec(vec![5.0, 0.2, 0.4]),
            bkg_params: Some(Array1::from_vec(vec![1.0, 2.0])),
            bkg_est_channels: 3,
            g: 10.0,
            b: 2.0,
            n: 8.0,
            peak_area: 8.0,
            g_err: 3.0,
            b_err: 1.0,
            n_err: 3.16,
            peak_area_err: 3.16,
        };

        assert!((fit.a() - 100.0).abs() < 1e-12);
        assert!((fit.mu() - 661.7).abs() < 1e-12);
        assert!((fit.sigma() - 2.0).abs() < 1e-12);
        assert!((fit.fwhm() - 4.709_640_09).abs() < 1e-8);
        assert!((fit.a_err() - 5.0).abs() < 1e-12);
        assert!((fit.mu_err() - 0.2).abs() < 1e-12);
        assert!((fit.sigma_err() - 0.1).abs() < 1e-12);
        assert!((fit.fwhm_err() - 0.235_482_004_5).abs() < 1e-9);

        let json = serde_json::to_string(&fit).unwrap();
        let decoded: Fit = serde_json::from_str(&json).unwrap();
        assert_eq!(fit, decoded);

        // Non-4.0 variance so `2.0 * sigma` != `2.0 + sigma`
        let mut fit9 = fit;
        fit9.params[2] = 9.0;
        assert!((fit9.sigma() - 3.0).abs() < 1e-12);
        assert!((fit9.sigma_err() - (0.4 / 6.0)).abs() < 1e-12);
    }
}
