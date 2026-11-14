//! The turn's context block in the log (X.1): a turn's request rebuilds
//! byte for byte in every later turn, instruction files, skills, memory
//! and the master's context included, so the provider's prompt cache holds
//! up to the previous turn's last tool result; a standing section that did
//! not change is not sent again, a changed one is sent with a line saying
//! it replaces the earlier one, a summary makes everything go again.

use std::sync::{Arc, Mutex};

use harness_agent_native::context::{self, SECTION_CAP};
use harness_agent_native::provider::{CONTEXT_LEAD, place_notes};
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    history_to_messages,
};
use harness_core::backend::{
    AgentBackend, StandingKey, StandingPart, TurnCtx, TurnExtra, TurnHost,
};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

impl Host {
    fn records(&self) -> Vec<ContextSent> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ContextSent(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Records every request. Each turn makes `steps` bash calls, then
/// answers in text.
struct Script {
    seen: Mutex<Vec<ChatRequest>>,
    steps: usize,
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len()
        };
        // Tool results since this turn's last user message.
        let done = req
            .messages
            .iter()
            .rev()
            .take_while(|m| !matches!(m, ChatMsg::User(_) | ChatMsg::UserImages { .. }))
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        Ok(if done < self.steps {
            Completion {
                tool_calls: vec![ToolCallRef {
                    call_id: format!("c{n}"),
                    tool: "bash".into(),
                    input: json!({"command": format!("echo step {n}")}),
                }],
                ..Default::default()
            }
        } else {
            Completion {
                text: format!("done {n}"),
                ..Default::default()
            }
        })
    }
}

fn script(steps: usize) -> Arc<Script> {
    Arc::new(Script {
        seen: Mutex::default(),
        steps,
    })
}

/// About 4 KiB of a typical `AGENTS.md`, as the node renders the files.
fn agents_md() -> String {
    let mut s = String::from("Instruction files\n==> AGENTS.md <==\n# Rules\n");
    let mut n = 0;
    while s.len() < 4096 {
        n += 1;
        s.push_str(&format!(
            "- Rule {n}: run `cargo fmt` and `cargo clippy` before you say a change works.\n"
        ));
    }
    s
}

fn skills() -> String {
    "Skills you can load with the `skill` tool:\n- release: cut a release\n- review: review a diff"
        .into()
}

fn memory(facts: &str) -> Vec<StandingPart> {
    vec![StandingPart {
        part: StandingKey::Memory,
        text: format!("What you remember about the user:\n{facts}"),
    }]
}

fn extra(instructions: Option<String>, minute: u32) -> TurnExtra {
    TurnExtra {
        instructions,
        skills_list: Some(skills()),
        standing: Some(memory("- f1: likes short answers")),
        context: Some(format!(
            "Context: the user's name is Ann. Their local time is 10:{minute:02}."
        )),
        environment: Some(format!(
            "[harness] Environment of this turn: the clock reads 10:{minute:02}."
        )),
        ..Default::default()
    }
}

