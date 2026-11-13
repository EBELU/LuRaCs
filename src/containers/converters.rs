use super::spectrum::Spectrum;

/// Convert the background spectrum to the foreground spectrum, discarding the original_foreground creating a new spectrum.
/// # Panics
/// Panics if the original spectrum has no background.
pub fn convert_background_to_spectrum(original: Spectrum) -> Spectrum {
    if original.background.is_none() {
        panic!("Given spectrum has no background.")
    }

    let mut new_spectrum = original.clone();

    new_spectrum.foreground = original.background.clone();
    new_spectrum.background = None;

    new_spectrum
}
