//! Forking a chat (ADOPTION_PLAN C.2, docs/fork.md): the one rule every
//! reader of a fork's log shares.
//!
//! A fork is a new chat on the same node whose log begins with a copy of
//! its parent's events up to the end of a completed turn, **at the same
//! seqs**, and then goes its own way. Nothing is renumbered, so every seq
//! inside a payload (`history_summarized.through_seq`, `mention.message_seq`,
//! a rewind's `message_seq`, `queued_delivered.seqs`) is still right and the
//! folds and the model's rebuild need no second code path. The fork's first
//! event is the parent's `session_created` with a new title, the folder the
//! node chose and `forked: {parent, through_seq}`; the event after the copy
//! is `session_forked`; background commands the parent had running at the
//! cut are closed by a `background_ended` that says they belong to the
//! parent ([`BackgroundReason::Forked`], which never calls back).
//!
//! [`inherited`] says which events are copies. What a copy is not: it costs
//! the fork nothing (the parent's usage already holds it), it grants
//! nothing (grants are bound to the parent's folder), its checkpoints are
//! not restorable in the fork, and the master runs none of the side effects
//! it ran when it first applied the event (a memory fact, an artifact, a
//! callback turn, a connector call). [`check`] is the node's and the
//! master's one check of a cut, as `retry::check` and `rewind::check` are
//! theirs; [`build`] writes the fork's events.
//! `web/src/fold.ts` is a second implementation of the fold, pinned by the
//! shared fold fixture.

use crate::RawEvent;
use crate::SessionId;
use crate::event::{
    BackgroundConfirmed, BackgroundEnded, BackgroundReason, EventKind, ForkedFrom, PeersSkipReason,
    PeersSkipped, SessionForked,
};

/// The event kinds [`check`] reads: a reader (the master, from its mirror)
/// may load only these.
pub const KINDS: &[&str] = &[
    "session_created",
    "turn_started",
    "turn_ended",
    "user_message",
    "conversation_rewound",
    "conversation_rewind_undone",
];

/// What the fork's title ends with, after the parent's.
pub const TITLE_SUFFIX: &str = " (fork)";

/// Where a fork stops: the end of one completed turn of the parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    /// The seq of that turn's `turn_ended`: the last event copied.
    pub through_seq: u64,
    pub turn_id: String,
}

/// Whether the event at `seq` of a chat whose first `through` events are
/// its parent's (`session_created.forked.through_seq`, 0 for a chat that is
/// no fork) is a copy. The first event is the fork's own creation (the
/// parent's, changed in its title, folder and `forked`) and never a copy:
/// the chat is claimed, listed and searched by it.
pub fn inherited(through: u64, seq: u64) -> bool {
    1 < seq && seq <= through
}

/// `through` of a chat from its `session_created`.
pub fn through(created: &crate::event::SessionCreated) -> u64 {
    created.forked.map_or(0, |f| f.through_seq)
}

/// `log` as what a chat's OWN turns say, for the readers that speak for
/// the chat as it is now (the "earlier chats" list, hand-off notes and
/// whether one is due): the copies of a fork are the parent's and are
/// already spoken for there. `through` is [`through`] of the chat (0: no
/// fork, the log as it is, borrowed). What stays of a copy are the rewind
/// events and the messages they went back to, so a rewind of the fork's own
/// that went back to a copied message still counts (`rewind::rewound`).
pub fn own(log: &[RawEvent], through: u64) -> std::borrow::Cow<'_, [RawEvent]> {
    if through < 2 {
        return std::borrow::Cow::Borrowed(log);
    }
    let targets: std::collections::HashSet<u64> = log
        .iter()
        .filter(|e| e.kind == "conversation_rewound")
        .filter_map(|e| e.payload.get("message_seq")?.as_u64())
        .collect();
    std::borrow::Cow::Owned(
        log.iter()
            .filter(|e| {
                !inherited(through, e.seq)
                    || matches!(
                        e.kind.as_str(),
                        "conversation_rewound" | "conversation_rewind_undone"
                    )
                    || (e.kind == "user_message" && targets.contains(&e.seq))
            })
            .cloned()
            .collect(),
    )
}

