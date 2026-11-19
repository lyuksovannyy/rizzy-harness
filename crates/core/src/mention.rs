//! Which agents a person's message calls (X.37, README "Calling several
//! agents"): one pure rule, used by the master to decide who answers and
//! mirrored exactly in `web/src/lib/mentionScan.ts` to draw the message;
//! both test suites read `web/src/__fixtures__/mentions.json`.
//!
//! A message CALLS the agents named in
//!
//! - its **opening run**: the mentions it begins with, separated by spaces,
//!   newlines and/or commas (`@coder @writer, yo how it is goin?`: the run
//!   ends at the first token that is not a resolved mention, or after a
//!   mention followed directly by `:`), and
//! - its **closing line**: a last line that holds nothing but resolved
//!   mentions (`...\n@coder @writer @alex`). One word, one unknown name or
//!   one punctuation mark in it and the line is text.
//!
//! A mention anywhere else is a plain mention: it calls nobody. A mention
//! is `@` at the start of a token (the line's start, or after a space or a
//! comma) and an agent's handle (`[a-z0-9-]+`, the rule of `@mention` of a
//! single agent: `@coder.md` and `@src/main.rs` are files, never the agent
//! `coder`) followed by the end, a space, `,` or `:`. It counts only when
//! the caller's resolver knows it: an agent of the chat's own workspace set
//! that may run as an agent, or the chat owner's own login (the person: no
//! turn). Anything else is text. Nothing inside a fenced code block (``` or
//! ~~~ lines) or an inline code span counts, and a closing line inside an
//! open fence is not a closing line.
//!
//! One pass over the text, no work that grows faster than its length;
//! at most [`MAX_CALLED`] + 1 agents are kept so the caller can refuse a
//! message that calls more.

use crate::RawEvent;
use crate::event::{EventKind, TurnCause, TurnStarted};

/// The most agents one message may call.
pub const MAX_CALLED: usize = 4;

/// The most resolved mentions (agents called, plain, tags and the person's
/// own name) one message may hold; the master refuses more, so a message
/// cannot carry unbounded resolved names.
pub const MAX_RESOLVED: usize = 16;

/// Whom a resolved mention names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    /// The agent with this id (an alias is resolved to it).
    Agent(String),
    /// The chat's owner, by their own login.
    Person,
    /// An agent that runs only as a sub-agent, by id: it may be mentioned
    /// and is drawn as a mention, but never triggers a turn, in any
    /// position. In the opening run and the closing line it is part of the
    /// run (it neither ends it nor makes the line "not only mentions"): its
    /// purpose is to tell the answering agent which agent to run as a
    /// sub-agent (the owner, 2026-11-19).
    Tag(String),
}

/// One resolved mention in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Byte range of `@name` in the text.
    pub start: usize,
    pub end: usize,
    pub who: Who,
    /// In the opening run or the closing line: it calls its agent.
    pub trigger: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mentions {
    /// Every resolved mention outside code, in text order.
    pub found: Vec<Found>,
    /// The agents called, each once, in order of first appearance; at most
    /// [`MAX_CALLED`] + 1 (more than [`MAX_CALLED`] is the caller's refusal).
    pub called: Vec<String>,
}

fn handle_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'
}

fn separator(c: char) -> bool {
    c.is_whitespace() || c == ','
}

/// The mention at byte `at` (`@` there): its end and what it names, when
/// `resolve` knows the handle and the token ends where a mention may.
fn mention_at(
    text: &str,
    at: usize,
    resolve: &dyn Fn(&str) -> Option<Who>,
) -> Option<(usize, Who)> {
    let bytes = text.as_bytes();
    let mut end = at + 1;
    while end < bytes.len() && handle_byte(bytes[end]) {
        end += 1;
    }
    if end == at + 1 {
        return None;
    }
    // The next character, if any, must end a token.
    if let Some(c) = text[end..].chars().next()
        && !(c.is_whitespace() || c == ',' || c == ':')
    {
        return None;
    }
    resolve(&text[at + 1..end]).map(|w| (end, w))
}