/// The session's log as the node writes it: `turn_started` (with the
/// environment line), the person's message, what the loop emitted, the end.
fn append(
    log: &mut Vec<RawEvent>,
    sid: SessionId,
    turn: &str,
    text: &str,
    extra: &TurnExtra,
    host: &Host,
) {
    let mut kinds = vec![
        EventKind::TurnStarted(TurnStarted {
            turn_id: turn.into(),
            agent: None,
            cause: None,
            environment: extra.environment.clone(),
        }),
        EventKind::UserMessage(UserMessage {
            turn_id: turn.into(),
            text: text.into(),
            attachments: vec![],
            reply: None,
            images: vec![],
            queued: false,
        }),
    ];
    kinds.extend(host.0.lock().unwrap().iter().cloned());
    kinds.push(EventKind::TurnEnded(TurnEnded {
        turn_id: turn.into(),
        reason: TurnEndReason::Completed,
        usage: None,
    }));
    for k in kinds {
        let (kind, payload) = k.into_parts();
        let seq = log.len() as u64 + 1;
        log.push(RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
    }
}

/// Run one turn on a backend of its own (as after a restart), recording
/// its log; returns the requests it made.
async fn turn(
    p: &Arc<Script>,
    config: NativeConfig,
    log: &mut Vec<RawEvent>,
    sid: SessionId,
    turn_id: &str,
    text: &str,
    extra: TurnExtra,
) -> (Vec<ChatRequest>, Vec<ContextSent>) {
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), config);
    let before = p.seen.lock().unwrap().len();
    let host = Host::default();
    let out = b
        .run_turn(TurnCtx {
            session_id: sid,
            turn_id: turn_id.into(),
            history: log.clone(),
            user_text: text.into(),
            // The same folder every turn: it is in the system prompt.
            cwd: std::env::temp_dir(),
            model: "x".into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra: extra.clone(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{out:?}");
    append(log, sid, turn_id, text, &extra, &host);
    let reqs = p.seen.lock().unwrap()[before..].to_vec();
    (reqs, host.records())
}

/// What the provider reads of a request, notes placed, one string.
fn bytes(req: &ChatRequest) -> String {
    place_notes(req.messages.clone())
        .iter()
        .map(|m| match m {
            ChatMsg::System(t) => format!("system:{t}"),
            ChatMsg::User(t) | ChatMsg::Opening(t) => format!("user:{t}"),
            ChatMsg::UserImages { text, .. } => format!("user:{text}"),
            ChatMsg::Tool { call_id, content } => format!("tool {call_id}:{content}"),
            ChatMsg::Assistant { text, tool_calls } => format!("assistant:{text}{tool_calls:?}"),
            ChatMsg::Note(t) | ChatMsg::CallNote(t) => format!("note:{t}"),
        })
        .collect::<Vec<_>>()
        .join("\u{0}")
}

/// Bytes `next` starts with that `last` also starts with.
fn common(last: &ChatRequest, next: &ChatRequest) -> usize {
    let (a, b) = (bytes(last), bytes(next));
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

fn opening(req: &ChatRequest) -> String {
    req.messages
        .iter()
        .rev()
        .find_map(|m| m.user_text())
        .unwrap()
        .to_string()
}

/// The provider's prompt cache across turns, with every kind of context a
/// coding chat has: turn N+1's first request holds turn N's last request
/// byte for byte, up to and including its last tool result, over three
/// turns, each on a backend of its own (a restart between turns).
/// Failed before X.1: the context was not logged, so the rebuilt message
/// lacked it and everything after it differed.
#[tokio::test]
async fn the_next_turn_rebuilds_this_turns_context_byte_for_byte() {
    let p = script(2);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let mut last: Option<ChatRequest> = None;
    for (i, text) in ["first", "second", "third"].into_iter().enumerate() {
        let id = format!("t{i}");
        let ex = extra(Some(agents_md()), i as u32);
        let (reqs, _) = turn(&p, NativeConfig::default(), &mut log, sid, &id, text, ex).await;
        if let Some(prev) = &last {
            let next = &reqs[0];
            assert!(next.messages.len() > prev.messages.len());
            assert_eq!(
                next.messages[..prev.messages.len()],
                prev.messages[..],
                "turn {i}: the cached prefix moved"
            );
            assert!(matches!(prev.messages.last(), Some(ChatMsg::Tool { .. })));
        }
        last = reqs.last().cloned();
    }
}

/// The measurement (X.1): how much of turn 2's first request matches turn
/// 1's last one (what the provider can read from its cache), with the
/// record and as before it (the same log without `context_sent`, which is
/// what the old build rebuilt), and how many bytes of a 4 KiB `AGENTS.md`
/// are no longer sent each turn.
#[tokio::test]
async fn the_cached_prefix_reaches_the_last_tool_result() {
    let p = script(3);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let (first, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "fix the bug",
        extra(Some(agents_md()), 1),
    )
    .await;
    let last = first.last().unwrap().clone();
    let without: Vec<RawEvent> = log
        .iter()
        .filter(|e| e.kind != "context_sent")
        .cloned()
        .collect();
    let mut old_log = without.clone();
    let mut new_log = log.clone();
    let (now, _) = turn(
        &p,
        NativeConfig::default(),
        &mut new_log,
        sid,
        "t2",
        "and the test",
        extra(Some(agents_md()), 2),
    )
    .await;
    let (before, _) = turn(
        &p,
        NativeConfig::default(),
        &mut old_log,
        sid,
        "t2",
        "and the test",
        extra(Some(agents_md()), 2),
    )
    .await;
    let whole = bytes(&last).len();
    let cached_now = common(&last, &now[0]);
    let cached_before = common(&last, &before[0]);
    let saved = opening(&before[0]).len() - opening(&now[0]).len();
    eprintln!(
        "turn 1's last request: {whole} bytes; turn 2 matches {cached_now} of them with the \
         record, {cached_before} without; turn 2's message is {saved} bytes shorter"
    );
    assert_eq!(
        cached_now, whole,
        "all of turn 1's last request is a prefix"
    );
    assert!(cached_before < whole / 2, "{cached_before} of {whole}");
    assert!(saved > 4096, "{saved}");
}

/// A standing section that did not change is not sent again; this turn's
/// own context is. The record marks only what it sent.
#[tokio::test]
async fn an_unchanged_section_is_not_sent_again() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let (r1, c1) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    let o1 = opening(&r1[0]);
    assert!(o1.contains("Rule 1:") && o1.contains("- release:") && o1.contains("likes short"));
    let keys: Vec<&str> = c1[0].sections.iter().map(|m| m.key.as_str()).collect();
    assert_eq!(keys, ["instructions", "skills", "memory"]);
    let (r2, c2) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t2",
        "two",
        extra(Some(agents_md()), 2),
    )
    .await;
    let o2 = opening(&r2[0]);
    assert!(!o2.contains("Rule 1:"), "{o2}");
    assert!(!o2.contains("- release:"), "{o2}");
    assert!(!o2.contains("likes short"), "{o2}");
    assert!(o2.contains("local time is 10:02"), "{o2}");
    assert!(o2.ends_with("\n\ntwo"), "{o2}");
    assert!(c2[0].sections.is_empty(), "{:?}", c2[0].sections);
}

/// A changed section is sent in full again, with the harness's line saying
/// it replaces the earlier one; the earlier turn's message keeps the old
/// text (its bytes do not move). One that is gone is said to be gone,
/// once.
#[tokio::test]
async fn a_changed_section_replaces_and_a_gone_one_is_withdrawn_once() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let (r1, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    let changed = format!("{}- Rule new: use pnpm.\n", agents_md());
    let (r2, c2) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t2",
        "two",
        extra(Some(changed), 2),
    )
    .await;
    let o2 = opening(&r2[0]);
    assert!(o2.contains("Rule new: use pnpm."), "{o2}");
    assert!(o2.contains("Rule 1:"), "sent in full: {o2}");
    assert!(
        o2.contains("The repository's instruction files below replace the ones sent earlier"),
        "{o2}"
    );
    assert!(!o2.contains("- release:"), "skills unchanged: {o2}");
    assert_eq!(c2[0].sections.len(), 1);
    assert_eq!(
        r2[0].messages[..r1[0].messages.len()],
        r1[0].messages[..],
        "turn 1's message as it was sent"
    );
    let (r3, c3) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t3",
        "three",
        extra(None, 3),
    )
    .await;
    let o3 = opening(&r3[0]);
    assert!(o3.contains("no longer apply: none were found"), "{o3}");
    assert_eq!(c3[0].sections[0].digest, "");
    let (r4, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t4",
        "four",
        extra(None, 4),
    )
    .await;
    let o4 = opening(&r4[0]);
    assert!(!o4.contains("no longer apply"), "said once: {o4}");
    // Back again: sent in full, a plain first time (nothing to replace).
    let (r5, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t5",
        "five",
        extra(Some(agents_md()), 5),
    )
    .await;
    let o5 = opening(&r5[0]);
    assert!(
        o5.contains("Rule 1:") && !o5.contains("replace the ones"),
        "{o5}"
    );
}

