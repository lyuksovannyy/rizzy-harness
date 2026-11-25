//! The one keyed limiter of the master (FIX_PLAN 4.2).
//!
//! [`FailureMap`] counts events per key inside a sliding window. A *key* is
//! whatever the caller chooses to be limited by (a lowercased login name, a
//! user id, a node id); the map never looks inside it. An event is a slot
//! with a millisecond timestamp and a reservation id; at most `max` slots
//! live per key for `window_ms`, and a key over its `max` is refused with
//! the milliseconds until its oldest slot expires. [`FailureMap::check`] only
//! reads; [`FailureMap::try_reserve`] checks and takes a slot under one lock,
//! so a burst of parallel attempts cannot all pass before the first is
//! counted. A [`Reservation`] is settled by `failed` (the slot stays
//! counted), `succeeded` (the key is cleared) or dropped (the slot is given
//! back, by id: it can never remove another attempt's slot); `clear` forgets
//! a key. The map holds at most `capacity` keys. Before a new key goes in
//! while full, keys with no live slot are dropped, then the key whose latest
//! slot is oldest, whoever it is. So a flood of more than `capacity` distinct
//! keys inside one window can push out older keys, locked ones included
//! (PLAN.md T12 prices that for logins: paced by the hash permits); a flood
//! of fewer cannot, and never changes another key's count. Empty keys are
//! always removed.

use std::collections::HashMap;
use std::sync::Mutex;

/// One failure, or a reservation that becomes one unless given back.
#[derive(Clone, Copy)]
struct Slot {
    at_ms: i64,
    id: u64,
}

struct Inner {
    map: HashMap<Box<str>, Vec<Slot>>,
    next_id: u64,
}

/// Failures per key in a sliding window, bounded in keys and in failures
/// per key.
pub struct FailureMap {
    inner: Mutex<Inner>,
    capacity: usize,
    max: usize,
    window_ms: i64,
}

impl FailureMap {
    pub fn new(capacity: usize, max: usize, window_ms: i64) -> Self {
        Self {
            inner: Mutex::new(Inner {
                map: HashMap::new(),
                next_id: 0,
            }),
            capacity,
            max,
            window_ms,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Milliseconds until `slots` (pruned) has room again.
    fn retry_ms(&self, slots: &[Slot], now_ms: i64) -> i64 {
        let oldest = slots.iter().map(|s| s.at_ms).min().unwrap_or(now_ms);
        (oldest + self.window_ms - now_ms).max(1)
    }

    /// Is `key` at its limit? Never adds a key, so it is safe to call before
    /// anything is paid for. `Err` carries the milliseconds to wait.
    pub fn check(&self, key: &str, now_ms: i64) -> Result<(), i64> {
        let mut inner = self.lock();
        let Some(slots) = inner.map.get_mut(key) else {
            return Ok(());
        };
        slots.retain(|s| now_ms - s.at_ms < self.window_ms);
        if slots.is_empty() {
            inner.map.remove(key);
            return Ok(());
        }
        if slots.len() >= self.max {
            return Err(self.retry_ms(slots, now_ms));
        }
        Ok(())
    }

    /// Take a slot for `key`, or `Err(ms to wait)` when its failures are
    /// used up. The slot counts as a failure until the returned
    /// [`Reservation`] is settled; dropping it unsettled gives it back.
    pub fn try_reserve<'a>(&'a self, key: &'a str, now_ms: i64) -> Result<Reservation<'a>, i64> {
        let mut inner = self.lock();
        let id = inner.next_id;
        inner.next_id += 1;
        if let Some(slots) = inner.map.get_mut(key) {
            slots.retain(|s| now_ms - s.at_ms < self.window_ms);
            if slots.len() >= self.max {
                return Err(self.retry_ms(slots, now_ms));
            }
            slots.push(Slot { at_ms: now_ms, id });
        } else {
            self.make_room(&mut inner.map, now_ms);
            let mut slots = Vec::with_capacity(self.max);
            slots.push(Slot { at_ms: now_ms, id });
            inner.map.insert(key.into(), slots);
        }
        Ok(Reservation {
            map: self,
            key,
            id,
            at_ms: now_ms,
            settled: false,
        })
    }

    /// Before a new key goes in: drop expired keys, then, if still full,
    /// the key whose latest failure is oldest. Linear, but only when full,
    /// and inserts are paced by the hash permits.
    fn make_room(&self, map: &mut HashMap<Box<str>, Vec<Slot>>, now_ms: i64) {
        if map.len() < self.capacity {
            return;
        }
        map.retain(|_, slots| slots.iter().any(|s| now_ms - s.at_ms < self.window_ms));
        while map.len() >= self.capacity {
            let oldest = map
                .iter()
                .min_by_key(|(_, slots)| slots.iter().map(|s| s.at_ms).max())
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => map.remove(&k),
                None => break,
            };
        }
    }

    fn release(&self, key: &str, id: u64) {
        let mut inner = self.lock();
        if let Some(slots) = inner.map.get_mut(key) {
            slots.retain(|s| s.id != id);
            if slots.is_empty() {
                inner.map.remove(key);
            }
        }
    }

    /// Make sure the failure `id` is counted: it may have been cleared by a
    /// parallel success (or evicted) while its hash ran.
    fn keep(&self, key: &str, id: u64, at_ms: i64) {
        let mut inner = self.lock();
        if inner
            .map
            .get(key)
            .is_some_and(|slots| slots.iter().any(|s| s.id == id))
        {
            return;
        }
        if !inner.map.contains_key(key) {
            self.make_room(&mut inner.map, at_ms);
        }
        let slots = inner
            .map
            .entry(key.into())
            .or_insert_with(|| Vec::with_capacity(self.max));
        if slots.len() < self.max {
            slots.push(Slot { at_ms, id });
        }
    }

    pub fn clear(&self, key: &str) {
        self.lock().map.remove(key);
    }

    /// Keys held (for tests and diagnostics).
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    /// Keys the map has room for without growing.
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.lock().map.capacity()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A taken slot. Settle it with [`Reservation::failed`] or
/// [`Reservation::succeeded`]; dropped unsettled (store error, cancelled
/// request, refused permit) it is given back.
pub struct Reservation<'a> {
    map: &'a FailureMap,
    key: &'a str,
    id: u64,
    at_ms: i64,
    settled: bool,
}

impl Reservation<'_> {
    /// The attempt failed: the slot stays counted.
    pub fn failed(mut self) {
        self.settled = true;
        self.map.keep(self.key, self.id, self.at_ms);
    }