/// The opening run's mentions as (start, end, who), from the first
/// character that is not white space.
fn opening_run(text: &str, resolve: &dyn Fn(&str) -> Option<Who>) -> Vec<(usize, usize, Who)> {
    let mut out = Vec::new();
    let mut at = text.len() - text.trim_start().len();
    while text[at..].starts_with('@') {
        let Some((end, who)) = mention_at(text, at, resolve) else {
            break;
        };
        out.push((at, end, who));
        if text[end..].starts_with(':') {
            break;
        }
        let rest = &text[end..];
        at = end + (rest.len() - rest.trim_start_matches(separator).len());
    }
    out
}

/// The closing line's mentions, when the last line holds nothing else.
fn closing_line(
    text: &str,
    line_start: usize,
    resolve: &dyn Fn(&str) -> Option<Who>,
) -> Vec<(usize, usize, Who)> {
    let end_of_text = text.trim_end().len();
    let mut out = Vec::new();
    let mut at = line_start;
    loop {
        let rest = &text[at..end_of_text];
        at += rest.len() - rest.trim_start_matches(separator).len();
        if at >= end_of_text {
            break;
        }
        if !text[at..].starts_with('@') {
            return Vec::new();
        }
        let Some((end, who)) = mention_at(text, at, resolve) else {
            return Vec::new();
        };
        // `:` is not a separator in a closing line.
        if text[end..].starts_with(':') {
            return Vec::new();
        }
        out.push((at, end, who));
        at = end;
    }
    out
}

/// Every resolved mention outside code, and the agents called.
pub fn scan(text: &str, resolve: &dyn Fn(&str) -> Option<Who>) -> Mentions {
    let mut found: Vec<Found> = Vec::new();
    let mut in_fence = false;
    let mut last_line = (0usize, false); // start of the last non-empty line, in a fence
    let mut line_start = 0usize;
    let body_end = text.trim_end().len();
    for line in text.split_inclusive('\n') {
        let start = line_start;
        line_start += line.len();
        let trimmed = line.trim_start();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if start < body_end {
            last_line = (start, in_fence || fence);
        }
        if fence {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        scan_line(text, start, start + line.len(), resolve, &mut found);
    }
    let mut marks: Vec<usize> = Vec::new();
    for (s, _, _) in opening_run(text, resolve) {
        marks.push(s);
    }
    if !text.trim().is_empty() && !last_line.1 {
        for (s, _, _) in closing_line(text, last_line.0, resolve) {
            marks.push(s);
        }
    }
    marks.sort_unstable();
    marks.dedup();
    for f in &mut found {
        f.trigger = !matches!(f.who, Who::Tag(_)) && marks.binary_search(&f.start).is_ok();
    }
    let mut called: Vec<String> = Vec::new();
    for f in &found {
        if let (true, Who::Agent(id)) = (f.trigger, &f.who)
            && !called.contains(id)
        {
            called.push(id.clone());
            if called.len() > MAX_CALLED {
                break;
            }
        }
    }
    Mentions { found, called }
}

/// The mentions of one line, skipping inline code (a run of backticks up
/// to the next run of as many; one that never closes is a plain character).
fn scan_line(
    text: &str,
    from: usize,
    to: usize,
    resolve: &dyn Fn(&str) -> Option<Who>,
    out: &mut Vec<Found>,
) {
    let bytes = text.as_bytes();
    let mut i = from;
    let mut token_start = true;
    while i < to {
        match bytes[i] {
            b'`' => {
                let mut n = 0;
                while i + n < to && bytes[i + n] == b'`' {
                    n += 1;
                }
                // A closing run of the same length, later on this line.
                let mut j = i + n;
                let mut closed = None;
                while j < to {
                    if bytes[j] == b'`' {
                        let mut m = 0;
                        while j + m < to && bytes[j + m] == b'`' {
                            m += 1;
                        }
                        if m == n {
                            closed = Some(j + m);
                            break;
                        }
                        j += m;
                    } else {
                        j += 1;
                    }
                }
                i = closed.unwrap_or(i + n);
                token_start = false;
            }
            b'@' if token_start => {
                if let Some((end, who)) = mention_at(text, i, resolve) {
                    out.push(Found {
                        start: i,
                        end,
                        who,
                        trigger: false,
                    });
                    i = end;
                    token_start = false;
                } else {
                    i += 1;
                    token_start = false;
                }
            }
            _ => {
                // Next character (a UTF-8 sequence is stepped whole).
                let c = text[i..].chars().next().unwrap_or(' ');
                token_start = separator(c);
                i += c.len_utf8();
            }
        }
    }
}

/// The ids of the tags in `m` (sub-agent-only agents it mentions), each
/// once, in order.
pub fn tags_of(m: &Mentions) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in &m.found {
        if let Who::Tag(id) = &f.who
            && !out.contains(id)
        {
            out.push(id.clone());
        }
    }
    out
}