/// A turn the master gave no context (a callback turn, one that reads
/// queued messages: `standing` is `None`) withdraws nothing.
#[tokio::test]
async fn no_master_context_withdraws_nothing() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    let mut ex = extra(Some(agents_md()), 2);
    ex.standing = None;
    ex.context = None;
    let (r2, c2) = turn(&p, NativeConfig::default(), &mut log, sid, "t2", "two", ex).await;
    assert!(
        !opening(&r2[0]).contains("no longer"),
        "{}",
        opening(&r2[0])
    );
    assert!(c2.is_empty(), "{c2:?}");
}

/// After a summary the window holds no block: every standing section is
/// sent again in the first turn after it (the instruction files survive
/// compaction), and the summary's transcript does not carry the blocks.
#[tokio::test]
async fn after_a_summary_every_standing_section_is_sent_again() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    let through = log.last().unwrap().seq;
    let (kind, payload) = EventKind::HistorySummarized(HistorySummarized {
        turn_id: "t1".into(),
        through_seq: through,
        text: "## Goal\nfix it".into(),
    })
    .into_parts();
    log.push(RawEvent {
        session_id: sid,
        seq: through + 1,
        ts_ms: 0,
        kind,
        payload,
    });
    assert!(
        harness_agent_native::summary_messages(&log)
            .iter()
            .filter_map(|m| m.user_text())
            .all(|t| !t.contains("Rule 1:") && !t.starts_with(CONTEXT_LEAD))
    );
    let (r2, c2) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t2",
        "two",
        extra(Some(agents_md()), 2),
    )
    .await;
    let o2 = opening(&r2[0]);
    assert!(o2.contains("Rule 1:") && o2.contains("- release:") && o2.contains("likes short"));
    assert!(!o2.contains("replace"), "{o2}");
    assert_eq!(c2[0].sections.len(), 3);
}

