//! Reusable count-rate (CPS) estimator for hardware detectors that only provide
//! cumulative spectra and livetimes (e.g. digiBase, digiDART, Detective X).
//!
//! Replaces duplicated buffering and subtraction logic across Python drivers.

/// Computes differential count-rate (Counts Per Second) from consecutive
/// cumulative spectrum observations and livetime counters.
#[derive(Debug, Clone, Default)]
pub struct CpsEstimator {
    last_total_counts: Option<f64>,
    last_livetime_secs: Option<f64>,
}

impl CpsEstimator {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_total_counts: None,
            last_livetime_secs: None,
        }
    }

    /// Reset internal history (e.g. when acquisition starts or spectrum is cleared).
    pub const fn reset(&mut self) {
        self.last_total_counts = None;
        self.last_livetime_secs = None;
    }

    /// Update with the latest cumulative counts and livetime.
    ///
    /// Returns:
    /// - `None` on the first observation (establishing baseline) or if `dt <= 0`.
    /// - `Some(cps)` on subsequent updates where `dt > 0`.
    pub fn update(&mut self, total_counts: f64, livetime_secs: f64) -> Option<f64> {
        let (prev_counts, prev_live) = match (self.last_total_counts, self.last_livetime_secs) {
            (Some(c), Some(l)) => (c, l),
            _ => {
                self.last_total_counts = Some(total_counts);
                self.last_livetime_secs = Some(livetime_secs);
                return None;
            }
        };

        let delta_live = livetime_secs - prev_live;
        if delta_live <= 0.0 {
            // No livetime progressed (paused or duplicate packet)
            self.last_total_counts = Some(total_counts);
            self.last_livetime_secs = Some(livetime_secs);
            return None;
        }

        let delta_counts = (total_counts - prev_counts).max(0.0);
        let dt = delta_live.max(1e-4);
        let cps = delta_counts / dt;

        self.last_total_counts = Some(total_counts);
        self.last_livetime_secs = Some(livetime_secs);

        Some(cps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cps_estimator_first_sample_is_none() {
        let mut est = CpsEstimator::new();
        assert_eq!(est.update(100.0, 10.0), None);
    }

    #[test]
    fn test_cps_estimator_normal_progression() {
        let mut est = CpsEstimator::new();
        assert_eq!(est.update(100.0, 10.0), None);

        // 1 second later, 50 new counts -> 50 CPS
        let cps = est.update(150.0, 11.0).unwrap();
        assert!((cps - 50.0).abs() < 1e-6);

        // 2 seconds later, 200 new counts -> 100 CPS
        let cps2 = est.update(350.0, 13.0).unwrap();
        assert!((cps2 - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_cps_estimator_zero_dt_returns_none() {
        let mut est = CpsEstimator::new();
        est.update(100.0, 10.0);
        assert_eq!(est.update(100.0, 10.0), None);
    }

    #[test]
    fn test_cps_estimator_reset() {
        let mut est = CpsEstimator::new();
        est.update(100.0, 10.0);
        assert!(est.update(150.0, 11.0).is_some());

        est.reset();
        // After reset, first sample is baseline again
        assert_eq!(est.update(200.0, 12.0), None);
        assert!(est.update(250.0, 13.0).is_some());
    }
}
