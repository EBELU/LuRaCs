use super::physure_helper::opt_vec_quantity_serde;
use chrono::{DateTime, Utc};
use ndarray::Array2;
use physure_core::quantity::Quantity;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use crate::utils::EfficiencySignature;
use crate::utils::ResolutionSignature;

// ---------------------------------------------------------------------------
// GenericInstrument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GenericInstrument {
    pub model: String,
    pub manufacturer: String,
    pub detector_type: String,

    pub detector_material: Option<String>,
    pub detector_shape: Option<String>,

    /// Dimensions in cm; each entry carries its own uncertainty.
    #[serde(with = "opt_vec_quantity_serde::cm")]
    pub detector_dimensions_cm: Option<Vec<Quantity>>,

    // --- Resolution ---
    pub resolution_fn: Option<ResolutionSignature>,
    pub resolution_params: Option<Vec<f64>>,
    pub resolution_e_points: Option<Vec<f64>>,

    /// FWHM values; each carries its own uncertainty.
    #[serde(with = "opt_vec_quantity_serde::dimensionless")]
    pub resolution_fwhm_points: Option<Vec<Quantity>>,

    pub resolution_created: Option<DateTime<Utc>>,

    // --- Efficiency ---
    pub int_efficiency_fn: Option<EfficiencySignature>,

    /// Polynomial parameters; each carries its own uncertainty.
    #[serde(with = "opt_vec_quantity_serde::dimensionless")]
    pub int_efficiency_params: Option<Vec<Quantity>>,

    pub int_efficiency_e_points: Option<Vec<f64>>,

    /// Efficiency points; each carries its own uncertainty.
    #[serde(with = "opt_vec_quantity_serde::dimensionless")]
    pub int_efficiency_eff_points: Option<Vec<Quantity>>,

    pub int_efficiency_created: Option<DateTime<Utc>>,
    pub int_efficiency_description: Option<String>,

    // --- Response Matrix ---
    pub response_matrix: Option<Array2<f64>>,
    pub response_matrix_shape: Option<[usize; 2]>,

    pub remark: String,
}

impl GenericInstrument {
    /// Python's `get_copy` → `.clone()`.
    pub fn get_copy(&self) -> Self {
        self.clone()
    }
}

// ---------------------------------------------------------------------------
// UniqueInstrument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UniqueInstrument {
    pub name: String,

    pub calibration_poly_order: Option<u32>,
    pub calibration_coefficients: Option<Vec<f64>>,
    pub calibration_energy_points: Option<Vec<f64>>,
    pub calibration_channel_points: Option<Vec<f64>>,
    pub calibration_date: Option<DateTime<Utc>>,

    #[serde(flatten)]
    pub generic: GenericInstrument,
}

impl UniqueInstrument {
    /// Mirrors Python's `UniqueInstrument('Name', **template.get_copy().__dict__)`.
    pub fn from_template(name: impl Into<String>, template: &GenericInstrument) -> Self {
        Self {
            name: name.into(),
            calibration_poly_order: None,
            calibration_coefficients: None,
            calibration_energy_points: None,
            calibration_channel_points: None,
            calibration_date: None,
            generic: template.clone(),
        }
    }
}

impl Deref for UniqueInstrument {
    type Target = GenericInstrument;
    fn deref(&self) -> &Self::Target {
        &self.generic
    }
}

impl DerefMut for UniqueInstrument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.generic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::physure_helper::{cm, dimensionless};

    #[test]
    fn test_generic_and_unique_instrument_lifecycle_and_serde() {
        let generic = GenericInstrument {
            model: "NaI-3x3".to_string(),
            manufacturer: "Acme".to_string(),
            detector_type: "Scintillator".to_string(),
            detector_dimensions_cm: Some(vec![Quantity::new_scalar(7.62, 0.1, cm(), None, None)]),
            resolution_fwhm_points: Some(vec![Quantity::new_scalar(
                45.0,
                1.0,
                dimensionless(),
                None,
                None,
            )]),
            ..Default::default()
        };
        let copy = generic.get_copy();
        assert_eq!(copy.model, "NaI-3x3");

        let mut unique = UniqueInstrument::from_template("Det-1", &generic);
        assert_eq!(unique.name, "Det-1");
        assert_eq!(unique.model, "NaI-3x3");
        unique.remark = "updated".to_string();
        assert_eq!(unique.generic.remark, "updated");

        let json = serde_json::to_string(&unique).unwrap();
        let decoded: UniqueInstrument = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, "Det-1");
        assert_eq!(decoded.detector_dimensions_cm.as_ref().unwrap().len(), 1);
    }
}