/// A log without any record (a session older than X.1) loads: the next
/// turn sends every section, and its own message is rebuilt from then on.
#[tokio::test]
async fn a_log_without_records_still_loads() {
    let p = script(1);
    let sid = SessionId::new();
    let mut log = Vec::new();
    turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    log.retain(|e| e.kind != "context_sent");
    for (i, e) in log.iter_mut().enumerate() {
        e.seq = i as u64 + 1;
    }
    let (r2, c2) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t2",
        "two",
        extra(Some(agents_md()), 2),
    )
    .await;
    assert!(opening(&r2[0]).contains("Rule 1:"));
    assert_eq!(c2[0].sections.len(), 3);
    let (r3, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t3",
        "three",
        extra(Some(agents_md()), 3),
    )
    .await;
    let prev = r2.last().unwrap();
    assert_eq!(r3[0].messages[..prev.messages.len()], prev.messages[..]);
}

/// A section past the cap is cut once, before it is sent: the record holds
/// the same cut text, and the rebuild puts it back unchanged.
#[tokio::test]
async fn a_section_past_the_cap_is_cut_the_same_when_sent_and_rebuilt() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let mut ex = extra(None, 1);
    ex.standing = Some(vec![StandingPart {
        part: StandingKey::Project,
        text: "é".repeat(SECTION_CAP),
    }]);
    let (r1, c1) = turn(&p, NativeConfig::default(), &mut log, sid, "t1", "one", ex).await;
    let o1 = opening(&r1[0]);
    assert!(o1.contains("[cut: the harness sends at most"), "cut");
    assert!(o1.len() < SECTION_CAP * 2, "{}", o1.len());
    assert!(o1.contains(&c1[0].text));
    let rebuilt = history_to_messages(&log);
    let again = rebuilt.iter().find_map(|m| m.user_text()).unwrap();
    assert_eq!(again, o1);
}

/// A secret in a section reaches neither the model nor the record (the
/// node would scrub the record; the message is scrubbed first so the two
/// stay the same bytes).
#[tokio::test]
async fn a_secret_in_a_section_is_scrubbed_before_it_is_sent() {
    let p = script(0);
    let sid = SessionId::new();
    let mut log = Vec::new();
    let config = NativeConfig {
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
            "sk-live-0123456789abcdef".to_string(),
        )]),
        ..Default::default()
    };
    let ex = extra(
        Some(format!("{}token: sk-live-0123456789abcdef\n", agents_md())),
        1,
    );
    let (r1, c1) = turn(&p, config, &mut log, sid, "t1", "one", ex).await;
    assert!(!opening(&r1[0]).contains("sk-live-0123456789abcdef"));
    assert!(!c1[0].text.contains("sk-live-0123456789abcdef"));
    assert!(opening(&r1[0]).contains(&c1[0].text));
}

