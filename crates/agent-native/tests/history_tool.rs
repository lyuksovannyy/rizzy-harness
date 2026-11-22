//! The `history` tool in the loop (PLAN.md T91): an agent reads its own
//! chat's earlier turns after a summary and a clearing, only while its
//! switch is on, and an earlier image comes back the way `read` returns one
//! (only to a model that takes images, by reference in the log, the bytes
//! kept by the host, the request rebuilding from the log byte for byte).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use harness_agent_native::history::History;
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    Rebuild, images, place_notes, rebuild,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{
    AgentBackend, ImageData, KeptImage, LogError, TurnCtx, TurnExtra, TurnHost,
};
use harness_core::event::*;
use harness_core::history::{HistorySource, StoredImage};
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

fn png(w: u32, h: u32, pad: usize) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0]);
    b.extend((0..pad).map(|i| (i % 251) as u8));
    b
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn sha(b: &[u8]) -> String {
    harness_tools::seen::hex(&harness_tools::seen::sha256(b))
}

type Log = Arc<Mutex<Vec<RawEvent>>>;
type Store = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// The session's own log and images, as the node's store gives them.
struct Own {
    log: Log,
    store: Store,
    reads: Arc<Mutex<usize>>,
}

#[async_trait::async_trait]
impl HistorySource for Own {
    async fn log(&self) -> Result<Vec<RawEvent>, String> {
        *self.reads.lock().unwrap() += 1;
        Ok(self.log.lock().unwrap().clone())
    }
    async fn image(&self, sha256: &str) -> Result<Option<StoredImage>, String> {
        Ok(self.store.lock().unwrap().get(sha256).map(|b| StoredImage {
            mime: "image/png".into(),
            bytes: b.clone(),
        }))
    }
}

struct Host {
    sid: SessionId,
    log: Log,
    store: Store,
    reads: Arc<Mutex<usize>>,
    /// The node's `denied_tools` hold `history`.
    denied: std::sync::atomic::AtomicBool,
}

impl Host {
    fn new() -> Self {
        Self {
            sid: SessionId::new(),
            log: Default::default(),
            store: Default::default(),
            reads: Default::default(),
            denied: Default::default(),
        }
    }
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
    fn events(&self) -> Vec<RawEvent> {
        self.log.lock().unwrap().clone()
    }
    fn loaded(&self) -> Vec<ImageData> {
        self.store
            .lock()
            .unwrap()
            .iter()
            .map(|(sha, b)| ImageData {
                sha256: sha.clone(),
                mime: "image/png".into(),
                data: b64(b).into(),
            })
            .collect()
    }
    fn finished(&self) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e.kind == "tool_call_finished")
            .map(|e| e.payload)
            .collect()
    }

    /// An earlier turn, as a node would have logged it.
    fn old_turn(&self, n: usize, text: &str, result: &str, images: Vec<ImageRef>) {
        let turn = format!("old{n}");
        self.push(EventKind::TurnStarted(TurnStarted {
            turn_id: turn.clone(),
            agent: None,
            cause: None,
            environment: None,
            continued_by: None,
            retry: None,
        }));
        self.push(EventKind::UserMessage(UserMessage {
            turn_id: turn.clone(),
            text: text.into(),
            attachments: vec![],
            reply: None,
            images,
            queued: false,
            triggers: vec![],
            tags: vec![],
            files: vec![],
            uploads: vec![],
        }));
        self.push(EventKind::AssistantMessage(AssistantMessage {
            turn_id: turn.clone(),
            message_id: format!("{turn}m"),
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: format!("{turn}c"),
                tool: "read".into(),
                input: json!({"path": "src/lib.rs"}),
            }],
            author: None,
            variant: None,
            blocks: vec![],
            thinking: None,
        }));
        self.push(EventKind::ToolCallFinished(ToolCallFinished {
            call_id: format!("{turn}c"),
            ok: true,
            output: result.into(),
            exit_code: None,
            truncated: false,
            sources: vec![],
            note: None,
            seen: None,
            image: None,
        }));
        self.push(EventKind::AssistantMessage(AssistantMessage {
            turn_id: turn.clone(),
            message_id: format!("{turn}a"),
            text: "answered".into(),
            tool_calls: vec![],
            author: None,
            variant: None,
            blocks: vec![],
            thinking: None,
        }));
        self.push(EventKind::TurnEnded(TurnEnded {
            turn_id: turn,
            reason: TurnEndReason::Completed,
            usage: None,
        }));
    }

    fn summarize(&self, text: &str) {
        let through_seq = self.events().len() as u64;
        self.push(EventKind::HistorySummarized(HistorySummarized {
            turn_id: String::new(),
            through_seq,
            text: text.into(),
            manual: false,
            focus: None,
            usage: None,
            tokens_after: None,
        }));
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), LogError> {
        self.push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn runs_unasked(&self, _: &str, risk: ToolRisk) -> bool {
        risk == ToolRisk::ReadOnly
    }
    fn tool_denied(&self, tool: &str) -> bool {
        tool == "history" && self.denied.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn history(&self) -> Option<Arc<dyn HistorySource>> {
        Some(Arc::new(Own {
            log: self.log.clone(),
            store: self.store.clone(),
            reads: self.reads.clone(),
        }))
    }
    async fn keep_image(&self, i: KeptImage) -> Result<(), String> {
        self.store.lock().unwrap().insert(i.sha256, i.bytes);
        Ok(())
    }
    async fn images(&self, sha256: &[String]) -> Vec<ImageData> {
        self.loaded()
            .into_iter()
            .filter(|d| sha256.contains(&d.sha256))
            .collect()
    }
}

/// A scripted model: step `n` calls `history` with `steps[n]`, then answers.
struct Model {
    steps: Vec<Vec<Value>>,
    tool: &'static str,
    seen: Mutex<Vec<ChatRequest>>,
}

impl Model {
    fn new(tool: &'static str, steps: Vec<Vec<Value>>) -> Arc<Self> {
        Arc::new(Self {
            steps,
            tool,
            seen: Mutex::default(),
        })
    }
    fn requests(&self) -> Vec<ChatRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Provider for Model {
    fn name(&self) -> &'static str {
        "model"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len() - 1
        };
        Ok(match self.steps.get(n) {
            Some(calls) => Completion {
                tool_calls: calls
                    .iter()
                    .enumerate()
                    .map(|(k, input)| ToolCallRef {
                        call_id: format!("c{n}_{k}"),
                        tool: self.tool.into(),
                        input: input.clone(),
                    })
                    .collect(),
                ..Default::default()
            },
            None => Completion {
                text: "done".into(),
                ..Default::default()
            },
        })
    }
}