/// [`through`] of the chat whose log (from its first event) is `log`; 0 when
/// the log does not begin with a creation, or the chat is no fork.
pub fn through_of(log: &[RawEvent]) -> u64 {
    match log.first().map(|e| e.typed()) {
        Some(Ok(EventKind::SessionCreated(c))) => through(&c),
        _ => 0,
    }
}

/// The fork's title from its parent's, for the first fork of a chat.
pub fn title(parent: &str) -> String {
    title_nth(parent, 1)
}

/// The title of the `n`th fork (1 for the first) of the original chat the
/// parent descends from: `X (fork)`, `X (fork 2)`, ... A suffix of this
/// kind already on the parent's title (the parent is itself a fork) is
/// stripped first, so a fork of a fork is the next number of the same
/// original, never `X (fork) (fork)`.
pub fn title_nth(parent: &str, n: u64) -> String {
    let base = strip_suffix(parent);
    if n <= 1 {
        format!("{base}{TITLE_SUFFIX}")
    } else {
        format!("{base} (fork {n})")
    }
}

/// `title` without one trailing ` (fork)` or ` (fork N)` (N from 2 up, no
/// leading zero). Nothing is stripped when the whole title would go.
fn strip_suffix(title: &str) -> &str {
    let Some(head) = title.strip_suffix(')') else {
        return title;
    };
    let Some(at) = head.rfind(" (fork") else {
        return title;
    };
    let rest = &head[at + " (fork".len()..];
    let numbered = rest
        .strip_prefix(' ')
        .is_some_and(|n| n.parse::<u64>().is_ok_and(|n| n >= 2) && !n.starts_with('0'));
    if (rest.is_empty() || numbered) && at > 0 {
        &title[..at]
    } else {
        title
    }
}

/// Whether the chat in `log` (the whole log, or [`KINDS`] of it) may be
/// forked at the end of turn `turn_id`: the cut, or why not in the
/// person's words. Refused: a turn that has not ended (the cut is never
/// inside a turn), one a rewind took out of the conversation, a sub-agent's
/// chat and a routine's run. The parent may be running a later turn: the
/// copy reads only what came before the cut.
pub fn check(log: &[RawEvent], turn_id: &str) -> Result<Cut, String> {
    if log.iter().any(|e| {
        e.kind == "session_created"
            && matches!(e.typed(), Ok(EventKind::SessionCreated(c))
                if c.parent_session.is_some())
    }) {
        return Err("a sub-agent's chat belongs to the agent that delegated it".into());
    }
    if log.iter().any(|e| {
        e.kind == "session_created"
            && matches!(e.typed(), Ok(EventKind::SessionCreated(c)) if c.no_callbacks)
    }) {
        return Err("a routine's own run is not forked; run the routine again".into());
    }
    let ended = log.iter().find(|e| {
        e.kind == "turn_ended"
            && matches!(e.typed(), Ok(EventKind::TurnEnded(t)) if t.turn_id == turn_id)
    });
    let Some(ended) = ended else {
        let started = log.iter().any(|e| {
            e.kind == "turn_started"
                && matches!(e.typed(), Ok(EventKind::TurnStarted(t)) if t.turn_id == turn_id)
        });
        return Err(if started {
            "that turn is still running; fork from a turn that has finished".into()
        } else {
            "that turn is not in this chat".into()
        });
    };
    let ranges = crate::rewind::rewound(log);
    if ranges.holds(ended.seq) || ranges.is_target(turn_id) {
        return Err("a rewind took that turn out of the conversation".into());
    }
    Ok(Cut {
        through_seq: ended.seq,
        turn_id: turn_id.to_string(),
    })
}

