//! Publishing on a node: the capability, the `publish` tool through the
//! real agent loop, its per-turn cap and node.toml's switches, "Save as
//! artifact" (checked at once, copied later), and fetches that are answered
//! outside the command loop, never acked, and never hold up a prompt.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use harness_agent_native::provider::{ChatMsg, ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, SessionId};
use harness_node::artifacts::StageConfig;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_ARTIFACTS, MasterToNode, NodeToMaster};
use harness_store::Store;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

/// Publishes every file named in the user's message in one step (more
/// than a turn may), then answers.
struct ManyPublishes;

#[async_trait::async_trait]
impl Provider for ManyPublishes {
    fn name(&self) -> &'static str {
        "many"
    }

    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        if matches!(req.messages.last(), Some(ChatMsg::Tool { .. })) {
            return Ok(Completion {
                text: "done".into(),
                ..Default::default()
            });
        }
        let calls = (0..11)
            .map(|i| ToolCallRef {
                call_id: format!("c{i}"),
                tool: "publish".into(),
                input: json!({"path": "app", "name": format!("app{i}")}),
            })
            .collect();
        Ok(Completion {
            tool_calls: calls,
            ..Default::default()
        })
    }
}

struct Node {
    dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    link: NodeLink,
    caps: Vec<String>,
    _rt: NodeRuntime,
}

async fn node(provider: Arc<dyn Provider>, edit: impl FnOnce(&mut NodePolicy)) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let data = dir.path().join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = NodePolicy {
        workspace_roots: vec![dir.path().into()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    edit(&mut policy);
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy,
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            provider,
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: Some(StageConfig::new(data)),
    })
    .await
    .unwrap();
    let mut link = rt.attach().await;
    let caps = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("{other:?}"),
    };
    Node {
        ws: ws.canonicalize().unwrap(),
        dir,
        link,
        caps,
        _rt: rt,
    }
}

fn mock() -> Arc<dyn Provider> {
    Arc::new(Mock {
        delay: Duration::from_millis(1),
    })
}

impl Node {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(60), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    /// Messages until `pred` holds for one; returns them all.
    async fn until(&mut self, pred: impl Fn(&NodeToMaster) -> bool) -> Vec<NodeToMaster> {
        let mut out = vec![];
        loop {
            let m = self.next().await;
            let done = pred(&m);
            out.push(m);
            if done {
                return out;
            }
        }
    }

    async fn create(&mut self) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            req_id: format!("create-{sid}"),
            session_id: sid,
            title: "t".into(),
            cwd: self.ws.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
            profile: None,
            agent: None,
            incognito: false,
        });
        self.until(|m| is_ack(m, &format!("create-{sid}"))).await;
        sid
    }

    fn prompt(&self, req_id: &str, sid: SessionId, text: &str) {
        self.send(MasterToNode::Prompt {
            req_id: req_id.into(),
            session_id: sid,
            text: text.into(),
            agent: None,
            attachments: vec![],
            compare: vec![],
            savings: None,
            context: None,
            reply: None,
            images: vec![],
        });
    }
}

fn is_ack(m: &NodeToMaster, id: &str) -> bool {
    matches!(m, NodeToMaster::Ack { req_id, .. } if req_id == id)
}

fn events(ms: &[NodeToMaster]) -> Vec<EventKind> {
    ms.iter()
        .filter_map(|m| match m {
            NodeToMaster::Event { event } => event.typed().ok(),
            _ => None,
        })
        .collect()
}

fn turn_ended(m: &NodeToMaster) -> bool {
    matches!(m, NodeToMaster::Event { event: RawEvent { kind, .. } } if kind == "turn_ended")
}