fn agent(vision: bool, history: bool) -> AgentSpec {
    let mut tools = vec!["read".to_string()];
    if history {
        tools.push("history".into());
    }
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(tools),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: if vision { vec!["m".into()] } else { vec![] },
        context_windows: Default::default(),
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

fn backend(p: Arc<dyn Provider>) -> NativeBackend {
    NativeBackend::new(
        p,
        vec![Arc::new(harness_tools::Read), Arc::new(History)],
        NativeConfig::default(),
    )
}

async fn turn(
    b: &NativeBackend,
    host: &Host,
    ws: &std::path::Path,
    spec: AgentSpec,
    turn_id: &str,
    text: &str,
) {
    let history = host.events();
    host.push(EventKind::TurnStarted(TurnStarted {
        continued_by: None,
        retry: None,
        cause: None,
        agent: None,
        turn_id: turn_id.into(),
        environment: None,
    }));
    host.push(EventKind::UserMessage(UserMessage {
        turn_id: turn_id.into(),
        text: text.into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: false,
        triggers: vec![],
        tags: vec![],
        files: vec![],
        uploads: vec![],
    }));
    b.run_turn(TurnCtx {
        session_id: host.sid,
        turn_id: turn_id.into(),
        history,
        user_text: text.into(),
        cwd: ws.to_path_buf(),
        model: "m".into(),
        agent: Some(spec),
        compare: vec![],
        parent_session: None,
        extra: TurnExtra::default(),
        cancel: Default::default(),
        host,
    })
    .await;
}

fn unquoted(answer: &str) -> String {
    answer
        .lines()
        .filter_map(|l| l.strip_prefix(OUTPUT_QUOTE))
        .collect::<Vec<_>>()
        .join("\n")
}

fn shown(msgs: &[ChatMsg]) -> Vec<(String, Option<String>)> {
    msgs.iter()
        .filter_map(|m| match m {
            ChatMsg::ToolImage { call_id, image, .. } => {
                Some((call_id.clone(), image.data.as_ref().map(|d| d.to_string())))
            }
            _ => None,
        })
        .collect()
}

fn rebuilt(log: &[RawEvent], loaded: &[ImageData], sees: bool) -> Vec<ChatMsg> {
    let mut m = vec![ChatMsg::System("x".into())];
    m.extend(rebuild(
        log,
        &Rebuild {
            blocks: true,
            agent: Some("dev"),
            ..Default::default()
        },
    ));
    images::attach(&mut m, loaded);
    images::tool_images(&mut m, sees, loaded);
    place_notes(m).split_off(1)
}

/// After two summaries the model is sent neither the old turns nor their
/// results; `history` gives the first turn's result back byte for byte, and
/// the call's own result in the log is quoted text, not a line of the
/// harness's.
#[tokio::test]
async fn an_agent_reads_an_old_result_after_summaries() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    let original: String = (0..400).map(|i| format!("fn item_{i}() {{}}\n")).collect();
    host.old_turn(1, "first question", &original, vec![]);
    host.summarize("The first turn read src/lib.rs.");
    host.old_turn(2, "second question", "short", vec![]);
    host.summarize("Two turns: a read, then a second one.");
    let p = Model::new(
        "history",
        vec![vec![json!({"turn": 1, "call": "old1c", "part": 1})]],
    );
    turn(
        &backend(p.clone()),
        &host,
        ws.path(),
        agent(false, true),
        "t3",
        "what did the first read show?",
    )
    .await;
    let reqs = p.requests();
    // The model was not sent the old result.
    let first: String = format!("{:?}", reqs[0].messages);
    assert!(!first.contains("item_399"), "the summary replaces it");
    assert!(reqs[0].tools.iter().any(|t| t.name == "history"));
    let fin = host.finished();
    let out = fin.last().unwrap();
    assert_eq!(out["ok"], json!(true), "{out}");
    let body = unquoted(out["output"].as_str().unwrap());
    assert!(body == original, "the result came back changed");
    // What the second request carries is that text, quoted.
    assert!(format!("{:?}", reqs[1].messages).contains("item_399"));
}