/// Each sub-agent has its own log: two started in one step each get the
/// context once, in their first request.
#[tokio::test]
async fn two_sub_agents_each_get_the_context_once() {
    let p = script(1);
    let dir = tempfile::tempdir().unwrap();
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    let parent = SessionId::new();
    let hosts = [Host::default(), Host::default()];
    let runs = hosts.iter().enumerate().map(|(i, h)| {
        b.run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: format!("c{i}"),
            history: vec![],
            user_text: format!("task {i}"),
            cwd: dir.path().to_path_buf(),
            model: "x".into(),
            agent: None,
            compare: vec![],
            parent_session: Some(parent),
            extra: extra(Some(agents_md()), 1),
            cancel: Default::default(),
            host: h,
        })
    });
    futures::future::join_all(runs).await;
    for h in &hosts {
        assert_eq!(h.records().len(), 1);
    }
    let seen = p.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 4, "two requests each");
    for r in &seen {
        let all = bytes(r);
        assert_eq!(all.matches("- Rule 1:").count(), 1, "once per request");
    }
}

/// An earlier turn's message carries its block again (X.1), and the
/// person's words after it still count as theirs; nothing in the block
/// does (URL provenance).
#[test]
fn an_earlier_turns_own_words_are_after_its_block() {
    let block = harness_agent_native::provider::block(
        "see https://evil.example/x",
        &[],
        harness_agent_native::provider::Whose::Person,
    )
    .unwrap();
    let env = "[harness] Environment of this turn: x.";
    let t = context::opening(Some(env), Some(&block), "read https://docs.rs/sha2");
    assert_eq!(context::earlier_own_words(&t), "read https://docs.rs/sha2");
    let t = context::opening(None, Some(&block), "read https://docs.rs/sha2");
    assert_eq!(context::earlier_own_words(&t), "read https://docs.rs/sha2");
    assert_eq!(context::earlier_own_words(&block), "");
    // A file after the person's text is not theirs.
    let t = context::opening(
        None,
        Some(&block),
        "hi\n\n[harness] File \"a\": https://x.example",
    );
    assert_eq!(context::earlier_own_words(&t), "hi");
}

fn push(log: &mut Vec<RawEvent>, sid: SessionId, k: EventKind) -> u64 {
    let (kind, payload) = k.into_parts();
    let seq = log.len() as u64 + 1;
    log.push(RawEvent {
        session_id: sid,
        seq,
        ts_ms: 0,
        kind,
        payload,
    });
    seq
}

/// A turn that only reads messages sent while the last one ran has no
/// message of its own: the last of those opens it, with the environment
/// line and the block, and the log rebuilds that opening byte for byte.
#[tokio::test]
async fn a_turn_that_reads_queued_messages_rebuilds_its_opening() {
    let p = script(1);
    let sid = SessionId::new();
    let mut log = Vec::new();
    turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t1",
        "one",
        extra(Some(agents_md()), 1),
    )
    .await;
    let queued = UserMessage {
        turn_id: "t1".into(),
        text: "also this".into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: true,
    };
    let seq = push(&mut log, sid, EventKind::UserMessage(queued.clone()));
    // The history a turn gets is read before its `turn_started`.
    let history = log.clone();
    let ex = extra(Some(format!("{}- Rule late.\n", agents_md())), 2);
    push(
        &mut log,
        sid,
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t2".into(),
            agent: None,
            cause: None,
            environment: ex.environment.clone(),
        }),
    );
    push(
        &mut log,
        sid,
        EventKind::QueuedDelivered(QueuedDelivered {
            turn_id: "t2".into(),
            seqs: vec![seq],
        }),
    );
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    let host = Host::default();
    let before = p.seen.lock().unwrap().len();
    b.run_turn(TurnCtx {
        session_id: sid,
        turn_id: "t2".into(),
        history,
        user_text: queued_text(&queued.model_text()),
        cwd: std::env::temp_dir(),
        model: "x".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: ex,
        cancel: Default::default(),
        host: &host,
    })
    .await;
    let last = p.seen.lock().unwrap().last().unwrap().clone();
    assert!(p.seen.lock().unwrap().len() > before);
    assert!(opening(&p.seen.lock().unwrap()[before]).contains("Rule late."));
    for k in host.0.lock().unwrap().iter().cloned() {
        push(&mut log, sid, k);
    }
    push(
        &mut log,
        sid,
        EventKind::TurnEnded(TurnEnded {
            turn_id: "t2".into(),
            reason: TurnEndReason::Completed,
            usage: None,
        }),
    );
    let (r3, _) = turn(
        &p,
        NativeConfig::default(),
        &mut log,
        sid,
        "t3",
        "three",
        extra(None, 3),
    )
    .await;
    assert_eq!(
        r3[0].messages[..last.messages.len()],
        last.messages[..],
        "the cached prefix moved"
    );
}

