//! Retrying a model call that failed for a passing reason (rate limit,
//! overloaded or failing server, dropped connection) before the agent's
//! fallbacks are tried. Only this layer retries: `genai` has no retry and
//! reqwest only resends HTTP/2 requests the server refused unprocessed, so
//! attempts do not multiply.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use harness_core::event::{EventKind, ModelRetry, RetryReason};

use crate::provider::ProviderError;

/// How often and how long to wait. The defaults are what other harnesses
/// document (3 retries, 2 s doubling, 60 s at most).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Retries after the first attempt, per model.
    pub retries: u32,
    /// The first wait; each later one doubles.
    pub base: Duration,
    /// No backoff is longer. A `Retry-After` above it is not waited for at
    /// all: that model is not retried (see `delay`).
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
    /// `Retry-After` wins over the backoff. When it is longer than `max`
    /// there is no retry: sending again before it has passed would fail
    /// again and count against the provider's limit, so the caller falls
    /// back to another model or ends the turn (`explain`).
    pub fn delay(&self, e: &ProviderError, n: u32) -> Option<Duration> {
        if n >= self.retries || !e.is_retryable() {
            return None;
        }
        Some(match e.retry_after() {
            Some(after) if after > self.max => return None,
            Some(after) => after,
            None => self.backoff(n),
        })
    }

    /// The error to show when `e` asked for a wait longer than `max`, in
    /// plain words and without the provider's response body.
    pub fn explain(&self, e: &ProviderError, model: &str) -> Option<String> {
        let after = e.retry_after().filter(|a| *a > self.max)?;
        if !e.is_retryable() {
            return None;
        }
        let why = match e {
            ProviderError::Status { status: 429, .. } => "rate limit",
            _ => "provider busy",
        };
        Some(format!(
            "The provider of {model} asked to wait {} before the next request ({why}). \
             Try again after that, or pick another model.",
            human(after)
        ))
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

/// A wait as a person says it, rounded up to a whole second, minute or hour.
fn human(d: Duration) -> String {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    let (n, unit) = match secs {
        0..=59 => (secs.max(1), "second"),
        60..=3599 => (secs.div_ceil(60), "minute"),
        _ => (secs.div_ceil(3600), "hour"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
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

/// The class of a retried failure, for the person: never the body.
pub fn reason(e: &ProviderError) -> RetryReason {
    match e {
        ProviderError::Status { status: 429, .. } => RetryReason::RateLimit,
        ProviderError::Status {
            status: 503 | 529, ..
        } => RetryReason::Overloaded,
        ProviderError::Status { .. } => RetryReason::ServerError,
        ProviderError::Http(_) | ProviderError::Stream(_) | ProviderError::UnknownModel(_) => {
            RetryReason::Network
        }
    }
}

/// The `model_retry` event for retry number `attempt` (from 1) of `model`
/// after `e`, sent again `wait` from now.
pub fn event(
    turn_id: &str,
    model: &str,
    attempt: u32,
    of: u32,
    wait: Duration,
    e: &ProviderError,
) -> EventKind {
    EventKind::ModelRetry(ModelRetry {
        turn_id: turn_id.to_string(),
        model: model.to_string(),
        attempt,
        of,
        at_ms: harness_core::now_ms().saturating_add(wait.as_millis() as i64),
        wait_ms: wait.as_millis() as u64,
        reason: reason(e),
    })
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
        // At the cap exactly is still waited for.
        assert_eq!(p.delay(&after(60), 0), Some(p.max));
        assert_eq!(p.delay(&after(30), 0), Some(Duration::from_secs(30)));
    }

    #[test]
    fn a_wait_over_the_cap_is_not_retried_and_is_explained() {
        let p = RetryPolicy::default();
        let e = ProviderError::Status {
            status: 429,
            body: "secret-ish body".into(),
            hint: RetryHint {
                after: Some(Duration::from_secs(300)),
                no_retry: false,
            },
        };
        assert_eq!(p.delay(&e, 0), None);
        let msg = p.explain(&e, "m").unwrap();
        assert_eq!(
            msg,
            "The provider of m asked to wait 5 minutes before the next request \
             (rate limit). Try again after that, or pick another model."
        );
        assert!(!msg.contains("secret"));
        // Within the cap there is nothing to explain.
        let e = ProviderError::Status {
            status: 429,
            body: String::new(),
            hint: RetryHint {
                after: Some(Duration::from_secs(60)),
                no_retry: false,
            },
        };
        assert_eq!(p.explain(&e, "m"), None);
    }

    #[test]
    fn waits_read_as_a_person_would_say_them() {
        let f = |s| human(Duration::from_secs(s));
        assert_eq!(f(1), "1 second");
        assert_eq!(f(90), "2 minutes");
        assert_eq!(f(300), "5 minutes");
        assert_eq!(f(3600), "1 hour");
        assert_eq!(f(7300), "3 hours");
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
    fn the_event_names_a_class_and_no_body() {
        let r = |e: &ProviderError| reason(e);
        assert_eq!(r(&status(429, "k")), RetryReason::RateLimit);
        assert_eq!(r(&status(503, "k")), RetryReason::Overloaded);
        assert_eq!(r(&status(529, "k")), RetryReason::Overloaded);
        assert_eq!(r(&status(502, "k")), RetryReason::ServerError);
        assert_eq!(r(&ProviderError::Http("x".into())), RetryReason::Network);
        let before = harness_core::now_ms();
        let e = status(429, "secret-ish body sk-live-123");
        let ev = event("t", "p::m", 2, 3, Duration::from_secs(4), &e);
        let (kind, payload) = ev.into_parts();
        assert_eq!(kind, "model_retry");
        assert!(!payload.to_string().contains("secret"), "{payload}");
        assert_eq!(payload["attempt"], 2);
        assert_eq!(payload["of"], 3);
        assert_eq!(payload["reason"], "rate_limit");
        assert_eq!(payload["wait_ms"], 4000);
        let at = payload["at_ms"].as_i64().unwrap();
        assert!(at >= before + 4000 && at <= harness_core::now_ms() + 4000);
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
