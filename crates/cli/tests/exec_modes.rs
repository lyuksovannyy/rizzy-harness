//! Execution modes on one node that offers both (N.10a;
//! docs/execution-modes.md): the confinements come from the node's own
//! start (`sandbox::start_with_exe`, as `rizzy-harness` runs it, with the
//! harness binary as the wrapper), the node runtime picks one per session
//! from the mode its log recorded. A `sandbox` session's command runs in
//! bubblewrap (its own PID namespace); a `host` session's, beside it in the
//! same node, does not; its background command is refused.
//!
//! Needs bubblewrap; with `HARNESS_REQUIRE_BWRAP=1` a missing one fails.
//! Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::ExecMode;
use harness_protocol::{MasterToNode, NodeToMaster};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
}

/// A node started as the binary starts one, offering `sandbox` and `host`;
/// `None` where bubblewrap cannot contain here.
async fn node() -> Option<Node> {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    let dir = tempfile::Builder::new()
        .prefix("exec-modes-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, ws) = (root.join("data"), root.join("home/proj"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
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
    let policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        exec: harness_node::exec::ExecOffers::new(Ok(()), Ok(()), vec![]),
        sandbox_tmp: s.tmp_root.clone(),
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
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert!(capabilities.contains(&"exec:sandbox".to_string()));
            assert!(capabilities.contains(&"exec:host".to_string()));
        }
        other => panic!("{other:?}"),
    }
    Some(Node {
        _dir: dir,
        ws,
        rt,
        link,
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
                approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
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

    /// Run one prompt to its turn's end; the outputs of its tool calls.
    async fn run(&mut self, sid: SessionId, text: &str) -> Vec<String> {
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
            })
            .unwrap();
        self.ack().await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            let ended = log.iter().filter(|e| e.kind == "turn_ended").count();
            if ended > 0 && log.last().is_some_and(|e| e.kind == "turn_ended") {
                return log
                    .iter()
                    .filter_map(|e| match e.typed() {
                        Ok(EventKind::ToolCallFinished(f)) => Some(f.output),
                        _ => None,
                    })
                    .collect();
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

fn own_pid_ns() -> String {
    std::fs::read_link("/proc/self/ns/pid")
        .unwrap()
        .display()
        .to_string()
}

/// One node, both modes, side by side: the `sandbox` session's command is
/// in a PID namespace of its own (`$$` is 2, its namespace is not the
/// node's); the `host` session's is in the node's, as the node's user, and
/// then the `sandbox` one is still contained (the choice is per session,
/// never per node).
#[tokio::test]
async fn a_host_sessions_command_runs_outside_bubblewrap_and_a_sandbox_sessions_inside() {
    let Some(mut n) = node().await else { return };
    let ours = own_pid_ns();
    let cmd = "run: echo pid=$$ ns=$(readlink /proc/self/ns/pid) uid=$(id -u)";
    let sandbox = n.create(ExecMode::Sandbox).await;
    let host = n.create(ExecMode::Host).await;
    for round in 0..2 {
        let out = n.run(sandbox, cmd).await.join("\n");
        assert!(
            out.contains("pid=2 "),
            "round {round}: not contained: {out}"
        );
        assert!(
            !out.contains(&ours),
            "round {round}: the node's namespace: {out}"
        );
        let out = n.run(host, cmd).await.join("\n");
        assert!(!out.contains("pid=2 "), "round {round}: {out}");
        assert!(
            out.contains(&format!("ns={ours}")),
            "round {round}: contained: {out}"
        );
        let uid = nix::unistd::getuid();
        assert!(out.contains(&format!("uid={uid}")), "round {round}: {out}");
    }
}

/// Background commands stay the `sandbox` mode's (R16): in a `host`
/// session one is refused, with the reason, and nothing keeps running.
#[tokio::test]
async fn a_host_session_gets_no_background_command() {
    let Some(mut n) = node().await else { return };
    let host = n.create(ExecMode::Host).await;
    let text = r#"script: [{"tool": "bash", "input": {"command": "sleep 976321", "background": true, "timeout_secs": 30}}]"#;
    let out = n.run(host, text).await.join("\n");
    assert!(
        out.contains(harness_tools::background::NOT_CONTAINED),
        "{out}"
    );
    let alive = std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .any(|p| {
            std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| c == b"sleep\0976321\0")
        });
    assert!(!alive, "a background command of the host session runs");
}
