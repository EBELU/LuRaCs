//! Composable reconnection policy for transport links (primarily Bluetooth/BLE).
//!
//! Reconnection is separated into:
//! - **BackoffStrategy**: How delay between retry attempts evolves (constant, linear, exponential).
//! - **ReconnectLimit**: Composable termination boundaries (max retry count, max elapsed duration).

use std::time::Duration;

/// Progression of delays between consecutive reconnection attempts.
#[derive(Debug, Clone, PartialEq)]
pub enum BackoffStrategy {
    Constant(Duration),
    Linear {
        initial: Duration,
        step: Duration,
        max: Duration,
    },
    Exponential {
        initial: Duration,
        multiplier: f64,
        max: Duration,
    },
}

impl BackoffStrategy {
    /// Calculate backoff delay for an attempt index (0-based).
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        match self {
            Self::Constant(d) => *d,
            Self::Linear { initial, step, max } => {
                let added = step.saturating_mul(attempt);
                (*initial + added).min(*max)
            }
            Self::Exponential {
                initial,
                multiplier,
                max,
            } => {
                let factor = multiplier.powi(attempt as i32);
                initial.mul_f64(factor).min(*max)
            }
        }
    }
}

/// Composable termination limits for reconnection loops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconnectLimit {
    /// Discontinue reconnection after `N` failed attempts.
    MaxAttempts(u32),
    /// Discontinue reconnection after total elapsed time exceeds `Duration`.
    MaxElapsed(Duration),
}

impl ReconnectLimit {
    /// Returns true if this limit has been reached or exceeded.
    #[must_use]
    pub const fn is_exceeded(&self, attempt: u32, elapsed: Duration) -> bool {
        match self {
            Self::MaxAttempts(max) => attempt >= *max,
            Self::MaxElapsed(max) => elapsed.as_millis() >= max.as_millis(),
        }
    }
}

/// Composable reconnection policy.
#[derive(Debug, Clone, PartialEq)]
pub struct ReconnectPolicy {
    pub strategy: BackoffStrategy,
    pub limits: Vec<ReconnectLimit>,
}

impl ReconnectPolicy {
    /// Non-reconnecting policy (fails immediately, used for USB and non-reconnecting links).
    #[must_use]
    pub const fn never() -> Self {
        Self {
            strategy: BackoffStrategy::Constant(Duration::ZERO),
            limits: Vec::new(),
        }
    }

    /// Whether this policy allows any reconnection at all.
    #[must_use]
    pub fn is_reconnectable(&self) -> bool {
        for limit in &self.limits {
            if let ReconnectLimit::MaxAttempts(max) = limit
                && *max == 0
            {
                return false;
            }
        }
        // If strategy is zero delay and has 0 attempts
        !(matches!(self.strategy, BackoffStrategy::Constant(d) if d.is_zero())
            && self.limits.is_empty())
    }

    /// Default policy recommended for Bluetooth BLE detectors:
    /// Initial 500ms, 1.5x backoff, capped at 10s, up to 10 attempts or 2 minutes.
    #[must_use]
    pub fn ble_default() -> Self {
        Self::exponential(Duration::from_millis(500), 1.5, Duration::from_secs(10))
            .with_max_attempts(10)
            .with_max_elapsed(Duration::from_secs(120))
    }

    /// Build an exponential backoff policy.
    #[must_use]
    pub const fn exponential(initial: Duration, multiplier: f64, max: Duration) -> Self {
        Self {
            strategy: BackoffStrategy::Exponential {
                initial,
                multiplier,
                max,
            },
            limits: Vec::new(),
        }
    }

    /// Build a linear backoff policy.
    #[must_use]
    pub const fn linear(initial: Duration, step: Duration, max: Duration) -> Self {
        Self {
            strategy: BackoffStrategy::Linear { initial, step, max },
            limits: Vec::new(),
        }
    }

