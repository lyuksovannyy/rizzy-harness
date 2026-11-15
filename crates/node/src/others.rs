//! Another chat's work in the folder an undo writes (PLAN.md T57).
//!
//! A turn's checkpoints are snapshots of the whole folder, so whatever
//! another chat changed there while the turn ran is part of the turn's
//! `after`, and an undo would put it back with the turn's own changes,
//! silently. Before an undo (or a rewind) writes, the node looks at the
//! logs of the other chats working in an overlapping folder, from the
//! undone turn's start until now:
//!
//! - files their `edit` and `write` calls wrote (the `seen` record of each
//!   `tool_call_finished`), and files their own undos wrote: certain;
//! - files their other calls may have changed (a shell command, a
//!   sub-agent's merge, an MCP tool: nothing records which files): the
//!   files that changed during that chat's turn, by its own checkpoints,
//!   or every file the undo writes when that turn has none;
//! - a background command of theirs that ran in the window: every file the
//!   undo writes.
//!
//! The undo is refused, naming the files and nothing about the other chat,
//! unless forced ("Undo anyway"). A forced undo tells each such chat which
//! of its files were put back. Nothing here is kept in memory: it is read
//! from the logs and the checkpoint store, so it holds across a restart.
//! With one chat in a folder nothing is read at all.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use harness_core::SessionId;
use harness_core::event::{CheckpointPhase, EventKind, FileSeen};

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

