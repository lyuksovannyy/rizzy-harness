//! node.toml's MCP `env` values are secrets of the node (X.3), whatever
//! they are: values that are words the harness writes itself (a field name,
//! an event kind, `function`, `workspace`) must not break any chat's log,
//! and a real secret must be gone from everything stored and sent (PLAN.md
//! T3). Found by the review of the project-MCP change, whose proof
//! (`review_scrubber_breaks_other_users_events`) stored a `session_created`
//! with a `"[REDACTED]"` key, after which the node did not start again.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::provider::{
    ChatMsg, ChatRequest, Completion, Provider, ProviderError, StopReason,
};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use harness_tools::mcp::ServerConfig;

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

/// A random-looking 40-character key.
const CANARY: &str = "k9Qz7WvR2pLx8TnB4cYh6JmD1sFa3GuE5oKiNwXe";

/// What an admin wrote as `[mcp_servers.x] env` values: schema words (a
/// field name, an event kind, an enum-ish word, a key of `session_created`)
/// and one real key.
const WORDS: &[&str] = &[
    "approval_mode",
    "message_id",
    "function",
    "container",
    "workspace",
    "assistant_message",
    "tool_calls",
    "tool_call_finished",
    "session_created",
    // Not in any text sent here, but in the text once a secret before it
    // is redacted: a second pass would change the message again.
    "] tail-word",
];

/// Every person's text the model was sent, in order.
static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The model: a first step that streams text (the key split across
/// flushes), thinks, and calls the MCP server's `echo` with the key as a
/// value and as a key; then an answer. A prompt holding `FAIL` gets a
/// provider error that quotes the key.
struct Scripted;

#[async_trait::async_trait]
impl Provider for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let since = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .unwrap_or(0);
        let user = req.messages[since].user_text().unwrap_or_default();
        SEEN.lock().unwrap().push(user.to_string());
        let tools = req.messages[since..]
            .iter()
            .filter(|m| matches!(m, ChatMsg::Tool { .. }))
            .count();
        if user.contains("FAIL") {
            return Err(ProviderError::Status {
                status: 400,
                body: format!("invalid key {CANARY} for approval_mode"),
                hint: Default::default(),
            });
        }
        if tools > 0 {
            let text = "done: assistant_message tool_calls in the workspace".to_string();
            let _ = deltas.send(text.clone());
            return Ok(Completion {
                text,
                tool_calls: vec![],
                usage: None,
                stop: StopReason::End,
                thinking: None,
            });
        }
        let pieces = [
            "streamed function ",
            &CANARY[..17],
            &CANARY[17..],
            " approval_mode",
        ];
        for p in pieces {
            let _ = deltas.send(p.to_string());
            // Past the loop's flush interval: each piece its own batch.
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        let mut input = serde_json::Map::new();
        input.insert(CANARY.into(), serde_json::json!(CANARY));
        input.insert("message_id".into(), serde_json::json!("approval_mode"));
        input.insert(
            "text".into(),
            serde_json::json!(format!("echo {CANARY} in a container")),
        );
        Ok(Completion {
            text: pieces.concat(),
            tool_calls: vec![ToolCallRef {
                call_id: harness_core::new_id(),
                tool: "mcp__x__echo".into(),
                input: serde_json::Value::Object(input),
            }],
            usage: None,
            stop: StopReason::ToolUse,
            thinking: Some(Thinking {
                model: req.model.clone(),
                blocks: vec![ThinkingBlock::Text {
                    text: format!("thinking about {CANARY} and the workspace"),
                }],
                prefix: String::new(),
                unsent: false,
            }),
        })
    }
}

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    rt: NodeRuntime,
    link: NodeLink,
    /// Every event the link carried (what the master mirrors and sends on).
    frames: Vec<RawEvent>,
}

async fn start_at(dir: tempfile::TempDir) -> Node {
    let root = dir.path().canonicalize().unwrap();
    let ws = root.join("ws");
    let _ = std::fs::create_dir(&ws);
    let mut x = ServerConfig::new(
        "python3",
        &[FAKE, "--started", root.join("started").to_str().unwrap()],
    );
    let mut env: BTreeMap<String, String> = WORDS
        .iter()
        .enumerate()
        .map(|(i, w)| (format!("V{i}"), w.to_string()))
        .collect();
    env.insert("KEY".into(), CANARY.into());
    x.env = serde_json::from_value(serde_json::to_value(env).unwrap()).unwrap();
    // One set for the node and its agents, as `rizzy-harness` builds them:
    // the node adds the `env` values, the stream scrubber sees them.
    let secrets = harness_core::SecretSet::default();
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.clone()],
            max_approval_mode: ApprovalMode::Auto,
            mcp_servers: BTreeMap::from([("x".to_string(), x)]),
            skills: harness_node::SkillsPolicy {
                roots: vec![],
                ..Default::default()
            },
            ..Default::default()
        },
        store: Store::open(&root.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Scripted),
            harness_tools::builtin(),
            NativeConfig {
                secrets: secrets.clone(),
                ..Default::default()
            },
        )),
        default_model: "mock".into(),
        secrets,
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .expect("the node starts");
    let mut link = rt.attach().await;
    assert!(matches!(
        link.from_node.recv().await.unwrap(),
        NodeToMaster::Hello { .. }
    ));
    Node {
        dir,
        ws,
        rt,
        link,
        frames: vec![],
    }
}

