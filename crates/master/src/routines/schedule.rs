//! Schedules: 5-field cron in a time zone, a fixed interval, or one time.
//! Every kind answers one question, `next_after`, so the scheduler, the
//! preview and the missed-run rule never look inside a schedule. Cron is
//! croner (with jiff time zones); interval and once are plain jiff.
//!
//! Rules: a cron time in a spring-forward gap fires once, at the first
//! valid instant after the gap; a time repeated when clocks go back fires
//! once (the first); an interval is anchored, so it never drifts.

use std::str::FromStr;

use croner::Cron;
use croner::parser::{CronParser, Seconds, Year};
use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{AmbiguousOffset, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::spec::strict_keys;

const MINUTE_MS: i64 = 60_000;
/// How many cron occurrences are checked against the shortest gap.
const GAP_CHECK: usize = 50;
/// The longest interval: a year.
const MAX_INTERVAL_MIN: i64 = 366 * 24 * 60;
/// The most occurrences any count walks (missed runs, chats per day).
const MAX_WALK: usize = 20_000;

/// A trigger's `config` for `kind = "schedule"`, as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    /// `expr`: minute hour day-of-month month day-of-week.
    Cron { expr: String, tz: String },
    /// Every `every_minutes`, at `anchor_ms + k * every_minutes`.
    Interval { every_minutes: i64, anchor_ms: i64 },
    /// `at`: a local date and time (`2026-11-10T09:00`) in `tz`.
    Once { at: String, tz: String },
}

/// The same, as a request may send it: the time zone and the anchor may be
/// left out (they default to the owner's zone and to now).
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
    },
    Interval {
        every_minutes: i64,
        #[serde(default)]
        anchor_ms: Option<i64>,
    },
    Once {
        at: String,
        #[serde(default)]
        tz: Option<String>,
    },
}

/// A schedule ready to answer `next_after`.
#[derive(Debug, Clone)]
pub enum Schedule {
    Cron { cron: Box<Cron>, tz: TimeZone },
    Interval { every_ms: i64, anchor_ms: i64 },
    Once { at: Timestamp },
}

fn zone(name: &str) -> Result<TimeZone, String> {
    if name.len() > 64 {
        return Err("the time zone name is too long".into());
    }
    TimeZone::get(name).map_err(|_| format!("unknown time zone {name:?}"))
}

/// Exactly five fields, minute resolution: croner would also take seconds,
/// years and nicknames such as `@reboot`, none of which a routine has.
fn parse_cron(expr: &str) -> Result<Cron, String> {
    if expr.split_whitespace().count() != 5 {
        return Err("a cron schedule has 5 fields: minute hour day month weekday".into());
    }
    CronParser::builder()
        .seconds(Seconds::Disallowed)
        .year(Year::Disallowed)
        .build()
        .parse(expr)
        .map_err(|e| format!("bad cron schedule {expr:?}: {e}"))
}

/// A local date and time in `tz` as an instant: a time skipped by a
/// spring-forward gap becomes the first valid minute after the gap, a time
/// repeated when clocks go back its first occurrence.
fn local_instant(at: DateTime, tz: &TimeZone) -> Option<Timestamp> {
    let mut t = at;
    // Gaps are at most a few hours; walk them a minute at a time.
    for _ in 0..(24 * 60) {
        let amb = tz.to_ambiguous_timestamp(t);
        match amb.offset() {
            AmbiguousOffset::Unambiguous { .. } | AmbiguousOffset::Fold { .. } => {
                return amb.earlier().ok();
            }
            AmbiguousOffset::Gap { .. } => {
                t = t.checked_add(jiff::SignedDuration::from_mins(1)).ok()?;
            }
        }
    }
    None
}

impl ScheduleConfig {
    /// A config from a request: unknown fields refused, the time zone
    /// defaulting to `default_tz` and an interval's anchor to `now_ms`.
    pub fn from_request(v: &Value, default_tz: &str, now_ms: i64) -> Result<Self, String> {
        let req: Request =
            serde_json::from_value(v.clone()).map_err(|e| format!("schedule: {e}"))?;
        let cfg = match req {
            Request::Cron { expr, tz } => ScheduleConfig::Cron {
                expr: expr.split_whitespace().collect::<Vec<_>>().join(" "),
                tz: tz.unwrap_or_else(|| default_tz.into()),
            },
            Request::Interval {
                every_minutes,
                anchor_ms,
            } => ScheduleConfig::Interval {
                every_minutes,
                anchor_ms: anchor_ms.unwrap_or(now_ms - now_ms.rem_euclid(MINUTE_MS)),
            },
            Request::Once { at, tz } => ScheduleConfig::Once {
                at: at.trim().into(),
                tz: tz.unwrap_or_else(|| default_tz.into()),
            },
        };
        strict_keys("schedule", v, &cfg)?;
        Ok(cfg)
    }

