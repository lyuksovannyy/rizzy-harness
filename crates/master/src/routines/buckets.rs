//! The webhook throttles (docs/routines.md, Webhooks): token buckets on the
//! routine clock. They are laid out so that someone without the secret can
//! never lock out the sender who has it:
//!
//! * calls for **unknown trigger ids** share one server-wide bucket, which
//!   no real trigger's calls touch;
//! * **wrong secrets** have a bucket per trigger that counts failing calls
//!   only: a call with the right secret is always checked and never refused
//!   by it;
//! * the **per-trigger rate** applies only once a call proved its secret.

use std::collections::HashMap;

const MINUTE: i64 = 60_000;
/// The window of the unknown-id and wrong-secret buckets, and of the
/// throttled `hook.rejected` audit line.
pub(crate) const WINDOW: i64 = 15 * MINUTE;
/// Calls for unknown ids per window, server-wide.
const UNKNOWN_PER_WINDOW: u32 = 100;
/// Wrong secrets per window, per trigger.
const BAD_SECRET_PER_WINDOW: u32 = 20;
/// Authenticated calls a trigger may make at once, before its per-minute
/// rate applies.
const BURST: u32 = 5;

/// How fast a bucket fills and how much it holds, in whole numbers: one
/// token is `window` units and every millisecond adds `count` units, so
/// no rounding ever lets a call through early or holds one back.
#[derive(Debug, Clone, Copy)]
struct Rate {
    count: i64,
    window: i64,
    burst: i64,
}

impl Rate {
    /// `count` per `window` milliseconds, holding at most `burst` tokens.
    fn new(count: u32, window: i64, burst: u32) -> Self {
        Self {
            count: i64::from(count.max(1)),
            window,
            burst: i64::from(burst.max(1)),
        }
    }

    fn unknown() -> Self {
        Self::new(UNKNOWN_PER_WINDOW, WINDOW, UNKNOWN_PER_WINDOW)
    }

    fn bad_secret() -> Self {
        Self::new(BAD_SECRET_PER_WINDOW, WINDOW, BAD_SECRET_PER_WINDOW)
    }

    fn calls(per_minute: u32) -> Self {
        Self::new(per_minute, MINUTE, BURST)
    }

    fn capacity(&self) -> i64 {
        self.burst * self.window
    }
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    /// Units held (`Rate`).
    level: i64,
    at: i64,
}

impl Bucket {
    fn full(rate: Rate, now: i64) -> Self {
        Self {
            level: rate.capacity(),
            at: now,
        }
    }

    fn refill(&mut self, rate: Rate, now: i64) {
        let gained = (now - self.at).max(0).saturating_mul(rate.count);
        self.level = self.level.saturating_add(gained).min(rate.capacity());
        self.at = self.at.max(now);
    }

    /// Take one token, or say in how many milliseconds one comes.
    fn take(&mut self, rate: Rate, now: i64) -> Result<(), i64> {
        self.refill(rate, now);
        if self.level >= rate.window {
            self.level -= rate.window;
            return Ok(());
        }
        let missing = rate.window - self.level;
        Err((missing + rate.count - 1) / rate.count)
    }

    fn is_full(&mut self, rate: Rate, now: i64) -> bool {
        self.refill(rate, now);
        self.level >= rate.capacity()
    }
}

/// Take a token from `key`'s bucket in `map`, which starts full.
fn take(map: &mut HashMap<String, Bucket>, key: &str, rate: Rate, now: i64) -> Result<(), i64> {
    map.entry(key.to_string())
        .or_insert_with(|| Bucket::full(rate, now))
        .take(rate, now)
}

/// The key of unknown ids in `HookLimits::rejected`.
pub(crate) const UNKNOWN_ID: &str = "";

/// Every throttle of the hooks endpoint, in `RoutinesHandle`.
#[derive(Debug, Default)]
pub(crate) struct HookLimits {
    unknown: Option<Bucket>,
    bad_secret: HashMap<String, Bucket>,
    calls: HashMap<String, Bucket>,
    /// Per trigger (`UNKNOWN_ID` for unknown ids): rejections not yet in an
    /// audit line, and when the last line was written.
    rejected: HashMap<String, (u32, i64)>,
}

