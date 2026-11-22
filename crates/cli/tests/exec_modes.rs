//! Execution modes on one node that offers both (N.10a;
//! docs/execution-modes.md): the confinements come from the node's own
//! start (`sandbox::start_with_exe`, as `rizzy-harness` runs it, with the
//! harness binary as the wrapper), the node runtime picks one per session
//! from the mode its log recorded. A `sandbox` session's command runs in
//! bubblewrap (its own PID namespace); a `host` session's, beside it in the
//! same node, does not; its background command runs under the guard alone.
//!
//! Needs bubblewrap; with `HARNESS_REQUIRE_BWRAP=1` a missing one fails.
//! Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;
#[path = "support/marker.rs"]
mod marker;
use marker::Mark;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::ExecMode;
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    data: PathBuf,
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
    // As the binary sets a node up: the data directory protected, the
    // operator's instruction file located in it (X.2), and file tools
    // allowed outside the session folder (the widest setting).
    let mut policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        exec: harness_node::exec::ExecOffers::new(Ok(()), Ok(()), vec![]),
        sandbox_tmp: s.tmp_root.clone(),
        allow_outside_workspace: true,
        ..Default::default()
    };
    policy.protect(&data);
    policy.locate_instructions(&data).unwrap();
    // New projects' folders (X.26, T77) beside the session folder.
    policy.projects.dir = Some(root.join("home/projects"));
    policy.locate_projects(&data).unwrap();
    let backend = NativeBackend::new(
        Arc::new(Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            sandbox: s.confinement,
            host_shell: s.uncontained,
            protected_dirs: Arc::new(vec![data.clone()]),
            allow_outside_workspace: true,
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
        data,
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

    /// A new project's folder, made by the node (`project.create`).
    async fn new_project(&mut self, name: &str) -> PathBuf {
        self.link
            .to_node
            .try_send(MasterToNode::ProjectCreate {
                req_id: "pc".into(),
                name: name.into(),
            })
            .unwrap();
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::ProjectCreated { path, error, .. } = m {
                return PathBuf::from(path.unwrap_or_else(|| panic!("{error:?}")));
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
                // A `host` chat in Ask, so its cards (which name the host
                // shell) are checked; a sandbox chat runs unasked.
                approval_mode: Some(if mode == ExecMode::Host {
                    harness_core::policy::ApprovalMode::Ask
                } else {
                    harness_core::policy::ApprovalMode::Auto
                }),
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
            .try_send(MasterToNode::Prompt(PromptMsg::test(sid, text)))
            .unwrap();
        self.ack().await.unwrap();
        // Only so that a turn that never ends does not hang the suite.
        let by = std::time::Instant::now() + Duration::from_secs(120);
        let mut answered = std::collections::HashSet::new();
        loop {
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            // A `host` session made in Ask asks: allow each card.
            for e in &log {
                if let Ok(EventKind::ApprovalRequested(a)) = e.typed()
                    && answered.insert(a.approval_id.clone())
                {
                    assert!(a.reason.contains("in the host shell"), "{}", a.reason);
                    self.link
                        .to_node
                        .try_send(MasterToNode::ApprovalRespond {
                            req_id: "a".into(),
                            session_id: sid,
                            approval_id: a.approval_id,
                            decision: harness_core::policy::Decision::Allow,
                            note: None,
                        })
                        .unwrap();
                }
            }
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

/// X.26, T77: the folder a node makes for a new project takes a session
/// in either mode: the folder passes the node's checks, and a command
/// writes there (contained in the sandbox mode, not in the host mode).
#[tokio::test]
async fn a_new_project_folder_runs_sessions_in_both_modes() {
    let Some(mut n) = node().await else { return };
    let folder = n.new_project("Both Modes").await;
    assert!(folder.join(".git").is_dir());
    n.ws = folder.clone();
    for (mode, file) in [
        (ExecMode::Sandbox, "in-sandbox"),
        (ExecMode::Host, "in-host"),
    ] {
        let sid = n.create(mode).await;
        let out = n
            .run(
                sid,
                &format!("run: echo pid=$$ > {file}; git log --oneline | wc -l"),
            )
            .await
            .join("\n");
        assert!(folder.join(file).is_file(), "{mode:?}: {out}");
        let pid = std::fs::read_to_string(folder.join(file)).unwrap();
        assert_eq!(
            pid.trim() == "pid=2",
            mode == ExecMode::Sandbox,
            "{mode:?}: {pid}"
        );
    }
}

/// The owner's decision of 2026-11-20 (PLAN.md T9): a `host` session's
/// background command is accepted and runs under the node's guard without
/// bubblewrap. It is in the node's PID namespace (a later command sees
/// it), its start note says so, and `bash_stop` ends it with the guard's
/// confirmation, in words that claim descendants only. (Before this the
/// start was refused: R16, review N-c.)
#[tokio::test]
async fn a_host_session_gets_a_background_command_under_the_guard() {
    let Some(mut n) = node().await else { return };
    let host = n.create(ExecMode::Host).await;
    let mark = Mark(976_321);
    let alive = || {
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .filter(|p| {
                std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| c == mark.cmdline())
            })
            .count()
    };
    let text = format!(
        r#"script: [{{"tool": "bash", "input": {{"command": "sleep {mark}", "background": true, "timeout_secs": 600}}}}]"#
    );
    let out = n.run(host, &text).await.join("\n");
    assert!(
        out.contains("running in the background as bg-1")
            && out.contains("This chat runs on the host")
            && !out.contains("Later commands cannot see it"),
        "{out}"
    );
    assert_eq!(
        alive(),
        1,
        "the host session's background command is not running"
    );
    // `run` returns at the first turn end it sees (the first turn's), so
    // read the stop's result from the log as it arrives.
    n.run(
        host,
        r#"script: [{"tool": "bash_stop", "input": {"id": "bg-1"}}]"#,
    )
    .await;
    let by = std::time::Instant::now() + Duration::from_secs(30);
    let out = loop {
        let out: String =
            n.rt.store()
                .events_after(host, 0, 10_000)
                .unwrap()
                .iter()
                .filter_map(|e| match e.typed() {
                    Ok(EventKind::ToolCallFinished(f)) => Some(f.output),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
        if out.contains("its guard confirmed the end") || std::time::Instant::now() > by {
            break out;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        out.contains("every process descended from it were killed")
            && out.contains("its guard confirmed the end"),
        "{out}"
    );
    assert_eq!(alive(), 0, "bash_stop left the command running");
}

/// N.10b on the real confinements: switching a chat moves its commands
/// between bubblewrap and the node's own namespace, both ways, and a
/// switch is refused while the chat's background command runs (it waits for
/// the end, then goes through).
#[tokio::test]
async fn a_switch_moves_the_real_confinement_and_waits_for_background_work() {
    let Some(mut n) = node().await else { return };
    let ours = own_pid_ns();
    let cmd = "run: echo pid=$$ ns=$(readlink /proc/self/ns/pid)";
    let sid = n.create(ExecMode::Sandbox).await;
    let out = n.run(sid, cmd).await.pop().unwrap();
    assert!(out.contains("pid=2 ") && !out.contains(&ours), "{out}");
    let switch = |n: &mut Node, sid, mode| {
        n.link
            .to_node
            .try_send(MasterToNode::SetExecMode {
                req_id: format!("x-{}", harness_core::new_id()),
                session_id: sid,
                mode,
            })
            .unwrap();
    };
    switch(&mut n, sid, ExecMode::Host);
    n.ack().await.unwrap();
    // The next command runs beside the node, and now (Auto, the creation's
    // mode stays) unasked: no card.
    let out = n.run(sid, cmd).await.pop().unwrap();
    assert!(
        !out.contains("pid=2 ") && out.contains(&format!("ns={ours}")),
        "{out}"
    );
    switch(&mut n, sid, ExecMode::Sandbox);
    n.ack().await.unwrap();
    let out = n.run(sid, cmd).await.pop().unwrap();
    assert!(out.contains("pid=2 ") && !out.contains(&ours), "{out}");

    // A background command runs: the switch is refused, naming what to
    // stop; once it is stopped and its end recorded, it goes through.
    let mark = Mark(976_377);
    let text = format!(
        r#"script: [{{"tool": "bash", "input": {{"command": "sleep {mark}", "background": true, "timeout_secs": 600}}}}]"#
    );
    n.run(sid, &text).await;
    switch(&mut n, sid, ExecMode::Host);
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("background"), "{e}");
    n.run(
        sid,
        r#"script: [{"tool": "bash_stop", "input": {"id": "bg-1"}}]"#,
    )
    .await;
    let by = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        switch(&mut n, sid, ExecMode::Host);
        match n.ack().await {
            Ok(()) => break,
            Err(e) => assert!(
                e.contains("background") && std::time::Instant::now() < by,
                "the switch never went through: {e}"
            ),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Review of PR 186 (the card of a host background start): with
/// `[sandbox] network = "off"` the node's gaps say what THIS machine's
/// Landlock does to a host command's TCP, and the card's sentence about
/// the network follows exactly that gap (`ExecOffers::host_net_open`):
/// listening is stopped where the kernel enforces Landlock's TCP rule (ABI
/// 4 and up) and open where it does not. Skipped, loudly, where the node
/// has no Landlock shell here.
#[test]
fn the_host_gap_for_a_network_off_node_follows_this_machines_landlock() {
    let dir = tempfile::Builder::new()
        .prefix("exec-modes-net-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let data = dir.path().canonicalize().unwrap().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let settings = harness_tools::sandbox::Settings {
        network: harness_tools::sandbox::Network::Off,
        ..Default::default()
    };
    let s = harness_tools::sandbox::start_with_exe(
        PathBuf::from(EXE),
        &settings,
        &data,
        std::slice::from_ref(&data),
        &data.join("node.toml"),
    )
    .unwrap();
    if !matches!(
        s.uncontained,
        harness_tools::sandbox::Confinement::Confined { .. }
    ) {
        assert!(!bwrap::required(), "no Landlock shell on this machine");
        eprintln!("SKIPPED: this machine has no Landlock host shell");
        return;
    }
    let mut policy = harness_node::NodePolicy::default();
    policy.execution.host = true;
    policy.sandbox.network = harness_tools::sandbox::Network::Off;
    let offers = harness_node::exec::ExecOffers::from_start(&s, &policy);
    eprintln!(
        "Landlock ABI {}: TCP stopped for a host command: {}",
        s.landlock_abi,
        s.host_tcp_stopped()
    );
    assert_eq!(s.host_tcp_stopped(), s.landlock_abi >= 4);
    assert_eq!(offers.host_net_open(), !s.host_tcp_stopped());
}

/// X.2, T62: the node operator's instruction file in its default place,
/// `AGENTS.md` in the node's data directory, is out of reach of every tool
/// in both modes, with file tools allowed outside the session folder:
/// `write` and `edit` are refused, the shell cannot append, replace or
/// link it. Each turn still reads it into its context (`context_sent`).
#[tokio::test]
async fn the_operators_file_in_the_data_directory_is_out_of_every_tools_reach() {
    let Some(mut n) = node().await else { return };
    let file = n.data.join("AGENTS.md");
    let rule = "RULE: answer in French.\n";
    std::fs::write(&file, rule).unwrap();
    let f = file.display().to_string();
    let shell = format!(
        "echo PLANTED >> {f}; echo PLANTED > {f}.new && mv {f}.new {f}; \
         ln -sf /etc/passwd {f}; rm -f {f}; echo done"
    );
    let script = serde_json::json!([
        {"tool": "write", "input": {"path": f, "content": "PLANTED"}},
        {"tool": "edit", "input": {"path": f, "old_string": "RULE", "new_string": "PLANTED"}},
        {"tool": "bash", "input": {"command": shell}},
    ]);
    for mode in [ExecMode::Sandbox, ExecMode::Host] {
        let sid = n.create(mode).await;
        let out = n.run(sid, &format!("script: {script}")).await;
        assert_eq!(out.len(), 3, "{mode:?}: {out:?}");
        assert!(
            out[0].contains("data directory"),
            "{mode:?} write: {}",
            out[0]
        );
        assert!(
            out[1].contains("data directory"),
            "{mode:?} edit: {}",
            out[1]
        );
        assert!(out[2].contains("done"), "{mode:?} bash: {}", out[2]);
        assert_eq!(
            std::fs::symlink_metadata(&file).map(|m| m.is_file()).ok(),
            Some(true),
            "{mode:?}: the file was replaced or removed"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), rule, "{mode:?}");
        assert!(!n.data.join("AGENTS.md.new").exists(), "{mode:?}");
        // The turn read it as the operator's section.
        let log = n.rt.store().events_after(sid, 0, 10_000).unwrap();
        let sent = log.iter().find_map(|e| match e.typed() {
            Ok(EventKind::ContextSent(c)) => c
                .sections
                .into_iter()
                .find(|s| s.key == "user_instructions"),
            _ => None,
        });
        let sent = sent.unwrap_or_else(|| panic!("{mode:?}: no operator's section"));
        assert!(
            sent.text.contains("RULE: answer in French."),
            "{}",
            sent.text
        );
    }
}

/// With the default `[sandbox] mode = "required"` Landlock keeps the node's
/// data directory out of a `host` chat's shell too (T82 case (i), T39): the
/// store of approved servers cannot be read or changed from it, and the
/// directory cannot be listed. (`mode = "off"` has no such rule: README and
/// docs/execution-modes.md say so, and `doctor` warns.)
#[tokio::test]
async fn a_host_chats_shell_cannot_read_or_change_the_data_directory() {
    let Some(mut n) = node().await else { return };
    let store = n.data.join("project-mcp.json");
    let secret = "{\"approved\": \"CANARY-STORE\"}\n";
    std::fs::write(&store, secret).unwrap();
    let (s, d) = (store.display().to_string(), n.data.display().to_string());
    let shell = format!("cat {s} 2>&1; echo FORGED >> {s} 2>&1; ls {d} 2>&1; echo done");
    let script = serde_json::json!([{"tool": "bash", "input": {"command": shell}}]);
    for mode in [ExecMode::Host, ExecMode::Sandbox] {
        let sid = n.create(mode).await;
        let out = n.run(sid, &format!("script: {script}")).await;
        assert_eq!(out.len(), 1, "{mode:?}: {out:?}");
        assert!(out[0].contains("done"), "{mode:?}: {}", out[0]);
        assert!(
            !out[0].contains("CANARY-STORE") && !out[0].contains("project-mcp.json\n"),
            "{mode:?}: the shell read the data directory: {}",
            out[0]
        );
        assert!(
            out[0].to_lowercase().contains("permission denied"),
            "{mode:?}: {}",
            out[0]
        );
        assert_eq!(std::fs::read_to_string(&store).unwrap(), secret, "{mode:?}");
    }
}

/// Plan mode end to end (X.7, T.7): in both execution modes the chat's
/// `bash` runs in the read-only shell, contained (a `host` chat's too, in
/// a PID namespace of its own), reads work and nothing is written; after
/// the person switches to auto the same command writes. A bubblewrap that
/// cannot size a tmpfs has no read-only shell: then `bash` is refused and
/// says why.
#[tokio::test]
async fn plan_mode_runs_bash_read_only_in_both_execution_modes() {
    let Some(mut n) = node().await else { return };
    std::fs::write(n.ws.join("kept.txt"), "original\n").unwrap();
    let shell = harness_tools::contain::find_bwrap()
        .map(|b| harness_tools::contain::sized_tmpfs(&b))
        .unwrap_or(false);
    for mode in [ExecMode::Sandbox, ExecMode::Host] {
        let sid = n.create(mode).await;
        n.link
            .to_node
            .try_send(MasterToNode::SetApprovalMode {
                lowering: true,
                req_id: "plan".into(),
                session_id: sid,
                mode: harness_core::policy::ApprovalMode::Plan,
            })
            .unwrap();
        n.ack().await.unwrap();
        let out = n
            .run(
                sid,
                "run: cat kept.txt; echo pid=$$; touch planned.txt; echo x >> kept.txt; \
                 echo t > $TMPDIR/t && cat $TMPDIR/t",
            )
            .await
            .join("\n");
        assert!(!n.ws.join("planned.txt").exists(), "{mode:?}: {out}");
        assert_eq!(
            std::fs::read_to_string(n.ws.join("kept.txt")).unwrap(),
            "original\n",
            "{mode:?}: {out}"
        );
        if !shell {
            assert!(
                !bwrap::required(),
                "HARNESS_REQUIRE_BWRAP=1 but there is no read-only shell here: {out}"
            );
            eprintln!("SKIPPED (no read-only shell with this bubblewrap): {out}");
            assert!(out.contains("no read-only shell"), "{mode:?}: {out}");
            continue;
        }
        assert!(out.contains("original"), "{mode:?}: reads: {out}");
        assert!(out.contains("pid=2"), "{mode:?}: not contained: {out}");
        assert!(out.contains("Read-only file system"), "{mode:?}: {out}");
        // The person leaves plan mode: the same kind of command writes.
        n.link
            .to_node
            .try_send(MasterToNode::SetApprovalMode {
                lowering: false,
                req_id: "auto".into(),
                session_id: sid,
                mode: harness_core::policy::ApprovalMode::Auto,
            })
            .unwrap();
        n.ack().await.unwrap();
        let out = n.run(sid, "run: touch planned.txt").await.join("\n");
        assert!(n.ws.join("planned.txt").exists(), "{mode:?}: {out}");
        std::fs::remove_file(n.ws.join("planned.txt")).unwrap();
    }
}