/// Whether `id` is a plain agent handle (what the harness may name in a
/// line it writes for the model).
pub fn valid_handle(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The one sentence that tells the answering agent which mentioned names
/// are agents it can run as sub-agents: `tags` already cut to the ones the
/// answering agent has among its own delegates (nothing is offered that its
/// definition does not allow). `None` when none. Pure: the same bytes every
/// time, so a replay rebuilds it; only plain handles are named. No new right:
/// `delegate` works as it always did.
pub fn tags_line(tags: &[String]) -> Option<String> {
    let names: Vec<String> = tags
        .iter()
        .filter(|t| valid_handle(t))
        .map(|t| format!("@{t}"))
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "The person's message mentions {} as agents you may run as sub-agents (with `delegate`) if the work calls for it; it did not call them.",
        names.join(", ")
    ))
}

/// The agents the last message that called several (`user_message.triggers`)
/// called but no turn was started for, with that message's seq: those a
/// node that restarted mid-list (or lost the list any other way) must say
/// were not asked. `None` when every agent after the first had its turn,
/// the list ended with a `peers_skipped` already, or a later message of
/// the person called nobody (the list is over by then). In the order they
/// would have run. Read from the log alone.
pub fn unasked(log: &[RawEvent]) -> Option<(u64, Vec<String>)> {
    struct Last {
        seq: u64,
        called: Vec<String>,
        asked: Vec<String>,
        done: bool,
    }
    let mut last: Option<Last> = None;
    for e in log {
        match e.typed() {
            Ok(EventKind::UserMessage(m)) if !m.queued => {
                last = (!m.triggers.is_empty()).then(|| Last {
                    seq: e.seq,
                    called: m.triggers,
                    asked: Vec::new(),
                    done: false,
                });
            }
            Ok(EventKind::TurnStarted(TurnStarted {
                cause: Some(TurnCause::Mention(m)),
                agent,
                ..
            })) => {
                if let (Some(l), Some(a)) = (last.as_mut(), agent)
                    && l.seq == m.message_seq
                {
                    l.asked.push(a.agent_id);
                }
            }
            Ok(EventKind::PeersSkipped(p)) => {
                if let Some(l) = last.as_mut()
                    && l.seq == p.message_seq
                {
                    l.done = true;
                }
            }
            _ => {}
        }
    }
    let l = last.filter(|l| !l.done)?;
    let missing: Vec<String> = l
        .called
        .into_iter()
        .skip(1)
        .filter(|a| !l.asked.contains(a))
        .collect();
    (!missing.is_empty()).then_some((l.seq, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str) -> Option<Who> {
        match name {
            "coder" | "writer" | "editor" | "dev" | "a" | "b" | "c" | "d" | "e" => Some(
                Who::Agent(if name == "dev" { "coder" } else { name }.into()),
            ),
            "alex" => Some(Who::Person),
            _ => None,
        }
    }

    fn called(text: &str) -> Vec<String> {
        scan(text, &r).called
    }

    #[test]
    fn the_owners_examples() {
        assert_eq!(
            called("@coder @writer, yo how it is goin?"),
            ["coder", "writer"]
        );
        assert_eq!(
            called("have anybody seen the gta6 release?\n@coder @writer @alex"),
            ["coder", "writer"]
        );
        assert!(called("my guy, the @coder have done good work, give him a raise").is_empty());
        let m = scan(
            "my guy, the @coder have done good work, give him a raise\n@coder @alex",
            &r,
        );
        assert_eq!(m.called, ["coder"]);
        assert!(!m.found[0].trigger, "the one in the middle stays plain");
        assert!(m.found[1].trigger && m.found[2].trigger);
    }

    #[test]
    fn a_hundred_thousand_mentions_are_one_pass() {
        let text = "@coder ".repeat(100_000);
        let m = scan(&text, &r);
        assert_eq!(m.called, ["coder"]);
        let many = format!("{}@a @b @c @d @e", "x ".repeat(50_000));
        assert!(scan(&many, &r).called.is_empty());
    }

    fn raw(seq: u64, k: EventKind) -> RawEvent {
        let (kind, payload) = k.into_parts();
        RawEvent {
            session_id: crate::SessionId::new(),
            seq,
            ts_ms: 0,
            kind,
            payload,
        }
    }

    fn message(triggers: &[&str]) -> EventKind {
        let mut m: crate::event::UserMessage = serde_json::from_value(serde_json::json!({
            "turn_id": "t1",
            "text": "x",
        }))
        .unwrap();
        m.triggers = triggers.iter().map(|s| s.to_string()).collect();
        EventKind::UserMessage(m)
    }

    fn mention_turn(seq: u64, agent: &str, of: u64) -> RawEvent {
        raw(
            seq,
            EventKind::TurnStarted(TurnStarted {
                turn_id: format!("m{seq}"),
                agent: Some(crate::event::AgentRef {
                    agent_id: agent.into(),
                    name: agent.into(),
                    prompt_hash: String::new(),
                    withheld: vec![],
                    effort: None,
                }),
                cause: Some(TurnCause::Mention(crate::event::MentionCall {
                    message_seq: of,
                    by: crate::event::CalledBy::Person,
                    tags: vec![],
                })),
                environment: None,
                continued_by: None,
                retry: None,
            }),
        )
    }

    /// What a restart must say: the agents after the first with no turn,
    /// once; nothing for a list that finished, was stopped (it said so), or
    /// was followed by a message that called nobody.
    #[test]
    fn unasked_reads_the_log_alone() {
        let called = ["a", "b", "c", "d"];
        let mut log = vec![raw(2, message(&called))];
        assert_eq!(
            unasked(&log),
            Some((2, vec!["b".into(), "c".into(), "d".into()]))
        );
        log.push(mention_turn(5, "b", 2));
        assert_eq!(unasked(&log), Some((2, vec!["c".into(), "d".into()])));
        // Another message's mention turn does not count for this one.
        log.push(mention_turn(6, "c", 99));
        assert_eq!(unasked(&log), Some((2, vec!["c".into(), "d".into()])));
        log.push(mention_turn(7, "c", 2));
        log.push(mention_turn(8, "d", 2));
        assert_eq!(unasked(&log), None, "every agent had its turn");
        // Said already.
        let mut log = vec![raw(2, message(&called))];
        log.push(raw(
            4,
            EventKind::PeersSkipped(crate::event::PeersSkipped {
                message_seq: 2,
                agents: vec!["b".into()],
                reason: crate::event::PeersSkipReason::Stopped,
            }),
        ));
        assert_eq!(unasked(&log), None);
        // A later message that called nobody ends the matter; a queued
        // message with triggers is not a message of the list.
        let mut log = vec![raw(2, message(&called)), raw(9, message(&[]))];
        assert_eq!(unasked(&log), None);
        log.truncate(1);
        let mut q = match message(&["x", "y"]) {
            EventKind::UserMessage(m) => m,
            _ => unreachable!(),
        };
        q.queued = true;
        log.push(raw(10, EventKind::UserMessage(q)));
        assert_eq!(
            unasked(&log),
            Some((2, vec!["b".into(), "c".into(), "d".into()]))
        );
        assert_eq!(unasked(&[]), None);
    }

    /// The tag line is one pure sentence naming only plain handles, nothing
    /// when there is nothing to name; the opening of a mention turn carries
    /// it.
    #[test]
    fn the_tags_line_is_pure_and_names_plain_handles_only() {
        assert_eq!(tags_line(&[]), None);
        assert_eq!(tags_line(&["Bad Name".into(), "x\ny".into()]), None);
        let l = tags_line(&["advisor".into(), "scout".into(), "no way".into()]).unwrap();
        assert_eq!(
            Some(l.clone()),
            tags_line(&["advisor".into(), "scout".into()])
        );
        assert!(l.contains("@advisor, @scout") && !l.contains('\n') && !l.contains("no way"));
        let call = crate::event::MentionCall {
            message_seq: 1,
            by: crate::event::CalledBy::Person,
            tags: vec!["advisor".into()],
        };
        assert!(
            crate::event::mention_opening("coder", &call)
                .ends_with(&tags_line(&["advisor".into()]).unwrap())
        );
        let none = crate::event::MentionCall {
            tags: vec![],
            ..call
        };
        assert!(!crate::event::mention_opening("coder", &none).contains("sub-agents"));
    }
}