/// The tool is offered only to an agent whose switch is on, and a call of
/// it by an agent without it is an unknown tool.
#[tokio::test]
async fn it_is_offered_only_with_the_switch_and_a_forged_call_is_refused() {
    let ws = tempfile::tempdir().unwrap();
    for (on, call) in [(false, true), (true, true)] {
        let host = Host::new();
        host.old_turn(1, "q", "a secret result", vec![]);
        let p = Model::new("history", if call { vec![vec![json!({})]] } else { vec![] });
        turn(
            &backend(p.clone()),
            &host,
            ws.path(),
            agent(false, on),
            "t2",
            "look back",
        )
        .await;
        let reqs = p.requests();
        let offered = reqs[0].tools.iter().any(|t| t.name == "history");
        assert_eq!(offered, on);
        let fin = host.finished();
        let out = fin.last().unwrap();
        if on {
            assert_eq!(out["ok"], json!(true), "{out}");
            assert!(out["output"].as_str().unwrap().contains("turns 1-2 of 2"));
        } else {
            assert_eq!(out["ok"], json!(false), "{out}");
            assert!(
                out["output"].as_str().unwrap().contains("unknown tool"),
                "{out}"
            );
            assert!(!out["output"].as_str().unwrap().contains("secret"));
            assert_eq!(*host.reads.lock().unwrap(), 0, "the log was not read");
        }
    }
}

fn image_turn(host: &Host, bytes: &[u8]) -> ImageRef {
    let r = ImageRef {
        media_id: "m1".into(),
        sha256: sha(bytes),
        mime: "image/png".into(),
        name: "mock.png".into(),
        width: 20,
        height: 10,
    };
    host.store
        .lock()
        .unwrap()
        .insert(sha(bytes), bytes.to_vec());
    host.old_turn(1, "what is this?", "ok", vec![r.clone()]);
    r
}

/// A model that takes images is sent an earlier image of the person; the
/// log records a reference only; the request rebuilds from the log and the
/// kept bytes byte for byte.
#[tokio::test]
async fn a_vision_model_gets_an_earlier_image_by_reference() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    let bytes = png(20, 10, 3_000);
    image_turn(&host, &bytes);
    let p = Model::new("history", vec![vec![json!({"turn": 1, "image": 1})]]);
    turn(
        &backend(p.clone()),
        &host,
        ws.path(),
        agent(true, true),
        "t2",
        "show it again",
    )
    .await;
    let reqs = p.requests();
    let sent = reqs[1].messages.clone();
    assert_eq!(shown(&sent), vec![("c0_0".into(), Some(b64(&bytes)))]);
    let fin = host.finished();
    let out = fin.last().unwrap();
    assert_eq!(out["ok"], json!(true), "{out}");
    assert_eq!(out["image"]["sha256"], json!(sha(&bytes)));
    assert!(out["output"].as_str().unwrap().contains("mock.png"));
    assert_eq!(out["note"], json!(images::HISTORY_IMAGE_SENT), "{out}");
    let raw = out.to_string();
    assert!(!raw.contains(&b64(&bytes)[..100]), "no bytes in the event");
    // The live request and a rebuild from the log agree byte for byte.
    let log = host.events();
    let cut = log
        .iter()
        .rposition(|e| e.kind == "assistant_message")
        .unwrap();
    let live = reqs[1].messages.clone().split_off(1);
    assert!(
        rebuilt(&log[..cut], &host.loaded(), true) == live,
        "the live request and its rebuild disagree"
    );
}

