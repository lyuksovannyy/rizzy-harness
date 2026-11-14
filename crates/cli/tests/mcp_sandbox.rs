//! MCP servers in a chat's own confinement (ADOPTION_PLAN X.3), on a node
//! started as the binary starts one (`sandbox::start_with_exe` with the
//! harness binary, offering `sandbox` and `host`): a `sandbox` chat's
//! server runs inside bubblewrap, under its guard, reading its requests
//! from the FIFO the guard hands it, and sees only the allow-listed
//! environment; a `host` chat's runs beside the node; both are gone after
//! the chat is deleted and after the node's stop.
//!
//! Needs bubblewrap and python3; with `HARNESS_REQUIRE_BWRAP=1` a missing
//! bubblewrap fails. Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::{ApprovalMode, ExecMode};
use harness_protocol::{MasterToNode, NodeToMaster};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
    marker: String,
}

async fn node() -> Option<Node> {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    let dir = tempfile::Builder::new()
        .prefix("mcp-sandbox-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, ws) = (root.join("data"), root.join("home/proj"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    // The server's script inside the chat's folder: the file view holds
    // the folder, not this repository.
    std::fs::copy(FAKE, ws.join("fake_mcp.py")).unwrap();
    let s = harness_tools::sandbox::start_with_exe(
        PathBuf::from(EXE),
        &harness_tools::sandbox::Settings::default(),
        &data,
        std::slice::from_ref(&data),
        &data.join("node.toml"),
    )
    .unwrap();
    if let Some(why) = s.sandbox_refusal() {
        assert!(!bwrap::required(), "the sandbox mode is not offered: {why}");
        eprintln!("SKIPPED: the sandbox mode is not offered here: {why}");
        return None;
    }
    let marker = format!("mcp-marker-{}", harness_core::new_id());
    let mut servers = BTreeMap::new();
    servers.insert(
        "fake".to_string(),
        harness_tools::mcp::ServerConfig::new(
            "python3",
            &[
                ws.join("fake_mcp.py").to_str().unwrap(),
                "--marker",
                &marker,
            ],
        ),
    );
    let policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: ApprovalMode::Auto,
        exec: harness_node::exec::ExecOffers::new(Ok(()), Ok(()), vec![]),
        sandbox_tmp: s.tmp_root.clone(),
        mcp_servers: servers,
        ..Default::default()
    };
    let backend = NativeBackend::new(
        Arc::new(Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            sandbox: s.confinement,
            host_shell: s.uncontained,
            protected_dirs: Arc::new(vec![data.clone()]),
            ..Default::default()
        },
    );
    let rt = harness_node::NodeRuntime::start(harness_node::NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: harness_store::Store::open(&data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        backend: Arc::new(backend),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
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
    Some(Node {
        _dir: dir,
        ws,
        rt,
        link,
        marker,
    })
}

impl Node {
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

    async fn create(&mut self, mode: ExecMode) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: Some(mode),
            })
            .unwrap();
        self.ack().await.unwrap();
        sid
    }

    async fn run(&mut self, sid: SessionId, text: &str) -> Vec<(bool, String)> {
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
        let by = std::time::Instant::now() + Duration::from_secs(60);
        let mut answered = std::collections::HashSet::new();
        loop {
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            // A `host` chat always asks: allow each card.
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
                            decision: harness_core::policy::Decision::Allow,
                        })
                        .unwrap();
                }
            }
            if log.last().is_some_and(|e| e.kind == "turn_ended") {
                return log
                    .iter()
                    .filter_map(|e| match e.typed() {
                        Ok(EventKind::ToolCallFinished(f)) => Some((f.ok, f.output)),
                        Ok(EventKind::Error(m)) => Some((false, format!("error: {}", m.message))),
                        _ => None,
                    })
                    .collect();
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Processes whose command line holds this node's marker (the guard,
    /// bubblewrap and the server itself, for a contained one).
    fn marked(&self) -> Vec<i32> {
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .filter(|p| {
                std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| {
                    c.windows(self.marker.len())
                        .any(|w| w == self.marker.as_bytes())
                })
            })
            .collect()
    }

    async fn none_marked(&self) -> bool {
        let by = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if self.marked().is_empty() {
                return true;
            }
            if std::time::Instant::now() > by {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn delete(&mut self, sid: SessionId) {
        self.link
            .to_node
            .try_send(MasterToNode::Delete {
                req_id: "d".into(),
                session_id: sid,
            })
            .unwrap();
        self.ack().await.unwrap();
    }
}

const SCRIPT: &str = r#"script: [{"tool": "mcp__fake__echo", "input": {"text": "through the fifo"}}, {"tool": "mcp__fake__pid", "input": {}}, {"tool": "mcp__fake__env", "input": {}}]"#;

fn own_pid_ns() -> String {
    std::fs::read_link("/proc/self/ns/pid")
        .unwrap()
        .display()
        .to_string()
}

#[tokio::test]
async fn a_sandbox_chats_server_runs_contained_and_ends_with_the_chat() {
    let Some(mut n) = node().await else { return };
    assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
    let sid = n.create(ExecMode::Sandbox).await;
    let out = n.run(sid, SCRIPT).await;
    assert_eq!(out.len(), 3, "{out:?}");
    assert_eq!(out[0], (true, "through the fifo".to_string()), "{out:?}");
    let (ok, pid) = &out[1];
    assert!(ok, "{pid}");
    assert!(!pid.contains(&own_pid_ns()), "not contained: {pid}");
    let (ok, env) = &out[2];
    assert!(ok, "{env}");
    assert!(env.lines().any(|l| l.starts_with("PATH=")), "{env}");
    assert!(!env.contains("CARGO_MANIFEST_DIR"), "{env}");
    assert!(!env.contains("HARNESS_"), "{env}");
    assert!(!n.marked().is_empty(), "no server process found");
    n.delete(sid).await;
    assert!(n.none_marked().await, "left: {:?}", n.marked());
}

#[tokio::test]
async fn host_and_sandbox_chats_servers_end_with_the_nodes_stop() {
    let Some(mut n) = node().await else { return };
    let host = n.create(ExecMode::Host).await;
    let out = n.run(host, SCRIPT).await;
    assert_eq!(out[0], (true, "through the fifo".to_string()), "{out:?}");
    assert!(out[1].1.contains(&own_pid_ns()), "{out:?}");
    let sandbox = n.create(ExecMode::Sandbox).await;
    let out = n.run(sandbox, SCRIPT).await;
    assert!(out[0].0, "{out:?}");
    assert!(!n.marked().is_empty());
    let left = n.rt.stop(Duration::from_secs(30)).await;
    assert!(left.is_empty(), "{left:?}");
    assert!(n.none_marked().await, "left: {:?}", n.marked());
}