impl Node {
    async fn recv(&mut self) -> NodeToMaster {
        let m = tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        if let NodeToMaster::Event { event } = &m {
            self.frames.push(event.clone());
        }
        m
    }

    async fn create(&mut self) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: format!("c-{sid}"),
                session_id: sid,
                title: "a chat about the workspace".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: None,
            })
            .unwrap();
        loop {
            if let NodeToMaster::Event { event } = self.recv().await
                && event.session_id == sid
                && event.kind == "session_created"
            {
                return sid;
            }
        }
    }

    /// One prompt of `owner`'s, to its turn's end.
    async fn run(&mut self, sid: SessionId, owner: &str, text: &str) {
        self.link
            .to_node
            .try_send(MasterToNode::Prompt {
                savings: None,
                context: None,
                reply: None,
                agent: None,
                attachments: vec![],
                compare: vec![],
                req_id: harness_core::new_id(),
                session_id: sid,
                text: text.into(),
                images: vec![],
                agent_named: false,
                files: vec![],
                owner: Some(owner.into()),
                identity: None,
                peers: vec![],
                tags: vec![],
            })
            .unwrap();
        loop {
            if let NodeToMaster::Event { event } = self.recv().await
                && event.session_id == sid
                && event.kind == "turn_ended"
            {
                return;
            }
        }
    }

    fn log(&self, sid: SessionId) -> Vec<RawEvent> {
        self.rt.store().events_after(sid, 0, 100_000).unwrap()
    }

    async fn restart(self) -> Node {
        self.rt.stop(Duration::from_secs(20)).await;
        let Node { dir, rt, link, .. } = self;
        drop(link);
        drop(rt);
        start_at(dir).await
    }
}

fn kinds(log: &[RawEvent]) -> Vec<&str> {
    log.iter().map(|e| e.kind.as_str()).collect()
}

/// Every event decodes, as its kind, and is what the typed event writes.
fn decodes(log: &[RawEvent]) {
    for e in log {
        let k = e
            .typed()
            .unwrap_or_else(|err| panic!("{} does not decode: {err}: {}", e.kind, e.payload));
        let (kind, payload) = k.into_parts();
        assert_eq!((kind.as_str(), &payload), (e.kind.as_str(), &e.payload));
    }
}

fn one<'a>(log: &'a [RawEvent], kind: &str) -> Vec<&'a RawEvent> {
    log.iter().filter(|e| e.kind == kind).collect()
}

