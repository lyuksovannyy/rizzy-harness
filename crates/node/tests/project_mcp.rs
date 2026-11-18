//! A repository's own MCP servers (ADOPTION_PLAN X.36, PLAN.md T82), over
//! the link as the master drives a node, with the scripted agent: off by
//! default (no `.mcp.json` is read, nothing is announced, every request is
//! refused); with `[mcp] project_servers = "ask"` a person approves each
//! entry for their own chats, pinned to the entry they were shown, and the
//! node's approved copy is what runs whatever the file says later; a new
//! entry runs nothing, a removed one stops; never in a `host` chat, never
//! in another person's chat; variables, remote servers and node.toml's
//! names cannot be approved; `env` values never leave the node. Unconfined
//! (no bubblewrap); the contained path is `cli/tests/project_mcp_sandbox.rs`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::EventKind;
use harness_core::policy::{ApprovalMode, Decision, ExecMode};
use harness_node::exec::ExecOffers;
use harness_node::policy::{McpPolicy, ProjectServers};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{
    CAP_PROJECT_MCP, MasterToNode, NodeToMaster, ProjectMcpList, ProjectMcpRequest, ProjectMcpState,
};
use harness_store::{Durability, Store};
use harness_tools::mcp::ServerConfig;

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

const ALICE: &str = "01ALICE";
const BOB: &str = "01BOB";

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    rt: NodeRuntime,
    link: NodeLink,
    caps: Vec<String>,
}

#[derive(Clone, Copy)]
struct Opts {
    on: bool,
    host: bool,
    sandbox: bool,
}

const ON: Opts = Opts {
    on: true,
    host: false,
    sandbox: true,
};

fn root_of(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().canonicalize().unwrap()
}

fn started(root: &Path, marker: &str) -> PathBuf {
    root.join(format!("started-{marker}"))
}

fn pids(file: &Path) -> Vec<i32> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("started ")?.parse().ok())
        .collect()
}

/// One `.mcp.json` entry: the fake server whose tool `who` answers
/// "served by <marker> tool who", noting each start in a file.
fn server(root: &Path, marker: &str) -> serde_json::Value {
    serde_json::json!({
        "command": "python3",
        "args": [FAKE, "--tool-names", "who", "--marker", marker,
                 "--started", started(root, marker).display().to_string()],
    })
}

fn write_mcp(ws: &Path, servers: serde_json::Value) {
    let p = ws.join(".mcp.json");
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&p).unwrap(),
        Ok(_) => std::fs::remove_file(&p).unwrap(),
        Err(_) => {}
    }
    std::fs::write(p, serde_json::json!({ "mcpServers": servers }).to_string()).unwrap();
}

fn offers(o: Opts) -> ExecOffers {
    let off = |w: &str| Err(format!("{w} is off in this test"));
    ExecOffers::new(
        if o.sandbox { Ok(()) } else { off("sandbox") },
        if o.host { Ok(()) } else { off("host") },
        vec![],
    )
}

async fn start_at(dir: tempfile::TempDir, o: Opts) -> Node {
    let root = root_of(&dir);
    let ws = root.join("ws");
    let _ = std::fs::create_dir(&ws);
    let data = root.join("data");
    let _ = std::fs::create_dir(&data);
    // node.toml's own server, `fake`: a project entry of that name is
    // hidden by it.
    let mut servers = BTreeMap::new();
    servers.insert(
        "fake".to_string(),
        ServerConfig::new(
            "python3",
            &[
                FAKE,
                "--tool-names",
                "who",
                "--marker",
                "node",
                "--started",
                started(&root, "node").to_str().unwrap(),
            ],
        ),
    );
    let mut policy = NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: ApprovalMode::Auto,
        mcp_servers: servers,
        mcp: McpPolicy {
            project_servers: if o.on {
                ProjectServers::Ask
            } else {
                ProjectServers::Off
            },
            approvals_file: Some(data.join("project-mcp.json")),
        },
        exec: offers(o),
        ..Default::default()
    };
    policy.protect(&data);
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&data.join("node.db"), Durability::Full).unwrap(),
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
    let NodeToMaster::Hello { capabilities, .. } = link.from_node.recv().await.unwrap() else {
        panic!("no hello")
    };
    Node {
        dir,
        ws,
        rt,
        link,
        caps: capabilities,
    }
}

