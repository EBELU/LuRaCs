use crate::containers::{Fit, nuclide::Emission};
use ndarray::Array1;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumString)]
pub enum FitType {
    None,
    Gaussian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::EnumString)]
pub enum BkgType {
    None,
    Linear,
    Quadratic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roi {
    pub tag: String,
    pub alias: String,
    pub spectrum: String,
    pub roi_bound: (f64, f64),
    pub region_bound: (f64, f64),
    pub fit_type: FitType,
    pub bkg_type: BkgType,
    pub bkg_est_channels: i32,
    pub fit: Option<Fit>,
    pub roi_counts: f64,
    pub live_time: f64,
    pub emission: Option<Emission>,
    pub meta: serde_json::Map<String, serde_json::Value>,
    pub merge: bool,
    pub movable: bool,
    pub poisson_weights: bool,
    pub owner_spectrum: Option<String>,
}

/// Fields `get_count_data` can address. Using an enum kills the runtime
/// `ValueError("Invalid field")` branch at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountField {
    RoiCounts,
    // Scaled by live time when cps == true
    A,
    AErr,
    G,
    GErr,
    B,
    BErr,
    N,
    NErr,
    PeakArea,
    PeakAreaErr,
    // Not scaled (channel-space quantities)
    Mu,
    MuErr,
    Sigma,
    SigmaErr,
    Fwhm,
    FwhmErr,
    Lower,
    Upper,
    RegionLower,
    RegionUpper,
}

impl CountField {
    pub const fn is_count(self) -> bool {
        use CountField::*;
        matches!(
            self,
            RoiCounts | A | AErr | G | GErr | B | BErr | N | NErr | PeakArea | PeakAreaErr
        )
    }
}

impl std::str::FromStr for CountField {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use CountField::*;
        Ok(match s {
            "roi_counts" => RoiCounts,
            "A" => A,
            "A_err" => AErr,
            "G" => G,
            "G_err" => GErr,
            "B" => B,
            "B_err" => BErr,
            "N" => N,
            "N_err" => NErr,
            "peak_area" => PeakArea,
            "peak_area_err" => PeakAreaErr,
            "mu" => Mu,
            "mu_err" => MuErr,
            "sigma" => Sigma,
            "sigma_err" => SigmaErr,
            "fwhm" => Fwhm,
            "fwhm_err" => FwhmErr,
            "lower" => Lower,
            "upper" => Upper,
            "region_lower" => RegionLower,
            "region_upper" => RegionUpper,
            other => return Err(format!("invalid ROI field: {other}")),
        })
    }
}

impl Roi {
    /// Returns `None` when there is nothing meaningful to return
    /// (cps requested but live_time is zero, or fit-dependent field with no fit).
    pub fn get_count_data(&self, field: CountField, cps: bool) -> Option<f64> {
        if cps && self.live_time == 0.0 {
            return None;
        }

        let fit = self.fit.as_ref();

        let raw = match field {
            CountField::RoiCounts => self.roi_counts,

            CountField::A => fit?.a(),
            CountField::AErr => fit?.a_err(),
            CountField::G => fit?.g,
            CountField::GErr => fit?.g_err,
            CountField::B => fit?.b,
            CountField::BErr => fit?.b_err,
            CountField::N => fit?.n,
            CountField::NErr => fit?.n_err,
            CountField::PeakArea => fit?.peak_area,
            CountField::PeakAreaErr => fit?.peak_area_err,

            CountField::Mu => fit?.mu(),
            CountField::MuErr => fit?.mu_err(),
            CountField::Sigma => fit?.sigma(),
            CountField::SigmaErr => fit?.sigma_err(),
            CountField::Fwhm => fit?.fwhm(),
            CountField::FwhmErr => fit?.fwhm_err(),

            CountField::Lower => fit?.lower,
            CountField::Upper => fit?.upper,
            CountField::RegionLower => fit?.region_lower?,
            CountField::RegionUpper => fit?.region_upper?,
        };

        Some(if cps && field.is_count() {
            raw / self.live_time
        } else {
            raw
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_count_field_from_str_and_get_count_data() {
        let all_fields = [
            ("roi_counts", CountField::RoiCounts, true),
            ("A", CountField::A, true),
            ("A_err", CountField::AErr, true),
            ("G", CountField::G, true),
            ("G_err", CountField::GErr, true),
            ("B", CountField::B, true),
            ("B_err", CountField::BErr, true),
            ("N", CountField::N, true),
            ("N_err", CountField::NErr, true),
            ("peak_area", CountField::PeakArea, true),
            ("peak_area_err", CountField::PeakAreaErr, true),
            ("mu", CountField::Mu, false),
            ("mu_err", CountField::MuErr, false),
            ("sigma", CountField::Sigma, false),
            ("sigma_err", CountField::SigmaErr, false),
            ("fwhm", CountField::Fwhm, false),
            ("fwhm_err", CountField::FwhmErr, false),
            ("lower", CountField::Lower, false),
            ("upper", CountField::Upper, false),
            ("region_lower", CountField::RegionLower, false),
            ("region_upper", CountField::RegionUpper, false),
        ];

        for (s, expected, is_cnt) in all_fields {
            let parsed = CountField::from_str(s).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(parsed.is_count(), is_cnt);
        }
        assert!(CountField::from_str("bogus").is_err());

        let fit = Fit {
            region_lower: Some(600.0),
            region_upper: Some(720.0),
            lower: 640.0,
            upper: 680.0,
            params: Array1::from_vec(vec![50.0, 661.7, 4.0]),
            param_errs: Array1::from_vec(vec![2.0, 0.1, 0.2]),
            bkg_params: None,
            bkg_est_channels: 3,
            g: 200.0,
            b: 40.0,
            n: 160.0,
            peak_area: 160.0,
            g_err: 14.0,
            b_err: 6.0,
            n_err: 15.0,
            peak_area_err: 15.0,
        };

        let mut roi = Roi {
            tag: "r1".to_string(),
            alias: "Cs137".to_string(),
            spectrum: "s1".to_string(),
            roi_bound: (640.0, 680.0),
            region_bound: (600.0, 720.0),
            fit_type: FitType::Gaussian,
            bkg_type: BkgType::Linear,
            bkg_est_channels: 3,
            fit: Some(fit),
            roi_counts: 200.0,
            live_time: 10.0,
            emission: None,
            meta: serde_json::Map::new(),
            merge: false,
            movable: true,
            poisson_weights: true,
            owner_spectrum: None,
        };

        for (_, f, is_cnt) in all_fields {
            let raw = roi.get_count_data(f, false).unwrap();
            let cps_val = roi.get_count_data(f, true).unwrap();
            if is_cnt {
                assert!((cps_val - raw / 10.0).abs() < 1e-12);
            } else {
                assert!((cps_val - raw).abs() < 1e-12);
            }
        }

        // Zero live_time -> None when cps == true
        roi.live_time = 0.0;
        assert!(roi.get_count_data(CountField::RoiCounts, true).is_none());

        // No fit -> fit fields return None
        roi.fit = None;
        assert!(roi.get_count_data(CountField::A, false).is_none());
        assert_eq!(roi.get_count_data(CountField::RoiCounts, false), Some(200.0));
    }
}
