//! Another chat's work in the folder an undo writes (PLAN.md T57).
//!
//! A turn's checkpoints are snapshots of the whole folder, so whatever
//! another chat changed there while the turn ran is part of the turn's
//! `after`, and an undo would put it back with the turn's own changes,
//! silently. Before an undo (or a rewind) writes, the node looks at the
//! logs of the other chats working in an overlapping folder, over the
//! undone span: from the undone turn's `turn_started` (written before its
//! `before` snapshot scans the folder) to the checkpoint of the state the
//! undo expects (after it, the changed-since check compares content; an
//! interrupted restore has no such state, so its window runs until now):
//!
//! - files their `edit` and `write` calls wrote (the `seen` record of each
//!   `tool_call_finished`), and files their own undos wrote: certain;
//! - files their other calls may have changed (a shell command, a
//!   sub-agent's merge, an MCP tool: nothing records which files): the
//!   files that changed during that chat's turn, by its own checkpoints,
//!   or every file the undo writes when that turn has none;
//! - a background command of theirs running in the window: every file;
//! - a deleted chat whose file-changing calls overlap the window (its
//!   tombstone): every file.
//!
//! A file whose whole change the undoing chat's own `edit`/`write` calls
//! explain (a chain from `before` to `after` through each call's `base`
//! and result) is dropped from every "may" set: nothing foreign can be in
//! it. The undo is refused, naming the files and nothing about the other
//! chat, unless forced ("Undo anyway"). A forced undo tells each such chat
//! which of its files were put back. Nothing here is kept in memory: it is
//! read from the logs, the tombstones and the checkpoint stores, so it
//! holds across a restart. With one chat in a folder nothing is read.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use harness_core::event::{CheckpointPhase, EventKind, FileSeen};
use harness_core::{RawEvent, SessionId};

use crate::git::{Names, Repo};

/// Tools that never change a file in the folder: their calls are skipped.
/// Any tool not named here (and `edit` or `write` without a record) counts
/// as one that may have, so a tool added later errs towards asking.
const NO_WRITES: &[&str] = &[
    "read",
    "grep",
    "glob",
    "outline",
    "tool_output",
    "graph_query",
    "bash_output",
    "bash_stop",
    "web_fetch",
    "web_search",
    "todo",
    "remember",
    "skill",
];

/// The file tools whose result records what they wrote.
const RECORDED: &[&str] = &["edit", "write"];

/// The kinds read of a chat's log.
pub(crate) const KINDS: &[&str] = &[
    "tool_call_started",
    "tool_call_finished",
    "checkpoint",
    "turn_ended",
    "background_started",
    "background_ended",
];

/// How far before the window its log is read: a background command started
/// up to this long before it may still have been running in it (the
/// longest timeout any node allows), and its turn started before that.
const LOOK_BACK_MS: i64 = harness_tools::MAX_TIMEOUT_MAX_S as i64 * 1000;

/// The undone span, and what the undoing chat's own calls wrote in it.
#[derive(Debug, Clone)]
pub(crate) struct Span {
    /// The undone turn's start (its `turn_started`).
    pub since: i64,
    /// The expected state's checkpoint (`i64::MAX`: an interrupted
    /// restore, until now).
    pub end: i64,
    /// The snapshots the undo goes from and over: with both, a file the
    /// chat's own chain explains is not another chat's.
    pub before: Option<String>,
    pub after: Option<String>,
    /// The undoing chat's own `edit`/`write` calls in the span, in order:
    /// real path, base, result.
    pub own: Vec<(String, Option<String>, String)>,
}

/// When the undone span of `turn_id` began, from the undoing chat's own
/// log: its `turn_started`, written before the `before` snapshot scans the
/// folder (review F2: the checkpoint event is written after it, so an edit
/// landing between the two would be in `after` and outside the window).
/// A restore's id has no turn: the folder was claimed while it was saved,
/// so its first checkpoint event serves.
pub(crate) fn span_start(events: &[RawEvent], turn_id: &str) -> i64 {
    events
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnStarted(t)) if t.turn_id == turn_id => Some(e.ts_ms),
            Ok(EventKind::Checkpoint(c)) if c.turn_id == turn_id => Some(e.ts_ms),
            _ => None,
        })
        .unwrap_or(0)
}

