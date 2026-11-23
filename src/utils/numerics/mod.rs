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
pub use peak_detection::median_filter as peak_median_filter;
pub use peak_detection::{
    PeakFinderError, PeakFinderResult, PeakResult, enforce_min_separation, find_peaks,
    local_mad_threshold, local_maxima, local_minima, mad_threshold, peak_discriminator,
    post_process_peaks,
};
