//! Token totals for a server admin: per person and model, and per
//! workspace, for a period (ADOPTION_PLAN X.18 / X.39). Numbers only.
//!
//! Privacy (PLAN.md: chats are private to their owner, admins too): the
//! answer is built per session in memory and then folded into buckets keyed
//! by person or workspace; no session id, title, folder, agent, node or event
//! time leaves this module, and no count of chats (a cut answer reports a
//! percentage). No schema change, no index: one range read per session on
//! the `events` primary key, filtered by `ts_ms` and `kind`, paid for by a
//! time limit that cuts the scan and says by how much.
//!
//! # What is summed, so every token is counted once
//!
//! The rule is the chat header's (`SessionView` in `harness-core`, `fold.ts`
//! in the UI): a chat's usage is
//!
//! - each turn's final usage, `turn_ended.usage`, which REPLACES the turn's
//!   running `turn_usage` totals (those are never added to it);
//! - a turn that ended without usage (interrupted, stopped) keeps its last
//!   running `turn_usage`; so does a turn that never ended (the next
//!   `turn_started` settles it, or it still runs and its running figure
//!   stands, as in the header);
//! - the cost of a `/compact` (`history_summarized.usage`), a failed one
//!   (`compact_failed.usage`), a hand-off note (`handoff_written.usage`) and
//!   a background sub-agent's end (`background_ended.child.usage`), tokens
//!   only (they are not turns).
//!
//! A sub-agent's own chat is NOT summed: its parent's turn usage already
//! includes it (`Usage::add` of the child's result), and a background one
//! comes in through `background_ended`. A chat with a `parent_id` whose
//! parent still exists is therefore skipped; an orphan (its parent deleted)
//! is counted, as nothing else counts it. A parent turn interrupted before
//! its child returned lost the child's tokens from the parent; they are
//! undercounted, never doubled.
//!
//! A chat's sums are added with `Usage::add`, which keeps at most
//! `MAX_USAGE_MODELS` models per chat (the rest stay in the sums,
//! unattributed), exactly as the header does.
//!
//! The period is `from_ms <= ts < to_ms`; a figure belongs to the time of the
//! event that settles it (the `turn_ended`, or the `turn_started` that
//! settles a turn that never ended). Known edges: a turn that began before
//! the period and ended without usage inside it lacks the running figures
//! from before the period; a hand-off note written after a chat went idle
//! before the period is not read (the chat's `updated_ms` ignores hand-off
//! notes); a running turn's figure is counted at its last report and
//! replaced by the end's figure in the period the end falls in only if the
//! end is before `to_ms` (otherwise it is left for that later period).

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use harness_core::event::{EventKind, ModelTokens, Usage};
use rusqlite::params;

use crate::{EFFECTIVE_WS, Result, Store, WS_JOINS};

/// Most models listed for one person or workspace; the rest stay in its
/// token sums (a price estimate counts them as unpriced).
pub const MAX_BUCKET_MODELS: usize = 256;

/// The four token classes (`input` includes the cache classes, as in the
/// log).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl Tally {
    fn add(&mut self, t: &ModelTokens) {
        self.input_tokens = self.input_tokens.saturating_add(t.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(t.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(t.cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.saturating_add(t.cache_write_tokens);
    }

    fn of(u: &Usage) -> ModelTokens {
        ModelTokens {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            cache_write_tokens: u.cache_write_tokens,
        }
    }
}

/// One model's share of a bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelUse {
    pub tally: Tally,
    /// Turns whose usage names this model.
    pub turns: u64,
}

/// A person's, or a workspace's, totals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bucket {
    /// Everything, including tokens no model is named for.
    pub tally: Tally,
    pub turns: u64,
    pub models: BTreeMap<String, ModelUse>,
}