/// The events of the fork `new` of the chat whose log is `parent_log`
/// (at least `1..=cut.through_seq`, contiguous), made at `now_ms`:
/// 1. the parent's `session_created` with the fork's `title` ([`title`] of
///    the parent's), `cwd` (the node decides it) and `forked`;
/// 2. the parent's events `2..=through_seq`, as they are, under the new id;
/// 3. `session_forked`;
/// 4. when the cut is inside a message that called several agents, a
///    `peers_skipped` ([`PeersSkipReason::Forked`]) for those not yet asked;
/// 5. a `background_ended` ([`BackgroundReason::Forked`]) for each command
///    or background agent started in the copy and not ended in it.
///
/// A chat whose first event is not a `session_created`, or whose seqs are
/// not `1, 2, 3, …`, is refused: nothing is built from a log the fold would
/// refuse.
pub fn build(
    parent_log: &[RawEvent],
    cut: &Cut,
    new: SessionId,
    title: &str,
    cwd: &str,
    now_ms: i64,
) -> Result<Vec<RawEvent>, String> {
    let n = cut.through_seq as usize;
    if n < 2 || parent_log.len() < n {
        return Err("the cut is not in the chat's log".into());
    }
    let copy = &parent_log[..n];
    if copy.iter().enumerate().any(|(i, e)| e.seq != i as u64 + 1) {
        return Err("the chat's log is not contiguous".into());
    }
    let parent = copy[0].session_id;
    if copy.iter().any(|e| e.session_id != parent) || parent == new {
        return Err("the chat's log is not one chat's".into());
    }
    let Ok(EventKind::SessionCreated(mut created)) = copy[0].typed() else {
        return Err("the chat's first event is not its creation".into());
    };
    if copy[n - 1].kind != "turn_ended" {
        return Err("the cut is not the end of a turn".into());
    }
    created.title = title.to_string();
    created.cwd = cwd.to_string();
    created.forked = Some(ForkedFrom {
        parent,
        through_seq: cut.through_seq,
        own_folder: false,
    });
    let mut out: Vec<RawEvent> = Vec::with_capacity(n + 2);
    for e in copy {
        let mut e = e.clone();
        e.session_id = new;
        out.push(e);
    }
    let (kind, payload) = EventKind::SessionCreated(created).into_parts();
    out[0].kind = kind;
    out[0].payload = payload;
    out[0].ts_ms = now_ms;
    // What ran in the parent at the cut.
    let mut open: Vec<(String, bool)> = Vec::new();
    for e in copy {
        match e.typed() {
            Ok(EventKind::BackgroundStarted(b)) => open.push((b.bg_id, b.child.is_some())),
            Ok(EventKind::BackgroundEnded(b)) => open.retain(|(id, _)| *id != b.bg_id),
            _ => {}
        }
    }
    let mut push = |k: EventKind| {
        let (kind, payload) = k.into_parts();
        let seq = out.len() as u64 + 1;
        out.push(RawEvent {
            session_id: new,
            seq,
            ts_ms: now_ms,
            kind,
            payload,
        });
    };
    push(EventKind::SessionForked(SessionForked {
        parent,
        through_seq: cut.through_seq,
        turn_id: cut.turn_id.clone(),
    }));
    // A cut inside a message that called several agents: the ones with no
    // turn at the cut are not asked in the fork, and the log says so now
    // rather than at a later node start (which would say "restarted").
    if let Some((message_seq, agents)) = crate::mention::unasked(copy) {
        push(EventKind::PeersSkipped(PeersSkipped {
            message_seq,
            agents,
            reason: PeersSkipReason::Forked,
        }));
    }
    for (bg_id, _) in open {
        push(EventKind::BackgroundEnded(BackgroundEnded {
            bg_id,
            reason: BackgroundReason::Forked,
            exit_code: None,
            signal: None,
            confirmed: BackgroundConfirmed::Unknown,
            ran_ms: 0,
            output_bytes: 0,
            tail: String::new(),
            tail_cut: false,
            delivered: false,
            child: None,
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;

    /// "X (fork)", "X (fork 2)", ...: counted per original chat, an existing
    /// suffix of the parent's title stripped first (never "X (fork) (fork)").
    #[test]
    fn titles_are_numbered_per_original_and_never_stack_suffixes() {
        assert_eq!(title_nth("X", 1), "X (fork)");
        assert_eq!(title_nth("X", 2), "X (fork 2)");
        assert_eq!(title_nth("X (fork)", 3), "X (fork 3)");
        assert_eq!(title_nth("X (fork 2)", 4), "X (fork 4)");
        assert_eq!(title_nth("X (fork 12)", 13), "X (fork 13)");
        assert_eq!(title("X (fork)"), "X (fork)");
        // Only one trailing suffix is the harness's; the rest is the title.
        assert_eq!(title_nth("X (fork) (fork)", 2), "X (fork) (fork 2)");
        // Not a suffix of ours: kept.
        assert_eq!(title_nth("X (forked)", 1), "X (forked) (fork)");
        assert_eq!(title_nth("X (fork 2x)", 1), "X (fork 2x) (fork)");
        assert_eq!(title_nth("X (fork 0)", 1), "X (fork 0) (fork)");
        assert_eq!(title_nth("X(fork)", 1), "X(fork) (fork)");
        // A title that is only the suffix keeps it as its name.
        assert_eq!(title_nth(" (fork)", 2), " (fork) (fork 2)");
        assert_eq!(title_nth("", 1), " (fork)");
    }
    use crate::policy::ApprovalMode;
    use crate::view::Item;

    struct Log {
        sid: SessionId,
        events: Vec<RawEvent>,
    }

    fn created(parent: Option<SessionId>, no_callbacks: bool) -> EventKind {
        EventKind::SessionCreated(SessionCreated {
            forked: None,
            title: "Parent".into(),
            cwd: "/w/proj".into(),
            model: "m".into(),
            backend: "b".into(),
            node_id: "n".into(),
            approval_mode: ApprovalMode::AcceptEdits,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: parent,
            no_callbacks,
            exec_mode: None,
        })
    }

    impl Log {
        fn new() -> Log {
            let mut l = Log {
                sid: SessionId::new(),
                events: vec![],
            };
            l.push(created(None, false));
            l
        }
        fn push(&mut self, k: EventKind) -> u64 {
            let (kind, payload) = k.into_parts();
            let seq = self.events.len() as u64 + 1;
            self.events.push(RawEvent {
                session_id: self.sid,
                seq,
                ts_ms: 7,
                kind,
                payload,
            });
            seq
        }
        fn started(&mut self, t: &str) -> u64 {
            self.push(EventKind::TurnStarted(TurnStarted {
                turn_id: t.into(),
                agent: None,
                cause: None,
                environment: None,
                retry: None,
                continued_by: None,
            }))
        }
        fn message(&mut self, t: &str) -> u64 {
            self.push(EventKind::UserMessage(
                serde_json::from_value(serde_json::json!({"turn_id": t, "text": "do it"})).unwrap(),
            ))
        }
        fn ended(&mut self, t: &str) -> u64 {
            self.push(EventKind::TurnEnded(TurnEnded {
                turn_id: t.into(),
                reason: TurnEndReason::Completed,
                usage: Some(Usage {
                    input_tokens: 100,
                    output_tokens: 10,
                    ..Usage::default()
                }),
            }))
        }
        fn turn(&mut self, t: &str) -> u64 {
            self.started(t);
            self.message(t);
            self.ended(t)
        }
        fn bg_started(&mut self, bg: &str, t: &str) -> u64 {
            self.push(EventKind::BackgroundStarted(BackgroundStarted {
                bg_id: bg.into(),
                call_id: "c".into(),
                turn_id: t.into(),
                timeout_s: 60,
                ends_by_ms: 1,
                child: None,
            }))
        }
        fn bg_ended(&mut self, bg: &str) -> u64 {
            self.push(EventKind::BackgroundEnded(BackgroundEnded {
                bg_id: bg.into(),
                reason: BackgroundReason::Exited,
                exit_code: Some(0),
                signal: None,
                confirmed: BackgroundConfirmed::Ended,
                ran_ms: 1,
                output_bytes: 0,
                tail: String::new(),
                tail_cut: false,
                delivered: false,
                child: None,
            }))
        }
    }

    /// What a fork's own readers see: nothing of the copy but the first
    /// event, the rewind events and the messages they name.
    #[test]
    fn own_drops_the_copy_and_keeps_what_a_rewind_names() {
        let mut l = Log::new();
        l.turn("t1"); // seqs 2..=4, the message is 3
        l.turn("t2"); // 5..=7
        let rewind = l.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq: 3,
            turn_id: "t1".into(),
            files: false,
        })); // 8
        let through = l.turn("t3"); // 9..=11
        l.turn("own"); // 12..=14
        let got = own(&l.events, through);
        let seqs: Vec<u64> = got.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![1, 3, rewind, 12, 13, 14]);
        // No fork: the log itself.
        assert!(matches!(own(&l.events, 0), std::borrow::Cow::Borrowed(_)));
        assert_eq!(through_of(&l.events), 0);
    }

    #[test]
    fn check_names_the_end_of_a_finished_turn() {
        let mut l = Log::new();
        let e1 = l.turn("t1");
        let e2 = l.turn("t2");
        // The parent may be running a later turn.
        l.started("t3");
        let cut = check(&l.events, "t1").unwrap();
        assert_eq!((cut.through_seq, cut.turn_id.as_str()), (e1, "t1"));
        assert_eq!(check(&l.events, "t2").unwrap().through_seq, e2);
    }

    #[test]
    fn check_refuses_a_running_turn_an_unknown_one_and_a_rewound_one() {
        let mut l = Log::new();
        l.turn("t1");
        l.started("t2");
        let e = check(&l.events, "t2").unwrap_err();
        assert!(e.contains("still running"), "{e}");
        let e = check(&l.events, "nope").unwrap_err();
        assert!(e.contains("not in this chat"), "{e}");
        l.message("t2");
        l.ended("t2");
        // t2's message is the target of a rewind: the turn is gone.
        let m = l
            .events
            .iter()
            .rposition(|e| e.kind == "user_message")
            .unwrap() as u64
            + 1;
        l.push(EventKind::ConversationRewound(ConversationRewound {
            message_seq: m,
            turn_id: "t2".into(),
            files: false,
        }));
        let e = check(&l.events, "t2").unwrap_err();
        assert!(e.contains("rewind"), "{e}");
        assert!(check(&l.events, "t1").is_ok());
        // Put back, it can be forked again.
        let r = l.events.len() as u64;
        l.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq: r },
        ));
        assert!(check(&l.events, "t2").is_ok());
    }

    #[test]
    fn check_refuses_a_sub_agents_chat_and_a_routines_run() {
        let mut sub = Log {
            sid: SessionId::new(),
            events: vec![],
        };
        sub.push(created(Some(SessionId::new()), false));
        sub.turn("t1");
        let e = check(&sub.events, "t1").unwrap_err();
        assert!(e.contains("sub-agent"), "{e}");
        let mut run = Log {
            sid: SessionId::new(),
            events: vec![],
        };
        run.push(created(None, true));
        run.turn("t1");
        let e = check(&run.events, "t1").unwrap_err();
        assert!(e.contains("routine"), "{e}");
    }

    #[test]
    fn the_first_event_is_never_a_copy() {
        assert!(!inherited(0, 1) && !inherited(0, 5));
        assert!(!inherited(9, 1), "the fork's own creation");
        assert!(inherited(9, 2) && inherited(9, 9));
        assert!(!inherited(9, 10));
    }

    #[test]
    fn a_fork_has_the_same_seqs_and_its_own_first_event_and_divider() {
        let mut l = Log::new();
        l.turn("t1");
        let t2 = l.turn("t2");
        // The parent has gone on.
        l.turn("t3");
        let cut = check(&l.events, "t2").unwrap();
        let new = SessionId::new();
        let f = build(&l.events, &cut, new, &title("Parent"), "/w/other", 1234).unwrap();
        assert_eq!(f.len() as u64, t2 + 1);
        for (i, e) in f.iter().enumerate() {
            assert_eq!(e.seq, i as u64 + 1);
            assert_eq!(e.session_id, new);
        }
        for (copy, original) in f.iter().zip(&l.events).take(t2 as usize).skip(1) {
            assert_eq!(
                (&copy.kind, &copy.payload, copy.ts_ms),
                (&original.kind, &original.payload, original.ts_ms),
                "event {} is the parent's, as it was",
                copy.seq
            );
        }
        let Ok(EventKind::SessionCreated(c)) = f[0].typed() else {
            panic!("{:?}", f[0]);
        };
        assert_eq!(c.title, "Parent (fork)");
        assert_eq!(c.cwd, "/w/other");
        assert_eq!(
            c.forked,
            Some(ForkedFrom {
                parent: l.sid,
                through_seq: t2,
                own_folder: false
            })
        );
        assert_eq!(c.parent_session, None, "a fork is not a sub-agent");
        assert_eq!(f[0].ts_ms, 1234);
        let Ok(EventKind::SessionForked(d)) = f[t2 as usize].typed() else {
            panic!("{:?}", f[t2 as usize]);
        };
        assert_eq!(
            (d.parent, d.through_seq, d.turn_id.as_str()),
            (l.sid, t2, "t2")
        );
        // The parent's own log is not touched.
        assert_eq!(l.events[0].session_id, l.sid);
        assert!(
            matches!(l.events[0].typed(), Ok(EventKind::SessionCreated(c)) if c.forked.is_none())
        );
    }

    #[test]
    fn what_ran_at_the_cut_is_closed_in_the_fork_and_never_calls_back() {
        let mut l = Log::new();
        l.started("t1");
        l.message("t1");
        l.bg_started("bg-1", "t1");
        l.bg_started("bg-2", "t1");
        l.bg_ended("bg-1");
        let end = l.ended("t1");
        // A later end is the parent's.
        l.bg_ended("bg-2");
        let cut = check(&l.events, "t1").unwrap();
        let f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        assert_eq!(f.len() as u64, end + 2, "the divider and one closing");
        let Ok(EventKind::BackgroundEnded(b)) = f[end as usize + 1].typed() else {
            panic!("{:?}", f.last());
        };
        assert_eq!(b.bg_id, "bg-2");
        assert_eq!(b.reason, BackgroundReason::Forked);
        assert!(!b.calls_back(), "a closing never resumes the chat");
        let note = b.model_note().unwrap();
        assert!(
            note.starts_with(NOTE_LEAD) && note.contains("forked"),
            "{note}"
        );
        let v = crate::view::SessionView::fold(&f).unwrap();
        assert!(v.running_background().is_empty());
    }

    /// A cut inside a message that called several agents: the agents that
    /// had no turn at the cut are said not to be asked, once, in the fork,
    /// with the reason that the chat was forked; so nothing about them
    /// appears at a later node start (`mention::unasked`). A list that had
    /// finished says nothing.
    #[test]
    fn a_cut_inside_a_list_of_agents_says_who_was_not_asked() {
        let mut l = Log::new();
        l.started("t1");
        let m = l.push(EventKind::UserMessage(
            serde_json::from_value(serde_json::json!({
                "turn_id": "t1", "text": "@a @b @c go", "triggers": ["a", "b", "c"]
            }))
            .unwrap(),
        ));
        let end = l.ended("t1");
        // The parent goes on with b's turn after the cut.
        l.push(EventKind::TurnStarted(TurnStarted {
            turn_id: "t2".into(),
            agent: Some(AgentRef {
                agent_id: "b".into(),
                name: "b".into(),
                prompt_hash: String::new(),
                withheld: vec![],
                effort: None,
            }),
            cause: Some(TurnCause::Mention(MentionCall {
                message_seq: m,
                by: CalledBy::Person,
                tags: vec![],
            })),
            environment: None,
            continued_by: None,
            retry: None,
        }));
        l.ended("t2");
        let cut = check(&l.events, "t1").unwrap();
        let f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        assert_eq!(f.len() as u64, end + 2, "the divider and the note");
        let Ok(EventKind::PeersSkipped(p)) = f[end as usize + 1].typed() else {
            panic!("{:?}", f.last());
        };
        assert_eq!(p.message_seq, m);
        assert_eq!(p.agents, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(p.reason, PeersSkipReason::Forked);
        assert_eq!(
            crate::mention::unasked(&f),
            None,
            "a restart has nothing left to say"
        );
        // The parent finished the list: a cut after it says nothing.
        l.started("t3");
        l.ended("t3");
        l.push(EventKind::TurnStarted(TurnStarted {
            turn_id: "t4".into(),
            agent: Some(AgentRef {
                agent_id: "c".into(),
                name: "c".into(),
                prompt_hash: String::new(),
                withheld: vec![],
                effort: None,
            }),
            cause: Some(TurnCause::Mention(MentionCall {
                message_seq: m,
                by: CalledBy::Person,
                tags: vec![],
            })),
            environment: None,
            continued_by: None,
            retry: None,
        }));
        let last = l.ended("t4");
        let cut = check(&l.events, "t4").unwrap();
        let f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        assert_eq!(f.len() as u64, last + 1, "only the divider");
    }

    #[test]
    fn build_refuses_a_log_the_fold_would_refuse() {
        let mut l = Log::new();
        let e = l.turn("t1");
        let cut = Cut {
            through_seq: e,
            turn_id: "t1".into(),
        };
        let mut gap = l.events.clone();
        gap.remove(2);
        assert!(build(&gap, &cut, SessionId::new(), &title("Parent"), "/w", 1).is_err());
        assert!(
            build(
                &l.events[..2],
                &cut,
                SessionId::new(),
                &title("Parent"),
                "/w",
                1
            )
            .is_err()
        );
        let mid = Cut {
            through_seq: 3,
            turn_id: "t1".into(),
        };
        assert!(build(&l.events, &mid, SessionId::new(), &title("Parent"), "/w", 1).is_err());
        assert!(
            build(&l.events, &cut, l.sid, &title("Parent"), "/w", 1).is_err(),
            "not onto itself"
        );
    }

    #[test]
    fn a_fork_of_a_fork_has_its_own_cut_and_keeps_the_earlier_divider() {
        let mut l = Log::new();
        l.turn("t1");
        let cut = check(&l.events, "t1").unwrap();
        let mut f1 = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        let f1_id = f1[0].session_id;
        // The fork's own turn.
        let first = f1.len() as u64 + 1;
        for (next, k) in (first..).zip(["turn_started", "user_message", "turn_ended"]) {
            let payload = match k {
                "turn_started" => serde_json::json!({"turn_id": "u1"}),
                "user_message" => serde_json::json!({"turn_id": "u1", "text": "x"}),
                _ => serde_json::json!({"turn_id": "u1", "reason": "completed"}),
            };
            f1.push(RawEvent {
                session_id: f1_id,
                seq: next,
                ts_ms: 9,
                kind: k.into(),
                payload,
            });
        }
        let cut2 = check(&f1, "u1").unwrap();
        let f2 = build(
            &f1,
            &cut2,
            SessionId::new(),
            &title_nth("Parent (fork)", 2),
            "/w",
            6,
        )
        .unwrap();
        let Ok(EventKind::SessionCreated(c)) = f2[0].typed() else {
            panic!()
        };
        assert_eq!(c.title, "Parent (fork 2)");
        assert_eq!(
            c.forked,
            Some(ForkedFrom {
                parent: f1_id,
                through_seq: cut2.through_seq,
                own_folder: false
            })
        );
        // The first fork's divider is part of the copy, as it was.
        assert_eq!(f2[cut.through_seq as usize].kind, "session_forked");
        let v = crate::view::SessionView::fold(&f2).unwrap();
        assert_eq!(v.usage.input_tokens, 0, "nothing in it is the fork's own");
    }

    fn fold_fork() -> (crate::view::SessionView, crate::view::SessionView, u64) {
        let mut l = Log::new();
        l.turn("t1");
        l.push(EventKind::ChatAllowAdded(ChatAllowAdded {
            grant_id: "g-parent".into(),
            approval_id: "a".into(),
            tool: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
            key: "k".into(),
        }));
        let end = l.turn("t2");
        let cut = check(&l.events, "t2").unwrap();
        let f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        let parent = crate::view::SessionView::fold(&l.events).unwrap();
        let fork = crate::view::SessionView::fold(&f).unwrap();
        (parent, fork, end)
    }

    /// An end the parent had not been resumed for at the cut is the
    /// parent's: the fork's divider clears what waits, as a person's turn
    /// does, so the master's sweep and the node's check never resume the
    /// fork for it. An end after the divider waits as ever.
    #[test]
    fn the_divider_clears_the_ends_that_waited_for_a_callback() {
        let mut l = Log::new();
        l.started("t1");
        l.message("t1");
        l.bg_started("bg-1", "t1");
        // Ended while the turn ran, and not told to it: it would call back.
        l.bg_ended("bg-1");
        l.ended("t1");
        assert_eq!(
            crate::event::waiting_ends(&l.events).len(),
            1,
            "waits in the parent"
        );
        let cut = check(&l.events, "t1").unwrap();
        let mut f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        assert!(crate::event::waiting_ends(&f).is_empty(), "not the fork's");
        let id = f[0].session_id;
        let mut own = Log {
            sid: id,
            events: std::mem::take(&mut f),
        };
        own.bg_started("bg-2", "t9");
        own.bg_ended("bg-2");
        let waiting = crate::event::waiting_ends(&own.events);
        assert_eq!(
            waiting
                .iter()
                .map(|w| w.2.bg_id.as_str())
                .collect::<Vec<_>>(),
            vec!["bg-2"],
            "an end of its own waits"
        );
    }

    #[test]
    fn a_forks_usage_starts_at_zero_and_the_parents_is_unchanged() {
        let (parent, fork, _) = fold_fork();
        assert_eq!(parent.usage.input_tokens, 200);
        assert_eq!(fork.usage.input_tokens, 0);
        assert_eq!(fork.usage.output_tokens, 0);
        // The meter still shows the context the model will be sent.
        assert_eq!(fork.turn_usage.is_some(), parent.turn_usage.is_some());
    }

    #[test]
    fn a_forks_view_has_a_divider_and_no_inherited_grants() {
        let (parent, fork, end) = fold_fork();
        assert_eq!(parent.chat_allows.len(), 1);
        assert!(
            fork.chat_allows.is_empty(),
            "grants are the parent's folder's"
        );
        assert!(fork.running_turn.is_none());
        let Some(Item::Forked {
            through_seq,
            turn_id,
            ..
        }) = fork.items.last()
        else {
            panic!("{:?}", fork.items.last());
        };
        assert_eq!((*through_seq, turn_id.as_str()), (end, "t2"));
        assert_eq!(fork.meta.as_ref().unwrap().title, "Parent (fork)");
    }

    #[test]
    fn a_forks_own_usage_and_grants_count() {
        let mut l = Log::new();
        let end = l.turn("t1");
        let cut = check(&l.events, "t1").unwrap();
        let mut f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        let id = f[0].session_id;
        let mut push = |k: EventKind| {
            let (kind, payload) = k.into_parts();
            let seq = f.len() as u64 + 1;
            f.push(RawEvent {
                session_id: id,
                seq,
                ts_ms: 9,
                kind,
                payload,
            });
        };
        push(EventKind::ChatAllowAdded(ChatAllowAdded {
            grant_id: "g-own".into(),
            approval_id: "a".into(),
            tool: "bash".into(),
            input: serde_json::json!({}),
            key: "k".into(),
        }));
        push(EventKind::TurnStarted(TurnStarted {
            turn_id: "u1".into(),
            agent: None,
            cause: None,
            environment: None,
            retry: None,
            continued_by: None,
        }));
        push(EventKind::TurnEnded(TurnEnded {
            turn_id: "u1".into(),
            reason: TurnEndReason::Completed,
            usage: Some(Usage {
                input_tokens: 40,
                output_tokens: 4,
                ..Usage::default()
            }),
        }));
        let v = crate::view::SessionView::fold(&f).unwrap();
        assert_eq!(v.usage.input_tokens, 40, "only the turn after the cut");
        assert!(v.chat_allows.contains_key("g-own"));
        assert!(end > 0);
    }

    #[test]
    fn inherited_checkpoints_are_marked_and_own_ones_are_not() {
        let mut l = Log::new();
        l.started("t1");
        l.message("t1");
        for phase in [CheckpointPhase::Before, CheckpointPhase::After] {
            l.push(EventKind::Checkpoint(Checkpoint {
                turn_id: "t1".into(),
                phase,
                git_ref: "refs/x".into(),
                files: vec!["a.rs".into()],
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }));
        }
        l.ended("t1");
        let cut = check(&l.events, "t1").unwrap();
        let mut f = build(&l.events, &cut, SessionId::new(), &title("Parent"), "/w", 5).unwrap();
        let id = f[0].session_id;
        for (k, p) in [
            ("turn_started", serde_json::json!({"turn_id": "u1"})),
            (
                "checkpoint",
                serde_json::json!({"turn_id": "u1", "phase": "before", "git_ref": "r"}),
            ),
        ] {
            let seq = f.len() as u64 + 1;
            f.push(RawEvent {
                session_id: id,
                seq,
                ts_ms: 9,
                kind: k.into(),
                payload: p,
            });
        }
        let v = crate::view::SessionView::fold(&f).unwrap();
        assert!(v.checkpoints["t1"].inherited);
        assert_eq!(v.checkpoints["t1"].files, vec!["a.rs".to_string()]);
        assert!(!v.checkpoints["u1"].inherited);
        let parent = crate::view::SessionView::fold(&l.events).unwrap();
        assert!(!parent.checkpoints["t1"].inherited);
        let j = serde_json::to_value(&parent.checkpoints["t1"]).unwrap();
        assert!(j.get("inherited").is_none(), "absent unless set");
    }
}