async fn start(o: Opts) -> Node {
    start_at(tempfile::tempdir().unwrap(), o).await
}

impl Node {
    fn root(&self) -> PathBuf {
        root_of(&self.dir)
    }

    /// Stop the node and start it again on the same data.
    async fn restart(self) -> Node {
        self.rt.stop(Duration::from_secs(20)).await;
        let Node { dir, rt, link, .. } = self;
        drop(link);
        drop(rt);
        tokio::time::sleep(Duration::from_millis(200)).await;
        start_at(dir, ON).await
    }

    async fn ack(&mut self, want: &str) -> Result<(), String> {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::Ack { ok, error, req_id } = m
                && req_id == want
            {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    async fn create(&mut self, owner: Option<&str>, exec_mode: Option<ExecMode>) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: format!("c-{sid}"),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Ask),
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode,
                owner: owner.map(str::to_string),
            })
            .unwrap();
        self.ack(&format!("c-{sid}")).await.unwrap();
        sid
    }

    /// A person's request about the folder's `.mcp.json`, and its answer.
    async fn ask(&mut self, user: &str, req: ProjectMcpRequest) -> Result<ProjectMcpList, String> {
        let folder = self.ws.display().to_string();
        self.ask_at(user, &folder, req).await
    }

    async fn ask_at(
        &mut self,
        user: &str,
        folder: &str,
        request: ProjectMcpRequest,
    ) -> Result<ProjectMcpList, String> {
        let req_id = harness_core::new_id();
        self.link
            .to_node
            .try_send(MasterToNode::ProjectMcp {
                req_id: req_id.clone(),
                user_id: user.into(),
                folder: folder.into(),
                request,
            })
            .unwrap();
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            if let NodeToMaster::ProjectMcpResult {
                req_id: r,
                list,
                error,
            } = m
                && r == req_id
            {
                return match (list, error) {
                    (Some(l), None) => Ok(*l),
                    (_, e) => Err(e.unwrap_or_default()),
                };
            }
        }
    }

    async fn approve(&mut self, user: &str, name: &str) -> ProjectMcpList {
        let l = self.ask(user, ProjectMcpRequest::List).await.unwrap();
        let digest = entry(&l, name).digest.clone().expect("a digest to approve");
        self.ask(
            user,
            ProjectMcpRequest::Approve {
                name: name.into(),
                digest,
            },
        )
        .await
        .unwrap()
    }

    /// One prompt to its turn's end, allowing every approval; the log.
    async fn run(&mut self, sid: SessionId, text: &str) -> Vec<harness_core::RawEvent> {
        self.link
            .to_node
            .try_send(MasterToNode::Prompt {
                req_id: format!("p-{sid}"),
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
                files: vec![],
            })
            .unwrap();
        self.ack(&format!("p-{sid}")).await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(30);
        let mut answered = std::collections::HashSet::new();
        let start = self
            .rt
            .store()
            .events_after(sid, 0, 100_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "turn_ended")
            .count();
        loop {
            let log = self.rt.store().events_after(sid, 0, 100_000).unwrap();
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
                            note: None,
                        })
                        .unwrap();
                }
            }
            if log.iter().filter(|e| e.kind == "turn_ended").count() > start {
                return log;
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The outputs of a turn that calls `tools` (each with `{}`), in order.
    async fn call(&mut self, sid: SessionId, tools: &[&str]) -> Vec<(bool, String)> {
        let steps: Vec<String> = tools
            .iter()
            .map(|t| format!(r#"{{"tool": "{t}", "input": {{}}}}"#))
            .collect();
        let before = self.rt.store().events_after(sid, 0, 100_000).unwrap().len();
        let log = self
            .run(sid, &format!("script: [{}]", steps.join(", ")))
            .await;
        outputs(&log[before..])
    }
}

fn entry<'a>(l: &'a ProjectMcpList, name: &str) -> &'a harness_protocol::ProjectMcpEntry {
    l.entries
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("no {name} in {l:?}"))
}

