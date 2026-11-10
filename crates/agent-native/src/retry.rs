//! Retrying a model call that failed for a passing reason (rate limit,
//! overloaded or failing server, dropped connection) before the agent's
//! fallbacks are tried. Only this layer retries: `genai` has no retry and
//! reqwest only resends HTTP/2 requests the server refused unprocessed, so
//! attempts do not multiply.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::provider::ProviderError;

/// How often and how long to wait. The defaults are what other harnesses
/// document (3 retries, 2 s doubling, 60 s at most).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt, per model.
    pub retries: u32,
    /// The first wait; each later one doubles.
    pub base: Duration,
    /// No wait is longer, `Retry-After` included.
    pub max: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            retries: 3,
            base: Duration::from_secs(2),
            max: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// No retries: a failure goes straight to the fallbacks.
    pub fn none() -> Self {
        Self {
            retries: 0,
            ..Self::default()
        }
    }

    /// The wait before retry number `n` (from 0) after `e`; `None` when `e`
    /// is not worth retrying or the retries are used up. The provider's
    /// `Retry-After` wins over the backoff, capped at `max`.
    pub fn delay(&self, e: &ProviderError, n: u32) -> Option<Duration> {
        if n >= self.retries || !e.is_retryable() {
            return None;
        }
        Some(match e.retry_after() {
            Some(after) => after.min(self.max),
            None => self.backoff(n),
        })
    }

    /// `base * 2^n` capped at `max`, then "equal jitter": half of it fixed,
    /// the other half random, so clients that failed together do not all
    /// come back at the same instant.
    fn backoff(&self, n: u32) -> Duration {
        let full = self
            .base
            .saturating_mul(1u32.checked_shl(n).unwrap_or(u32::MAX))
            .min(self.max);
        let half = full / 2;
        half + half.mul_f64(jitter())
    }
}

/// A number in `[0, 1)`, from the standard library's per-process random
/// hasher keys (no generator dependency for a backoff jitter).
fn jitter() -> f64 {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Wait `d` unless cancelled first; `false` when cancelled.
pub async fn wait(d: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        _ = tokio::time::sleep(d) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::RetryHint;

    fn status(status: u16, body: &str) -> ProviderError {
        ProviderError::Status {
            status,
            body: body.into(),
            hint: RetryHint::default(),
        }
    }

    #[test]
    fn only_passing_failures_are_retried() {
        let p = RetryPolicy::default();
        for s in [429, 500, 502, 503, 504, 529] {
            assert!(p.delay(&status(s, "busy"), 0).is_some(), "{s}");
        }
        for s in [400, 401, 402, 403, 404, 413, 422] {
            assert!(p.delay(&status(s, "no"), 0).is_none(), "{s}");
        }
        assert!(p.delay(&ProviderError::Http("refused".into()), 0).is_some());
        assert!(p.delay(&ProviderError::Stream("cut".into()), 0).is_some());
        assert!(
            p.delay(&ProviderError::UnknownModel("x".into()), 0)
                .is_none()
        );
        // A used-up quota is a 429 too, and no wait fixes it.
        let quota = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        assert!(p.delay(&status(429, quota), 0).is_none());
        // The provider says not to.
        let told = ProviderError::Status {
            status: 503,
            body: String::new(),
            hint: RetryHint {
                after: None,
                no_retry: true,
            },
        };
        assert!(p.delay(&told, 0).is_none());
    }

    #[test]
    fn backoff_doubles_with_jitter_and_stops() {
        let p = RetryPolicy::default();
        let e = status(503, "");
        for (n, full) in [(0, 2), (1, 4), (2, 8)] {
            let d = p.delay(&e, n).unwrap();
            let full = Duration::from_secs(full);
            assert!(d >= full / 2 && d <= full, "retry {n}: {d:?}");
        }
        assert_eq!(p.delay(&e, 3), None, "three retries, then no more");
        let long = RetryPolicy { retries: 40, ..p };
        assert!(long.delay(&e, 39).unwrap() <= long.max);
    }

    #[test]
    fn retry_after_wins_and_is_capped() {
        let p = RetryPolicy::default();
        let after = |secs| ProviderError::Status {
            status: 429,
            body: String::new(),
            hint: RetryHint {
                after: Some(Duration::from_secs(secs)),
                no_retry: false,
            },
        };
        assert_eq!(p.delay(&after(7), 0), Some(Duration::from_secs(7)));
        assert_eq!(p.delay(&after(3600), 0), Some(p.max));
    }

    #[test]
    fn hint_reads_the_headers() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "3".parse().unwrap());
        assert_eq!(
            RetryHint::from_headers(&h).after,
            Some(Duration::from_secs(3))
        );
        h.insert("retry-after-ms", "1500".parse().unwrap());
        assert_eq!(
            RetryHint::from_headers(&h).after,
            Some(Duration::from_millis(1500))
        );
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            "retry-after",
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        h.insert("x-should-retry", "false".parse().unwrap());
        assert_eq!(
            RetryHint::from_headers(&h),
            RetryHint {
                after: None,
                no_retry: true
            }
        );
    }

    #[test]
    fn log_line_has_no_body() {
        let e = status(429, "secret-ish body with the user's text");
        assert_eq!(e.log_line(), "status 429");
        let e = ProviderError::Http("error sending request for url (https://k@x/)".into());
        assert!(!e.log_line().contains("http"));
    }

    #[tokio::test]
    async fn wait_ends_at_once_on_cancel() {
        let c = CancellationToken::new();
        c.cancel();
        let t = std::time::Instant::now();
        assert!(!wait(Duration::from_secs(60), &c).await);
        assert!(t.elapsed() < Duration::from_secs(1));
    }
}
