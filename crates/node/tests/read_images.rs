//! `read` of an image file through the node (ADOPTION_PLAN X.23, PLAN.md
//! T71): the bytes go into the chat's `session_images`, the log and what
//! the node publishes hold a reference only, and deleting the chat removes
//! them.

use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

fn png(pad: usize) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\x01\0\0\0\0\x80\x08\x06\0\0\0".to_vec();
    b.extend((0..pad).map(|i| (i * 7 % 256) as u8));
    b
}

struct Node {
    _dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    node: NodeRuntime,
    link: NodeLink,
    published: Vec<String>,
}

async fn start() -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (ws, data) = (base.join("ws"), base.join("data"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = NodePolicy {
        workspace_roots: vec![base.clone()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    policy.protect(&data);
    policy.skills.roots = vec![];
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        backend: std::sync::Arc::new(NativeBackend::new(
            std::sync::Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig {
                protected_dirs: std::sync::Arc::new(policy.protected_dirs.clone()),
                ..Default::default()
            },
        )),
        policy,
        store: Store::open(&data.join("node.db"), Durability::Full).unwrap(),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let _hello = link.from_node.recv().await.unwrap();
    Node {
        _dir: dir,
        ws,
        node,
        link,
        published: vec![],
    }
}

impl Node {
    async fn send(&mut self, m: MasterToNode) -> Result<(), String> {
        self.link.to_node.try_send(m).unwrap();
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
                .await
                .expect("no ack")
                .expect("link closed");
            self.published.push(serde_json::to_string(&msg).unwrap());
            if let NodeToMaster::Ack { ok, error, .. } = msg {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    fn log(&self, sid: SessionId) -> Vec<harness_core::RawEvent> {
        self.node.store().events_after(sid, 0, 10_000).unwrap()
    }

    /// Everything the node published until `sid`'s turn ended.
    async fn drain_turn(&mut self, sid: SessionId) {
        let by = std::time::Instant::now() + Duration::from_secs(20);
        while !self.log(sid).iter().any(|e| e.kind == "turn_ended") {
            assert!(std::time::Instant::now() < by, "the turn did not end");
            if let Ok(Some(m)) =
                tokio::time::timeout(Duration::from_millis(50), self.link.from_node.recv()).await
            {
                self.published.push(serde_json::to_string(&m).unwrap());
            }
        }
        while let Ok(m) = self.link.from_node.try_recv() {
            self.published.push(serde_json::to_string(&m).unwrap());
        }
    }
}

fn agent() -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        agent_id: "dev".into(),
        name: "dev".into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(vec!["read".into()]),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec!["mock".into()],
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

/// The image's bytes are kept with the chat, the event and every message
/// the node published name them by hash only, and they go with the chat.
#[tokio::test]
async fn the_chat_keeps_the_bytes_the_log_a_reference_and_delete_removes_them() {
    let mut n = start().await;
    let bytes = png(200_000);
    std::fs::write(n.ws.join("screen.png"), &bytes).unwrap();
    let sid = SessionId::new();
    let ws = n.ws.display().to_string();
    n.send(MasterToNode::Create {
        exec_mode: None,
        no_callbacks: false,
        profile: None,
        incognito: false,
        agent: None,
        req_id: "c".into(),
        session_id: sid,
        title: "t".into(),
        cwd: ws,
        model: None,
        approval_mode: Some(ApprovalMode::Auto),
    })
    .await
    .unwrap();
    n.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: Some(agent()),
        attachments: vec![],
        compare: vec![],
        req_id: "p".into(),
        session_id: sid,
        text: r#"script: [{"tool": "read", "input": {"path": "screen.png"}}]"#.into(),
        images: vec![],
        agent_named: false,
        files: vec![],
        owner: None,
        peers: vec![],
        tags: vec![],
    })
    .await
    .unwrap();
    n.drain_turn(sid).await;
    let fin = n
        .log(sid)
        .into_iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => Some(f),
            _ => None,
        })
        .unwrap();
    assert!(fin.ok, "{}", fin.output);
    let img = fin.image.expect("a reference");
    let sha = harness_tools::seen::hex(&harness_tools::seen::sha256(&bytes));
    assert_eq!(
        (img.sha256.as_str(), img.bytes),
        (sha.as_str(), bytes.len() as u64)
    );
    let kept = n
        .node
        .store()
        .session_images(&sid.to_string(), std::slice::from_ref(&sha))
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].bytes, bytes);
    assert_eq!(kept[0].name, "screen.png");
    // Neither the log nor anything the node sent the master holds bytes.
    use base64::Engine as _;
    let probe = base64::engine::general_purpose::STANDARD.encode(&bytes[..3_000]);
    let probe = &probe[100..400];
    assert!(
        n.published.iter().any(|m| m.contains(&sha)),
        "the reference went out"
    );
    assert!(n.published.iter().all(|m| !m.contains(probe)));
    assert!(n.published.iter().all(|m| m.len() < 64 * 1024));
    // Deleting the chat deletes them.
    n.node.delete(sid).await.unwrap();
    assert!(
        n.node
            .store()
            .session_images(&sid.to_string(), &[sha])
            .unwrap()
            .is_empty()
    );
}