/// The undoing chat's own `edit`/`write` results in `[since, end]`, in
/// order: (real path, base, result).
pub(crate) fn own_writes(
    events: &[RawEvent],
    since: i64,
    end: i64,
) -> Vec<(String, Option<String>, String)> {
    let mut tools: HashMap<String, (String, i64)> = HashMap::new();
    let mut out = Vec::new();
    for e in events {
        match e.typed() {
            Ok(EventKind::ToolCallStarted(c)) => {
                tools.insert(c.call_id, (c.tool, e.ts_ms));
            }
            Ok(EventKind::ToolCallFinished(f)) if f.ok => {
                let Some((tool, start)) = tools.get(&f.call_id) else {
                    continue;
                };
                if RECORDED.contains(&tool.as_str())
                    && *start >= since
                    && e.ts_ms <= end
                    && let Some(seen) = f.seen
                {
                    out.push((seen.path, seen.base, seen.sha256));
                }
            }
            _ => {}
        }
    }
    out
}

/// What other chats did to the files an undo writes.
#[derive(Debug, Default)]
pub(crate) struct Foreign {
    /// Files another chat certainly changed.
    pub wrote: BTreeSet<String>,
    /// Files another chat's commands may have changed.
    pub may: BTreeSet<String>,
    /// Files a background command of another chat may have changed.
    pub background: BTreeSet<String>,
    /// Files a chat deleted since may have changed.
    pub deleted: BTreeSet<String>,
    /// Per chat: the files attributed to it under its own folder, and
    /// whether it may have changed others nothing names (its turn has no
    /// checkpoints, or a background command): what it is told after a
    /// forced undo.
    pub chats: Vec<(SessionId, BTreeSet<String>, bool)>,
}

impl Foreign {
    pub fn is_empty(&self) -> bool {
        self.wrote.is_empty()
            && self.may.is_empty()
            && self.background.is_empty()
            && self.deleted.is_empty()
    }

    /// Every file named, certain or not.
    pub fn all(&self) -> BTreeSet<String> {
        let mut all = self.wrote.clone();
        all.extend(self.maybe());
        all
    }

    /// The files named that are not certain.
    pub fn maybe(&self) -> BTreeSet<String> {
        let mut m: BTreeSet<String> = self.may.clone();
        m.extend(self.background.iter().cloned());
        m.extend(self.deleted.iter().cloned());
        m.difference(&self.wrote).cloned().collect()
    }

    /// Why the undo is refused, naming files only: never the other chat,
    /// its text or its person (chats are private to their owner).
    pub fn refusal(&self) -> String {
        format!("nothing was changed: {}", self.why())
    }

    /// [`Self::refusal`] without its start, to follow another refusal.
    pub fn why(&self) -> String {
        let mut parts = Vec::new();
        let not = |s: &BTreeSet<String>| -> BTreeSet<String> {
            s.difference(&self.wrote).cloned().collect()
        };
        if !self.wrote.is_empty() {
            parts.push(format!(
                "another chat working in this folder changed {} during this turn",
                listed(&self.wrote)
            ));
        }
        let may = not(&self.may);
        if !may.is_empty() {
            parts.push(format!(
                "another chat ran commands in this folder during this turn, so {} may hold its \
                 changes",
                listed(&may)
            ));
        }
        let bg: BTreeSet<String> = not(&self.background).difference(&may).cloned().collect();
        if !bg.is_empty() {
            parts.push(format!(
                "a background command of another chat was running in this folder during this \
                 turn; any of {} may hold its changes",
                listed(&bg)
            ));
        }
        let gone: BTreeSet<String> = not(&self.deleted)
            .difference(&may)
            .cloned()
            .collect::<BTreeSet<_>>()
            .difference(&bg)
            .cloned()
            .collect();
        if !gone.is_empty() {
            parts.push(format!(
                "a chat deleted since changed files in this folder during this turn; any of {} \
                 may hold its changes",
                listed(&gone)
            ));
        }
        format!(
            "undo would overwrite another chat's work, not this turn's: {}. \"Undo anyway\" \
             overwrites it, and that chat is told its files were put back",
            parts.join("; ")
        )
    }
}

/// "`a`, `b` and 3 more", control characters made visible.
pub(crate) fn listed(paths: &BTreeSet<String>) -> String {
    let bytes: Vec<Vec<u8>> = paths.iter().map(|p| p.as_bytes().to_vec()).collect();
    Names::of(&bytes).listed()
}