    /// The attempt succeeded: the key's failures are forgotten.
    pub fn succeeded(mut self) {
        self.settled = true;
        self.map.clear(self.key);
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.map.release(self.key, self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: i64 = 1000;

    #[test]
    fn reservations_count_until_given_back() {
        let m = FailureMap::new(10, 3, W);
        let a = m.try_reserve("bob", 0).unwrap();
        let b = m.try_reserve("bob", 0).unwrap();
        let c = m.try_reserve("bob", 0).unwrap();
        assert_eq!(
            m.try_reserve("bob", 1).err(),
            Some(999),
            "full while pending"
        );
        assert!(m.check("bob", 1).is_err());
        drop(c); // cancelled: given back
        a.failed();
        b.failed();
        let d = m.try_reserve("bob", 2).unwrap();
        d.failed();
        assert!(m.try_reserve("bob", 3).is_err());
        assert!(m.check("bob", W + 2).is_ok(), "window over");
        assert!(m.is_empty(), "expired keys go");
    }

    #[test]
    fn a_dropped_reservation_leaves_no_key() {
        let m = FailureMap::new(10, 5, W);
        drop(m.try_reserve("x", 0).unwrap());
        assert!(m.is_empty());
        assert!(m.check("never", 0).is_ok());
        assert!(m.is_empty(), "check adds nothing");
    }

    #[test]
    fn a_failure_cleared_by_a_parallel_success_still_counts() {
        let m = FailureMap::new(10, 5, W);
        let guess = m.try_reserve("bob", 0).unwrap();
        let owner = m.try_reserve("bob", 0).unwrap();
        owner.succeeded();
        guess.failed();
        assert_eq!(m.len(), 1);
        for t in 1..5 {
            m.try_reserve("bob", t).unwrap().failed();
        }
        assert!(m.check("bob", 5).is_err());
    }

    #[test]
    fn the_map_is_bounded_and_drops_expired_then_oldest() {
        let m = FailureMap::new(3, 5, W);
        m.try_reserve("a", 0).unwrap().failed();
        m.try_reserve("b", 10).unwrap().failed();
        m.try_reserve("c", 20).unwrap().failed();
        m.try_reserve("a", 30).unwrap().failed(); // a is now the newest
        m.try_reserve("d", 40).unwrap().failed();
        assert_eq!(m.len(), 3);
        assert!(m.lock().map.contains_key("a"));
        assert!(
            !m.lock().map.contains_key("b"),
            "oldest latest-failure goes"
        );
        // Expired keys go before any live one.
        m.try_reserve("e", W + 25).unwrap().failed();
        let inner = m.lock();
        assert!(!inner.map.contains_key("c"));
        assert!(inner.map.contains_key("a") && inner.map.contains_key("d"));
    }
}
