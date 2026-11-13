use physure_core::quantity::Quantity;

use crate::containers::dimensionless;

/// Intrinsic peak efficiency of a detector.
///
/// `eff = cps / (activity · yield · solid_angle)`
/// `solid_angle = area / (d² · 4π)`
pub fn u_intrinsic_efficiency(
    activity_bq: &Quantity,
    source_detector_distance_cm: &Quantity,
    emission_yield: &Quantity,
    detector_area_cm2: &Quantity,
    cps: &Quantity,
) -> Quantity {
    let four_pi =
        Quantity::new_scalar(4.0 * std::f64::consts::PI, 0.0, dimensionless(), None, None);

    let d_squared = source_detector_distance_cm * source_detector_distance_cm;
    let solid_angle = detector_area_cm2 / (d_squared * four_pi);
    cps / (activity_bq * emission_yield * solid_angle)
}