#[tokio::test]
async fn the_capability_follows_node_toml() {
    let on = node(mock(), |_| {}).await;
    assert!(on.caps.iter().any(|c| c == CAP_ARTIFACTS));
    let mut off = node(mock(), |p| p.artifacts.enabled = false).await;
    assert!(!off.caps.iter().any(|c| c == CAP_ARTIFACTS));
    // Off, the tool says so and nothing is staged.
    std::fs::write(off.ws.join("app"), b"bin").unwrap();
    let sid = off.create().await;
    off.prompt("p", sid, "publish: app");
    let seen = off.until(turn_ended).await;
    let ev = events(&seen);
    assert!(ev.iter().any(|e| matches!(e,
        EventKind::ToolCallFinished(f) if !f.ok && f.output.contains("not available"))));
    assert!(!ev.iter().any(|e| matches!(e, EventKind::ArtifactStaged(_))));
}

#[tokio::test]
async fn publish_stages_through_the_agent_loop() {
    let mut n = node(mock(), |_| {}).await;
    std::fs::write(n.ws.join("app"), b"the binary").unwrap();
    let sid = n.create().await;
    n.prompt("p", sid, "publish: app as myapp");
    let seen = n.until(turn_ended).await;
    let ev = events(&seen);
    let staged = ev
        .iter()
        .find_map(|e| match e {
            EventKind::ArtifactStaged(a) => Some(a.clone()),
            _ => None,
        })
        .expect("staged");
    assert_eq!(staged.name, "myapp");
    assert_eq!(staged.size, 10);
    assert_eq!(staged.sha256, hex::encode(Sha256::digest(b"the binary")));
    assert_eq!(staged.by, harness_core::artifacts::StagedBy::Agent);
    let started = ev
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallStarted(t) if t.tool == "publish" => Some(t.call_id.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(staged.call_id.as_deref(), Some(started.as_str()));
    let out = ev
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f.output.clone()),
            _ => None,
        })
        .unwrap();
    assert!(out.starts_with("Published `myapp` (10 B, sha256 "), "{out}");
    // A secret-like name is refused to the agent.
    std::fs::write(n.ws.join(".env"), b"KEY=1").unwrap();
    n.prompt("p2", sid, "publish: .env");
    let ev = events(&n.until(turn_ended).await);
    assert!(ev.iter().any(|e| matches!(e,
        EventKind::ToolCallFinished(f) if !f.ok && f.output.contains("secret"))));
}

#[tokio::test]
async fn at_most_ten_publishes_per_turn() {
    let mut n = node(Arc::new(ManyPublishes), |_| {}).await;
    std::fs::write(n.ws.join("app"), b"x").unwrap();
    let sid = n.create().await;
    n.prompt("p", sid, "go");
    let ev = events(&n.until(turn_ended).await);
    let staged = ev
        .iter()
        .filter(|e| matches!(e, EventKind::ArtifactStaged(_)))
        .count();
    assert_eq!(staged, 10);
    assert!(ev.iter().any(|e| matches!(e,
        EventKind::ToolCallFinished(f) if f.call_id == "c10" && f.output.contains("at most 10"))));
}

#[tokio::test]
async fn denied_tools_blocks_publish() {
    let mut n = node(mock(), |p| p.denied_tools = vec!["publish".into()]).await;
    std::fs::write(n.ws.join("app"), b"x").unwrap();
    let sid = n.create().await;
    n.prompt("p", sid, "publish: app");
    let ev = events(&n.until(turn_ended).await);
    assert!(ev.iter().any(|e| matches!(e,
        EventKind::ApprovalRequested(a) if a.reason.contains("denied by this node's policy"))));
    assert!(!ev.iter().any(|e| matches!(e, EventKind::ArtifactStaged(_))));
}