impl HookLimits {
    /// A call for a trigger id that does not exist. `Err`: refuse it, and
    /// a token comes in that many milliseconds.
    pub(crate) fn unknown_id(&mut self, now: i64) -> Result<(), i64> {
        let rate = Rate::unknown();
        self.unknown
            .get_or_insert_with(|| Bucket::full(rate, now))
            .take(rate, now)
    }

    /// A call for `trigger` with a wrong secret.
    pub(crate) fn bad_secret(&mut self, trigger: &str, now: i64) -> Result<(), i64> {
        take(&mut self.bad_secret, trigger, Rate::bad_secret(), now)
    }

    /// An authenticated call for `trigger`, at `per_minute`.
    pub(crate) fn call(&mut self, trigger: &str, per_minute: u32, now: i64) -> Result<(), i64> {
        take(&mut self.calls, trigger, Rate::calls(per_minute), now)
    }

    /// Count a rejected call under `key`. Returns the count to write in a
    /// `hook.rejected` line when one is due: at most one per key and window,
    /// so a flood of bad calls cannot flood the audit log.
    pub(crate) fn rejected(&mut self, key: &str, now: i64) -> Option<u32> {
        let (count, last) = self
            .rejected
            .entry(key.to_string())
            .or_insert((0, i64::MIN));
        *count += 1;
        if now.saturating_sub(*last) < WINDOW {
            return None;
        }
        let n = std::mem::take(count);
        *last = now;
        Some(n)
    }

    /// Forget buckets that are full again and old audit counts: they hold
    /// nothing a fresh entry would not (hourly).
    pub(crate) fn prune(&mut self, per_minute: u32, now: i64) {
        let (bad, calls) = (Rate::bad_secret(), Rate::calls(per_minute));
        self.bad_secret.retain(|_, b| !b.is_full(bad, now));
        self.calls.retain(|_, b| !b.is_full(calls, now));
        self.rejected
            .retain(|_, (_, last)| now.saturating_sub(*last) < WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_rate_has_a_burst_then_refills() {
        let mut l = HookLimits::default();
        for _ in 0..BURST {
            assert!(l.call("t", 10, 0).is_ok());
        }
        // Ten a minute: the next token in six seconds.
        assert_eq!(l.call("t", 10, 0), Err(6_000));
        assert_eq!(l.call("t", 10, 1_000), Err(5_000));
        assert!(l.call("t", 10, 6_000).is_ok());
        assert!(l.call("t", 10, 6_000).is_err());
        assert!(l.call("other", 10, 6_000).is_ok(), "per trigger");
    }

    #[test]
    fn wrong_secrets_and_unknown_ids_have_their_own_buckets() {
        let mut l = HookLimits::default();
        for _ in 0..BAD_SECRET_PER_WINDOW {
            assert!(l.bad_secret("t", 0).is_ok());
        }
        let wait = l.bad_secret("t", 0).unwrap_err();
        assert_eq!(wait, WINDOW / i64::from(BAD_SECRET_PER_WINDOW));
        // Neither the call rate nor the unknown-id bucket felt it.
        assert!(l.call("t", 10, 0).is_ok());
        assert!(l.unknown_id(0).is_ok());
        for _ in 1..UNKNOWN_PER_WINDOW {
            assert!(l.unknown_id(0).is_ok());
        }
        assert!(l.unknown_id(0).is_err());
        assert!(l.bad_secret("u", 0).is_ok(), "another trigger");
        assert!(l.unknown_id(WINDOW).is_ok(), "refilled");
    }

    #[test]
    fn rejections_are_audited_once_a_window() {
        let mut l = HookLimits::default();
        assert_eq!(l.rejected("t", 0), Some(1));
        assert_eq!(l.rejected("t", 10), None);
        assert_eq!(l.rejected("t", 20), None);
        assert_eq!(l.rejected(UNKNOWN_ID, 20), Some(1), "its own key");
        assert_eq!(l.rejected("t", WINDOW), Some(3));
    }

    #[test]
    fn prune_keeps_only_what_matters() {
        let mut l = HookLimits::default();
        l.call("busy", 10, 0).unwrap();
        l.bad_secret("busy", 0).unwrap();
        l.rejected("busy", 0);
        l.prune(10, 1_000);
        assert_eq!((l.calls.len(), l.bad_secret.len()), (1, 1));
        l.prune(10, WINDOW);
        assert!(l.calls.is_empty() && l.bad_secret.is_empty() && l.rejected.is_empty());
    }
}