    /// Check the config, including the shortest gap between two runs.
    pub fn compile(&self, min_interval_minutes: u32, now_ms: i64) -> Result<Schedule, String> {
        let min_ms = i64::from(min_interval_minutes) * MINUTE_MS;
        let s = self.schedule()?;
        match (self, &s) {
            (ScheduleConfig::Interval { every_minutes, .. }, _) => {
                if *every_minutes * MINUTE_MS < min_ms || *every_minutes > MAX_INTERVAL_MIN {
                    return Err(format!(
                        "an interval runs every {min_interval_minutes} minutes to 366 days"
                    ));
                }
            }
            (ScheduleConfig::Cron { .. }, _) => {
                let mut t = ts(now_ms);
                let mut prev: Option<Timestamp> = None;
                for _ in 0..GAP_CHECK {
                    let Some(next) = s.next_after(t) else { break };
                    if let Some(p) = prev
                        && next.as_millisecond() - p.as_millisecond() < min_ms
                    {
                        return Err(format!(
                            "this schedule runs more often than every {min_interval_minutes} minutes"
                        ));
                    }
                    prev = Some(next);
                    t = next;
                }
                if prev.is_none() {
                    return Err("this schedule never runs".into());
                }
            }
            (ScheduleConfig::Once { .. }, _) => {}
        }
        Ok(s)
    }

    /// The schedule, without the gap check (stored configs).
    pub fn schedule(&self) -> Result<Schedule, String> {
        Ok(match self {
            ScheduleConfig::Cron { expr, tz } => Schedule::Cron {
                cron: Box::new(parse_cron(expr)?),
                tz: zone(tz)?,
            },
            ScheduleConfig::Interval {
                every_minutes,
                anchor_ms,
            } => {
                if *every_minutes < 1 {
                    return Err("an interval is at least a minute".into());
                }
                Schedule::Interval {
                    every_ms: every_minutes * MINUTE_MS,
                    anchor_ms: *anchor_ms,
                }
            }
            ScheduleConfig::Once { at, tz } => {
                let local = DateTime::from_str(at)
                    .map_err(|_| format!("bad date and time {at:?}; use 2026-11-10T09:00"))?;
                let at = local_instant(local, &zone(tz)?)
                    .ok_or_else(|| format!("{at:?} does not exist in {tz}"))?;
                Schedule::Once { at }
            }
        })
    }