/// The note a chat gets when another chat's forced undo put back files it
/// changed or may have: those attributed to it in its own folder, and, when
/// it may have changed others nothing names, a sentence without a list (no
/// path outside what it can tell is its own). Nothing about that chat.
pub(crate) fn reverted_note(files: &BTreeSet<String>, unlisted: bool) -> String {
    let what = match (files.is_empty(), unlisted) {
        (false, false) => format!(
            "earlier versions of {}, which this chat changed or may have changed",
            listed(files)
        ),
        (false, true) => format!(
            "earlier versions of {}, which this chat changed or may have changed, and of other \
             files in this folder its commands may have changed",
            listed(files)
        ),
        (true, _) => "earlier versions of files in this folder, which this chat's commands may \
                      have changed"
            .to_string(),
    };
    format!(
        "An undo in another chat working in this folder put back {what}. This chat's changes to \
         them are no longer in the folder, and this chat cannot bring them back: make them again \
         if they are still needed."
    )
}

/// One other chat: its id, its folder (real path) and its own checkpoint
/// repository when it has one (a chat in a folder containing this one, or
/// inside it, may have its own; its paths are relative to its own folder).
pub(crate) struct Other {
    pub id: SessionId,
    pub cwd: PathBuf,
    pub repo: Option<Repo>,
}

/// The other chats' work on `touched` (paths relative to the checkpoint's
/// folder `repo.root`) over `span`. `others` are the chats whose folder
/// overlaps it, without the undoing chat and its sub-agents; `tombstones`
/// every deleted chat's (folder, from, to).
pub(crate) async fn foreign_work(
    store: &harness_store::Store,
    repo: &Repo,
    others: Vec<Other>,
    tombstones: &[(String, i64, i64)],
    span: &Span,
    touched: &BTreeSet<String>,
) -> Foreign {
    let mut out = Foreign::default();
    if touched.is_empty() {
        return out;
    }
    let root = real(&repo.root);
    let all = || touched.clone();
    let (since, end) = (span.since, span.end);
    for (folder, from, to) in tombstones {
        if overlaps(Path::new(folder), &root) && *from <= end && *to >= since {
            out.deleted.extend(all());
        }
    }
    for other in others {
        let sid = other.id;
        let theirs = other.repo.as_ref().unwrap_or(repo);
        let their_root = real(&theirs.root);
        // A path of theirs as ours, if it is in this folder.
        let ours = |p: &str| relative(&root, &their_root.join(p));
        let events = store
            .call(move |s| s.events_since_turn_at(sid, since - LOOK_BACK_MS, KINDS))
            .await;
        let mut mine = BTreeSet::new();
        let mut unlisted = false;
        match events {
            Ok(events) => {
                let w = read_window(&events, since, end);
                for path in w.recorded {
                    if let Some(rel) = relative(&root, Path::new(&path))
                        && touched.contains(&rel)
                    {
                        out.wrote.insert(rel.clone());
                        mine.insert(rel);
                    }
                }
                for rel in w
                    .restored
                    .iter()
                    .filter_map(|p| ours(p))
                    .filter(|p| touched.contains(p))
                {
                    out.wrote.insert(rel.clone());
                    mine.insert(rel);
                }
                for (before, after) in w.turns {
                    let changed = match (before, after) {
                        (Some(b), Some(a))
                            if crate::git::exists(theirs, &b).await
                                && crate::git::exists(theirs, &a).await =>
                        {
                            crate::git::changed(theirs, &b, &a).await.ok().map(|c| {
                                c.into_iter()
                                    .filter_map(|(_, p)| ours(&p))
                                    .filter(|p| touched.contains(p))
                                    .collect::<BTreeSet<_>>()
                            })
                        }
                        _ => None,
                    };
                    match changed {
                        Some(c) => {
                            mine.extend(c.iter().cloned());
                            out.may.extend(c);
                        }
                        // No checkpoints of that turn (pruned, or a chat
                        // that takes none): any file.
                        None => {
                            unlisted = true;
                            out.may.extend(all());
                        }
                    }
                }
                if w.background {
                    unlisted = true;
                    out.background.extend(all());
                }
            }
            Err(e) => {
                tracing::warn!(session = %sid, error = %e, "another chat's log unreadable");
                unlisted = true;
                out.may.extend(all());
            }
        }
        // Only what lies in its own folder is named to it (review F5).
        let under = relative(&root, &other.cwd);
        mine.retain(|p| match &under {
            Some(prefix) => p == prefix || p.starts_with(&format!("{prefix}/")),
            // Its folder is this one or contains it.
            None => other.cwd.starts_with(&root) || root.starts_with(&other.cwd),
        });
        out.chats.push((sid, mine, unlisted));
    }
    // What the undoing chat's own calls explain is no one else's (F3).
    let explained = explained(repo, span, &root, &out.maybe()).await;
    for set in [&mut out.may, &mut out.background, &mut out.deleted] {
        set.retain(|p| !explained.contains(p));
    }
    let left = out.maybe();
    out.chats = std::mem::take(&mut out.chats)
        .into_iter()
        .filter_map(|(id, mut files, unlisted)| {
            files.retain(|p| out.wrote.contains(p) || !explained.contains(p));
            // "Other files" only while some unexplained file is left.
            let unlisted = unlisted && !left.is_empty();
            (!files.is_empty() || unlisted).then_some((id, files, unlisted))
        })
        .collect();
    out
}