#[tokio::test]
async fn save_as_artifact_checks_then_copies_and_fetches_are_never_acked() {
    let mut n = node(mock(), |_| {}).await;
    std::fs::write(n.ws.join("notes.txt"), b"hello notes").unwrap();
    let sid = n.create().await;
    // A refusal comes back on the ack.
    n.send(MasterToNode::ArtifactStage {
        req_id: "bad".into(),
        session_id: sid,
        path: "../outside".into(),
        name: None,
    });
    let seen = n.until(|m| is_ack(m, "bad")).await;
    assert!(matches!(
        seen.last(),
        Some(NodeToMaster::Ack { ok: false, .. })
    ));
    n.send(MasterToNode::ArtifactStage {
        req_id: "s".into(),
        session_id: sid,
        path: "notes.txt".into(),
        name: Some("Notes".into()),
    });
    let seen = n
        .until(|m| matches!(m, NodeToMaster::Event { event } if event.kind == "artifact_staged"))
        .await;
    assert!(
        seen.iter()
            .any(|m| matches!(m, NodeToMaster::Ack { req_id, ok: true, .. } if req_id == "s"))
    );
    let Some(EventKind::ArtifactStaged(a)) = events(&seen).into_iter().last() else {
        panic!()
    };
    assert_eq!(
        (a.name.as_str(), a.by),
        ("Notes", harness_core::artifacts::StagedBy::User)
    );
    // A fetch is answered by its chunk alone.
    n.send(MasterToNode::ArtifactFetch {
        req_id: "f".into(),
        session_id: sid,
        stage_id: a.stage_id.clone(),
        offset: 0,
        len: 1024,
    });
    let mut seen = n
        .until(|m| matches!(m, NodeToMaster::ArtifactChunk { req_id, .. } if req_id == "f"))
        .await;
    n.send(MasterToNode::ArtifactRelease {
        req_id: "rel".into(),
        session_id: sid,
        stage_id: a.stage_id.clone(),
    });
    seen.extend(n.until(|m| is_ack(m, "rel")).await);
    assert!(!seen.iter().any(|m| is_ack(m, "f")), "fetches get no ack");
    let chunk = seen
        .iter()
        .find_map(|m| match m {
            NodeToMaster::ArtifactChunk {
                req_id, data, eof, ..
            } if req_id == "f" => Some((data.clone(), *eof)),
            _ => None,
        })
        .expect("the chunk");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(chunk.0)
            .unwrap(),
        b"hello notes"
    );
    assert!(chunk.1);
    // Released: the stage is gone, the source is not.
    n.send(MasterToNode::ArtifactFetch {
        req_id: "f2".into(),
        session_id: sid,
        stage_id: a.stage_id,
        offset: 0,
        len: 10,
    });
    let seen = n
        .until(|m| matches!(m, NodeToMaster::ArtifactChunk { .. }))
        .await;
    assert!(
        matches!(seen.last(), Some(NodeToMaster::ArtifactChunk { error: Some(e), .. }) if e == "unknown stage")
    );
    assert!(n.ws.join("notes.txt").exists());
    // A file that vanished between the check and the copy is reported.
    n.send(MasterToNode::ArtifactStage {
        req_id: "gone".into(),
        session_id: sid,
        path: "missing.txt".into(),
        name: None,
    });
    let seen = n.until(|m| is_ack(m, "gone")).await;
    assert!(
        matches!(seen.last(), Some(NodeToMaster::Ack { ok: false, error: Some(e), .. }) if e.contains("no such file"))
    );
}

/// Copying a 200 MB file runs beside the command loop: a prompt sent
/// right after is acked before the copy ends.
#[tokio::test]
async fn a_large_stage_does_not_delay_a_prompt_ack() {
    let mut n = node(mock(), |_| {}).await;
    let big = std::fs::File::create(n.ws.join("big.bin")).unwrap();
    big.set_len(200 * 1024 * 1024).unwrap();
    let sid = n.create().await;
    let other = n.create().await;
    n.send(MasterToNode::ArtifactStage {
        req_id: "s".into(),
        session_id: sid,
        path: "big.bin".into(),
        name: None,
    });
    n.until(|m| is_ack(m, "s")).await;
    n.prompt("p", other, "hello");
    let seen = n
        .until(|m| {
            is_ack(m, "p")
                || matches!(m, NodeToMaster::Event { event } if event.kind == "artifact_staged")
        })
        .await;
    assert!(
        is_ack(seen.last().unwrap(), "p"),
        "the prompt was acked first"
    );
    let seen = n
        .until(|m| matches!(m, NodeToMaster::Event { event } if event.kind == "artifact_staged"))
        .await;
    let Some(EventKind::ArtifactStaged(a)) = events(&seen).into_iter().last() else {
        panic!()
    };
    assert_eq!(a.size, 200 * 1024 * 1024);
    drop(n.dir);
}
