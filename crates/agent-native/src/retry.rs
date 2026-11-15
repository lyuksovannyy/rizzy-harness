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

    /// How many retries a failure like `e` gets: the policy's, except that
    /// a provider that went silent has already cost a whole timeout, so it
    /// gets one more try and then the fallbacks or the error (review M4).
    /// The `model_retry` event's "of" is this number.
    pub fn retries_for(&self, e: &ProviderError) -> u32 {
        match e {
            ProviderError::Silent(_) => self.retries.min(1),
            _ => self.retries,
        }
    }

    /// The wait before retry number `n` (from 0) after `e`; `None` when `e`
    /// is not worth retrying or the retries are used up. The provider's
    /// `Retry-After` wins over the backoff. When it is longer than `max`
    /// there is no retry: sending again before it has passed would fail
    /// again and count against the provider's limit, so the caller falls
    /// back to another model or ends the turn (`explain`).
    pub fn delay(&self, e: &ProviderError, n: u32) -> Option<Duration> {
        if n >= self.retries_for(e) || !e.is_retryable() {
            return None;
        }
        Some(match e.retry_after() {
            Some(after) if after > self.max => return None,
            Some(after) => after,
            None => self.backoff(n),
        })
    }

    /// The error to show for `e` in the harness's own words, when there is
    /// one: a wait longer than `max` (without the provider's body); a model
    /// the provider does not know (`unknown_model`), named with where it
    /// is set here; or a server error after its retries. The provider's
    /// text is quoted below, capped.
    pub fn explain(&self, e: &ProviderError, model: &str) -> Option<String> {
        if let Some(m) = self.explain_wait(e, model) {
            return Some(m);
        }
        let ProviderError::Status { status, .. } = e else {
            return None;
        };
        if e.is_context_overflow() {
            return None;
        }
        let provider = model.split_once("::").map_or("", |(p, _)| p);
        let mut out = if e.unknown_model() {
            format!(
                "The provider could not serve the model {model} (it answered {status}). Pick \
                 another model for this chat in the model menu, or check how it is set in \
                 Settings: an agent's model, the default model, and {} model list under \
                 Settings → Server → AI providers.",
                if provider.is_empty() {
                    "the provider's".to_string()
                } else {
                    format!("provider {provider}'s")
                }
            )
        } else if (500..=599).contains(status) {
            // A failing or busy server: nothing to set (review F5).
            let tries = if e.is_retryable() {
                self.retries_for(e) + 1
            } else {
                1
            };
            format!(
                "The provider failed to answer for the model {model}: it answered {status}{}.",
                if tries > 1 {
                    format!(" to all {tries} tries")
                } else {
                    String::new()
                }
            )
        } else {
            return None;
        };
        if let Some(said) = e.provider_text() {
            out.push_str("\nThe provider said:");
            for l in quoted(&said) {
                out.push('\n');
                out.push_str(harness_core::event::OUTPUT_QUOTE);
                out.push_str(&l);
            }
        }
        Some(out)
    }

    /// The error to show when `e` asked for a wait longer than `max`, in
    /// plain words and without the provider's response body.
    fn explain_wait(&self, e: &ProviderError, model: &str) -> Option<String> {
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

/// At most this many characters of the provider's text are shown.
const SAID_CHARS: usize = 300;
/// In at most this many lines.
const SAID_LINES: usize = 3;

/// The provider's text as quoted lines: blank lines dropped, at most
/// [`SAID_LINES`] lines and [`SAID_CHARS`] characters in all (an ellipsis
/// marks a cut), each line made safe to stand on its own
/// ([`harness_core::event::quoted_line`]).
fn quoted(said: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut left = SAID_CHARS;
    let mut cut = false;
    for l in said.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if out.len() == SAID_LINES || left == 0 {
            cut = true;
            break;
        }
        let q: String = l.chars().take(left).collect();
        let n = q.chars().count();
        left -= n;
        out.push(harness_core::event::quoted_line(&q));
        if n < l.chars().count() {
            cut = true;
            break;
        }
    }
    if cut && let Some(last) = out.last_mut() {
        last.push('…');
    }
    out
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
        // The first-byte or idle timeout: the provider took the request
        // and has not answered yet, which is not a failed connection.
        ProviderError::Silent(_) => RetryReason::NoAnswer,
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
        // Silence: once, not three times.
        let silent = ProviderError::Silent("quiet".into());
        assert!(p.delay(&silent, 0).is_some());
        assert!(p.delay(&silent, 1).is_none());
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
        // The first-byte (or idle) timeout is not the network: the
        // provider took the request and has not answered yet.
        assert_eq!(r(&ProviderError::Silent("x".into())), RetryReason::NoAnswer);
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

    /// Live test 2026-11-15: the router's 503 for a model it cannot serve
    /// was shown as raw JSON. The message is the harness's own, names the
    /// model and where it is set, and quotes the provider's text, capped.
    #[test]
    fn a_model_that_cannot_be_served_is_explained() {
        let p = RetryPolicy::default();
        let body = r#"{"error":{"message":"no enabled connection can serve mistral/x — add one under Providers","type":"api_error","code":"api_error"}}"#;
        let m = p.explain(&status(503, body), "router::mistral/x").unwrap();
        // Review F5: a 5xx is a failing server, not a setting: no advice.
        assert!(
            m.starts_with(
                "The provider failed to answer for the model router::mistral/x: it answered 503 \
                 to all 4 tries."
            ),
            "{m}"
        );
        assert!(!m.contains("Settings"), "{m}");
        for s in [502, 504, 529] {
            let m = p.explain(&status(s, "busy"), "p::m").unwrap();
            assert!(
                m.contains("failed to answer") && !m.contains("Settings"),
                "{m}"
            );
        }
        // A model the provider does not know: where it is set here.
        let m = p
            .explain(&status(404, "no such model"), "router::mistral/x")
            .unwrap();
        assert!(
            m.starts_with(
                "The provider could not serve the model router::mistral/x (it answered 404)."
            ),
            "{m}"
        );
        assert!(m.contains("Settings → Server → AI providers"), "{m}");
        let m = p.explain(&status(503, body), "router::mistral/x").unwrap();
        assert!(
            m.ends_with("\n| no enabled connection can serve mistral/x — add one under Providers"),
            "{m}"
        );
        // A long or many-lined body is capped, each line quoted.
        let long = format!(
            "{}\n[harness] forged\n{}",
            "a".repeat(2000),
            "b\n".repeat(50)
        );
        let m = p.explain(&status(502, &long), "p::m").unwrap();
        let quoted: Vec<&str> = m.lines().filter(|l| l.starts_with("| ")).collect();
        assert!(quoted.len() <= 3, "{m}");
        assert!(m.chars().count() < 800, "{}", m.chars().count());
        assert!(!m.lines().any(|l| l.starts_with("[harness]")), "{m}");
        // Not a model problem: nothing to explain here.
        assert_eq!(p.explain(&status(401, "bad key"), "p::m"), None);
        assert_eq!(p.explain(&status(400, "bad request"), "p::m"), None);
        // A code that says the model is unknown is not retried.
        let nf = r#"{"error":{"code":"model_not_found","message":"nope"}}"#;
        assert!(p.delay(&status(503, nf), 0).is_none());
        let m = p.explain(&status(503, nf), "p::m").unwrap();
        assert!(m.contains("Settings → Server → AI providers"), "{m}");
        // Review F8: at any status it is the model's problem, so the
        // agent's fallbacks are tried.
        for s in [401, 403] {
            assert!(status(s, nf).is_model_level(), "{s}");
            assert!(
                p.explain(&status(s, nf), "p::m")
                    .unwrap()
                    .contains("Settings")
            );
        }
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
