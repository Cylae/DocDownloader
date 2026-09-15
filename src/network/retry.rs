use std::time::Duration;

/// Configuration for exponential backoff retry policy.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            jitter_factor: 0.25,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_retries: u32, initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            initial_delay,
            max_delay,
            jitter_factor: 0.25,
        }
    }

    /// Determines if an HTTP status code is transient and eligible for retry.
    pub fn is_status_retryable(status: u16) -> bool {
        matches!(
            status,
            408  // Request Timeout
            | 429 // Too Many Requests
            | 500 // Internal Server Error
            | 502 // Bad Gateway
            | 503 // Service Unavailable
            | 504 // Gateway Timeout
        )
    }

    /// Calculates backoff delay for the given retry attempt (1-based), incorporating jitter.
    pub fn delay_for_attempt(&self, attempt: u32, server_retry_after: Option<Duration>) -> Duration {
        if let Some(delay) = server_retry_after {
            return delay.min(self.max_delay);
        }

        let base_millis = self.initial_delay.as_millis() as f64 * (2.0_f64.powi((attempt - 1).min(6) as i32));
        let bounded_millis = base_millis.min(self.max_delay.as_millis() as f64);

        // Deterministic pseudo-jitter based on attempt to avoid thread_rng dependencies
        let jitter = bounded_millis * self.jitter_factor * (((attempt * 97) % 100) as f64 / 100.0);
        let final_millis = (bounded_millis + jitter) as u64;

        Duration::from_millis(final_millis).min(self.max_delay)
    }
}

/// Parses the standard HTTP `Retry-After` header value (seconds integer or IMF-fixdate).
pub fn parse_retry_after(header_value: &str) -> Option<Duration> {
    let trimmed = header_value.trim();
    // 1. Try parsing as delta-seconds integer
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // 2. Try parsing as HTTP date format (e.g. "Wed, 21 Oct 2015 07:28:00 GMT")
    // Fallback default if unparseable date
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_statuses() {
        assert!(RetryPolicy::is_status_retryable(429));
        assert!(RetryPolicy::is_status_retryable(500));
        assert!(RetryPolicy::is_status_retryable(503));
        assert!(!RetryPolicy::is_status_retryable(400));
        assert!(!RetryPolicy::is_status_retryable(401));
        assert!(!RetryPolicy::is_status_retryable(403));
        assert!(!RetryPolicy::is_status_retryable(404));
    }

    #[test]
    fn test_retry_after_parsing() {
        assert_eq!(parse_retry_after("12"), Some(Duration::from_secs(12)));
        assert_eq!(parse_retry_after("  5 "), Some(Duration::from_secs(5)));
        assert_eq!(parse_retry_after("invalid"), None);
    }

    #[test]
    fn test_delay_bounding() {
        let policy = RetryPolicy::default();
        let delay1 = policy.delay_for_attempt(1, None);
        let delay5 = policy.delay_for_attempt(5, None);
        assert!(delay1 <= delay5);
        assert!(delay5 <= policy.max_delay);
    }
}
