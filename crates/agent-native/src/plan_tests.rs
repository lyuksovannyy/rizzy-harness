//! Plan mode in the loop (ADOPTION_PLAN X.7, PLAN.md T67): what it offers,
//! what it refuses (before any card, with the plain sentence, nothing run),
//! the delegates it starts, the harness's line it sends, a change of mode
//! during a turn, and that the parallel batches of X.19 still form. Then
//! read-only agents outside plan mode (T.7, PLAN.md T70): an agent that
//! changes nothing else gets its shell only as the read-only shell, so on
//! a node without one (these tests: no bubblewrap) it gets none and is
//! told why.
//!
//! The host stands in for the node: it answers the chat's mode as a
//! session does (`approval_mode`, `runs_unasked`), and records every
//! approval it is asked for. The tools are stand-ins under the real names,
//! each counting its runs. The node's own floor (`Session::approve`) and
//! the read-only shell itself are tested in `crates/node/tests` and
//! `crates/cli/tests/read_only_shell.rs`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use harness_tools::{ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::provider::*;
use crate::*;

struct Host {
    events: Mutex<Vec<EventKind>>,
    mode: Mutex<ApprovalMode>,
    /// The tools the host was asked to approve, in order.
    asked: Mutex<Vec<String>>,
    /// The agents started as children.
    children: Mutex<Vec<String>>,
    /// The node's `denied_tools`.
    denied: Mutex<Vec<String>>,
}

impl Host {
    fn new(mode: ApprovalMode) -> Self {
        Self {
            events: Mutex::default(),
            mode: Mutex::new(mode),
            asked: Mutex::default(),
            children: Mutex::default(),
            denied: Mutex::default(),
        }
    }