fn outputs(log: &[harness_core::RawEvent]) -> Vec<(bool, String)> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => Some((f.ok, f.output)),
            _ => None,
        })
        .collect()
}

fn errors(log: &[harness_core::RawEvent]) -> Vec<String> {
    log.iter()
        .filter_map(|e| match e.typed() {
            Ok(EventKind::Error(x)) => Some(x.message),
            _ => None,
        })
        .collect()
}

fn unknown(out: &(bool, String), tool: &str) -> bool {
    !out.0 && out.1.contains(&format!("unknown tool `{tool}`"))
}

/// Somebody opens `fifo` for reading: a writer that waits in `open` is let
/// through. The flag says whether that happened.
fn watch_fifo(fifo: &Path) -> Arc<AtomicBool> {
    nix::unistd::mkfifo(fifo, nix::sys::stat::Mode::S_IRWXU).unwrap();
    let opened = Arc::new(AtomicBool::new(false));
    let (o, f) = (opened.clone(), fifo.to_path_buf());
    std::thread::spawn(move || {
        if std::fs::OpenOptions::new().write(true).open(&f).is_ok() {
            o.store(true, Ordering::SeqCst);
        }
    });
    opened
}

fn release_fifo(fifo: &Path) {
    // Let the waiting writer go, whoever opened it.
    let _ = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NONBLOCK)
        .open(fifo);
}

use std::os::unix::fs::OpenOptionsExt;

/// Off (the default): nothing is announced, every request is refused, no
/// request or chat reads `.mcp.json` (a FIFO there would let a waiting
/// writer through), and its servers never start.
#[tokio::test]
async fn off_reads_nothing_and_announces_nothing() {
    let mut n = start(Opts { on: false, ..ON }).await;
    assert!(!n.caps.iter().any(|c| c == CAP_PROJECT_MCP), "{:?}", n.caps);
    let fifo = n.ws.join(".mcp.json");
    let opened = watch_fifo(&fifo);
    let e = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap_err();
    assert!(e.contains("project_servers"), "{e}");
    let e = n
        .ask(
            ALICE,
            ProjectMcpRequest::Approve {
                name: "proj".into(),
                digest: "0".repeat(64),
            },
        )
        .await
        .unwrap_err();
    assert!(e.contains("project_servers"), "{e}");
    let sid = n.create(Some(ALICE), None).await;
    let out = n.call(sid, &["mcp__proj__who", "mcp__fake__who"]).await;
    assert!(unknown(&out[0], "mcp__proj__who"), "{out:?}");
    assert_eq!(out[1], (true, "served by node tool who".into()));
    let n = n.restart_off().await;
    assert!(
        !opened.load(Ordering::SeqCst),
        "the node opened .mcp.json with the feature off"
    );
    release_fifo(&n.ws.join(".mcp.json"));
}

impl Node {
    async fn restart_off(self) -> Node {
        self.rt.stop(Duration::from_secs(20)).await;
        let Node { dir, rt, link, .. } = self;
        drop(link);
        drop(rt);
        tokio::time::sleep(Duration::from_millis(200)).await;
        start_at(dir, Opts { on: false, ..ON }).await
    }
}