/// A model that takes no images is told so and given no bytes.
#[tokio::test]
async fn a_model_without_vision_is_refused_the_image_with_the_reason() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    image_turn(&host, &png(20, 10, 100));
    let p = Model::new("history", vec![vec![json!({"turn": 1, "image": 1})]]);
    turn(
        &backend(p.clone()),
        &host,
        ws.path(),
        agent(false, true),
        "t2",
        "show it again",
    )
    .await;
    assert!(p.requests().iter().all(|r| shown(&r.messages).is_empty()));
    let out = host.finished().pop().unwrap();
    assert!(out.get("image").is_none());
    assert!(
        out["note"]
            .as_str()
            .unwrap()
            .contains("does not take images"),
        "{out}"
    );
}

/// Bytes the store no longer has are said so; an image number out of range
/// is an error with the count.
#[tokio::test]
async fn missing_bytes_and_a_wrong_number_are_said() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    let bytes = png(20, 10, 100);
    image_turn(&host, &bytes);
    host.store.lock().unwrap().clear();
    let p = Model::new(
        "history",
        vec![
            vec![json!({"turn": 1, "image": 1})],
            vec![json!({"turn": 1, "image": 5})],
        ],
    );
    turn(
        &backend(p.clone()),
        &host,
        ws.path(),
        agent(true, true),
        "t2",
        "show it again",
    )
    .await;
    let fin = host.finished();
    let gone = &fin[fin.len() - 2];
    assert_eq!(gone["ok"], json!(false));
    assert!(
        gone["output"].as_str().unwrap().contains("no longer has"),
        "{gone}"
    );
    let range = fin.last().unwrap();
    assert_eq!(range["ok"], json!(false));
    assert!(
        range["output"].as_str().unwrap().contains("1 image"),
        "{range}"
    );
}

fn system_of(req: &ChatRequest) -> String {
    match &req.messages[0] {
        ChatMsg::System(s) => s.clone(),
        other => panic!("{other:?}"),
    }
}

/// The summary's line tells the model that what it replaces can be read in
/// full with `history`, only while the tool is offered; the same bytes on
/// the next turn (the cache prefix does not move) and no sentence without.
#[tokio::test]
async fn the_summary_says_where_the_rest_is_only_when_the_tool_is_on() {
    let ws = tempfile::tempdir().unwrap();
    let mut systems = Vec::new();
    for on in [true, true, false] {
        let host = Host::new();
        host.old_turn(1, "first", "a result", vec![]);
        host.summarize("The first turn read a file.");
        let p = Model::new("history", vec![]);
        turn(
            &backend(p.clone()),
            &host,
            ws.path(),
            agent(false, on),
            "t2",
            "go on",
        )
        .await;
        systems.push(system_of(&p.requests()[0]));
    }
    let line = "Summary of the earlier conversation (the messages it replaces can be read \
                in full with the `history` tool):\nThe first turn read a file.";
    assert!(systems[0].contains(line), "{}", systems[0]);
    assert_eq!(systems[0], systems[1]);
    assert!(!systems[2].contains("history"), "{}", systems[2]);
    assert!(
        systems[2].contains("Summary of the earlier conversation:\nThe first turn read a file.")
    );
}

/// A tool the node does not have, or denies, is not promised.
#[tokio::test]
async fn the_summary_does_not_promise_a_tool_the_node_lacks() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    host.old_turn(1, "first", "a result", vec![]);
    host.summarize("The first turn read a file.");
    let p = Model::new("history", vec![]);
    let b = NativeBackend::new(
        p.clone(),
        vec![Arc::new(harness_tools::Read)],
        NativeConfig::default(),
    );
    turn(&b, &host, ws.path(), agent(false, true), "t2", "go on").await;
    assert!(!system_of(&p.requests()[0]).contains("history"));
}

/// A tool the node denies (`denied_tools`) is not promised either, though
/// it is registered and the agent has it.
#[tokio::test]
async fn the_summary_does_not_promise_a_tool_the_node_denies() {
    let ws = tempfile::tempdir().unwrap();
    let host = Host::new();
    host.denied.store(true, std::sync::atomic::Ordering::SeqCst);
    host.old_turn(1, "first", "a result", vec![]);
    host.summarize("The first turn read a file.");
    let p = Model::new("history", vec![]);
    turn(
        &backend(p.clone()),
        &host,
        ws.path(),
        agent(false, true),
        "t2",
        "go on",
    )
    .await;
    let system = system_of(&p.requests()[0]);
    assert!(!system.contains("history"), "{system}");
    assert!(system.contains("Summary of the earlier conversation:\nThe first turn read a file."));
}
