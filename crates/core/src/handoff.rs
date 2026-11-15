//! Hand-off notes (ADOPTION_PLAN X.16, PLAN.md T60, docs/handoff-notes.md):
//! what the next chat of a project learns of this one.
//!
//! A note has two parts. The **fixed part** ([`Facts`]) is built from the
//! log alone, never stored and never written by a model: what the person
//! asked first, the files the chat's turns changed (their `checkpoint`
//! `after` events, less the turns undone), the task list's open and done
//! items, and how the last turn ended. Every reader builds it again from
//! the log, so it is current even when the model part is off, failed or
//! not written yet. The **model part** is a `handoff_written` event, written
//! by the node between turns once the chat has gone idle ([`due`]).
//!
//! Everything a note holds that a model or a tool chose (the person's own
//! words aside) is model text to whoever reads it: file names, task items,
//! the model part. Readers quote every line ([`crate::event::quoted_line`]
//! behind [`crate::event::OUTPUT_QUOTE`]).

use crate::event::{CheckpointPhase, EventKind, RawEvent, TurnEndReason};
use crate::todo::{TodoItem, TodoStatus};

/// Characters of the model part kept (about 300 tokens): the note is a
/// pointer for the next chat, not a second summary.
pub const NOTE_CHARS: usize = 1500;
/// Characters of the person's first message the fixed part keeps.
pub const ASKED_CHARS: usize = 300;
/// Changed files the fixed part names; the rest are counted.
pub const MAX_FILES: usize = 12;
/// Open task items the fixed part names; the rest are counted.
pub const MAX_OPEN: usize = 8;
/// How long a chat waits idle (after its last turn ended) before its note
/// is written: Claude Code writes its recap after three minutes away and
/// Codex its memories from idle chats only (research round 1); ten
/// minutes keeps a person who pauses to read from paying for a note
/// mid-work, and is short enough that the next chat usually finds one.
pub const IDLE_MS: i64 = 10 * 60 * 1000;

/// The event kinds [`Facts::from_log`] and [`due`] read: a reader may load
/// only these.
pub const KINDS: &[&str] = &[
    "user_message",
    "turn_started",
    "turn_ended",
    "checkpoint",
    "todo_updated",
    "handoff_written",
];

/// The fixed part of a chat's note, from its log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The person's first message, cut to [`ASKED_CHARS`].
    pub asked: Option<String>,
    /// Turns the person started (not callback turns).
    pub turns: u32,
    /// Files the chat's turns changed and did not undo, first change first
    /// (paths as the checkpoint names them, relative to the chat's folder).
    pub files: Vec<String>,
    /// The latest task list, if one is set and not empty.
    pub todo: Option<Vec<TodoItem>>,
    /// How the last turn ended.
    pub ended: Option<TurnEndReason>,
}