/// The core rule: an approved entry runs as the node copied it; whatever
/// the agent makes of the file afterwards (another command, a link, a
/// FIFO, a huge file, a folder, a new entry), every later turn, every new
/// chat and a node restart start the approved copy and nothing new.
#[tokio::test]
async fn the_approved_copy_runs_whatever_the_file_says_later() {
    let mut n = start(ON).await;
    assert!(n.caps.iter().any(|c| c == CAP_PROJECT_MCP));
    let root = n.root();
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "v1") }));
    let sid = n.create(Some(ALICE), None).await;
    // Not approved: not started.
    let out = n.call(sid, &["mcp__proj__who"]).await;
    assert!(unknown(&out[0], "mcp__proj__who"), "{out:?}");
    assert!(pids(&started(&root, "v1")).is_empty());
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    let e = entry(&l, "proj");
    assert_eq!(e.state, ProjectMcpState::New);
    assert_eq!(e.command, "python3");
    assert!(e.args.iter().any(|a| a == "v1"), "{e:?}");
    let l = n.approve(ALICE, "proj").await;
    assert_eq!(entry(&l, "proj").state, ProjectMcpState::Approved, "{l:?}");
    // The next turn of the same chat starts it.
    let out = n.call(sid, &["mcp__proj__who"]).await;
    assert_eq!(out[0], (true, "served by v1 tool who".into()));

    // The agent rewrites the entry and adds one.
    write_mcp(
        &n.ws,
        serde_json::json!({ "proj": server(&root, "v2"), "extra": server(&root, "extra") }),
    );
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    let e = entry(&l, "proj");
    assert_eq!(e.state, ProjectMcpState::Changed, "{l:?}");
    assert!(e.args.iter().any(|a| a == "v2"));
    assert!(e.approved_args.as_ref().unwrap().iter().any(|a| a == "v1"));
    assert_eq!(entry(&l, "extra").state, ProjectMcpState::New);
    let fresh = n.create(Some(ALICE), None).await;
    let out = n.call(fresh, &["mcp__proj__who", "mcp__extra__who"]).await;
    assert_eq!(out[0], (true, "served by v1 tool who".into()), "{out:?}");
    assert!(unknown(&out[1], "mcp__extra__who"), "{out:?}");

    // A link, a FIFO, a huge file, a folder: the approved copy, nothing new.
    let p = n.ws.join(".mcp.json");
    let elsewhere = root.join("elsewhere.json");
    std::fs::write(
        &elsewhere,
        serde_json::json!({"mcpServers": {"proj": server(&root, "v3")}}).to_string(),
    )
    .unwrap();
    std::fs::remove_file(&p).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &p).unwrap();
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    assert!(l.unreadable.as_deref().unwrap().contains("link"), "{l:?}");
    let c = n.create(Some(ALICE), None).await;
    assert_eq!(
        n.call(c, &["mcp__proj__who"]).await[0],
        (true, "served by v1 tool who".into())
    );
    std::fs::remove_file(&p).unwrap();
    nix::unistd::mkfifo(&p, nix::sys::stat::Mode::S_IRWXU).unwrap();
    let c = n.create(Some(ALICE), None).await;
    assert_eq!(
        n.call(c, &["mcp__proj__who"]).await[0],
        (true, "served by v1 tool who".into())
    );
    std::fs::remove_file(&p).unwrap();
    std::fs::write(&p, vec![b' '; 10 << 20]).unwrap();
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    assert!(l.unreadable.as_deref().unwrap().contains("larger"), "{l:?}");
    std::fs::remove_file(&p).unwrap();
    std::fs::create_dir(&p).unwrap();
    let c = n.create(Some(ALICE), None).await;
    assert_eq!(
        n.call(c, &["mcp__proj__who"]).await[0],
        (true, "served by v1 tool who".into())
    );

    // A node restart: the old chat, and a new one, still run the copy.
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "v2") }));
    let mut n = n.restart().await;
    let out = n.call(sid, &["mcp__proj__who"]).await;
    assert_eq!(out[0], (true, "served by v1 tool who".into()), "{out:?}");
    for m in ["v2", "v3", "extra"] {
        assert!(pids(&started(&root, m)).is_empty(), "{m} started");
    }
    // Approving the change takes it up at the next turn.
    n.approve(ALICE, "proj").await;
    let out = n.call(sid, &["mcp__proj__who"]).await;
    assert_eq!(out[0], (true, "served by v2 tool who".into()), "{out:?}");
    n.rt.stop(Duration::from_secs(20)).await;
}

