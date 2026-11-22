//! The first turn after a rewind (X.46, docs/rewind.md): what the loop
//! sends and what the log rebuilds afterwards are the same bytes, whatever
//! started the turn (a person's message, a callback), with the harness's
//! line about the files in both, once, and not when no file is still
//! changed. The pattern of `turn_compaction_review.rs`: a real loop over a
//! host that keeps a log, the first request compared with the rebuild of
//! the log up to its answer.

use std::sync::{Arc, Mutex};

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    Rebuild, place_notes, rebuild, split_summary,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{AgentBackend, TurnCtx, TurnExtra, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

struct Host {
    sid: SessionId,
    log: Mutex<Vec<RawEvent>>,
}

impl Host {
    fn push(&self, k: EventKind) -> u64 {
        let mut log = self.log.lock().unwrap();
        let seq = log.last().map_or(1, |e| e.seq + 1);
        let (kind, payload) = k.into_parts();
        log.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
        seq
    }
    fn log(&self) -> Vec<RawEvent> {
        self.log.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

#[derive(Default)]
struct Model {
    seen: Mutex<Vec<Vec<ChatMsg>>>,
}

#[async_trait::async_trait]
impl Provider for Model {
    fn name(&self) -> &'static str {
        "model"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.messages.clone());
        Ok(Completion {
            text: "done".into(),
            ..Default::default()
        })
    }
}

fn agent() -> AgentSpec {
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec![]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: vec![("m".to_string(), 32_768)].into_iter().collect(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates: vec![],
        caps: None,
    }
}

fn started(t: &str, cause: Option<TurnCause>) -> EventKind {
    EventKind::TurnStarted(TurnStarted {
        turn_id: t.into(),
        agent: None,
        cause,
        environment: None,
        retry: None,
        continued_by: None,
    })
}

fn said(t: &str, text: &str) -> EventKind {
    EventKind::UserMessage(serde_json::from_value(json!({"turn_id": t, "text": text})).unwrap())
}

fn checkpoint(t: &str, phase: CheckpointPhase, files: &[&str]) -> EventKind {
    EventKind::Checkpoint(Checkpoint {
        turn_id: t.into(),
        phase,
        git_ref: "r".into(),
        files: files.iter().map(|f| f.to_string()).collect(),
        excluded: vec![],
        excluded_more: 0,
        head_moved: None,
    })
}

fn reply(t: &str, id: &str, text: &str) -> EventKind {
    EventKind::AssistantMessage(AssistantMessage {
        turn_id: t.into(),
        message_id: id.into(),
        text: text.into(),
        tool_calls: vec![],
        author: None,
        variant: None,
        blocks: vec![],
        thinking: None,
    })
}

fn ended(t: &str) -> EventKind {
    EventKind::TurnEnded(TurnEnded {
        turn_id: t.into(),
        reason: TurnEndReason::Completed,
        usage: None,
    })
}

/// What a later request rebuilds from `log`, as the loop builds it, less
/// the system prompt.
fn rebuilt(log: &[RawEvent]) -> Vec<ChatMsg> {
    let (_summary, recent) = split_summary(log);
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(rebuild(
        recent,
        &Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    place_notes(m).split_off(1)
}

#[derive(Clone, Copy, Debug)]
enum Start {
    Person,
    Callback,
}

/// Two turns, the second changing `changed`, rewound (files kept); then a
/// turn started by `start`. Returns the first request of that turn and
/// what the log rebuilds for the same point.
async fn first_turn_after(
    start: Start,
    changed: &[&str],
    undo: bool,
) -> (Vec<ChatMsg>, Vec<ChatMsg>, Vec<ChatMsg>) {
    let sid = SessionId::new();
    let host = Host {
        sid,
        log: Mutex::default(),
    };
    host.push(started("t1", None));
    host.push(said("t1", "first ask"));
    host.push(reply("t1", "m1", "first answer"));
    host.push(ended("t1"));
    host.push(started("t2", None));
    let m2 = host.push(said("t2", "second ask"));
    if !changed.is_empty() {
        host.push(checkpoint("t2", CheckpointPhase::Before, &[]));
        host.push(checkpoint("t2", CheckpointPhase::After, changed));
    }
    host.push(reply("t2", "m2", "second answer"));
    host.push(ended("t2"));
    let r = host.push(EventKind::ConversationRewound(ConversationRewound {
        message_seq: m2,
        turn_id: "t2".into(),
        files: false,
    }));
    if undo {
        host.push(EventKind::ConversationRewindUndone(
            ConversationRewindUndone { rewind_seq: r },
        ));
    }
    let history = host.log();
    let cause = match start {
        Start::Person => None,
        Start::Callback => Some(TurnCause::Callback(vec!["bg-1".into()])),
    };
    host.push(started("t3", cause.clone()));
    if matches!(start, Start::Person) {
        host.push(said("t3", "third ask"));
    }
    let model = Arc::new(Model::default());
    let b = NativeBackend::new(model.clone(), vec![], NativeConfig::default());
    b.run_turn(TurnCtx {
        session_id: sid,
        turn_id: "t3".into(),
        history,
        user_text: if matches!(start, Start::Person) {
            "third ask".into()
        } else {
            String::new()
        },
        cwd: Default::default(),
        model: "m".into(),
        agent: Some(agent()),
        compare: vec![],
        parent_session: None,
        extra: TurnExtra {
            cause,
            ..Default::default()
        },
        cancel: Default::default(),
        host: &host,
    })
    .await;
    let live = model.seen.lock().unwrap()[0][1..].to_vec();
    let log = host.log();
    let cut = log
        .iter()
        .position(|e| e.kind == "assistant_message" && e.seq > r)
        .unwrap();
    let want = rebuilt(&log[..cut]);
    // And what the whole log rebuilds to later, for the second turn's look.
    let later = rebuilt(&log);
    (live, want, later)
}

fn lines(msgs: &[ChatMsg]) -> Vec<String> {
    let mut out = vec![];
    for m in msgs {
        let t = match m {
            ChatMsg::Note(t) | ChatMsg::Opening(t) | ChatMsg::User(t) => t,
            _ => continue,
        };
        if t.contains("took this conversation back") {
            out.push(t.clone());
        }
    }
    out
}

#[tokio::test]
async fn the_line_about_kept_files_is_the_same_live_and_rebuilt_for_every_start() {
    for start in [Start::Person, Start::Callback] {
        let (live, want, later) = first_turn_after(start, &["src/a.rs"], false).await;
        assert_eq!(live, want, "{start:?}: live and rebuilt differ");
        let l = lines(&live);
        assert_eq!(l.len(), 1, "{start:?}: once: {live:?}");
        assert!(
            l[0].contains("src/a.rs") && l[0].contains("kept as they are on disk"),
            "{start:?}"
        );
        // The next turn's rebuild still holds it once, in the same place.
        assert_eq!(lines(&later).len(), 1, "{start:?}");
    }
}

#[tokio::test]
async fn the_line_does_not_say_files_were_kept_when_none_is_still_changed() {
    for start in [Start::Person, Start::Callback] {
        let (live, want, _) = first_turn_after(start, &[], false).await;
        assert_eq!(live, want, "{start:?}: live and rebuilt differ");
        let l = lines(&live);
        assert_eq!(l.len(), 1, "{start:?}: {live:?}");
        assert!(
            !l[0].contains("kept") && !l[0].contains("files"),
            "{start:?}: {}",
            l[0]
        );
        assert!(l[0].contains("no longer part of it"), "{start:?}");
    }
}

#[tokio::test]
async fn an_undone_rewind_leaves_no_line() {
    for start in [Start::Person, Start::Callback] {
        let (live, want, _) = first_turn_after(start, &["src/a.rs"], true).await;
        assert_eq!(live, want, "{start:?}");
        assert!(lines(&live).is_empty(), "{start:?}: {live:?}");
    }
}

// ---- a sub-agent after a rewind ------------------------------------------

/// Takes `delegate` as the node does and keeps what it is handed.
struct Delegating {
    inner: Host,
    handed: Mutex<Option<(String, Vec<String>)>>,
}

#[async_trait::async_trait]
impl TurnHost for Delegating {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.inner.emit(kind).await
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    async fn delegate(
        &self,
        _agent: AgentSpec,
        task: String,
        _refs: harness_core::refs::SharedRefs,
        observed: Arc<Vec<String>>,
        _cancel: tokio_util::sync::CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
        _folder: Option<harness_core::subagent::SubFolder>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        let child = SessionId::new();
        started(child);
        *self.handed.lock().unwrap() = Some((task, observed.to_vec()));
        Ok(harness_core::backend::DelegateOutcome {
            child,
            text: "looked".into(),
            reason: TurnEndReason::Completed,
            usage: None,
            sources: vec![],
            finish: None,
            error: None,
            note: None,
            unapplied: None,
            applied: true,
        })
    }
}

/// Plays the lead: delegates once, then answers.
struct Lead(Mutex<Vec<Vec<ChatMsg>>>);

#[async_trait::async_trait]
impl Provider for Lead {
    fn name(&self) -> &'static str {
        "lead"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.0.lock().unwrap().push(req.messages.clone());
        if req
            .messages
            .iter()
            .any(|m| matches!(m, ChatMsg::Tool { .. }))
        {
            return Ok(Completion {
                text: "done".into(),
                ..Default::default()
            });
        }
        Ok(Completion {
            tool_calls: vec![ToolCallRef {
                call_id: "d1".into(),
                tool: "delegate".into(),
                input: json!({"agent": "reader", "task": "look at the visible thing"}),
            }],
            ..Default::default()
        })
    }
}

/// Question 7 of the review: a delegation hands the child its task, the
/// turn's master context and the parent's `observed` strings (what the web
/// tools may follow: the person's own words and trusted results of the
/// request the parent is sent). Never the parent's history, and, since that
/// request is the visible history, nothing of a rewound turn: the person's
/// words of a rewound message are not in what the child may follow.
#[tokio::test]
async fn a_sub_agent_after_a_rewind_is_handed_nothing_of_the_rewound_turns() {
    let sid = SessionId::new();
    let inner = Host {
        sid,
        log: Mutex::default(),
    };
    inner.push(started("t1", None));
    inner.push(said("t1", "visible words https://visible.example/a"));
    inner.push(reply("t1", "m1", "first answer"));
    inner.push(ended("t1"));
    inner.push(started("t2", None));
    let m2 = inner.push(said("t2", "REWOUND-WORDS https://rewound.example/b"));
    inner.push(reply("t2", "m2", "REWOUND-ANSWER"));
    inner.push(ended("t2"));
    inner.push(EventKind::ConversationRewound(ConversationRewound {
        message_seq: m2,
        turn_id: "t2".into(),
        files: false,
    }));
    let history = inner.log();
    inner.push(started("t3", None));
    inner.push(said("t3", "now delegate"));
    let host = Delegating {
        inner,
        handed: Mutex::default(),
    };
    let lead = Arc::new(Lead(Mutex::default()));
    let b = NativeBackend::new(lead.clone(), vec![], NativeConfig::default());
    let mut spec = agent();
    spec.delegates = vec![{
        let mut r = agent();
        r.agent_id = "reader".into();
        r.name = "reader".into();
        r
    }];
    let out = b
        .run_turn(TurnCtx {
            session_id: sid,
            turn_id: "t3".into(),
            history,
            user_text: "now delegate".into(),
            cwd: Default::default(),
            model: "m".into(),
            agent: Some(spec),
            compare: vec![],
            parent_session: None,
            extra: TurnExtra::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{:?}", out.error);
    let (task, observed) = host.handed.lock().unwrap().clone().expect("delegated");
    assert_eq!(task, "look at the visible thing");
    let seen = observed.join("\n");
    assert!(seen.contains("visible.example"), "{seen}");
    assert!(
        !seen.contains("REWOUND") && !seen.contains("rewound.example"),
        "the child may follow what a rewound message held: {seen}"
    );
    // And the parent's own requests never held it.
    let all = format!("{:?}", lead.0.lock().unwrap());
    assert!(!all.contains("REWOUND"), "{all}");
}