    fn results(&self) -> Vec<(String, bool, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some((f.call_id.clone(), f.ok, f.output.clone())),
                _ => None,
            })
            .collect()
    }

    fn result(&self, id: &str) -> (bool, String) {
        let r = self.results();
        let (_, ok, out) = r.iter().find(|(i, ..)| i == id).expect(id);
        (*ok, out.clone())
    }

    fn cards(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, EventKind::ApprovalRequested(_)))
            .count()
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _c: &str, tool: &str, _i: &Value, _risk: ToolRisk) -> Decision {
        self.asked.lock().unwrap().push(tool.into());
        // As the node: refused in plan mode, never asked.
        if self.mode.lock().unwrap().refuses(tool) {
            Decision::Deny
        } else {
            Decision::Allow
        }
    }
    fn runs_unasked(&self, tool: &str, risk: ToolRisk) -> bool {
        let m = *self.mode.lock().unwrap();
        !m.needs_approval(risk) && !m.refuses(tool)
    }
    fn approval_mode(&self) -> Option<ApprovalMode> {
        Some(*self.mode.lock().unwrap())
    }
    fn tool_denied(&self, tool: &str) -> bool {
        self.denied.lock().unwrap().iter().any(|t| t == tool)
    }
    async fn delegate(
        &self,
        agent: AgentSpec,
        _task: String,
        _refs: SharedRefs,
        _observed: Arc<Vec<String>>,
        _cancel: tokio_util::sync::CancellationToken,
        started: Box<dyn FnOnce(SessionId) + Send>,
    ) -> Result<harness_core::backend::DelegateOutcome, String> {
        self.children.lock().unwrap().push(agent.agent_id.clone());
        let child = SessionId::new();
        started(child);
        Ok(harness_core::backend::DelegateOutcome {
            child,
            text: format!("{} looked", agent.agent_id),
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

/// Sends the calls of step `n` in its `n`-th answer, then answers; before
/// step `switch.0` it sets the host's mode to `switch.1` (the person's
/// change during the turn).
struct Rec {
    steps: Vec<Vec<(&'static str, Value)>>,
    seen: Mutex<Vec<ChatRequest>>,
    switch: Option<(usize, ApprovalMode, Arc<Host>)>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "rec"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .map_or(0, |i| i + 1);
        let step = req.messages[since..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Assistant { .. }))
            .count();
        if let Some((at, mode, host)) = &self.switch
            && step == *at
        {
            *host.mode.lock().unwrap() = *mode;
        }
        let tool_calls = self
            .steps
            .get(step)
            .map(|s| {
                s.iter()
                    .map(|(t, i)| ToolCallRef {
                        call_id: i["id"].as_str().unwrap().into(),
                        tool: (*t).into(),
                        input: i.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Completion {
            text: "the plan".into(),
            tool_calls,
            usage: None,
            stop: Default::default(),
        })
    }
}

/// A stand-in under a real tool's name and risk: counts its runs, sleeps
/// `ms`, answers its `id`.
struct Stand {
    name: &'static str,
    risk: ToolRisk,
    parallel: bool,
    runs: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl harness_tools::Tool for Stand {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        "stand-in"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        self.risk
    }
    fn parallel_safe(&self) -> bool {
        self.parallel
    }
    async fn run(&self, _ctx: &ToolCtx, input: Value) -> ToolOutcome {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(input["ms"].as_u64().unwrap_or(0))).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        ToolOutcome::ok(format!("ran {}", input["id"].as_str().unwrap_or("?")))
    }
}

/// Every tool the registry can hold, under its real name and risk (the
/// builtin set, `remember`, `recall`, the web tools, `graph_query`).
const REGISTRY: &[(&str, ToolRisk, bool)] = &[
    ("bash", ToolRisk::Mutating, false),
    ("bash_output", ToolRisk::ReadOnly, false),
    ("bash_stop", ToolRisk::ReadOnly, false),
    ("read", ToolRisk::ReadOnly, true),
    ("write", ToolRisk::Mutating, false),
    ("edit", ToolRisk::Mutating, false),
    ("grep", ToolRisk::ReadOnly, true),
    ("glob", ToolRisk::ReadOnly, true),
    ("tool_output", ToolRisk::ReadOnly, true),
    ("outline", ToolRisk::ReadOnly, true),
    ("publish", ToolRisk::Mutating, false),
    ("todo", ToolRisk::ChatOnly, false),
    ("skill", ToolRisk::ReadOnly, false),
    ("remember", ToolRisk::ReadOnly, false),
    ("recall", ToolRisk::ReadOnly, false),
    ("web_search", ToolRisk::ReadOnly, false),
    ("web_fetch", ToolRisk::ReadOnly, false),
    ("graph_query", ToolRisk::ReadOnly, false),
];

struct Run {
    out: TurnOutcome,
    rec: Arc<Rec>,
    runs: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

fn spec(id: &str, tools: &[&str], delegates: Vec<AgentSpec>) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "p".into(),
        tools: ToolSelection::List(tools.iter().map(|s| s.to_string()).collect()),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates,
        caps: None,
    }
}

/// A lead with every registered tool and two delegates: `scout` (reads
/// only, and `bash`) and `coder` (writes).
fn lead() -> AgentSpec {
    let mut all: Vec<&str> = REGISTRY.iter().map(|(n, ..)| *n).collect();
    all.push("mcp");
    spec(
        "lead",
        &all,
        vec![
            spec("scout", &["read", "grep", "bash", "remember"], vec![]),
            spec("coder", &["read", "write", "edit"], vec![]),
        ],
    )
}

async fn run(
    host: Arc<Host>,
    steps: Vec<Vec<(&'static str, Value)>>,
    switch: Option<(usize, ApprovalMode)>,
) -> Run {
    run_with(host, steps, switch, Default::default()).await
}

async fn run_with(
    host: Arc<Host>,
    steps: Vec<Vec<(&'static str, Value)>>,
    switch: Option<(usize, ApprovalMode)>,
    extra: harness_core::backend::TurnExtra,
) -> Run {
    run_as(host, lead(), steps, switch, extra).await
}

async fn run_as(
    host: Arc<Host>,
    agent: AgentSpec,
    steps: Vec<Vec<(&'static str, Value)>>,
    switch: Option<(usize, ApprovalMode)>,
    extra: harness_core::backend::TurnExtra,
) -> Run {
    let rec = Arc::new(Rec {
        steps,
        seen: Mutex::default(),
        switch: switch.map(|(at, m)| (at, m, host.clone())),
    });
    let runs = Arc::new(AtomicUsize::new(0));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let tools: Vec<Arc<dyn harness_tools::Tool>> = REGISTRY
        .iter()
        .map(|(name, risk, parallel)| {
            Arc::new(Stand {
                name,
                risk: *risk,
                parallel: *parallel,
                runs: runs.clone(),
                running: running.clone(),
                most: most.clone(),
            }) as Arc<dyn harness_tools::Tool>
        })
        .collect();
    // The default confinement has no read-only shell (no bubblewrap).
    let b = NativeBackend::new(rec.clone(), tools, NativeConfig::default());
    let dir = tempfile::tempdir().unwrap();
    let out = b
        .run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: "t".into(),
            history: Vec::<RawEvent>::new(),
            user_text: "plan the change".into(),
            cwd: dir.path().to_path_buf(),
            model: "rec".into(),
            agent: Some(agent),
            compare: vec![],
            parent_session: None,
            extra,
            cancel: Default::default(),
            host: &*host,
        })
        .await;
    Run {
        out,
        rec,
        runs,
        most,
    }
}

fn call(tool: &'static str, id: &str) -> (&'static str, Value) {
    (tool, json!({"id": id}))
}

/// Every tool that changes something is refused with the sentence, before
/// any card, and runs nothing; this holds whether or not it was offered.
#[tokio::test]
async fn every_mutating_tool_is_refused_with_no_card_and_no_run() {
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let refused = [
        "write",
        "edit",
        "publish",
        "remember",
        "mcp__srv__do",
        "bash",
    ];
    let r = run(
        host.clone(),
        vec![
            refused
                .iter()
                .enumerate()
                .map(|(i, t)| call(t, &format!("c{i}")))
                .collect(),
        ],
        None,
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(r.runs.load(Ordering::SeqCst), 0, "a refused tool ran");
    assert_eq!(host.cards(), 0, "a card was shown");
    assert!(host.asked.lock().unwrap().is_empty(), "the host was asked");
    for (i, t) in refused.iter().enumerate() {
        let (ok, out) = host.result(&format!("c{i}"));
        assert!(!ok, "{t}");
        assert!(out.starts_with("[harness] plan mode: `"), "{t}: {out}");
        if *t == "bash" {
            // No bubblewrap in this test: no read-only shell, said why.
            assert!(out.contains("no read-only shell"), "{out}");
        } else {
            assert!(out.contains("nothing was changed"), "{t}: {out}");
        }
    }
}

/// The request offers only what plan mode lets run: no `write`, `edit`,
/// `publish`, `remember`; no `bash` without a read-only shell; the
/// delegate list has only the agent that reads; and the harness's line
/// is in the turn's message.
#[tokio::test]
async fn the_request_offers_only_what_plan_mode_runs() {
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let r = run(host.clone(), vec![], None).await;
    let reqs = r.rec.seen.lock().unwrap().clone();
    let names: Vec<&str> = reqs[0].tools.iter().map(|t| t.name.as_str()).collect();
    for gone in ["write", "edit", "publish", "remember", "bash"] {
        assert!(!names.contains(&gone), "{gone} offered: {names:?}");
    }
    for kept in [
        "read",
        "grep",
        "glob",
        "outline",
        "tool_output",
        "todo",
        "skill",
        "recall",
        "web_search",
        "web_fetch",
        "graph_query",
        DELEGATE,
    ] {
        assert!(names.contains(&kept), "{kept} not offered: {names:?}");
    }
    let delegate = reqs[0].tools.iter().find(|t| t.name == DELEGATE).unwrap();
    assert!(delegate.description.contains("scout"));
    assert!(!delegate.description.contains("coder"));
    let text = reqs[0]
        .messages
        .iter()
        .filter_map(|m| m.user_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("[harness] Plan mode is on"), "{text}");
    assert!(
        text.contains("there is no shell in plan mode here"),
        "{text}"
    );
    // The line went into the log with the block it was sent in.
    let logged = host.events.lock().unwrap().iter().any(|e| match e {
        EventKind::ContextSent(c) => c
            .sections
            .iter()
            .any(|s| s.text.contains("Plan mode is on")),
        _ => false,
    });
    assert!(logged, "the plan line is not in context_sent");

    // Out of plan mode none of this: every tool offered, no line.
    let host = Arc::new(Host::new(ApprovalMode::Auto));
    let r = run(host, vec![], None).await;
    let reqs = r.rec.seen.lock().unwrap().clone();
    let names: Vec<&str> = reqs[0].tools.iter().map(|t| t.name.as_str()).collect();
    for kept in ["write", "edit", "publish", "remember", "bash"] {
        assert!(names.contains(&kept), "{kept}: {names:?}");
    }
    let text = reqs[0]
        .messages
        .iter()
        .filter_map(|m| m.user_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("Plan mode"), "{text}");
}

/// A delegate that can write is refused before a child exists; one that
/// only reads is started (its own calls inherit the mode at the node).
#[tokio::test]
async fn a_writer_delegate_is_refused_and_a_reader_runs() {
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let r = run(
        host.clone(),
        vec![vec![
            (
                DELEGATE,
                json!({"id": "d1", "agent": "coder", "task": "change it"}),
            ),
            (
                DELEGATE,
                json!({"id": "d2", "agent": "scout", "task": "look"}),
            ),
        ]],
        None,
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(*host.children.lock().unwrap(), ["scout"]);
    let (ok, out) = host.result("d1");
    assert!(!ok);
    assert!(out.contains("@coder was not started"), "{out}");
    let (ok, out) = host.result("d2");
    assert!(ok, "{out}");
    assert_eq!(host.cards(), 0);
}

/// Read-only calls still run together in plan mode (X.19).
#[tokio::test]
async fn reads_still_batch_in_plan_mode() {
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let r = run(
        host.clone(),
        vec![vec![
            ("read", json!({"id": "r1", "ms": 200})),
            ("grep", json!({"id": "r2", "ms": 200})),
            ("glob", json!({"id": "r3", "ms": 200})),
        ]],
        None,
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(r.runs.load(Ordering::SeqCst), 3);
    assert_eq!(r.most.load(Ordering::SeqCst), 3, "the reads did not batch");
    assert!(host.results().iter().all(|(_, ok, _)| *ok));
}

/// The person switches to plan mode during a turn: the next call that
/// would change something is refused, though the turn offered it.
#[tokio::test]
async fn switching_to_plan_mode_during_a_turn_refuses_the_next_call() {
    let host = Arc::new(Host::new(ApprovalMode::Auto));
    let r = run(
        host.clone(),
        vec![vec![call("write", "w1")], vec![call("write", "w2")]],
        Some((1, ApprovalMode::Plan)),
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(r.runs.load(Ordering::SeqCst), 1, "only the first write ran");
    assert!(host.result("w1").0);
    let (ok, out) = host.result("w2");
    assert!(!ok);
    assert!(out.contains("plan mode: `write` was not run"), "{out}");
}

/// The same refused call again and again ends the turn (the repeated-call
/// guard counts refusals, T40): a model that ignores the sentence does not
/// loop.
#[tokio::test]
async fn a_model_that_keeps_writing_is_stopped() {
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let steps = (0..20)
        .map(|_| vec![("write", json!({"id": "same", "path": "x", "content": "y"}))])
        .collect();
    let r = run(host.clone(), steps, None).await;
    assert_eq!(r.out.reason, TurnEndReason::Error, "{:?}", r.out);
    assert_eq!(r.runs.load(Ordering::SeqCst), 0);
    assert!(host.results().len() < 20, "{}", host.results().len());
}

/// Review H1, the turns that start after the switch: a callback turn (a
/// background command ended) and a turn that reads a message the person
/// queued while plan mode began are plan-mode turns like any: the write is
/// refused, the line is sent.
#[tokio::test]
async fn a_callback_turn_and_a_queued_message_run_in_plan_mode() {
    let queued = UserMessage {
        turn_id: "earlier".into(),
        text: "also write the file".into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: true,
        files: vec![],
    };
    for extra in [
        harness_core::backend::TurnExtra {
            cause: Some(TurnCause::Callback(vec!["bg-1".into()])),
            ..Default::default()
        },
        harness_core::backend::TurnExtra {
            queued: vec![queued.clone()],
            ..Default::default()
        },
    ] {
        let callback = extra.cause.is_some();
        let host = Arc::new(Host::new(ApprovalMode::Plan));
        let r = run_with(host.clone(), vec![vec![call("write", "w")]], None, extra).await;
        assert_eq!(r.runs.load(Ordering::SeqCst), 0, "callback {callback}");
        let (ok, out) = host.result("w");
        assert!(
            !ok && out.contains("plan mode"),
            "callback {callback}: {out}"
        );
        let sent = r.rec.seen.lock().unwrap()[0].clone();
        let text = format!("{:?}", sent.messages);
        assert!(text.contains("Plan mode is on"), "callback {callback}");
    }
}

/// The names a request offered and the text of its messages.
fn offered(r: &Run) -> (Vec<String>, String) {
    let reqs = r.rec.seen.lock().unwrap().clone();
    let names = reqs[0].tools.iter().map(|t| t.name.clone()).collect();
    let text = reqs[0]
        .messages
        .iter()
        .filter_map(|m| m.user_text())
        .collect::<Vec<_>>()
        .join("\n");
    (names, text)
}

/// A read-only agent (Scout as shipped: reading tools and the shell) on a
/// node with no read-only shell, in every mode but plan: no `bash`,
/// `bash_output` or `bash_stop`, the context block says why, and a call
/// by name runs nothing, asks nothing and says why (T.7, PLAN.md T70).
#[tokio::test]
async fn a_read_only_agent_gets_no_shell_where_the_node_has_none() {
    let scout = spec(
        "scout",
        &[
            "read",
            "grep",
            "glob",
            "outline",
            "skill",
            "bash",
            "bash_output",
            "bash_stop",
            "tool_output",
        ],
        vec![],
    );
    for mode in [
        ApprovalMode::AskAll,
        ApprovalMode::Ask,
        ApprovalMode::AcceptEdits,
        ApprovalMode::Auto,
    ] {
        let host = Arc::new(Host::new(mode));
        let r = run_as(
            host.clone(),
            scout.clone(),
            vec![vec![call("bash", "b"), call("bash_output", "o")]],
            None,
            Default::default(),
        )
        .await;
        let (names, text) = offered(&r);
        for gone in ["bash", "bash_output", "bash_stop"] {
            assert!(
                !names.contains(&gone.to_string()),
                "{mode:?}: {gone} offered: {names:?}"
            );
        }
        assert!(names.contains(&"read".to_string()), "{names:?}");
        assert!(text.contains("there is no `bash` here"), "{mode:?}: {text}");
        assert_eq!(
            r.runs.load(Ordering::SeqCst),
            0,
            "{mode:?}: a shell tool ran"
        );
        assert!(host.asked.lock().unwrap().is_empty(), "{mode:?}: asked");
        for id in ["b", "o"] {
            let (ok, out) = host.result(id);
            assert!(!ok, "{mode:?}");
            assert!(
                out.contains("read-only shell, and this node has none"),
                "{mode:?}: {out}"
            );
        }
    }
}

/// An agent that changes things keeps its ordinary shell, described as
/// such; the node's `denied_tools` narrowing it to a reader gives it the
/// read-only shell's rule (none here), never the other way round; a spec
/// that says anything about its shell is not read (there is no such
/// field: a hostile master's extra keys are dropped).
#[tokio::test]
async fn what_the_node_offers_decides_the_shell() {
    let coder = spec("coder", &["read", "write", "bash"], vec![]);
    let host = Arc::new(Host::new(ApprovalMode::Auto));
    let r = run_as(host, coder.clone(), vec![], None, Default::default()).await;
    let reqs = r.rec.seen.lock().unwrap().clone();
    let bash = reqs[0]
        .tools
        .iter()
        .find(|t| t.name == "bash")
        .expect("bash");
    assert!(
        !bash.description.contains("READ-ONLY"),
        "{}",
        bash.description
    );

    // The node denies `write`: what is left only reads, so its shell
    // must be the read-only one, and there is none here.
    let host = Arc::new(Host::new(ApprovalMode::Auto));
    host.denied.lock().unwrap().push("write".into());
    let r = run_as(host, coder, vec![], None, Default::default()).await;
    let (names, text) = offered(&r);
    assert!(!names.contains(&"bash".to_string()), "{names:?}");
    assert!(text.contains("there is no `bash` here"), "{text}");

    // A spec from a hostile master asking for a writable shell for a
    // reader: the keys mean nothing; the tool list decides.
    let hostile: AgentSpec = serde_json::from_value(json!({
        "agent_id": "scout", "name": "Scout", "system_prompt": "p", "prompt_hash": "h",
        "tools": ["read", "bash"],
        "read_only_shell": false, "writable_shell": true, "shell": "writable",
        "features": {"commands": true, "read_only_commands": false}
    }))
    .unwrap();
    assert!(!hostile.changes_things(|_| true));
    let host = Arc::new(Host::new(ApprovalMode::Auto));
    let r = run_as(
        host.clone(),
        hostile,
        vec![vec![call("bash", "b")]],
        None,
        Default::default(),
    )
    .await;
    let (names, _) = offered(&r);
    assert!(!names.contains(&"bash".to_string()), "{names:?}");
    assert_eq!(r.runs.load(Ordering::SeqCst), 0);
    assert!(host.result("b").1.contains("read-only shell"));
}

/// Plan mode starts a reader with a shell, and refuses a delegate whose
/// list holds a tool the shared list does not know (it changes things
/// until someone lists it), whatever the node registered.
#[tokio::test]
async fn plan_mode_judges_delegates_by_the_shared_list() {
    let lead = spec(
        "lead",
        &["read"],
        vec![
            spec(
                "scout",
                &["read", "bash", "bash_output", "bash_stop", "tool_output"],
                vec![],
            ),
            spec("odd", &["read", "notebook_edit"], vec![]),
            spec("poster", &["read", "publish"], vec![]),
        ],
    );
    let host = Arc::new(Host::new(ApprovalMode::Plan));
    let r = run_as(
        host.clone(),
        lead,
        vec![vec![
            (
                DELEGATE,
                json!({"id": "d1", "agent": "scout", "task": "look"}),
            ),
            (
                DELEGATE,
                json!({"id": "d2", "agent": "odd", "task": "look"}),
            ),
            (
                DELEGATE,
                json!({"id": "d3", "agent": "poster", "task": "look"}),
            ),
        ]],
        None,
        Default::default(),
    )
    .await;
    assert_eq!(r.out.reason, TurnEndReason::Completed, "{:?}", r.out);
    assert_eq!(*host.children.lock().unwrap(), ["scout"]);
    for id in ["d2", "d3"] {
        let (ok, out) = host.result(id);
        assert!(!ok && out.contains("it can change files"), "{id}: {out}");
    }
}