    /// A stored config; `None` when it cannot be read.
    pub fn read(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    /// The zone `now.*` is shown in for runs of this trigger (an interval
    /// has none: the owner's applies).
    pub fn tz(&self) -> Option<&str> {
        match self {
            ScheduleConfig::Cron { tz, .. } | ScheduleConfig::Once { tz, .. } => Some(tz),
            ScheduleConfig::Interval { .. } => None,
        }
    }
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

impl Schedule {
    /// The first occurrence strictly after `t`, if there is one.
    pub fn next_after(&self, t: Timestamp) -> Option<Timestamp> {
        match self {
            Schedule::Cron { cron, tz } => cron
                .find_next_occurrence(&t.to_zoned(tz.clone()), false)
                .ok()
                .map(|z| z.timestamp()),
            Schedule::Interval {
                every_ms,
                anchor_ms,
            } => {
                let now = t.as_millisecond();
                let k = (now - anchor_ms).div_euclid(*every_ms) + 1;
                Timestamp::from_millisecond(anchor_ms + k * every_ms).ok()
            }
            Schedule::Once { at } => (*at > t).then_some(*at),
        }
    }

    /// `next_after` in milliseconds.
    pub fn next_after_ms(&self, ms: i64) -> Option<i64> {
        self.next_after(ts(ms)).map(|t| t.as_millisecond())
    }

    /// How late occurrence `at_ms` may still run: half the gap to the next
    /// one, at most an hour. Later than that it counts as missed.
    pub fn grace_ms(&self, at_ms: i64) -> i64 {
        let hour = 60 * MINUTE_MS;
        let gap = match self {
            Schedule::Interval { every_ms, .. } => Some(*every_ms),
            _ => self.next_after_ms(at_ms).map(|n| n - at_ms),
        };
        gap.map_or(hour, |g| (g / 2).min(hour))
    }

    /// Occurrences from `from_ms` (included) to `to_ms` (included), counted
    /// up to a bound.
    pub fn count_between(&self, from_ms: i64, to_ms: i64) -> usize {
        let mut n = 0;
        let mut t = from_ms;
        if t > to_ms {
            return 0;
        }
        n += 1;
        while n < MAX_WALK {
            match self.next_after_ms(t) {
                Some(next) if next <= to_ms => {
                    n += 1;
                    t = next;
                }
                _ => break,
            }
        }
        n
    }
}

/// The next `n` runs of all `schedules` after `now_ms`, earliest first.
pub fn next_runs(schedules: &[Schedule], now_ms: i64, n: usize) -> Vec<i64> {
    let mut all = Vec::new();
    for s in schedules {
        let mut t = now_ms;
        for _ in 0..n {
            let Some(next) = s.next_after_ms(t) else {
                break;
            };
            all.push(next);
            t = next;
        }
    }
    all.sort_unstable();
    all.dedup();
    all.truncate(n);
    all
}

/// About how many chats `schedules` start a day: their runs over the next
/// week, divided by seven (one decimal).
pub fn chats_per_day(schedules: &[Schedule], now_ms: i64) -> f64 {
    let week = now_ms + 7 * 24 * 60 * MINUTE_MS;
    let runs: usize = schedules
        .iter()
        .map(|s| match s.next_after_ms(now_ms) {
            Some(first) => s.count_between(first, week),
            None => 0,
        })
        .sum();
    (runs as f64 / 7.0 * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn cron(expr: &str, tz: &str) -> Schedule {
        let cfg = ScheduleConfig::Cron {
            expr: expr.into(),
            tz: tz.into(),
        };
        cfg.compile(5, at("2026-01-01T00:00:00Z").as_millisecond())
            .unwrap()
    }

    fn compile(v: Value) -> Result<Schedule, String> {
        let now = at("2026-11-08T10:00:00Z").as_millisecond();
        ScheduleConfig::from_request(&v, "UTC", now)?.compile(5, now)
    }

    #[test]
    fn cron_in_a_time_zone() {
        let s = cron("30 8 * * 1-5", "Europe/Riga");
        // Sunday 8 November 2026: the next weekday at 08:30 Riga (EET).
        let next = s.next_after(at("2026-11-08T12:00:00Z")).unwrap();
        assert_eq!(next, at("2026-11-09T06:30:00Z"));
        // Friday to Monday skips the weekend.
        let fri = s.next_after(at("2026-11-13T07:00:00Z")).unwrap();
        assert_eq!(fri, at("2026-11-16T06:30:00Z"));
        // Day-of-week 0 and 7 are both Sunday, names work.
        for expr in ["0 9 * * 0", "0 9 * * 7", "0 9 * * SUN"] {
            let s = cron(expr, "UTC");
            assert_eq!(
                s.next_after(at("2026-11-02T00:00:00Z")).unwrap(),
                at("2026-11-08T09:00:00Z"),
                "{expr}"
            );
        }
    }

    #[test]
    fn spring_forward_gap_fires_once_after_it() {
        // 28 March 2027, Riga: 03:00 EET jumps to 04:00 EEST.
        let s = cron("30 3 * * *", "Europe/Riga");
        let first = s.next_after(at("2027-03-27T12:00:00Z")).unwrap();
        // 04:00 EEST = 01:00 UTC: the first valid instant after the gap.
        assert_eq!(first, at("2027-03-28T01:00:00Z"));
        let second = s.next_after(first).unwrap();
        assert_eq!(second, at("2027-03-29T00:30:00Z"), "03:30 EEST next day");
    }

    #[test]
    fn fall_back_overlap_fires_once() {
        // 25 October 2026, Riga: 04:00 EEST goes back to 03:00 EET, so
        // 03:30 happens twice.
        let s = cron("30 3 * * *", "Europe/Riga");
        let first = s.next_after(at("2026-10-24T12:00:00Z")).unwrap();
        assert_eq!(first, at("2026-10-25T00:30:00Z"), "the first 03:30 (EEST)");
        let second = s.next_after(first).unwrap();
        assert_eq!(
            second,
            at("2026-10-26T01:30:00Z"),
            "no second run that night"
        );
    }

    #[test]
    fn once_in_a_gap_or_overlap() {
        let once = |local: &str| {
            ScheduleConfig::Once {
                at: local.into(),
                tz: "Europe/Riga".into(),
            }
            .schedule()
            .unwrap()
        };
        let Schedule::Once { at: gap } = once("2027-03-28T03:30") else {
            panic!()
        };
        assert_eq!(gap, at("2027-03-28T01:00:00Z"));
        let Schedule::Once { at: fold } = once("2026-10-25T03:30") else {
            panic!()
        };
        assert_eq!(fold, at("2026-10-25T00:30:00Z"));
        let s = once("2026-11-10T09:00");
        assert_eq!(
            s.next_after(at("2026-11-08T00:00:00Z")),
            Some(at("2026-11-10T07:00:00Z"))
        );
        assert_eq!(s.next_after(at("2026-11-10T07:00:00Z")), None, "once only");
    }

    #[test]
    fn interval_is_anchored() {
        let anchor = at("2026-11-08T10:00:00Z").as_millisecond();
        let s = Schedule::Interval {
            every_ms: 15 * MINUTE_MS,
            anchor_ms: anchor,
        };
        // Late ticks never shift the grid.
        let late = anchor + 15 * MINUTE_MS + 7_000;
        assert_eq!(s.next_after_ms(late), Some(anchor + 30 * MINUTE_MS));
        assert_eq!(s.next_after_ms(anchor), Some(anchor + 15 * MINUTE_MS));
        let mut t = anchor;
        for _ in 0..1000 {
            t = s.next_after_ms(t).unwrap();
        }
        assert_eq!(t, anchor + 1000 * 15 * MINUTE_MS, "no drift");
        assert_eq!(s.next_after_ms(anchor - 1), Some(anchor));
    }

    #[test]
    fn refused_schedules() {
        let e = compile(json!({"type": "cron", "expr": "* * * * *"})).unwrap_err();
        assert!(e.contains("more often"), "{e}");
        assert!(compile(json!({"type": "cron", "expr": "*/5 * * * *"})).is_ok());
        let e = compile(json!({"type": "cron", "expr": "0 30 8 * * *"})).unwrap_err();
        assert!(e.contains("5 fields"), "{e}");
        assert!(compile(json!({"type": "cron", "expr": "0 8 * * * 2027"})).is_err());
        assert!(compile(json!({"type": "cron", "expr": "@reboot"})).is_err());
        assert!(compile(json!({"type": "cron", "expr": "61 8 * * *"})).is_err());
        let e = compile(json!({"type": "cron", "expr": "0 8 * * *", "tz": "Mars/Olympus"}))
            .unwrap_err();
        assert!(e.contains("time zone"), "{e}");
        assert!(compile(json!({"type": "interval", "every_minutes": 4})).is_err());
        assert!(compile(json!({"type": "interval", "every_minutes": 5})).is_ok());
        assert!(compile(json!({"type": "once", "at": "next tuesday"})).is_err());
        assert!(compile(json!({"type": "weekly"})).is_err());
        let e = compile(json!({"type": "cron", "expr": "0 8 * * *", "every": 1})).unwrap_err();
        assert!(e.contains("every"), "{e}");
    }

    #[test]
    fn requests_fill_in_the_zone_and_anchor() {
        let now = at("2026-11-08T10:00:30Z").as_millisecond();
        let c = ScheduleConfig::from_request(
            &json!({"type": "cron", "expr": " 0  8 * * * "}),
            "Europe/Riga",
            now,
        )
        .unwrap();
        assert_eq!(
            c,
            ScheduleConfig::Cron {
                expr: "0 8 * * *".into(),
                tz: "Europe/Riga".into()
            }
        );
        let i = ScheduleConfig::from_request(
            &json!({"type": "interval", "every_minutes": 30}),
            "UTC",
            now,
        )
        .unwrap();
        assert_eq!(
            i,
            ScheduleConfig::Interval {
                every_minutes: 30,
                anchor_ms: now - 30_000
            }
        );
        let stored = serde_json::to_string(&i).unwrap();
        assert_eq!(ScheduleConfig::read(&stored), Some(i));
    }

    #[test]
    fn grace_counts_and_previews() {
        let anchor = at("2026-11-08T10:00:00Z").as_millisecond();
        let five = Schedule::Interval {
            every_ms: 5 * MINUTE_MS,
            anchor_ms: anchor,
        };
        assert_eq!(five.grace_ms(anchor), 150_000);
        let daily = cron("0 8 * * *", "UTC");
        assert_eq!(daily.grace_ms(anchor), 3_600_000);
        assert_eq!(five.count_between(anchor, anchor + 60 * MINUTE_MS), 13);
        assert_eq!(chats_per_day(std::slice::from_ref(&five), anchor), 288.0);
        assert_eq!(chats_per_day(std::slice::from_ref(&daily), anchor), 1.0);
        let runs = next_runs(&[daily, five], anchor, 5);
        assert_eq!(runs.len(), 5);
        assert_eq!(runs[0], anchor + 5 * MINUTE_MS);
    }
}