/// The files of `candidates` whose whole change from `before` to `after`
/// the undoing chat's own `edit`/`write` calls explain, one after the
/// other: `before`'s content is the first call's base, each result the
/// next base, and the last result `after`'s content. Nothing else can then
/// be in `after`'s version of it.
async fn explained(
    repo: &Repo,
    span: &Span,
    root: &Path,
    candidates: &BTreeSet<String>,
) -> BTreeSet<String> {
    let (Some(before), Some(after)) = (&span.before, &span.after) else {
        return BTreeSet::new();
    };
    let mut chains: HashMap<String, Vec<(Option<String>, String)>> = HashMap::new();
    for (path, base, result) in &span.own {
        if let Some(rel) = relative(root, Path::new(path))
            && candidates.contains(&rel)
        {
            chains
                .entry(rel)
                .or_default()
                .push((base.clone(), result.clone()));
        }
    }
    if chains.is_empty() {
        return BTreeSet::new();
    }
    let paths: Vec<String> = chains.keys().cloned().collect();
    let from = crate::git::blob_hashes(repo, before, &paths).await;
    let to = crate::git::blob_hashes(repo, after, &paths).await;
    chains
        .into_iter()
        .filter(|(p, chain)| {
            let (Some(first), Some(last)) = (from.get(p), to.get(p)) else {
                return false;
            };
            chain_explains(first, chain, last)
        })
        .map(|(p, _)| p)
        .collect()
}

/// Whether `chain` (base, result per call, in order) takes `first` to
/// `last` with nothing between (`""`: no file).
fn chain_explains(first: &str, chain: &[(Option<String>, String)], last: &str) -> bool {
    if last.is_empty() || chain.is_empty() {
        return false;
    }
    let mut at = first;
    for (base, result) in chain {
        if base.as_deref() != Some(at) {
            return false;
        }
        at = result;
    }
    at == last
}

/// One chat's log over the window.
#[derive(Debug, Default)]
struct Window {
    /// Paths (real, absolute) its `edit` and `write` calls recorded.
    recorded: Vec<String>,
    /// Paths (relative to its own folder) its own undos wrote.
    restored: Vec<String>,
    /// Its turns with a call that may have changed files, by their
    /// checkpoints (before, after).
    turns: Vec<(Option<String>, Option<String>)>,
    /// A background command of it ran in the window.
    background: bool,
}

