pub mod calibration;
pub mod compression;
pub mod deconvolution;
pub mod filters;
pub mod fitting;
pub mod peak_detection;

// Backward-compatible module aliases:
pub use filters::savgol;
pub use fitting::optimizer;
pub use fitting::polyfit;

// Top-level re-exports for convenience and backward compatibility
pub use calibration::*;
pub use compression::*;
pub use deconvolution::*;
pub use filters::*;
pub use fitting::*;
pub use peak_detection::{
    enforce_min_separation, find_peaks, local_mad_threshold, local_maxima, local_minima,
    mad_threshold, peak_discriminator, post_process_peaks, PeakFinderError, PeakFinderResult,
    PeakResult,
};
pub use peak_detection::median_filter as peak_median_filter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filters() {
        let signal = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0];
        let mean = mean_filter(&signal, 3, EdgeMode::Mirror);
        assert_eq!(mean.len(), signal.len());

        let median = filters::median_filter(&signal, 3, EdgeMode::Mirror);
        assert_eq!(median.len(), signal.len());

        let gauss = gaussian_filter_1d(&signal, 5, 1.0, EdgeMode::Mirror);
        assert_eq!(gauss.len(), signal.len());

        let (sg, _) = savgol_filter(&signal, 5, 2, 0).expect("savgol filter failed");
        assert_eq!(sg.len(), signal.len());
    }

    #[test]
    fn test_fitting() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![2.0, 4.0, 6.0, 8.0, 10.0];
        let res = polyfit_weighted(&x, &y, None, 1, false).expect("polyfit failed");
        assert!((res.coefficients[0] - 2.0).abs() < 1e-6);
        assert!(res.coefficients[1].abs() < 1e-6);

        let a = nalgebra::DMatrix::<f64>::identity(3, 3);
        let inv = pseudo_inverse(&a).expect("pinv failed");
        assert_eq!(inv, a);
    }

    #[test]
    fn test_calibration() {
        let sig = "{}*EXP(-{}*E)".parse::<EfficiencySignature>().expect("parse failed");
        assert_eq!(sig, EfficiencySignature::ExpAtten);

        let res_sig = "{}/SQRT(E)".parse::<ResolutionSignature>().expect("parse failed");
        assert_eq!(res_sig, ResolutionSignature::KOverSqrtE);
    }

    #[test]
    fn test_deconvolution() {
        let dense = ndarray::Array2::<f64>::eye(4);
        let csr = CsrMatrix::from_dense(&dense);
        assert_eq!(csr.nrows(), 4);
        assert_eq!(csr.ncols(), 4);
        assert_eq!(csr.nnz(), 4);

        let x = nalgebra::DVector::from_vec(vec![1.0, 2.0, 3.0, 4.0]);
        let mut y = nalgebra::DVector::zeros(4);
        csr.forward(&x, &mut y);
        assert_eq!(x, y);
    }

    #[test]
    fn test_peak_detection() {
        let peaks = vec![10, 12, 30, 32, 50];
        let separated = enforce_min_separation(&peaks, 5);
        assert_eq!(separated, vec![10, 30, 50]);
    }

    #[test]
    fn test_compression() {
        let counts = vec![0, 10, 50, 100, 25, 5, 0];
        let compressed = compress_spectrum(&counts).expect("compress failed");
        let decompressed = decompress_spectrum(&compressed, counts.len()).expect("decompress failed");
        assert_eq!(counts, decompressed);
    }
}
