//! An image file `read` returns to the model (ADOPTION_PLAN X.23, PLAN.md
//! T71): only to a model that takes images, by reference in the log with
//! the bytes kept by the host, within the caps per step and per request,
//! the same bytes on every later request whatever happens to the file,
//! under the same approval rules as any `read`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    Rebuild, images, place_notes, rebuild,
};
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::backend::{
    AgentBackend, ImageData, KeptImage, LogError, TurnCtx, TurnExtra, TurnHost,
};
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// A PNG header of `w` x `h` and `pad` bytes after it, so every image is
/// distinct and has a known size.
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

// ---- a host with a real log and an image store --------------------------

#[derive(Default)]
struct Host {
    sid: SessionId,
    log: Mutex<Vec<RawEvent>>,
    store: Mutex<HashMap<String, Vec<u8>>>,
    mode: Option<ApprovalMode>,
    unasked: bool,
    deny: bool,
    denied_tool: bool,
    asked: Mutex<usize>,
}

impl Host {
    fn new(history: Vec<RawEvent>) -> Self {
        let sid = history
            .first()
            .map_or_else(SessionId::new, |e| e.session_id);
        Self {
            sid,
            log: Mutex::new(history),
            unasked: true,
            ..Default::default()
        }
    }
    fn push(&self, k: EventKind) {
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
    }
    fn log(&self) -> Vec<RawEvent> {
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
        self.log()
            .into_iter()
            .filter(|e| e.kind == "tool_call_finished")
            .map(|e| e.payload)
            .collect()
    }
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), LogError> {
        self.push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        *self.asked.lock().unwrap() += 1;
        // The node's policy (`denied_tools`) answers before anyone is asked.
        if self.deny || self.denied_tool {
            Decision::Deny
        } else {
            Decision::Allow
        }
    }
    fn runs_unasked(&self, _: &str, risk: ToolRisk) -> bool {
        self.unasked && !self.denied_tool && risk == ToolRisk::ReadOnly
    }
    fn approval_mode(&self) -> Option<ApprovalMode> {
        self.mode
    }
    fn tool_denied(&self, tool: &str) -> bool {
        self.denied_tool && tool == "read"
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

// ---- a scripted model that records every request ------------------------

struct Model {
    steps: Vec<Vec<Value>>,
    seen: Mutex<Vec<Vec<ChatMsg>>>,
}

impl Model {
    fn new(steps: Vec<Vec<Value>>) -> Arc<Self> {
        Arc::new(Self {
            steps,
            seen: Mutex::default(),
        })
    }
    fn requests(&self) -> Vec<Vec<ChatMsg>> {
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
            s.push(req.messages.clone());
            s.len() - 1
        };
        Ok(match self.steps.get(n) {
            Some(calls) => Completion {
                tool_calls: calls
                    .iter()
                    .enumerate()
                    .map(|(k, input)| ToolCallRef {
                        call_id: format!("c{n}_{k}"),
                        tool: "read".into(),
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

fn agent(vision: bool) -> AgentSpec {
    AgentSpec {
        agent_id: "dev".into(),
        name: "Dev".into(),
        system_prompt: "Be brief.".into(),
        tools: ToolSelection::List(vec!["read".into()]),
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
        vec![Arc::new(harness_tools::Read)],
        NativeConfig::default(),
    )
}

fn open(host: &Host, turn: &str) {
    host.push(EventKind::TurnStarted(TurnStarted {
        continued_by: None,
        retry: None,
        cause: None,
        agent: None,
        turn_id: turn.into(),
        environment: None,
    }));
    host.push(EventKind::UserMessage(UserMessage {
        turn_id: turn.into(),
        text: "What is in the image?".into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: false,
        triggers: vec![],
        files: vec![],
    }));
}

async fn turn(b: &NativeBackend, host: &Host, ws: &std::path::Path, vision: bool, turn_id: &str) {
    let history = host.log();
    open(host, turn_id);
    b.run_turn(TurnCtx {
        session_id: host.sid,
        turn_id: turn_id.into(),
        history,
        user_text: "What is in the image?".into(),
        cwd: ws.to_path_buf(),
        model: "m".into(),
        agent: Some(agent(vision)),
        compare: vec![],
        parent_session: None,
        extra: TurnExtra::default(),
        cancel: Default::default(),
        host,
    })
    .await;
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

/// What a later request rebuilds from the log and the host's stored bytes,
/// as the loop builds one (less the system prompt).
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
    images::tool_images(&mut m, sees, loaded);
    place_notes(m).split_off(1)
}

/// A model that takes images is sent the bytes `read` found, after the
/// step's results; the log records a reference (hash, type, size), never
/// the bytes; the host keeps the bytes; the result's note is the harness's
/// fixed sentence that the file's text is data.
#[tokio::test]
async fn a_vision_model_sees_the_image_and_the_log_holds_only_a_reference() {
    let ws = tempfile::tempdir().unwrap();
    let bytes = png(640, 480, 20_000);
    std::fs::write(ws.path().join("shot.txt"), &bytes).unwrap();
    let p = Model::new(vec![vec![json!({"path": "shot.txt"})]]);
    let host = Host::new(vec![]);
    turn(&backend(p.clone()), &host, ws.path(), true, "t1").await;
    let reqs = p.requests();
    assert_eq!(reqs.len(), 2);
    let sent = reqs[1].clone();
    assert_eq!(shown(&sent), vec![("c0_0".into(), Some(b64(&bytes)))]);
    // After the step's result, never before it.
    let at = sent
        .iter()
        .position(|m| matches!(m, ChatMsg::ToolImage { .. }))
        .unwrap();
    assert!(matches!(&sent[at - 1], ChatMsg::Tool { content, .. }
        if content.contains("PNG image, 640x480 pixels")
            && content.contains(images::TOOL_IMAGE_SENT)));
    let fin = host.finished();
    let raw = fin[0].to_string();
    assert!(raw.len() < 2_000, "the event is small: {} bytes", raw.len());
    assert!(!raw.contains(&b64(&bytes)[..200]), "no bytes in the event");
    let sha = harness_tools::seen::hex(&harness_tools::seen::sha256(&bytes));
    assert_eq!(fin[0]["image"]["sha256"], json!(sha));
    assert_eq!(fin[0]["image"]["bytes"], json!(bytes.len()));
    assert_eq!(fin[0]["image"]["width"], json!(640));
    assert_eq!(host.store.lock().unwrap().get(&sha), Some(&bytes));
}

/// A model that does not take images gets the harness's line and no bytes;
/// nothing is kept and the log has no reference.
#[tokio::test]
async fn a_model_without_vision_gets_the_note_and_no_bytes() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
    let p = Model::new(vec![vec![json!({"path": "a.png"})]]);
    let host = Host::new(vec![]);
    turn(&backend(p.clone()), &host, ws.path(), false, "t1").await;
    let reqs = p.requests();
    assert!(reqs.iter().all(|r| shown(r).is_empty()));
    let fin = host.finished();
    assert!(fin[0].get("image").is_none());
    assert!(
        fin[0]["note"]
            .as_str()
            .unwrap()
            .contains("does not take images"),
        "{}",
        fin[0]
    );
    assert!(host.store.lock().unwrap().is_empty());
}

/// The request rebuilds byte for byte from the log and the kept bytes,
/// also after the file changed on disk, after it was deleted, and on a new
/// backend (a node restart): the bytes read are the bytes kept.
#[tokio::test]
async fn rebuild_is_byte_for_byte_after_the_file_changed_and_a_restart() {
    let ws = tempfile::tempdir().unwrap();
    let first = png(32, 32, 3_000);
    std::fs::write(ws.path().join("ui.png"), &first).unwrap();
    let p = Model::new(vec![vec![json!({"path": "ui.png"})]]);
    let host = Host::new(vec![]);
    turn(&backend(p.clone()), &host, ws.path(), true, "t1").await;
    let log = host.log();
    let cut = log
        .iter()
        .rposition(|e| e.kind == "assistant_message")
        .unwrap();
    // What the provider got (notes already placed), less the system prompt.
    let live = p.requests()[1].clone().split_off(1);
    // The file changes, then goes.
    std::fs::write(ws.path().join("ui.png"), png(64, 64, 10)).unwrap();
    assert_eq!(rebuilt(&log[..cut], &host.loaded(), true), live);
    std::fs::remove_file(ws.path().join("ui.png")).unwrap();
    assert_eq!(rebuilt(&log[..cut], &host.loaded(), true), live);
    // A restart: a new backend, the history from the log, the next turn's
    // first request shows the same bytes.
    let p2 = Model::new(vec![]);
    turn(&backend(p2.clone()), &host, ws.path(), true, "t2").await;
    let again = p2.requests();
    assert_eq!(shown(&again[0]), vec![("c0_0".into(), Some(b64(&first)))]);
}

/// One step of six image reads (a parallel batch): four are shown, the
/// fifth and sixth get the per-step line; the next step's read shows, and
/// the request keeps only the newest four.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_caps_per_step_and_per_request() {
    let ws = tempfile::tempdir().unwrap();
    for k in 0..7 {
        std::fs::write(ws.path().join(format!("{k}.png")), png(8, 8, 100 + k)).unwrap();
    }
    let p = Model::new(vec![
        (0..6)
            .map(|k| json!({"path": format!("{k}.png")}))
            .collect(),
        vec![json!({"path": "6.png"})],
    ]);
    let host = Host::new(vec![]);
    turn(&backend(p.clone()), &host, ws.path(), true, "t1").await;
    let log = host.log();
    assert!(
        log.windows(2)
            .any(|w| w.iter().all(|e| e.kind == "tool_call_started")),
        "the reads ran as a batch"
    );
    let reqs = p.requests();
    let ids = |r: &[ChatMsg]| shown(r).into_iter().map(|(c, _)| c).collect::<Vec<_>>();
    assert_eq!(ids(&reqs[1]), ["c0_0", "c0_1", "c0_2", "c0_3"]);
    assert_eq!(ids(&reqs[2]), ["c0_1", "c0_2", "c0_3", "c1_0"]);
    let fin = host.finished();
    for f in &fin[4..6] {
        assert!(f.get("image").is_none());
        assert!(f["note"].as_str().unwrap().contains("per step"), "{f}");
    }
    assert_eq!(host.store.lock().unwrap().len(), 5);
}

/// Plan mode allows reads: the image comes back. Ask-every-call asks for
/// the read, and a denied read returns nothing and keeps nothing; a node
/// that denies `read` refuses it.
#[tokio::test]
async fn plan_mode_ask_every_call_and_denied_tools() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.png"), png(10, 10, 100)).unwrap();
    let run = |mode: Option<ApprovalMode>, unasked: bool, deny: bool, denied_tool: bool| {
        let ws = ws.path().to_path_buf();
        async move {
            let p = Model::new(vec![vec![json!({"path": "a.png"})]]);
            let mut host = Host::new(vec![]);
            host.mode = mode;
            host.unasked = unasked;
            host.deny = deny;
            host.denied_tool = denied_tool;
            turn(&backend(p.clone()), &host, &ws, true, "t1").await;
            let asked = *host.asked.lock().unwrap();
            let kept = host.store.lock().unwrap().len();
            (shown(&p.requests()[1]).len(), asked, kept)
        }
    };
    let (n, _, kept) = run(Some(ApprovalMode::Plan), true, false, false).await;
    assert_eq!((n, kept), (1, 1));
    assert_eq!(
        run(Some(ApprovalMode::AskAll), false, false, false).await,
        (1, 1, 1)
    );
    assert_eq!(
        run(Some(ApprovalMode::AskAll), false, true, false).await,
        (0, 1, 0)
    );
    let (n, _, kept) = run(None, true, false, true).await;
    assert_eq!((n, kept), (0, 0));
}

/// A summary is text: the summariser's transcript says an image was read
/// and carries no bytes; an old image result cleared to a stub loses its
/// image with it.
#[test]
fn a_summary_is_told_and_clearing_drops_the_image() {
    use harness_agent_native::shorten;
    let sid = SessionId::new();
    let host = Host::new(vec![]);
    let _ = sid;
    open(&host, "t");
    let call = |i: usize| ToolCallRef {
        call_id: format!("c{i}"),
        tool: "read".into(),
        input: json!({"path": format!("{i}.png")}),
    };
    for i in 0..3 {
        host.push(EventKind::AssistantMessage(AssistantMessage {
            author: None,
            variant: None,
            turn_id: "t".into(),
            message_id: format!("m{i}"),
            text: String::new(),
            tool_calls: vec![call(i)],
            blocks: vec![],
            thinking: None,
        }));
        host.push(EventKind::ToolCallFinished(ToolCallFinished {
            call_id: format!("c{i}"),
            ok: true,
            output: format!("{i}.png: PNG image, 8x8 pixels, 100 bytes."),
            exit_code: None,
            truncated: false,
            sources: vec![],
            note: Some(images::TOOL_IMAGE_SENT.into()),
            seen: None,
            image: Some(ToolImage {
                sha256: format!("{i:064}"),
                mime: "image/png".into(),
                width: 8,
                height: 8,
                bytes: 100,
            }),
        }));
    }
    let log = host.log();
    let msgs = rebuild(&log, &Rebuild::default());
    assert_eq!(
        msgs.iter()
            .filter(|m| matches!(m, ChatMsg::ToolImage { .. }))
            .count(),
        3
    );
    let (msgs, spans) = harness_agent_native::rebuild_spans(&log, &Rebuild::default());
    let st = shorten::State {
        cleared: Some(Cleared {
            turn_id: "t".into(),
            results: 1,
        }),
        ..Default::default()
    };
    let out = shorten::apply(&msgs, &spans, &st, shorten::How::REQUEST);
    let ids: Vec<&str> = out
        .iter()
        .filter_map(|m| match m {
            ChatMsg::ToolImage { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["c1", "c2"], "the cleared result's image goes with it");
    let first = msgs
        .iter()
        .find(|m| matches!(m, ChatMsg::ToolImage { .. }))
        .unwrap();
    let t = images::tool_image_text(first).unwrap();
    assert!(t.contains("shown the image file \"0.png\""), "{t}");
}