impl Bucket {
    fn add(&mut self, s: &SessionSum) {
        self.tally.add(&Tally::of(&s.usage));
        self.turns += s.turns;
        for (m, t) in &s.usage.by_model {
            let at_cap = self.models.len() >= MAX_BUCKET_MODELS && !self.models.contains_key(m);
            if at_cap {
                continue;
            }
            self.models.entry(m.clone()).or_default().tally.add(t);
        }
        for (m, n) in &s.model_turns {
            if let Some(u) = self.models.get_mut(m) {
                u.turns += n;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonUsage {
    /// `None`: chats with no account (started with the API token).
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub bucket: Bucket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceUsage {
    /// `None`: Personal chats (all people's together, never named).
    pub workspace_id: Option<String>,
    pub name: Option<String>,
    pub bucket: Bucket,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageTotals {
    pub people: Vec<PersonUsage>,
    pub workspaces: Vec<WorkspaceUsage>,
    /// The time limit ran out: the share (1..=99) of the chats that were
    /// read, most recently active first. `None`: all of them.
    pub covered_percent: Option<u8>,
}

/// What one chat adds up to in the period (never leaves this module).
#[derive(Default)]
struct SessionSum {
    usage: Usage,
    turns: u64,
    model_turns: BTreeMap<String, u64>,
}

impl SessionSum {
    /// A settled cost: `turn` when it is a turn's (counted as one).
    fn settle(&mut self, u: &Usage, turn: bool) {
        self.usage.add(u);
        if turn {
            self.turns += 1;
            for m in u.by_model.keys() {
                *self.model_turns.entry(m.clone()).or_default() += 1;
            }
        }
    }
}

/// The one chat's events of the period, in `seq` order, folded as the
/// header's fold does (see the module comment).
fn sum_session(
    events: impl Iterator<Item = (i64, EventKind)>,
    after: &dyn Fn() -> bool,
) -> SessionSum {
    let mut sum = SessionSum::default();
    // The turn whose running figure is held, and what it is.
    let mut live: Option<(String, Usage, i64)> = None;
    let mut running: Option<String> = None;
    let mut ended: HashSet<String> = HashSet::new();
    for (ts, k) in events {
        match k {
            EventKind::TurnStarted(t) => {
                if let Some((_, u, _)) = live.take() {
                    sum.settle(&u, true);
                }
                running = Some(t.turn_id);
            }
            EventKind::TurnUsage(t) => {
                // Only the running turn's counts (a log cut at the period's
                // start has no `turn_started`: then any turn that has not
                // ended), and a total below the last is stale.
                let ok = match &running {
                    Some(r) => *r == t.turn_id,
                    None => !ended.contains(&t.turn_id),
                };
                if !ok {
                    continue;
                }
                let tokens = |u: &Usage| u.input_tokens.saturating_add(u.output_tokens);
                if let Some((id, last, _)) = &live
                    && *id == t.turn_id
                    && tokens(&t.usage) < tokens(last)
                {
                    continue;
                }
                running = Some(t.turn_id.clone());
                live = Some((t.turn_id, t.usage.capped(), ts));
            }
            EventKind::TurnEnded(t) => {
                let held = match &live {
                    Some((id, _, _)) if *id == t.turn_id => live.take().map(|(_, u, _)| u),
                    _ => None,
                };
                match (&t.usage, &held) {
                    (Some(u), _) => sum.settle(u, true),
                    (None, Some(u)) => sum.settle(u, true),
                    (None, None) => {}
                }
                ended.insert(t.turn_id);
                running = None;
            }
            EventKind::HistorySummarized(h) => {
                if let Some(u) = &h.usage {
                    sum.settle(u, false);
                }
            }
            EventKind::CompactFailed(c) => {
                if let Some(u) = &c.usage {
                    sum.settle(u, false);
                }
            }
            EventKind::HandoffWritten(h) => {
                if let Some(u) = &h.usage {
                    sum.settle(u, false);
                }
            }
            EventKind::BackgroundEnded(b) => {
                if let Some(u) = b.child.as_ref().and_then(|c| c.usage.as_ref()) {
                    sum.settle(u, false);
                }
            }
            _ => {}
        }
    }
    // A turn still running (or never ended) at the end of what was read:
    // its running figure, as the header shows it, unless a later turn or end
    // exists after the period (then that one settles it, in its own period).
    if let Some((_, u, _)) = live
        && !after()
    {
        sum.settle(&u, true);
    }
    sum
}

const KINDS: &str = "'turn_started','turn_usage','turn_ended','history_summarized',\
'compact_failed','handoff_written','background_ended'";

impl Store {
    /// Token totals for `from_ms <= ts < to_ms` (see the module comment for
    /// what is counted). Reads at most `limit` of wall time; then the answer
    /// holds what was read and says what share that was.
    pub fn usage_totals(&self, from_ms: i64, to_ms: i64, limit: Duration) -> Result<UsageTotals> {
        let started = Instant::now();
        struct Row {
            sid: String,
            owner: Option<String>,
            username: Option<String>,
            ws_id: Option<String>,
            ws_name: Option<String>,
        }
        // The chats that were active in the period (a chat's `updated_ms` is
        // its last event's time), most recent first, without sub-agents
        // whose parent exists.
        let rows: Vec<Row> = {
            let conn = self.lock();
            let mut st = conn.prepare_cached(&format!(
                "SELECT s.session_id, s.owner_id, u.username, ew.workspace_id,
                        CASE WHEN ew.personal_owner IS NULL THEN ew.name END
                 FROM sessions s
                 LEFT JOIN users u ON u.user_id = s.owner_id
                 LEFT JOIN sessions p ON p.session_id = s.parent_id
                 {WS_JOINS}
                 LEFT JOIN workspaces ew ON ew.workspace_id = {EFFECTIVE_WS}
                 WHERE s.updated_ms >= ?1 AND (s.parent_id IS NULL OR p.session_id IS NULL)
                 ORDER BY s.updated_ms DESC, s.session_id"
            ))?;
            let mapped = st.query_map(params![from_ms], |r| {
                let ws_name: Option<String> = r.get(4)?;
                let ws_id: Option<String> = r.get(3)?;
                Ok(Row {
                    sid: r.get(0)?,
                    owner: r.get(1)?,
                    username: r.get(2)?,
                    // A Personal workspace has no name here: it is not shown.
                    ws_id: ws_name.as_ref().and(ws_id),
                    ws_name,
                })
            })?;
            mapped.collect::<rusqlite::Result<_>>()?
        };
        let total = rows.len();
        let mut people: BTreeMap<Option<String>, (Option<String>, Bucket)> = BTreeMap::new();
        let mut spaces: BTreeMap<Option<String>, (Option<String>, Bucket)> = BTreeMap::new();
        let mut covered_percent = None;
        for (i, row) in rows.iter().enumerate() {
            if started.elapsed() >= limit {
                covered_percent = Some(((i * 100) / total.max(1)).min(99) as u8);
                break;
            }
            let sum = {
                let conn = self.lock();
                // What a fork copied from its parent cost the parent, which
                // holds it (ADOPTION_PLAN C.2): the fork's sums start at its
                // own first turn.
                let through = crate::session_forks::through(&conn, &row.sid)?;
                let mut st = conn.prepare_cached(&format!(
                    "SELECT ts_ms, kind, payload, seq FROM events
                     WHERE session_id = ?1 AND ts_ms >= ?2 AND ts_ms < ?3 AND kind IN ({KINDS})
                     ORDER BY seq"
                ))?;
                let mut events = Vec::new();
                for r in st.query_map(params![row.sid, from_ms, to_ms], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                })? {
                    let (ts, kind, payload, seq) = r?;
                    if harness_core::fork::inherited(through, seq.max(0) as u64) {
                        continue;
                    }
                    let Ok(v) = serde_json::from_str(&payload) else {
                        continue;
                    };
                    if let Ok(k) = EventKind::from_parts(&kind, v) {
                        events.push((ts, k));
                    }
                }
                // Is there a turn start or end after the period? Asked only
                // when a figure is still held at the end.
                let later = || {
                    conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM events WHERE session_id = ?1 AND ts_ms >= ?2
                                       AND kind IN ('turn_started','turn_ended'))",
                        params![row.sid, to_ms],
                        |r| r.get::<_, bool>(0),
                    )
                    .unwrap_or(false)
                };
                sum_session(events.into_iter(), &later)
            };
            if sum.usage == Usage::default() && sum.turns == 0 {
                continue;
            }
            people
                .entry(row.owner.clone())
                .or_insert_with(|| (row.username.clone(), Bucket::default()))
                .1
                .add(&sum);
            spaces
                .entry(row.ws_id.clone())
                .or_insert_with(|| (row.ws_name.clone(), Bucket::default()))
                .1
                .add(&sum);
        }
        let mut people: Vec<PersonUsage> = people
            .into_iter()
            .map(|(user_id, (username, bucket))| PersonUsage {
                user_id,
                username,
                bucket,
            })
            .collect();
        people.sort_by(|a, b| {
            (
                a.username.is_none(),
                a.username.as_deref().map(str::to_lowercase),
            )
                .cmp(&(
                    b.username.is_none(),
                    b.username.as_deref().map(str::to_lowercase),
                ))
                .then_with(|| a.user_id.cmp(&b.user_id))
        });
        let mut workspaces: Vec<WorkspaceUsage> = spaces
            .into_iter()
            .map(|(workspace_id, (name, bucket))| WorkspaceUsage {
                workspace_id,
                name,
                bucket,
            })
            .collect();
        workspaces.sort_by(|a, b| {
            (a.name.is_none(), a.name.as_deref().map(str::to_lowercase))
                .cmp(&(b.name.is_none(), b.name.as_deref().map(str::to_lowercase)))
                .then_with(|| a.workspace_id.cmp(&b.workspace_id))
        });
        Ok(UsageTotals {
            people,
            workspaces,
            covered_percent,
        })
    }
}