/// The file changes between the list the person saw and their click: the
/// approval is refused and says to reload.
#[tokio::test]
async fn a_change_between_the_list_and_the_click_is_refused() {
    let mut n = start(ON).await;
    let root = n.root();
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "v1") }));
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    let shown = entry(&l, "proj").digest.clone().unwrap();
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "evil") }));
    let l = n
        .ask(
            ALICE,
            ProjectMcpRequest::Approve {
                name: "proj".into(),
                digest: shown,
            },
        )
        .await
        .unwrap();
    assert!(l.note.as_deref().unwrap().contains("changed"), "{l:?}");
    assert_eq!(entry(&l, "proj").state, ProjectMcpState::New);
    let sid = n.create(Some(ALICE), None).await;
    assert!(unknown(
        &n.call(sid, &["mcp__proj__who"]).await[0],
        "mcp__proj__who"
    ));
    assert!(pids(&started(&root, "evil")).is_empty());
    n.rt.stop(Duration::from_secs(20)).await;
}

/// Approvals are a person's own: Bob's chat in Alice's folder starts
/// nothing she approved and his list shows the entry as new; a chat
/// without an owner (the API token's) starts nothing either.
#[tokio::test]
async fn one_persons_approval_never_reaches_another_persons_chat() {
    let mut n = start(ON).await;
    let root = n.root();
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "v1") }));
    n.approve(ALICE, "proj").await;
    let bob = n.create(Some(BOB), None).await;
    assert!(unknown(
        &n.call(bob, &["mcp__proj__who"]).await[0],
        "mcp__proj__who"
    ));
    let token = n.create(None, None).await;
    assert!(unknown(
        &n.call(token, &["mcp__proj__who"]).await[0],
        "mcp__proj__who"
    ));
    assert!(pids(&started(&root, "v1")).is_empty());
    let l = n.ask(BOB, ProjectMcpRequest::List).await.unwrap();
    assert_eq!(entry(&l, "proj").state, ProjectMcpState::New);
    let alice = n.create(Some(ALICE), None).await;
    assert_eq!(
        n.call(alice, &["mcp__proj__who"]).await[0],
        (true, "served by v1 tool who".into())
    );
    // A forged owner is refused at create.
    let e = {
        n.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "forged".into(),
                session_id: SessionId::new(),
                title: "t".into(),
                cwd: n.ws.display().to_string(),
                model: None,
                approval_mode: None,
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: None,
                owner: Some("alice\nINFO forged".into()),
            })
            .unwrap();
        n.ack("forged").await.unwrap_err()
    };
    assert!(e.contains("owner"), "{e}");
    n.rt.stop(Duration::from_secs(20)).await;
}