/// What a chat's events say happened in `[since, end]`: a call counts when
/// it ran at any time in it (it started by `end` and ended at `since` or
/// later), so a write that landed before the `after` snapshot scanned the
/// folder counts even if its result was logged after that checkpoint.
fn read_window(events: &[RawEvent], since: i64, end: i64) -> Window {
    struct Call {
        tool: String,
        turn: String,
        start: i64,
    }
    let mut calls: Vec<(String, Call)> = Vec::new();
    let mut finished: HashMap<String, (i64, bool, Option<FileSeen>)> = HashMap::new();
    let mut checkpoints: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let mut turn_end: HashMap<String, i64> = HashMap::new();
    let mut bg_started: HashMap<String, i64> = HashMap::new();
    let mut bg_ended: HashMap<String, i64> = HashMap::new();
    let mut w = Window::default();
    for e in events {
        let Ok(k) = e.typed() else { continue };
        match k {
            EventKind::ToolCallStarted(c) => calls.push((
                c.call_id,
                Call {
                    tool: c.tool,
                    turn: c.turn_id,
                    start: e.ts_ms,
                },
            )),
            EventKind::ToolCallFinished(f) => {
                finished.insert(f.call_id, (e.ts_ms, f.ok, f.seen));
            }
            EventKind::Checkpoint(c) => match c.phase {
                CheckpointPhase::Before => {
                    checkpoints.entry(c.turn_id).or_default().0 = Some(c.git_ref);
                }
                CheckpointPhase::After => {
                    checkpoints.entry(c.turn_id).or_default().1 = Some(c.git_ref);
                }
                // Its own undo in the window (no undo runs while another
                // chat's turn does): the files it wrote.
                CheckpointPhase::BeforeRestore if e.ts_ms >= since && e.ts_ms <= end => {
                    w.restored.extend(c.files);
                }
                _ => {}
            },
            EventKind::TurnEnded(t) => {
                turn_end.insert(t.turn_id, e.ts_ms);
            }
            EventKind::BackgroundStarted(b) => {
                bg_started.insert(b.bg_id, e.ts_ms);
            }
            EventKind::BackgroundEnded(b) => {
                bg_ended.insert(b.bg_id, e.ts_ms);
            }
            _ => {}
        }
    }
    let mut writing: Vec<String> = Vec::new();
    for (id, call) in &calls {
        let fin = finished.get(id);
        // A call without a result was cut (a restart): it ran until its
        // turn ended, or for all we know until now.
        let stop = fin
            .map(|f| f.0)
            .or_else(|| turn_end.get(&call.turn).copied())
            .unwrap_or(i64::MAX);
        if stop < since || call.start > end || NO_WRITES.contains(&call.tool.as_str()) {
            continue;
        }
        match fin {
            Some((_, true, Some(seen))) if RECORDED.contains(&call.tool.as_str()) => {
                w.recorded.push(seen.path.clone());
            }
            // A file tool that failed wrote nothing.
            Some((_, false, _)) if RECORDED.contains(&call.tool.as_str()) => {}
            _ => {
                if !writing.contains(&call.turn) {
                    writing.push(call.turn.clone());
                }
            }
        }
    }
    w.turns = writing
        .iter()
        .map(|t| checkpoints.get(t).cloned().unwrap_or((None, None)))
        .collect();
    w.background = bg_started
        .iter()
        .any(|(id, start)| *start <= end && bg_ended.get(id).is_none_or(|stop| *stop >= since));
    w
}

/// The time span of a chat's calls that may have changed files, its
/// background commands and its own undos, from its log: what its write
/// tombstone keeps once it is deleted. `None`: it changed nothing.
pub(crate) fn write_span(events: &[RawEvent], now: i64) -> Option<(i64, i64)> {
    let mut tools: HashMap<String, (String, i64)> = HashMap::new();
    let mut span: Option<(i64, i64)> = None;
    let mut add = |a: i64, b: i64| {
        span = Some(match span {
            Some((x, y)) => (x.min(a), y.max(b)),
            None => (a, b),
        });
    };
    let mut bg: HashMap<String, i64> = HashMap::new();
    let mut open_calls: HashMap<String, i64> = HashMap::new();
    for e in events {
        match e.typed() {
            Ok(EventKind::ToolCallStarted(c)) if !NO_WRITES.contains(&c.tool.as_str()) => {
                tools.insert(c.call_id.clone(), (c.tool, e.ts_ms));
                open_calls.insert(c.call_id, e.ts_ms);
            }
            Ok(EventKind::ToolCallFinished(f)) => {
                open_calls.remove(&f.call_id);
                if let Some((tool, start)) = tools.get(&f.call_id)
                    && (f.ok || !RECORDED.contains(&tool.as_str()))
                {
                    add(*start, e.ts_ms);
                }
            }
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::BeforeRestore => {
                add(e.ts_ms, e.ts_ms);
            }
            Ok(EventKind::BackgroundStarted(b)) => {
                bg.insert(b.bg_id, e.ts_ms);
            }
            Ok(EventKind::BackgroundEnded(b)) => {
                if let Some(start) = bg.remove(&b.bg_id) {
                    add(start, e.ts_ms);
                }
            }
            _ => {}
        }
    }
    // Cut calls and commands still running when it is deleted: until now.
    for start in open_calls.into_values().chain(bg.into_values()) {
        add(start, now);
    }
    span
}

