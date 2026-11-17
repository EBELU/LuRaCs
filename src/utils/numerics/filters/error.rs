use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FilterError {
    #[error("signal must not be empty")]
    EmptySignal,
    #[error("kernel must not be empty")]
    EmptyKernel,
    #[error("window_size / kernel length must be a positive odd number (got {0})")]
    EvenWindow(usize),
    #[error("sigma must be positive and finite (got {0})")]
    InvalidSigma(f64),
    #[error("polyorder ({polyorder}) must be < window_length ({window})")]
    PolyorderTooLarge { polyorder: usize, window: usize },
    #[error("deriv ({deriv}) must be <= polyorder ({polyorder})")]
    DerivTooLarge { deriv: usize, polyorder: usize },
    #[error("signal length ({signal}) must be > half window_length ({half})")]
    SignalTooShort { signal: usize, half: usize },
    #[error("pseudo-inverse of Vandermonde matrix failed (SVD did not converge)")]
    PinvFailed,
}

pub type FilterResult<T> = std::result::Result<T, FilterError>;