/// A `host` chat never starts a repository's server (the chat is told
/// why); a node that offers only host chats refuses the list.
#[tokio::test]
async fn a_host_chat_never_starts_a_repository_server() {
    let mut n = start(Opts { host: true, ..ON }).await;
    let root = n.root();
    write_mcp(&n.ws, serde_json::json!({ "proj": server(&root, "v1") }));
    n.approve(ALICE, "proj").await;
    let sid = n.create(Some(ALICE), Some(ExecMode::Host)).await;
    let before = n.rt.store().events_after(sid, 0, 100_000).unwrap().len();
    let log = n
        .run(sid, r#"script: [{"tool": "mcp__proj__who", "input": {}}]"#)
        .await;
    let out = outputs(&log[before..]);
    assert!(unknown(&out[0], "mcp__proj__who"), "{out:?}");
    assert!(
        errors(&log).iter().any(|e| e.contains("runs on the host")),
        "{:?}",
        errors(&log)
    );
    assert!(pids(&started(&root, "v1")).is_empty());
    n.rt.stop(Duration::from_secs(20)).await;

    let mut n = start(Opts {
        host: true,
        sandbox: false,
        ..ON
    })
    .await;
    let e = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap_err();
    assert!(e.contains("only host chats"), "{e}");
}

/// Variables, remote servers, a name node.toml has, a name of the wrong
/// shape: listed with the reason, never approved; node.toml's own server
/// keeps its name.
#[tokio::test]
async fn variables_remote_servers_and_node_toml_names_cannot_be_approved() {
    let mut n = start(ON).await;
    let root = n.root();
    let mut home = server(&root, "home");
    home["args"]
        .as_array_mut()
        .unwrap()
        .push("${HOME}/.ssh/id_ed25519".into());
    let mut env = server(&root, "envvar");
    env["env"] = serde_json::json!({"TOKEN": "${GITHUB_TOKEN}"});
    write_mcp(
        &n.ws,
        serde_json::json!({
            "home": home,
            "envvar": env,
            "remote": {"type": "http", "url": "https://example.invalid/mcp"},
            "fake": server(&root, "shadow"),
            "Fake": server(&root, "shadow"),
        }),
    );
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    for (name, state, why) in [
        ("home", ProjectMcpState::Unsupported, "variable"),
        ("envvar", ProjectMcpState::Unsupported, "variable"),
        ("remote", ProjectMcpState::Unsupported, "remote"),
        ("fake", ProjectMcpState::Hidden, "node.toml"),
        ("Fake", ProjectMcpState::Unsupported, "server name"),
    ] {
        let e = entry(&l, name);
        assert_eq!(e.state, state, "{e:?}");
        assert!(e.reason.as_deref().unwrap().contains(why), "{e:?}");
        assert!(e.digest.is_none(), "{e:?}");
    }
    // Even with a digest of the entry worked out by hand.
    for name in ["home", "envvar", "remote", "fake", "Fake"] {
        let l = n
            .ask(
                ALICE,
                ProjectMcpRequest::Approve {
                    name: name.into(),
                    digest: "a".repeat(64),
                },
            )
            .await
            .unwrap();
        assert!(l.note.as_deref().unwrap().contains("not approved"), "{l:?}");
    }
    let sid = n.create(Some(ALICE), None).await;
    let out = n.call(sid, &["mcp__fake__who", "mcp__home__who"]).await;
    assert_eq!(out[0], (true, "served by node tool who".into()));
    assert!(unknown(&out[1], "mcp__home__who"));
    for m in ["home", "envvar", "shadow"] {
        assert!(pids(&started(&root, m)).is_empty(), "{m} started");
    }
    n.rt.stop(Duration::from_secs(20)).await;
}

/// Removing narrows at once: an entry gone from the file is not started
/// by a new chat (read when its table is built) nor, after a person's
/// list, by a running one; a removed approval stops its server at the next
/// turn. Remove is always allowed, also for a folder gone since.
#[tokio::test]
async fn a_removed_entry_or_approval_stops() {
    let mut n = start(ON).await;
    let root = n.root();
    write_mcp(
        &n.ws,
        serde_json::json!({ "a": server(&root, "a"), "b": server(&root, "b") }),
    );
    n.approve(ALICE, "a").await;
    n.approve(ALICE, "b").await;
    let sid = n.create(Some(ALICE), None).await;
    let out = n.call(sid, &["mcp__a__who", "mcp__b__who"]).await;
    assert!(out.iter().all(|o| o.0), "{out:?}");
    // `b` leaves the file: a new chat's table read finds it gone.
    write_mcp(&n.ws, serde_json::json!({ "a": server(&root, "a") }));
    let fresh = n.create(Some(ALICE), None).await;
    let out = n.call(fresh, &["mcp__a__who", "mcp__b__who"]).await;
    assert!(out[0].0 && unknown(&out[1], "mcp__b__who"), "{out:?}");
    // ... and so does the running chat's next turn.
    let out = n.call(sid, &["mcp__b__who"]).await;
    assert!(unknown(&out[0], "mcp__b__who"), "{out:?}");
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    assert_eq!(entry(&l, "b").state, ProjectMcpState::Removed);
    // Alice removes her approval of `a`: its process ends, the tool goes.
    let pid = *pids(&started(&root, "a")).last().unwrap();
    let l = n
        .ask(ALICE, ProjectMcpRequest::Remove { name: "a".into() })
        .await
        .unwrap();
    assert_eq!(entry(&l, "a").state, ProjectMcpState::New);
    let out = n.call(sid, &["mcp__a__who"]).await;
    assert!(unknown(&out[0], "mcp__a__who"), "{out:?}");
    let by = std::time::Instant::now() + Duration::from_secs(10);
    while std::path::Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .unwrap_or_default()
            .contains(") Z")
    {
        assert!(
            std::time::Instant::now() < by,
            "the removed server still runs"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The folder gone: the approval of `b` can still be removed.
    let folder = n.ws.display().to_string();
    std::fs::rename(&n.ws, root.join("moved")).unwrap();
    let l = n
        .ask_at(
            ALICE,
            &folder,
            ProjectMcpRequest::Remove { name: "b".into() },
        )
        .await
        .unwrap();
    assert!(l.note.as_deref().unwrap().contains("removed"), "{l:?}");
    n.rt.stop(Duration::from_secs(20)).await;
}

/// An entry's `env` values reach the server and nothing else: not the
/// list, not the log (the server's own output of them is scrubbed).
#[tokio::test]
async fn env_values_never_leave_the_node() {
    const CANARY: &str = "canary-value-77231-zz";
    let mut n = start(ON).await;
    write_mcp(
        &n.ws,
        serde_json::json!({"envy": {"command": "python3", "args": [FAKE], "env": {"MY_TOKEN": CANARY}}}),
    );
    let l = n.ask(ALICE, ProjectMcpRequest::List).await.unwrap();
    assert_eq!(entry(&l, "envy").env_keys, ["MY_TOKEN"]);
    let l = n.approve(ALICE, "envy").await;
    assert!(!serde_json::to_string(&l).unwrap().contains(CANARY));
    let sid = n.create(Some(ALICE), None).await;
    let out = n.call(sid, &["mcp__envy__env"]).await;
    assert!(out[0].0, "{out:?}");
    assert!(out[0].1.contains("MY_TOKEN=[REDACTED]"), "{}", out[0].1);
    let log = n.rt.store().events_after(sid, 0, 100_000).unwrap();
    let raw: String = log.iter().map(|e| e.payload.to_string()).collect();
    assert!(!raw.contains(CANARY));
    // The approvals file holds it, privately.
    use std::os::unix::fs::PermissionsExt;
    let f = n.root().join("data/project-mcp.json");
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    n.rt.stop(Duration::from_secs(20)).await;
}

/// What a master can name is checked like a chat's folder: outside the
/// roots, the node's data directory, a bad account id: refused. A flood
/// of requests is refused past the node's two at once, and the node keeps
/// answering.
#[tokio::test]
async fn folders_accounts_and_floods_are_checked() {
    let mut n = start(ON).await;
    let data = n.root().join("data").display().to_string();
    let ws = n.ws.display().to_string();
    for (user, folder) in [
        (ALICE, "/etc".to_string()),
        (ALICE, data),
        ("../x", ws.clone()),
        ("", ws),
    ] {
        assert!(
            n.ask_at(user, &folder, ProjectMcpRequest::List)
                .await
                .is_err(),
            "{user} {folder}"
        );
    }
    write_mcp(
        &n.ws,
        serde_json::json!({ "proj": server(&n.root(), "v1") }),
    );
    let folder = n.ws.display().to_string();
    for i in 0..50 {
        n.link
            .to_node
            .try_send(MasterToNode::ProjectMcp {
                req_id: format!("flood-{i}"),
                user_id: ALICE.into(),
                folder: folder.clone(),
                request: ProjectMcpRequest::List,
            })
            .unwrap();
    }
    let (mut ok, mut busy) = (0, 0);
    while ok + busy < 50 {
        let m = tokio::time::timeout(Duration::from_secs(20), n.link.from_node.recv())
            .await
            .unwrap()
            .unwrap();
        if let NodeToMaster::ProjectMcpResult { list, error, .. } = m {
            match (list, error) {
                (Some(_), _) => ok += 1,
                (_, Some(e)) if e.contains("try again") => busy += 1,
                other => panic!("{other:?}"),
            }
        }
    }
    assert!(ok >= 1 && busy >= 1, "{ok} {busy}");
    assert!(n.ask(ALICE, ProjectMcpRequest::List).await.is_ok());
    n.rt.stop(Duration::from_secs(20)).await;
}