/// Where `p` really is (as given when it cannot be resolved).
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// The same folder, or one inside the other.
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// `path` (real, absolute, as a `seen` record has it) relative to `root`,
/// `/`-separated as a checkpoint names it; `None` outside, or for `root`.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::*;

    fn ev(seq: u64, ts: i64, k: EventKind) -> RawEvent {
        let (kind, payload) = k.into_parts();
        RawEvent {
            session_id: SessionId::new(),
            seq,
            ts_ms: ts,
            kind,
            payload,
        }
    }

    fn started(seq: u64, ts: i64, call: &str, tool: &str) -> RawEvent {
        ev(
            seq,
            ts,
            EventKind::ToolCallStarted(ToolCallStarted {
                turn_id: "t".into(),
                call_id: call.into(),
                tool: tool.into(),
                input: serde_json::json!({}),
                author: None,
            }),
        )
    }

    fn finished(seq: u64, ts: i64, call: &str, ok: bool, path: Option<&str>) -> RawEvent {
        ev(
            seq,
            ts,
            EventKind::ToolCallFinished(ToolCallFinished {
                image: None,
                call_id: call.into(),
                ok,
                output: String::new(),
                exit_code: None,
                truncated: false,
                sources: vec![],
                note: None,
                seen: path.map(|p| FileSeen {
                    path: p.into(),
                    sha256: "0".repeat(64),
                    base: None,
                }),
            }),
        )
    }

    #[test]
    fn reads_and_failed_edits_count_for_nothing_recorded_writes_are_named() {
        let w = read_window(
            &[
                started(1, 100, "r", "read"),
                finished(2, 101, "r", true, Some("/f/a")),
                started(3, 102, "e", "edit"),
                finished(4, 103, "e", false, None),
                started(5, 104, "w", "write"),
                finished(6, 105, "w", true, Some("/f/b")),
            ],
            50,
            i64::MAX,
        );
        assert_eq!(w.recorded, vec!["/f/b".to_string()]);
        assert!(w.turns.is_empty());
        assert!(!w.background);
    }

    #[test]
    fn a_call_counts_only_if_it_ran_in_the_window() {
        let events = [
            started(1, 100, "b", "bash"),
            finished(2, 110, "b", true, None),
        ];
        assert!(read_window(&events, 111, i64::MAX).turns.is_empty());
        assert_eq!(read_window(&events, 105, i64::MAX).turns.len(), 1);
        // Started after the expected state's checkpoint: the changed-since
        // check's, not this one's (review F4).
        assert!(read_window(&events, 50, 99).turns.is_empty());
        // Started before it, its result logged after: it counts.
        assert_eq!(read_window(&events, 50, 105).turns.len(), 1);
        // An unknown tool is taken to write.
        let mcp = [
            started(1, 100, "m", "mcp__x__y"),
            finished(2, 110, "m", true, None),
        ];
        assert_eq!(read_window(&mcp, 50, i64::MAX).turns.len(), 1);
    }

    /// Review F2: the window starts at the turn's `turn_started`, written
    /// before the `before` snapshot scans the folder, not at the checkpoint
    /// event written after it. Another chat's edit one millisecond before
    /// that checkpoint event is inside it.
    #[test]
    fn the_window_starts_before_the_snapshot_scan() {
        let own = [
            ev(
                1,
                1_000,
                EventKind::TurnStarted(TurnStarted {
                    turn_id: "t".into(),
                    agent: None,
                    cause: None,
                    environment: None,
                }),
            ),
            ev(
                2,
                1_500,
                EventKind::Checkpoint(Checkpoint {
                    turn_id: "t".into(),
                    phase: CheckpointPhase::Before,
                    git_ref: "b".into(),
                    files: vec![],
                    excluded: vec![],
                    excluded_more: 0,
                }),
            ),
        ];
        let since = span_start(&own, "t");
        assert_eq!(since, 1_000);
        let other = [
            started(1, 1_498, "e", "edit"),
            finished(2, 1_499, "e", true, Some("/f/a")),
        ];
        assert_eq!(read_window(&other, since, i64::MAX).recorded, vec!["/f/a"]);
    }

    fn bg(seq: u64, ts: i64, started: bool) -> RawEvent {
        let k = if started {
            EventKind::BackgroundStarted(BackgroundStarted {
                bg_id: "bg-1".into(),
                call_id: "c".into(),
                turn_id: "t".into(),
                timeout_s: 60,
                ends_by_ms: ts + 60_000,
            })
        } else {
            EventKind::BackgroundEnded(BackgroundEnded {
                bg_id: "bg-1".into(),
                reason: BackgroundReason::Exited,
                exit_code: Some(0),
                signal: None,
                confirmed: BackgroundConfirmed::Unknown,
                ran_ms: 1,
                output_bytes: 0,
                tail: String::new(),
                tail_cut: false,
                delivered: false,
            })
        };
        ev(seq, ts, k)
    }

    /// (The integration tests cannot start one: background commands need
    /// the shell's containment, which `crates/node/tests` runs without.)
    #[test]
    fn a_background_command_counts_while_it_runs_in_the_window() {
        let max = i64::MAX;
        // Still running.
        assert!(read_window(&[bg(1, 100, true)], 500, max).background);
        // Ended inside the window.
        assert!(read_window(&[bg(1, 100, true), bg(2, 600, false)], 500, max).background);
        // Ended before it.
        assert!(!read_window(&[bg(1, 100, true), bg(2, 400, false)], 500, max).background);
        // Started after the window's end (review F4).
        assert!(!read_window(&[bg(1, 700, true)], 500, 600).background);
    }

    #[test]
    fn a_chain_of_own_writes_explains_a_file_only_end_to_end() {
        let c = |pairs: &[(&str, &str)]| -> Vec<(Option<String>, String)> {
            pairs
                .iter()
                .map(|(b, r)| (Some(b.to_string()), r.to_string()))
                .collect()
        };
        assert!(chain_explains("a", &c(&[("a", "b"), ("b", "c")]), "c"));
        // Something changed it between two of its calls.
        assert!(!chain_explains("a", &c(&[("a", "b"), ("x", "c")]), "c"));
        // Before the first, or after the last.
        assert!(!chain_explains("z", &c(&[("a", "b")]), "b"));
        assert!(!chain_explains("a", &c(&[("a", "b")]), "y"));
        // A new file: nothing before it.
        assert!(chain_explains("", &c(&[("", "b")]), "b"));
        // A base not known explains nothing; neither does a deleted file.
        assert!(!chain_explains("a", &[(None, "b".into())], "b"));
        assert!(!chain_explains("a", &c(&[("a", "")]), ""));
    }

    #[test]
    fn a_deleted_chats_span_covers_its_writing_calls_only() {
        let events = [
            started(1, 100, "r", "read"),
            finished(2, 101, "r", true, Some("/f/a")),
            started(3, 200, "b", "bash"),
            finished(4, 210, "b", true, None),
            started(5, 300, "e", "edit"),
            finished(6, 301, "e", false, None),
        ];
        assert_eq!(write_span(&events, 1_000), Some((200, 210)));
        assert_eq!(write_span(&events[..2], 1_000), None);
        assert_eq!(write_span(&[bg(1, 400, true)], 1_000), Some((400, 1_000)));
    }

    /// Review F5: a chat whose share nothing names learns no path.
    #[test]
    fn a_chat_is_told_only_its_own_files() {
        let one: BTreeSet<String> = ["sub/x.py".to_string()].into();
        let n = reverted_note(&one, false);
        assert!(
            n.contains("`sub/x.py`") && !n.contains("other files"),
            "{n}"
        );
        let n = reverted_note(&BTreeSet::new(), true);
        assert!(
            n.contains("files in this folder") && !n.contains('`'),
            "{n}"
        );
    }

    #[test]
    fn paths_outside_the_folder_are_not_named() {
        let root = Path::new("/w/proj");
        assert_eq!(
            relative(root, Path::new("/w/proj/a/b.py")).as_deref(),
            Some("a/b.py")
        );
        assert_eq!(relative(root, Path::new("/w/projx/a")), None);
        assert_eq!(relative(root, Path::new("/w/proj")), None);
    }
}
