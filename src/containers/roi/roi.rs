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
    pub fn is_count(self) -> bool {
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
