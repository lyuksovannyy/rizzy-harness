//! MCP servers on a node (ADOPTION_PLAN X.3), over the link as the master
//! drives a node, with the scripted agent: only node.toml's servers exist
//! (a repository's `.mcp.json` or `.codex/config.toml`, a prompt, an agent
//! spec naming another server start nothing); a call is asked about with
//! the MCP wording and returns the server's output; the server process is
//! gone after the chat is deleted and after the node's stop; its `env`
//! values never reach the log. Unconfined (no bubblewrap); the contained
//! path is `cli/tests/mcp_sandbox.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::EventKind;
use harness_core::policy::{ApprovalMode, Decision};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use harness_tools::mcp::ServerConfig;

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    rt: NodeRuntime,
    link: NodeLink,
}

fn started(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("started-{name}"))
}

fn pids(file: &Path) -> Vec<i32> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("started ")?.parse().ok())
        .collect()
}

fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        !s.rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z')
    })
}

async fn gone(pid: i32) -> bool {
    let by = std::time::Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        if std::time::Instant::now() > by {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    true
}

async fn node() -> Node {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let ws = root.join("ws");
    std::fs::create_dir(&ws).unwrap();
    // A repository that declares servers of its own, as other harnesses
    // read them: nothing here may start.
    let evil = format!(
        r#"{{"mcpServers": {{"evil": {{"command": "python3", "args": ["{FAKE}", "--started", "{}"]}}}}}}"#,
        started(&root, "evil").display()
    );
    std::fs::write(ws.join(".mcp.json"), &evil).unwrap();
    std::fs::create_dir(ws.join(".codex")).unwrap();
    std::fs::write(
        ws.join(".codex/config.toml"),
        format!(
            "[mcp_servers.evil]\ncommand = \"python3\"\nargs = [\"{FAKE}\", \"--started\", \"{}\"]\n",
            started(&root, "evil").display()
        ),
    )
    .unwrap();
    let mut fake = ServerConfig::new(
        "python3",
        &[FAKE, "--started", started(&root, "fake").to_str().unwrap()],
    );
    fake.env =
        serde_json::from_value(serde_json::json!({"FAKE_TOKEN": "tok-secret-4242"})).unwrap();
    let mut servers = BTreeMap::new();
    servers.insert("fake".to_string(), fake);
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.clone()],
            max_approval_mode: ApprovalMode::Auto,
            mcp_servers: servers,
            ..Default::default()
        },
        store: Store::open(&root.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = rt.attach().await;
    assert!(matches!(
        link.from_node.recv().await.unwrap(),
        NodeToMaster::Hello { .. }
    ));
    Node { dir, ws, rt, link }
}

impl Node {
    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    async fn ack(&mut self) -> Result<(), String> {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::Ack { ok, error, .. } = m {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    async fn create(&mut self, agent: Option<harness_core::agent::AgentSpec>) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Ask),
                profile: None,
                agent,
                incognito: false,
                no_callbacks: false,
                exec_mode: None,
            })
            .unwrap();
        self.ack().await.unwrap();
        sid
    }

    /// One prompt to its turn's end, allowing every approval; the log.
    async fn run(&mut self, sid: SessionId, text: &str) -> Vec<harness_core::RawEvent> {
        self.link
            .to_node
            .try_send(MasterToNode::Prompt {
                req_id: "p".into(),
                session_id: sid,
                text: text.into(),
                agent: None,
                attachments: vec![],
                compare: vec![],
                reply: None,
                savings: None,
                context: None,
                images: vec![],
                agent_named: false,
            })
            .unwrap();
        self.ack().await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(30);
        let mut answered = std::collections::HashSet::new();
        loop {
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            for e in &log {
                if let Ok(EventKind::ApprovalRequested(a)) = e.typed()
                    && answered.insert(a.approval_id.clone())
                {
                    self.link
                        .to_node
                        .try_send(MasterToNode::ApprovalRespond {
                            req_id: "a".into(),
                            session_id: sid,
                            approval_id: a.approval_id,
                            decision: Decision::Allow,
                        })
                        .unwrap();
                }
            }
            if log.last().is_some_and(|e| e.kind == "turn_ended") {
                return log;
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn outputs(log: &[harness_core::RawEvent]) -> Vec<(bool, String)> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => Some((f.ok, f.output)),
            _ => None,
        })
        .collect()
}

const SCRIPT: &str = r#"script: [{"tool": "mcp__fake__echo", "input": {"text": "hello from mcp"}}, {"tool": "mcp__evil__echo", "input": {"text": "x"}}, {"tool": "mcp__fake__env", "input": {}}]"#;

#[tokio::test]
async fn only_node_toml_servers_run_and_a_call_is_asked_about() {
    let mut n = node().await;
    // An agent spec over the link that names another server's tool, and
    // `mcp`: it chooses among node.toml's tools, it adds none.
    let agent: harness_core::agent::AgentSpec = serde_json::from_value(serde_json::json!({
        "agent_id": "x", "name": "X", "system_prompt": "", "prompt_hash": "h",
        "tools": ["mcp", "mcp__evil__echo"],
    }))
    .unwrap();
    let sid = n.create(Some(agent)).await;
    let log = n.run(sid, SCRIPT).await;
    let out = outputs(&log);
    assert_eq!(out.len(), 3, "{out:?}");
    assert_eq!(out[0], (true, "hello from mcp".to_string()));
    assert!(
        !out[1].0 && out[1].1.contains("unknown tool `mcp__evil__echo`"),
        "{out:?}"
    );
    // Its `env` value reached the server, and never the log.
    assert!(out[2].0, "{out:?}");
    assert!(!out[2].1.contains("tok-secret-4242"), "{}", out[2].1);
    let raw: String = log.iter().map(|e| e.payload.to_string()).collect();
    assert!(!raw.contains("tok-secret-4242"));
    // Asked about, as a program the harness did not write.
    let reasons: Vec<String> = log
        .iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ApprovalRequested(a)) => Some(format!("{}: {}", a.tool, a.reason)),
            _ => None,
        })
        .collect();
    assert!(
        reasons
            .iter()
            .any(|r| r.starts_with("mcp__fake__echo: ") && r.contains("MCP server")),
        "{reasons:?}"
    );
    assert!(
        !started(&n.root(), "evil").exists(),
        "a repository's server started"
    );
    assert_eq!(pids(&started(&n.root(), "fake")).len(), 1);
}

#[tokio::test]
async fn the_server_is_gone_after_the_chat_is_deleted_and_after_a_stop() {
    let mut n = node().await;
    let sid = n.create(None).await;
    n.run(sid, SCRIPT).await;
    let first = pids(&started(&n.root(), "fake"))[0];
    assert!(alive(first));
    n.link
        .to_node
        .try_send(MasterToNode::Delete {
            req_id: "d".into(),
            session_id: sid,
        })
        .unwrap();
    n.ack().await.unwrap();
    assert!(gone(first).await, "the deleted chat's server still runs");

    let sid = n.create(None).await;
    n.run(sid, SCRIPT).await;
    let second = pids(&started(&n.root(), "fake"))[1];
    assert!(alive(second));
    n.rt.stop(Duration::from_secs(20)).await;
    assert!(gone(second).await, "the server outlived the node's stop");
}