impl Facts {
    pub fn from_log(log: &[RawEvent]) -> Facts {
        let mut f = Facts::default();
        // Per turn, in order: its changed files; undone turns are dropped.
        let mut changed: Vec<(String, Vec<String>)> = Vec::new();
        for e in log {
            match e.kind.as_str() {
                "user_message" if f.asked.is_none() => {
                    if let Ok(EventKind::UserMessage(m)) = e.typed()
                        && !m.text.trim().is_empty()
                    {
                        f.asked = Some(cut(m.text.trim(), ASKED_CHARS));
                    }
                }
                "turn_started" => {
                    if let Ok(EventKind::TurnStarted(t)) = e.typed()
                        && t.cause.is_none()
                    {
                        f.turns += 1;
                    }
                }
                "turn_ended" => {
                    if let Ok(EventKind::TurnEnded(t)) = e.typed() {
                        f.ended = Some(t.reason);
                    }
                }
                "checkpoint" => match e.typed() {
                    Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After => {
                        changed.retain(|(t, _)| *t != c.turn_id);
                        if !c.files.is_empty() {
                            changed.push((c.turn_id, c.files));
                        }
                    }
                    Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::Restored => {
                        changed.retain(|(t, _)| *t != c.turn_id);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        for (_, files) in changed {
            for p in files {
                if !f.files.contains(&p) {
                    f.files.push(p);
                }
            }
        }
        f.todo = crate::todo::latest(log).filter(|l| !l.is_empty());
        f
    }

    /// Whether there is anything to hand off: a file changed or a task list.
    /// A chat of questions and answers has none (its summary or last reply
    /// says what it was).
    pub fn worth(&self) -> bool {
        !self.files.is_empty() || self.todo.is_some()
    }

    /// The fixed part as lines, unquoted (the reader quotes each).
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(a) = &self.asked {
            let mut first = a.lines();
            out.push(format!("Asked first: {}", first.next().unwrap_or("")));
            out.extend(first.map(|l| format!("  {l}")));
        }
        if !self.files.is_empty() {
            let more = self.files.len().saturating_sub(MAX_FILES);
            let mut l = format!(
                "Files changed ({}): {}",
                self.files.len(),
                self.files[..self.files.len().min(MAX_FILES)].join(", ")
            );
            if more > 0 {
                l.push_str(&format!(" and {more} more"));
            }
            out.push(l);
        }
        if let Some(items) = &self.todo {
            let done = items
                .iter()
                .filter(|i| i.status == TodoStatus::Done)
                .count();
            let open: Vec<&TodoItem> = items
                .iter()
                .filter(|i| i.status != TodoStatus::Done)
                .collect();
            out.push(format!(
                "Task list: {done} of {} done, {} open",
                items.len(),
                open.len()
            ));
            for i in open.iter().take(MAX_OPEN) {
                out.push(format!("  [{}] {}", i.status.as_str(), i.content));
            }
            if open.len() > MAX_OPEN {
                out.push(format!("  and {} more open", open.len() - MAX_OPEN));
            }
        }
        if let Some(r) = self.ended {
            out.push(format!(
                "Last turn: {} ({} turn{} by the person)",
                ended_words(r),
                self.turns,
                if self.turns == 1 { "" } else { "s" }
            ));
        }
        out
    }
}

fn ended_words(r: TurnEndReason) -> &'static str {
    match r {
        TurnEndReason::Completed => "completed",
        TurnEndReason::Cancelled => "stopped by the person",
        TurnEndReason::Error => "ended on an error",
        TurnEndReason::Interrupted => "interrupted (the node restarted)",
        TurnEndReason::Stopped => "stopped (the node was stopped)",
        TurnEndReason::Limit => "used its step limit, work left",
    }
}

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// The model part, cut to [`NOTE_CHARS`] at a line break where one is
/// near the end, trimmed. Empty in, empty out.
pub fn cut_note(text: &str) -> String {
    let t = text.trim();
    if t.chars().count() <= NOTE_CHARS {
        return t.to_string();
    }
    let head: String = t.chars().take(NOTE_CHARS - 1).collect();
    let keep = match head.rfind('\n') {
        Some(i) if i > head.len() / 2 => &head[..i],
        _ => head.as_str(),
    };
    format!("{}…", keep.trim_end())
}

/// The latest note in `log`, with its own seq.
pub fn latest(log: &[RawEvent]) -> Option<(u64, crate::event::HandoffWritten)> {
    log.iter()
        .rev()
        .filter(|e| e.kind == "handoff_written")
        .find_map(|e| match e.typed() {
            Ok(EventKind::HandoffWritten(h)) => Some((e.seq, h)),
            _ => None,
        })
}

/// Whether a note is due for this log: its last turn has ended and no turn
/// started since, no note came after that end (one note per stretch of
/// activity; a note whose model failed counts, so a failing provider is
/// not asked again until the chat goes on), and since the latest note's
/// `through_seq` (or ever) a turn changed files or the task list. The
/// node writes it from its own head; the master asks once the chat has
/// been idle [`IDLE_MS`]. Reads only [`KINDS`].
pub fn due(log: &[RawEvent]) -> bool {
    let Some(end) = log.iter().rposition(|e| e.kind == "turn_ended") else {
        return false;
    };
    if log[end..].iter().any(|e| e.kind == "turn_started") {
        return false;
    }
    let from = match latest(log) {
        Some((seq, _)) if seq > log[end].seq => return false,
        Some((_, h)) => h.through_seq,
        None => 0,
    };
    let since = &log[log.partition_point(|e| e.seq <= from)..];
    let changed = since.iter().any(|e| match e.kind.as_str() {
        "todo_updated" => true,
        "checkpoint" => matches!(
            e.typed(),
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After && !c.files.is_empty()
        ),
        _ => false,
    });
    changed && Facts::from_log(log).worth()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::event::*;

    fn log(kinds: Vec<EventKind>) -> Vec<RawEvent> {
        let sid = SessionId::new();
        kinds
            .into_iter()
            .enumerate()
            .map(|(i, k)| {
                let (kind, payload) = k.into_parts();
                RawEvent {
                    session_id: sid,
                    seq: i as u64 + 1,
                    ts_ms: 0,
                    kind,
                    payload,
                }
            })
            .collect()
    }

    fn started(t: &str) -> EventKind {
        EventKind::TurnStarted(TurnStarted {
            turn_id: t.into(),
            agent: None,
            cause: None,
            environment: None,
        })
    }

    fn said(t: &str, text: &str) -> EventKind {
        EventKind::UserMessage(UserMessage {
            turn_id: t.into(),
            text: text.into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
            files: vec![],
        })
    }

    fn ended(t: &str, reason: TurnEndReason) -> EventKind {
        EventKind::TurnEnded(TurnEnded {
            turn_id: t.into(),
            reason,
            usage: None,
        })
    }

    fn after(t: &str, files: &[&str]) -> EventKind {
        EventKind::Checkpoint(Checkpoint {
            turn_id: t.into(),
            phase: CheckpointPhase::After,
            git_ref: "r".into(),
            files: files.iter().map(|f| f.to_string()).collect(),
            excluded: vec![],
            excluded_more: 0,
        })
    }

    fn todo(t: &str, items: &[(&str, TodoStatus)]) -> EventKind {
        EventKind::TodoUpdated(TodoUpdated {
            turn_id: t.into(),
            call_id: "c".into(),
            items: items
                .iter()
                .map(|(c, s)| TodoItem {
                    content: c.to_string(),
                    status: *s,
                })
                .collect(),
        })
    }

    fn note(through: u64) -> EventKind {
        EventKind::HandoffWritten(HandoffWritten {
            through_seq: through,
            text: Some("n".into()),
            model: Some("p::m".into()),
            failed: vec![],
            usage: None,
        })
    }

    /// A scripted coding chat: the fixed part says what was asked, which
    /// files changed (an undone turn's left out), the open items and how
    /// the last turn ended.
    #[test]
    fn the_fixed_part_comes_from_the_log() {
        use TodoStatus::*;
        let l = log(vec![
            started("t1"),
            said("t1", "Add a --json flag to the CLI\nand tests"),
            after("t1", &["src/cli.rs", "src/main.rs"]),
            todo(
                "t1",
                &[
                    ("Parse flag", Done),
                    ("Write tests", InProgress),
                    ("Docs", Pending),
                ],
            ),
            ended("t1", TurnEndReason::Completed),
            started("t2"),
            said("t2", "also try a yaml output"),
            after("t2", &["src/yaml.rs", "src/cli.rs"]),
            EventKind::Checkpoint(Checkpoint {
                turn_id: "t2".into(),
                phase: CheckpointPhase::Restored,
                git_ref: "r".into(),
                files: vec![],
                excluded: vec![],
                excluded_more: 0,
            }),
            ended("t2", TurnEndReason::Limit),
        ]);
        let f = Facts::from_log(&l);
        assert_eq!(
            f.asked.as_deref(),
            Some("Add a --json flag to the CLI\nand tests")
        );
        assert_eq!(f.files, vec!["src/cli.rs", "src/main.rs"]);
        assert_eq!(f.turns, 2);
        assert_eq!(f.ended, Some(TurnEndReason::Limit));
        assert!(f.worth());
        assert_eq!(
            f.lines(),
            vec![
                "Asked first: Add a --json flag to the CLI",
                "  and tests",
                "Files changed (2): src/cli.rs, src/main.rs",
                "Task list: 1 of 3 done, 2 open",
                "  [in_progress] Write tests",
                "  [pending] Docs",
                "Last turn: used its step limit, work left (2 turns by the person)",
            ]
        );
    }

    #[test]
    fn a_question_chat_has_nothing_to_hand_off() {
        let l = log(vec![
            started("t1"),
            said("t1", "what does this repo do?"),
            ended("t1", TurnEndReason::Completed),
        ]);
        assert!(!Facts::from_log(&l).worth());
        assert!(!due(&l));
    }

    #[test]
    fn a_note_is_due_once_per_stretch_of_changes() {
        let mut k = vec![
            started("t1"),
            said("t1", "fix it"),
            after("t1", &["a.rs"]),
            ended("t1", TurnEndReason::Completed),
        ];
        assert!(due(&log(k.clone())));
        // Running: not due.
        k.push(started("t2"));
        assert!(!due(&log(k.clone())));
        k.pop();
        // Written: not again.
        k.push(note(4));
        assert!(!due(&log(k.clone())));
        // A later turn that changed nothing: still not.
        k.extend([
            started("t2"),
            said("t2", "thanks"),
            ended("t2", TurnEndReason::Completed),
        ]);
        assert!(!due(&log(k.clone())));
        // One that changed a file: due again, and the latest note wins.
        k.extend([
            started("t3"),
            said("t3", "and b"),
            after("t3", &["b.rs"]),
            ended("t3", TurnEndReason::Completed),
        ]);
        assert!(due(&log(k.clone())));
        // A failed note (no text) covers it too: no retry loop.
        let n = k.len() as u64;
        k.push(EventKind::HandoffWritten(HandoffWritten {
            through_seq: n,
            text: None,
            model: None,
            failed: vec![NoteFailure {
                model: "p::m".into(),
                reason: "failed".into(),
            }],
            usage: None,
        }));
        assert!(!due(&log(k)));
    }

    #[test]
    fn notes_are_cut() {
        let long = format!("{}\n{}", "a".repeat(1000), "b".repeat(1000));
        let c = cut_note(&long);
        assert!(c.chars().count() <= NOTE_CHARS, "{}", c.chars().count());
        assert_eq!(c, format!("{}…", "a".repeat(1000)));
        assert_eq!(cut_note("  short \n"), "short");
    }
}