/// The kinds read of another chat's log.
const KINDS: &[&str] = &[
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

/// What other chats did to the files an undo writes.
#[derive(Debug, Default)]
pub(crate) struct Foreign {
    /// Files another chat certainly changed.
    pub wrote: BTreeSet<String>,
    /// Files another chat may have changed (its commands), not in `wrote`.
    pub may: BTreeSet<String>,
    /// Per chat, the files it changed or may have changed in its own turns:
    /// what it is told after a forced undo.
    pub chats: Vec<(SessionId, BTreeSet<String>)>,
}

impl Foreign {
    pub fn is_empty(&self) -> bool {
        self.wrote.is_empty() && self.may.is_empty()
    }

    /// Every file named, certain or not.
    pub fn all(&self) -> BTreeSet<String> {
        self.wrote.union(&self.may).cloned().collect()
    }

    /// Why the undo is refused, naming files only: never the other chat,
    /// its text or its person (chats are private to their owner).
    pub fn refusal(&self) -> String {
        format!("nothing was changed: {}", self.why())
    }

    /// [`Self::refusal`] without its start, to follow another refusal.
    pub fn why(&self) -> String {
        let mut parts = Vec::new();
        if !self.wrote.is_empty() {
            parts.push(format!(
                "another chat working in this folder changed {} during or after this turn",
                listed(&self.wrote)
            ));
        }
        if !self.may.is_empty() {
            parts.push(format!(
                "another chat ran commands in this folder during or after this turn, so {} \
                 may hold its changes",
                listed(&self.may)
            ));
        }
        format!(
            "undo would overwrite another chat's work, not this turn's: {}. \"Undo anyway\" \
             overwrites it, and that chat is told which of its files were put back",
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
/// changed. Nothing about that chat.
pub(crate) fn reverted_note(files: &BTreeSet<String>) -> String {
    format!(
        "An undo in another chat working in this folder put back earlier versions of {}, which \
         this chat changed or may have changed. This chat's changes to them are no longer in \
         the folder, and this chat cannot bring them back: make them again if they are still \
         needed.",
        listed(files)
    )
}

/// The other chats' work on `touched` (paths relative to the checkpoint's
/// folder `repo.root`) from `since_ms` on. `others` are the chats whose
/// folder overlaps it, without the undoing chat and its sub-agents.
pub(crate) async fn foreign_work(
    store: &harness_store::Store,
    repo: &Repo,
    others: Vec<SessionId>,
    since_ms: i64,
    touched: &BTreeSet<String>,
) -> Foreign {
    let mut out = Foreign::default();
    if others.is_empty() || touched.is_empty() {
        return out;
    }
    let root = real(&repo.root);
    let all = || touched.clone();
    for sid in others {
        let events = store
            .call(move |s| s.events_since_turn_at(sid, since_ms - LOOK_BACK_MS, KINDS))
            .await;
        let mine = match events {
            Ok(events) => {
                let w = read_window(&events, since_ms);
                let mut mine = BTreeSet::new();
                for path in w.recorded {
                    if let Some(rel) = relative(&root, Path::new(&path))
                        && touched.contains(&rel)
                    {
                        out.wrote.insert(rel.clone());
                        mine.insert(rel);
                    }
                }
                for rel in w.restored.into_iter().filter(|p| touched.contains(p)) {
                    out.wrote.insert(rel.clone());
                    mine.insert(rel);
                }
                for (before, after) in w.turns {
                    let changed = match (before, after) {
                        (Some(b), Some(a))
                            if crate::git::exists(repo, &b).await
                                && crate::git::exists(repo, &a).await =>
                        {
                            crate::git::changed(repo, &b, &a).await.ok().map(|c| {
                                c.into_iter()
                                    .map(|(_, p)| p)
                                    .filter(|p| touched.contains(p))
                                    .collect::<BTreeSet<_>>()
                            })
                        }
                        _ => None,
                    };
                    // No checkpoints of that turn here (another folder's,
                    // pruned, or a chat that takes none): any file.
                    let changed = changed.unwrap_or_else(all);
                    mine.extend(changed.iter().cloned());
                    out.may.extend(changed);
                }
                if w.background {
                    out.may.extend(all());
                }
                mine
            }
            Err(e) => {
                tracing::warn!(session = %sid, error = %e, "another chat's log unreadable");
                out.may.extend(all());
                BTreeSet::new()
            }
        };
        if !mine.is_empty() {
            out.chats.push((sid, mine));
        }
    }
    out.may = out.may.difference(&out.wrote).cloned().collect();
    out
}

/// One chat's log, from the window's start on.
#[derive(Debug, Default)]
struct Window {
    /// Paths (real, absolute) its `edit` and `write` calls recorded.
    recorded: Vec<String>,
    /// Paths (relative) its own undos wrote.
    restored: Vec<String>,
    /// Its turns with a call that may have changed files, by their
    /// checkpoints (before, after).
    turns: Vec<(Option<String>, Option<String>)>,
    /// A background command of it ran in the window.
    background: bool,
}

fn read_window(events: &[harness_core::RawEvent], since: i64) -> Window {
    struct Call {
        tool: String,
        turn: String,
    }
    let mut calls: HashMap<String, Call> = HashMap::new();
    let mut finished: HashMap<String, (i64, bool, Option<FileSeen>)> = HashMap::new();
    let mut checkpoints: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
    let mut turn_end: HashMap<String, i64> = HashMap::new();
    let mut bg_started: HashMap<String, i64> = HashMap::new();
    let mut bg_ended: HashMap<String, i64> = HashMap::new();
    let mut started_order: Vec<String> = Vec::new();
    let mut w = Window::default();
    for e in events {
        let Ok(k) = e.typed() else { continue };
        match k {
            EventKind::ToolCallStarted(c) => {
                started_order.push(c.call_id.clone());
                calls.insert(
                    c.call_id,
                    Call {
                        tool: c.tool,
                        turn: c.turn_id,
                    },
                );
            }
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
                // Its own undo, in the window: the files it wrote.
                CheckpointPhase::BeforeRestore if e.ts_ms >= since => {
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
    for id in started_order {
        let Some(call) = calls.get(&id) else { continue };
        let fin = finished.get(&id);
        // A call without a result was cut (a restart): it ran until its
        // turn ended, or for all we know until now.
        let end = fin
            .map(|f| f.0)
            .or_else(|| turn_end.get(&call.turn).copied())
            .unwrap_or(i64::MAX);
        if end < since || NO_WRITES.contains(&call.tool.as_str()) {
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
    w.background = bg_started.iter().any(|(id, _)| {
        // Still running, or ended in the window.
        bg_ended.get(id).is_none_or(|end| *end >= since)
    });
    w
}

/// Where `p` really is (as given when it cannot be resolved).
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `path` (real, absolute, as a `seen` record has it) relative to `root`,
/// `/`-separated as a checkpoint names it; `None` outside.
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
    use harness_core::RawEvent;
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
        );
        assert_eq!(w.recorded, vec!["/f/b".to_string()]);
        assert!(w.turns.is_empty());
        assert!(!w.background);
    }

    #[test]
    fn a_command_counts_only_if_it_ended_in_the_window() {
        let events = [
            started(1, 100, "b", "bash"),
            finished(2, 110, "b", true, None),
        ];
        assert!(read_window(&events, 111).turns.is_empty());
        assert_eq!(read_window(&events, 105).turns.len(), 1);
        // An unknown tool is taken to write.
        let mcp = [
            started(1, 100, "m", "mcp__x__y"),
            finished(2, 110, "m", true, None),
        ];
        assert_eq!(read_window(&mcp, 50).turns.len(), 1);
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
        // Still running.
        assert!(read_window(&[bg(1, 100, true)], 500).background);
        // Ended inside the window.
        assert!(read_window(&[bg(1, 100, true), bg(2, 600, false)], 500).background);
        // Ended before it.
        assert!(!read_window(&[bg(1, 100, true), bg(2, 400, false)], 500).background);
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
