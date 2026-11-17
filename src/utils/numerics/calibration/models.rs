//! Numerical approximations for detector efficiency and energy resolution.
//!
//! Each family has a `Signature` enum whose string form is the signature
//! used in the XML serialization (e.g. `"{}*EXP(-{}*E)"`). `FromStr` and
//! `Display` make the enum round-trip through that string form.
//!
//! All lookup results have the same type — `EvalFn` — regardless of how many
//! parameters the underlying formula takes. `resolution`, which takes one
//! scalar in the Python version, is wrapped to accept a one-element slice so
//! the call site is uniform:
//!
//! ```ignore
//! let f = efficiency::lookup(EfficiencySignature::ExpPoly);
//! let eff = f(661.7, &params)?;
//! ```
//!
//! For hot loops over an energy axis, the parameter count is checked on every
//! call. Extract the function pointer once and rely on a pre-validated
//! parameter slice if the overhead ever matters.

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
#[error("{fname}: expected {expected} parameters, got {got}")]
pub struct WrongParamCount {
    pub fname: &'static str,
    pub expected: usize,
    pub got: usize,
}

#[derive(Debug, Error)]
#[error("unknown function signature: {0:?}")]
pub struct UnknownSignature(pub String);

/// Common return type for every evaluation function.
pub type EvalResult = Result<f64, WrongParamCount>;

/// Common function-pointer type for every lookup result.
pub type EvalFn = fn(f64, &[f64]) -> EvalResult;

// ===========================================================================
// Efficiency functions
// ===========================================================================

pub mod efficiency {
    use serde::{Deserialize, Serialize};

    use super::*;

    /// Exponential attenuation: `C0 * exp(-C1 * E)`.
    pub fn exp_atten(e: f64, params: &[f64]) -> EvalResult {
        if params.len() != 2 {
            return Err(WrongParamCount {
                fname: "exp_atten",
                expected: 2,
                got: params.len(),
            });
        }
        let (c0, c1) = (params[0], params[1]);
        Ok(c0 * (-c1 * e).exp())
    }

    /// Exponential polynomial in `ln(E)`:
    /// `exp(A + B·ln E + C·(ln E)^2 + D·(ln E)^3)`.
    pub fn exp_polynomial(e: f64, params: &[f64]) -> EvalResult {
        if params.len() != 4 {
            return Err(WrongParamCount {
                fname: "exp_polynomial",
                expected: 4,
                got: params.len(),
            });
        }
        let (a, b, c, d) = (params[0], params[1], params[2], params[3]);
        let ln_e = e.ln();
        let ln_e2 = ln_e * ln_e;
        let ln_e3 = ln_e2 * ln_e;
        Ok((a + b * ln_e + c * ln_e2 + d * ln_e3).exp())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub enum EfficiencySignature {
        ExpAtten,
        ExpPoly,
    }

    impl EfficiencySignature {
        /// The string form used in XML serialization.
        pub fn as_str(self) -> &'static str {
            match self {
                Self::ExpAtten => "{}*EXP(-{}*E)",
                Self::ExpPoly => "EXP({}+{}*LOG(E)+{}*LOG(E)^2+{}*LOG(E)^3)",
            }
        }

        pub fn expected_params(self) -> usize {
            match self {
                Self::ExpAtten => 2,
                Self::ExpPoly => 4,
            }
        }

        pub const ALL: &'static [Self] = &[Self::ExpAtten, Self::ExpPoly];
    }

    impl std::fmt::Display for EfficiencySignature {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl std::str::FromStr for EfficiencySignature {
        type Err = UnknownSignature;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s.trim() {
                "{}*EXP(-{}*E)" => Ok(Self::ExpAtten),
                "EXP({}+{}*LOG(E)+{}*LOG(E)^2+{}*LOG(E)^3)" => Ok(Self::ExpPoly),
                other => Err(UnknownSignature(other.to_string())),
            }
        }
    }

    /// Look up the evaluation function for a signature.
    pub fn lookup(sig: EfficiencySignature) -> EvalFn {
        match sig {
            EfficiencySignature::ExpAtten => exp_atten,
            EfficiencySignature::ExpPoly => exp_polynomial,
        }
    }
}

// ===========================================================================
// Resolution functions
// ===========================================================================

pub mod resolution {
    use serde::{Deserialize, Serialize};

    use super::*;

    /// Energy resolution: `a / sqrt(E)`.
    ///
    /// The Python version takes `(E, a)` with a scalar `a`. To keep the same
    /// function-pointer type as the efficiency functions, the slice-based
    /// wrapper below is what `lookup` returns; `value` is the direct scalar
    /// call for callers that don't go through the lookup table.
    pub fn value(e: f64, a: f64) -> f64 {
        a / e.sqrt()
    }

    /// Slice-parameter wrapper matching `EvalFn`.
    pub fn from_slice(e: f64, params: &[f64]) -> EvalResult {
        if params.len() != 1 {
            return Err(WrongParamCount {
                fname: "resolution",
                expected: 1,
                got: params.len(),
            });
        }
        Ok(value(e, params[0]))
    }

    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
    )]
    pub enum ResolutionSignature {
        KOverSqrtE,
    }

    impl ResolutionSignature {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::KOverSqrtE => "{}/SQRT(E)",
            }
        }

        pub fn expected_params(self) -> usize {
            match self {
                Self::KOverSqrtE => 1,
            }
        }

        pub const ALL: &'static [Self] = &[Self::KOverSqrtE];
    }

    impl std::fmt::Display for ResolutionSignature {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.as_str())
        }
    }

    impl std::str::FromStr for ResolutionSignature {
        type Err = UnknownSignature;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            match s.trim() {
                "{}/SQRT(E)" => Ok(Self::KOverSqrtE),
                other => Err(UnknownSignature(other.to_string())),
            }
        }
    }

    pub fn lookup(sig: ResolutionSignature) -> EvalFn {
        match sig {
            ResolutionSignature::KOverSqrtE => from_slice,
        }
    }
}

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

pub use efficiency::EfficiencySignature;
pub use resolution::ResolutionSignature;