    /// Build a constant delay policy.
    #[must_use]
    pub const fn constant(delay: Duration) -> Self {
        Self {
            strategy: BackoffStrategy::Constant(delay),
            limits: Vec::new(),
        }
    }

    /// Attach a custom limit to the policy.
    #[must_use]
    pub fn with_limit(mut self, limit: ReconnectLimit) -> Self {
        self.limits.push(limit);
        self
    }

    /// Attach a max attempts limit.
    #[must_use]
    pub fn with_max_attempts(self, attempts: u32) -> Self {
        self.with_limit(ReconnectLimit::MaxAttempts(attempts))
    }

    /// Attach a max cumulative duration limit.
    #[must_use]
    pub fn with_max_elapsed(self, max: Duration) -> Self {
        self.with_limit(ReconnectLimit::MaxElapsed(max))
    }

    /// Evaluate the policy for the given attempt count and total elapsed time.
    ///
    /// Returns `Some(delay)` to wait before the next attempt, or `None` if
    /// any of the composable limits have been reached (signaling give up).
    #[must_use]
    pub fn next_attempt(&self, attempt: u32, elapsed: Duration) -> Option<Duration> {
        if !self.is_reconnectable() {
            return None;
        }

        // Check if any composable limit is exceeded.
        for limit in &self.limits {
            if limit.is_exceeded(attempt, elapsed) {
                return None;
            }
        }

        Some(self.strategy.delay_for_attempt(attempt))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_never_policy() {
        let policy = ReconnectPolicy::never();
        assert!(!policy.is_reconnectable());
        assert_eq!(policy.next_attempt(0, Duration::ZERO), None);
    }

    #[test]
    fn test_exponential_backoff() {
        let policy = ReconnectPolicy::exponential(
            Duration::from_millis(100),
            2.0,
            Duration::from_millis(500),
        )
        .with_max_attempts(4);

        assert!(policy.is_reconnectable());
        // Attempt 0: 100ms
        assert_eq!(
            policy.next_attempt(0, Duration::ZERO),
            Some(Duration::from_millis(100))
        );
        // Attempt 1: 200ms
        assert_eq!(
            policy.next_attempt(1, Duration::from_millis(100)),
            Some(Duration::from_millis(200))
        );
        // Attempt 2: 400ms
        assert_eq!(
            policy.next_attempt(2, Duration::from_millis(300)),
            Some(Duration::from_millis(400))
        );
        // Attempt 3: 800ms -> capped at 500ms
        assert_eq!(
            policy.next_attempt(3, Duration::from_millis(700)),
            Some(Duration::from_millis(500))
        );
        // Attempt 4: MaxAttempts(4) exceeded!
        assert_eq!(policy.next_attempt(4, Duration::from_millis(1200)), None);
    }

    #[test]
    fn test_max_elapsed_limit() {
        let policy = ReconnectPolicy::constant(Duration::from_millis(50))
            .with_max_elapsed(Duration::from_secs(1));

        assert_eq!(
            policy.next_attempt(0, Duration::from_millis(500)),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            policy.next_attempt(1, Duration::from_millis(999)),
            Some(Duration::from_millis(50))
        );
        // Exceeded elapsed 1s
        assert_eq!(
            policy.next_attempt(2, Duration::from_millis(1001)),
            None
        );
    }

    #[test]
    fn test_linear_backoff() {
        let policy = ReconnectPolicy::linear(
            Duration::from_millis(100),
            Duration::from_millis(50),
            Duration::from_millis(200),
        );

        assert_eq!(policy.next_attempt(0, Duration::ZERO), Some(Duration::from_millis(100)));
        assert_eq!(policy.next_attempt(1, Duration::ZERO), Some(Duration::from_millis(150)));
        assert_eq!(policy.next_attempt(2, Duration::ZERO), Some(Duration::from_millis(200)));
        assert_eq!(policy.next_attempt(3, Duration::ZERO), Some(Duration::from_millis(200)));
    }
}