/// Turn 4 makes no request that fits; the summary made then replaces turn
/// 1, the only one that carried the instruction files. Unsent so far this
/// turn (the model had them), they go again in the request sent after the
/// summary, and the turn's record says so, so turn 5 rebuilds it.
struct Overflowing {
    seen: Mutex<Vec<ChatRequest>>,
    failed: Mutex<bool>,
}

#[async_trait::async_trait]
impl Provider for Overflowing {
    fn name(&self) -> &'static str {
        "overflowing"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        if req.tools.is_empty() {
            return Ok(Completion {
                text: "## Goal\nshort".into(),
                ..Default::default()
            });
        }
        self.seen.lock().unwrap().push(req.clone());
        let user = opening(req);
        if user.ends_with("four") && !std::mem::replace(&mut *self.failed.lock().unwrap(), true) {
            return Err(ProviderError::Status {
                status: 400,
                body: r#"{"error":{"message":"too long","code":"context_length_exceeded"}}"#.into(),
                hint: Default::default(),
            });
        }
        Ok(Completion {
            text: "ok".into(),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn a_summary_mid_turn_sends_the_withheld_sections_again() {
    let p = Arc::new(Overflowing {
        seen: Mutex::default(),
        failed: Mutex::default(),
    });
    let sid = SessionId::new();
    let mut log = Vec::new();
    let run = async |log: &mut Vec<RawEvent>, id: &str, text: &str| {
        let host = Host::default();
        let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
        let ex = extra(Some(agents_md()), 1);
        let before = p.seen.lock().unwrap().len();
        b.run_turn(TurnCtx {
            session_id: sid,
            turn_id: id.into(),
            history: log.clone(),
            user_text: text.into(),
            cwd: std::env::temp_dir(),
            model: "x".into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra: ex.clone(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
        append(log, sid, id, text, &ex, &host);
        (p.seen.lock().unwrap()[before..].to_vec(), host.records())
    };
    // A long first message, so its summary shrinks the history.
    run(&mut log, "t1", &"word ".repeat(2000)).await;
    run(&mut log, "t2", "two").await;
    run(&mut log, "t3", "three").await;
    let (r4, c4) = run(&mut log, "t4", "four").await;
    assert_eq!(r4.len(), 2, "the overflowing request and its retry");
    assert!(!opening(&r4[0]).contains("Rule 1:"), "withheld at first");
    assert!(
        opening(&r4[1]).contains("Rule 1:"),
        "sent after the summary"
    );
    assert_eq!(c4.len(), 2);
    assert!(c4[1].sections.iter().any(|m| m.key == "instructions"));
    let (r5, c5) = run(&mut log, "t5", "five").await;
    let last = r4.last().unwrap();
    assert_eq!(r5[0].messages[..last.messages.len()], last.messages[..]);
    assert!(!opening(&r5[0]).contains("Rule 1:"));
    assert!(c5[0].sections.is_empty(), "{:?}", c5[0].sections);
}