#[tokio::test]
async fn schema_words_in_node_env_leave_every_log_whole() {
    let mut n = start_at(tempfile::tempdir().unwrap()).await;
    let ws = n.ws.display().to_string();
    let alice = n.create().await;
    let bob = n.create().await;
    n.run(
        alice,
        "01ALICE",
        "hello function world, approval_mode please",
    )
    .await;
    n.run(bob, "01BOB", "FAIL now in the workspace").await;

    let mut stored = vec![];
    for sid in [alice, bob] {
        let log = n.log(sid);
        decodes(&log);
        let view = SessionView::fold(&log).expect("the log folds");
        let meta = serde_json::to_value(&view).unwrap()["meta"].clone();
        assert_eq!(meta["approval_mode"], "auto", "{meta}");
        assert_eq!(meta["cwd"], ws.as_str());
        assert_eq!(meta["model"], "mock");
        assert_eq!(meta["title"], "a chat about the [REDACTED]");
        // What the link carried is what was stored, event for event.
        let sent: Vec<&RawEvent> = n.frames.iter().filter(|e| e.session_id == sid).collect();
        assert_eq!(sent, log.iter().collect::<Vec<_>>());
        stored.extend(log);
    }
    let all = serde_json::to_string(&stored).unwrap() + &serde_json::to_string(&n.frames).unwrap();
    assert!(!all.contains(CANARY), "the key reached the log");

    let a = n.log(alice);
    // The person's text, the streamed and final text, the thinking.
    let um = &one(&a, "user_message")[0].payload;
    assert_eq!(um["text"], "hello [REDACTED] world, [REDACTED] please");
    let streamed: String = one(&a, "assistant_delta")
        .iter()
        .map(|e| e.payload["text"].as_str().unwrap().to_string())
        .collect();
    assert!(!streamed.contains(CANARY), "{streamed}");
    assert!(
        streamed.starts_with("streamed [REDACTED] [REDACTED]"),
        "{streamed}"
    );
    let am = &one(&a, "assistant_message")[0].payload;
    assert_eq!(am["text"], "streamed [REDACTED] [REDACTED] [REDACTED]");
    assert!(am["thinking"].to_string().contains("[REDACTED]"), "{am}");
    // The tool's input, the key as a key and as a value; a harness word
    // as a key of the model's JSON is the model's, so it is scrubbed.
    let call = &one(&a, "tool_call_started")[0].payload;
    assert_eq!(call["input"]["[REDACTED]"], "[REDACTED]", "{call}");
    assert!(call["input"].get("message_id").is_none(), "{call}");
    // The MCP server's answer.
    let done = &one(&a, "tool_call_finished")[0].payload;
    assert!(done["ok"].as_bool().unwrap(), "{done}");
    assert!(
        done["output"]
            .as_str()
            .unwrap()
            .contains("echo [REDACTED] in a [REDACTED]"),
        "{done}"
    );
    // The provider's error in Bob's chat.
    let b = n.log(bob);
    assert!(
        one(&b, "error").iter().any(|e| e.payload["message"]
            .as_str()
            .unwrap()
            .contains("[REDACTED]")),
        "{:?}",
        kinds(&b)
    );

    // The node starts again on these logs and both chats go on.
    let mut n = n.restart().await;
    n.run(alice, "01ALICE", "again").await;
    n.run(bob, "01BOB", "again").await;
    for sid in [alice, bob] {
        let log = n.log(sid);
        decodes(&log);
        assert_eq!(one(&log, "turn_ended").len(), 2, "{:?}", kinds(&log));
        SessionView::fold(&log).unwrap();
    }
    n.rt.stop(Duration::from_secs(20)).await;
}

/// The person's message is scrubbed once, and the log stores the text the
/// model was sent (review R3): a secret that appears only once another is
/// redacted (`] tail-word` after `[REDACTED`) is not redacted a second time
/// in the log.
#[tokio::test]
async fn the_message_is_stored_as_the_model_was_sent_it() {
    let mut n = start_at(tempfile::tempdir().unwrap()).await;
    let sid = n.create().await;
    let text = format!("say {CANARY} tail-word now");
    n.run(sid, "01ALICE", &text).await;
    let log = n.log(sid);
    let stored = one(&log, "user_message")[0].payload["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(stored, "say [REDACTED] tail-word now");
    let seen = SEEN.lock().unwrap().clone();
    assert!(
        seen.iter().any(|t| t.contains(&stored)),
        "the model was sent another text: {seen:?}"
    );
    n.rt.stop(Duration::from_secs(20)).await;
}

/// The shipped build orders JSON keys as written (serde_json's
/// `preserve_order`, through genai): a scrub keeps every key where it was,
/// a renamed one too.
#[test]
fn key_order_survives_a_scrub() {
    let probe: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(r#"{"b": 1, "a": 2}"#).unwrap();
    assert_eq!(
        probe.keys().next().map(String::as_str),
        Some("b"),
        "this build sorts keys: the test would prove nothing"
    );
    let set = harness_core::SecretSet::new(vec![harness_core::Secret::new(CANARY.into())]);
    let input: serde_json::Value = serde_json::from_str(&format!(
        r#"{{"zeta": 1, "{CANARY}": "v {CANARY}", "mid": {{"y": 1, "{CANARY}": 2, "a": 3}}, "alpha": 4}}"#
    ))
    .unwrap();
    let (kind, mut p) = EventKind::ToolCallStarted(ToolCallStarted {
        turn_id: "t".into(),
        call_id: "c".into(),
        tool: format!("x {CANARY}"),
        input,
        author: None,
    })
    .into_parts();
    let before: Vec<String> = p.as_object().unwrap().keys().cloned().collect();
    set.scrub_event(&kind, &mut p);
    let after: Vec<String> = p.as_object().unwrap().keys().cloned().collect();
    assert_eq!(before, after);
    let keys =
        |v: &serde_json::Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(keys(&p["input"]), ["zeta", "[REDACTED]", "mid", "alpha"]);
    assert_eq!(keys(&p["input"]["mid"]), ["y", "[REDACTED]", "a"]);
}
