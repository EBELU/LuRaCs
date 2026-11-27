use super::spectrum::Spectrum;

/// Convert the background spectrum to the foreground spectrum, discarding the original_foreground creating a new spectrum.
/// # Panics
/// Panics if the original spectrum has no background.
pub fn convert_background_to_spectrum(original: Spectrum) -> Spectrum {
    if original.background.is_none() {
        panic!("Given spectrum has no background.")
    }

    let mut new_spectrum = original.clone();

    new_spectrum.foreground = original.background;
    new_spectrum.background = None;

    new_spectrum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::spectrum::SpectrumData;
    use ndarray::Array1;

    #[test]
    fn test_convert_background_to_spectrum_success() {
        let mut s = Spectrum::new(4, "fg");
        let bg = SpectrumData {
            y_axis: Array1::from_vec(vec![1.0, 2.0, 3.0, 4.0]),
            channels: 4,
            total_counts: 10,
            live_time: 5.0,
            real_time: None,
            avg_dose_rate: None,
            avg_cps: None,
            start_date: None,
            end_date: None,
            spectrum_name: Some("bg".to_string()),
            instrument: None,
        };
        s.set_background(bg, None);
        let converted = convert_background_to_spectrum(s);
        assert!(converted.background.is_none());
        assert_eq!(converted.foreground.unwrap().total_counts, 10);
    }

    #[test]
    #[should_panic(expected = "Given spectrum has no background.")]
    fn test_convert_background_to_spectrum_panics_without_background() {
        let s = Spectrum::new(4, "empty");
        drop(convert_background_to_spectrum(s));
    }
}
