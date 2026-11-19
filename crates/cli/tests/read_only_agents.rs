//! Read-only agents with a shell (ADOPTION_PLAN T.7, PLAN.md T70;
//! docs/read-only-shell.md): the built-in Scout, as the master sends it,
//! delegated to by a lead on a node started as the binary starts one. Its
//! `bash` always runs in the read-only shell, whatever the chat's approval
//! and execution mode and whatever the spec says, because the node judges
//! from the tools it offers (the shared rule, `AgentSpec::changes_things`).
//! Scout reads (`git log`, a pipeline), writes nothing, works in its
//! parent's folder (no copy, nothing merged back) and asks no card.
//!
//! Needs bubblewrap that can size a tmpfs; with `HARNESS_REQUIRE_BWRAP=1` a
//! missing one fails. Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_core::SessionId;
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::{EventKind, Profile};
use harness_core::policy::{ApprovalMode, ExecMode};
use harness_protocol::{MasterToNode, NodeToMaster};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    data: PathBuf,
    rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
}

/// What one turn did: every tool result of every session (the lead's and
/// its children's), and how many cards were shown.
struct Turn {
    outputs: Vec<(SessionId, String, String)>,
    cards: usize,
}

impl Turn {
    fn of(&self, tool: &str) -> Vec<&str> {
        self.outputs
            .iter()
            .filter(|(_, t, _)| t == tool)
            .map(|(_, _, o)| o.as_str())
            .collect()
    }
    fn all(&self) -> String {
        self.outputs
            .iter()
            .map(|(_, t, o)| format!("[{t}] {o}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A node offering `sandbox` and `host`, its policy changed by `tune`;
/// `None` (said) where it has no read-only shell here.
async fn node(tune: impl FnOnce(&mut harness_node::NodePolicy)) -> Option<Node> {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    let dir = tempfile::Builder::new()
        .prefix("ro-agents-")
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
    if let Err(why) = s.confinement.read_only_shell() {
        assert!(
            !bwrap::required(),
            "HARNESS_REQUIRE_BWRAP=1 but no read-only shell: {why}"
        );
        eprintln!("SKIPPED: no read-only shell here: {why}");
        return None;
    }
    let mut policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: ApprovalMode::Auto,
        exec: harness_node::exec::ExecOffers::new(Ok(()), Ok(()), vec![]),
        sandbox_tmp: s.tmp_root.clone(),
        ..Default::default()
    };
    policy.skills.roots.clear();
    tune(&mut policy);
    policy.protect(&data);
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
        checkpoints_dir: Some(data.join("checkpoints")),
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
    git(&ws, &["init", "-q", "-b", "main"]);
    std::fs::write(ws.join("lib.rs"), "fn one() {}\n").unwrap();
    git(&ws, &["add", "."]);
    git(&ws, &["commit", "-q", "-m", "first commit"]);
    std::fs::write(ws.join("lib.rs"), "fn one() {}\nfn two() {}\n").unwrap();
    git(&ws, &["commit", "-q", "-am", "add two"]);
    Some(Node {
        _dir: dir,
        ws,
        data,
        rt,
        link,
    })
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        // Nor the global ignore file (`~/.config/git/ignore`).
        .env("XDG_CONFIG_HOME", "/nonexistent")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// Every file of `dir` with its bytes, `.git` included.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let m = std::fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap_or_default()));
            }
        }
    }
    out.sort();
    out
}

/// The lead's prompt: one `delegate` call to `agent` with `task` (a
/// scripted call, not `delegate:`, whose `|` would split a pipeline).
fn delegate(agent: &str, task: &str) -> String {
    let call = json!([{"tool": "delegate", "input": {"agent": agent, "task": task}}]);
    format!("script: {call}")
}

/// The built-in Scout as the master resolves it.
fn scout() -> AgentSpec {
    harness_agents::builtins()
        .into_iter()
        .find(|a| a.id == "scout")
        .unwrap()
        .spec
}

/// A lead with every tool, which may delegate to `delegates`.
fn lead(delegates: Vec<AgentSpec>) -> AgentSpec {
    AgentSpec {
        agent_id: "lead".into(),
        name: "Lead".into(),
        tools: ToolSelection::All(harness_core::agent::AllTools::All),
        delegates,
        caps: Some(Default::default()),
        ..scout()
    }
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

    async fn create(&mut self, exec: ExecMode, mode: ApprovalMode) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(mode),
                profile: None,
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: Some(exec),
            })
            .unwrap();
        self.ack().await.unwrap();
        sid
    }

    async fn set_mode(&mut self, sid: SessionId, mode: ApprovalMode) {
        self.link
            .to_node
            .try_send(MasterToNode::SetApprovalMode {
                lowering: true,
                req_id: "m".into(),
                session_id: sid,
                mode,
            })
            .unwrap();
        self.ack().await.unwrap();
    }

    /// One prompt as `agent` to its turn's end, every card in any session
    /// allowed (and counted).
    async fn run(&mut self, sid: SessionId, agent: AgentSpec, text: &str) -> Turn {
        // Only what this turn adds: the parent's events from here on and
        // the sessions it starts.
        let known: std::collections::HashSet<SessionId> = self
            .rt
            .store()
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .filter(|s| *s != sid)
            .collect();
        let from = self.rt.store().events_after(sid, 0, 10_000).unwrap().len();
        self.link
            .to_node
            .try_send(MasterToNode::Prompt {
                req_id: "p".into(),
                session_id: sid,
                text: text.into(),
                agent: Some(agent),
                attachments: vec![],
                compare: vec![],
                reply: None,
                savings: None,
                context: None,
                images: vec![],
                agent_named: false,
                files: vec![],
                owner: None,
                identity: None,
            })
            .unwrap();
        self.ack().await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(60);
        let mut answered = std::collections::HashSet::new();
        loop {
            let sessions: Vec<SessionId> = self
                .rt
                .store()
                .sessions()
                .unwrap()
                .into_iter()
                .map(|s| s.session_id)
                .filter(|s| !known.contains(s))
                .collect();
            let skip = |s: &SessionId| if *s == sid { from } else { 0 };
            for s in &sessions {
                for e in self
                    .rt
                    .store()
                    .events_after(*s, 0, 10_000)
                    .unwrap()
                    .into_iter()
                    .skip(skip(s))
                {
                    if let Ok(EventKind::ApprovalRequested(a)) = e.typed()
                        && answered.insert(a.approval_id.clone())
                    {
                        let _ = self.link.to_node.try_send(MasterToNode::ApprovalRespond {
                            req_id: "a".into(),
                            session_id: *s,
                            approval_id: a.approval_id,
                            decision: harness_core::policy::Decision::Allow,
                            note: None,
                        });
                    }
                }
            }
            let log = self.rt.store().events_after(sid, 0, 10_000).unwrap();
            if log.last().is_some_and(|e| e.kind == "turn_ended") {
                let mut outputs = Vec::new();
                for s in &sessions {
                    let log: Vec<_> = self
                        .rt
                        .store()
                        .events_after(*s, 0, 10_000)
                        .unwrap()
                        .into_iter()
                        .skip(skip(s))
                        .collect();
                    let mut names = std::collections::HashMap::new();
                    for e in &log {
                        match e.typed() {
                            Ok(EventKind::ToolCallStarted(c)) => {
                                names.insert(c.call_id, c.tool);
                            }
                            Ok(EventKind::ToolCallFinished(f)) => outputs.push((
                                *s,
                                names.get(&f.call_id).cloned().unwrap_or_default(),
                                f.output,
                            )),
                            _ => {}
                        }
                    }
                }
                return Turn {
                    outputs,
                    cards: answered.len(),
                };
            }
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The child sessions' recorded folders.
    fn child_folders(&self, parent: SessionId) -> Vec<PathBuf> {
        self.rt
            .store()
            .sessions()
            .unwrap()
            .into_iter()
            .filter(|s| s.session_id != parent)
            .map(|s| PathBuf::from(s.cwd))
            .collect()
    }
}

/// The writes of the read-only shell's own tests (cli/tests/read_only_shell.rs),
/// as one command: each must fail.
const WRITES: &str = "touch made.txt; echo x > made2.txt; echo y >> lib.rs; sed -i s/one/ONE/ lib.rs; \
     mkdir dir; rm lib.rs; mv lib.rs moved.rs; chmod 000 lib.rs; ln -s lib.rs link; ln lib.rs hard; \
     cat > here.txt <<EOF\nhi\nEOF\n git add -A; git commit -qm sneaky; git checkout -b other; \
     git update-ref refs/heads/x HEAD; python3 -c \"open('py.txt','w').write('x')\"; \
     echo t > \"$TMPDIR/t\" && cat \"$TMPDIR/t\"; echo end";

/// Scout runs `git log` and a pipeline, and every write fails, in a
/// sandbox chat in Auto and in a host chat in Ask: the same read-only
/// shell, contained (a host chat's too), no card for any of its commands,
/// the parent's folder (with its `.git`) byte for byte as it was, Scout
/// in the parent's folder, and no merge-back in the parent's result.
#[tokio::test]
async fn scout_reads_and_writes_nothing_in_every_mode() {
    let Some(mut n) = node(|_| {}).await else {
        return;
    };
    for (exec, mode) in [
        (ExecMode::Sandbox, ApprovalMode::Auto),
        (ExecMode::Host, ApprovalMode::Ask),
        (ExecMode::Sandbox, ApprovalMode::AcceptEdits),
    ] {
        let sid = n.create(exec, mode).await;
        let before = tree(&n.ws);
        let task = json!([
            {"tool": "bash", "input": {"command": "git log --oneline | wc -l; git log -1 --format=%s; echo pid=$$"}},
            {"tool": "bash", "input": {"command": "grep -rn fn . --include=*.rs | sort | uniq -c | wc -l; git diff HEAD~1 --stat"}},
            {"tool": "bash", "input": {"command": WRITES}},
        ]);
        let t = n
            .run(
                sid,
                lead(vec![scout()]),
                &delegate("scout", &format!("script: {task}")),
            )
            .await;
        let bash = t.of("bash");
        assert_eq!(bash.len(), 3, "{exec:?}: {}", t.all());
        assert!(
            bash[0].starts_with("2\nadd two\npid=2"),
            "{exec:?}: {}",
            bash[0]
        );
        assert!(
            bash[1].starts_with("2\n") && bash[1].contains("lib.rs | 1 +"),
            "{exec:?}: {}",
            bash[1]
        );
        assert!(
            bash[2].contains("Read-only file system"),
            "{exec:?}: {}",
            bash[2]
        );
        assert!(
            bash[2].contains("\nt\nend"),
            "{exec:?}: /tmp is writable: {}",
            bash[2]
        );
        assert_eq!(
            tree(&n.ws),
            before,
            "{exec:?}: the folder changed: {}",
            t.all()
        );
        assert_eq!(
            t.cards, 0,
            "{exec:?} {mode:?}: a card for a read-only command"
        );
        let delegate = t.of("delegate");
        assert!(
            delegate[0].contains("Sub-agent @scout finished: done."),
            "{}",
            delegate[0]
        );
        assert!(
            !delegate[0].contains("applied"),
            "a merge-back: {}",
            delegate[0]
        );
        for f in n.child_folders(sid) {
            assert_eq!(f.canonicalize().unwrap(), n.ws.canonicalize().unwrap());
        }
        assert!(
            !n.data.join("sessions").exists()
                || std::fs::read_dir(n.data.join("sessions")).unwrap().count() == 0,
            "a sub-agent folder was made"
        );
    }
}

/// A spec from a hostile master cannot get Scout a writable shell: keys
/// that ask for one are dropped (there is no such field), and the node
/// judges by the tools it offers; one that only reads gets the read-only
/// shell. In plan mode Scout with its shell is still started, and its
/// shell is the same one.
#[tokio::test]
async fn no_spec_gets_a_reader_a_writable_shell_and_plan_mode_starts_it() {
    let Some(mut n) = node(|_| {}).await else {
        return;
    };
    let mut hostile = serde_json::to_value(scout()).unwrap();
    hostile["read_only_shell"] = json!(false);
    hostile["writable_shell"] = json!(true);
    hostile["features"] = json!({"commands": true, "read_only_commands": false});
    let hostile: AgentSpec = serde_json::from_value(hostile).unwrap();
    let sid = n.create(ExecMode::Host, ApprovalMode::Auto).await;
    let t = n
        .run(
            sid,
            lead(vec![hostile]),
            &delegate("scout", "run: touch hostile.txt; echo rc=$?"),
        )
        .await;
    assert!(!n.ws.join("hostile.txt").exists(), "{}", t.all());
    assert!(
        t.of("bash")[0].contains("Read-only file system"),
        "{}",
        t.all()
    );

    n.set_mode(sid, ApprovalMode::Plan).await;
    let t = n
        .run(
            sid,
            lead(vec![scout()]),
            &delegate("scout", "run: git log --oneline | head -1; touch p.txt"),
        )
        .await;
    assert!(
        t.of("delegate")[0].contains("finished: done"),
        "{}",
        t.all()
    );
    assert!(t.of("bash")[0].contains("add two"), "{}", t.all());
    assert!(!n.ws.join("p.txt").exists());
}

/// The node's `denied_tools = ["bash"]` still removes it: refused by the
/// node's policy, nothing run, and Scout is still a reader in the parent's
/// folder.
#[tokio::test]
async fn denied_tools_still_remove_the_shell() {
    let Some(mut n) = node(|p| p.denied_tools = vec!["bash".into()]).await else {
        return;
    };
    let sid = n.create(ExecMode::Sandbox, ApprovalMode::Auto).await;
    let t = n
        .run(
            sid,
            lead(vec![scout()]),
            &delegate("scout", "run: echo ran > ran.txt"),
        )
        .await;
    let bash = t.of("bash");
    assert!(bash.iter().all(|o| !o.contains("ran")), "{}", t.all());
    assert!(t.all().contains("denied"), "{}", t.all());
    assert!(!n.ws.join("ran.txt").exists());
    for f in n.child_folders(sid) {
        assert_eq!(f.canonicalize().unwrap(), n.ws.canonicalize().unwrap());
    }
}

/// A top-level read-only agent's background command runs in the read-only
/// shell too (it cannot write), and `bash_output` reads it. The repeated
/// -call guard counts its `bash` as no change: four identical reads, a
/// command, and the fifth read is refused (a writable shell's command
/// would have started the count again).
#[tokio::test]
async fn a_readers_background_command_and_the_repeated_call_guard() {
    let Some(mut n) = node(|_| {}).await else {
        return;
    };
    let reader = AgentSpec {
        agent_id: "looker".into(),
        name: "Looker".into(),
        tools: ToolSelection::List(
            ["read", "grep", "bash", "bash_output", "bash_stop"]
                .map(String::from)
                .to_vec(),
        ),
        ..scout()
    };
    let sid = n.create(ExecMode::Sandbox, ApprovalMode::Auto).await;
    let script = json!([
        {"tool": "bash", "input": {"command": "sleep 4; touch bg.txt; echo rc=$?", "background": true, "timeout_secs": 30}},
        {"tool": "bash", "input": {"command": "sleep 3"}},
        {"tool": "bash_output", "input": {"id": "bg-1"}},
    ]);
    let t = n
        .run(sid, reader.clone(), &format!("script: {script}"))
        .await;
    assert!(!n.ws.join("bg.txt").exists(), "{}", t.all());
    let out = t.of("bash_output");
    assert!(out[0].contains("Read-only file system"), "{}", t.all());

    let read = json!({"tool": "read", "input": {"path": "lib.rs"}});
    let script = json!([
        read, read, read, read,
        {"tool": "bash", "input": {"command": "echo looked"}},
        read,
    ]);
    let t = n.run(sid, reader, &format!("script: {script}")).await;
    let reads = t.of("read");
    let last = reads.last().unwrap();
    assert!(last.starts_with("Not run: this exact call"), "{}", t.all());
}

/// A follow-up to Scout (PR 127's `session`) is still Scout in the
/// read-only shell: same child, the parent's folder, `git log` works,
/// writes fail, nothing is merged back.
#[tokio::test]
async fn a_continued_scout_is_still_read_only() {
    let Some(mut n) = node(|_| {}).await else {
        return;
    };
    let sid = n.create(ExecMode::Sandbox, ApprovalMode::Auto).await;
    let t = n
        .run(
            sid,
            lead(vec![scout()]),
            &delegate("scout", "run: git log --oneline | wc -l"),
        )
        .await;
    let first = t.of("delegate")[0].to_string();
    let child = first
        .strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .unwrap()
        .to_string();
    let before = tree(&n.ws);
    let call = json!([{"tool": "delegate", "input": {
        "agent": "scout", "session": child,
        "task": "run: touch again.txt; git log -1 --format=%s"
    }}]);
    let t = n
        .run(sid, lead(vec![scout()]), &format!("script: {call}"))
        .await;
    let out = t.of("delegate")[0];
    assert!(out.starts_with(&format!("session: {child}")), "{out}");
    assert!(!out.contains("applied"), "a merge-back: {out}");
    // The child existed before this turn: its follow-up's results are read
    // from its own log.
    let cid: SessionId = child.parse().unwrap();
    let last =
        n.rt.store()
            .events_after(cid, 0, 10_000)
            .unwrap()
            .into_iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(f)) => Some(f.output),
                _ => None,
            })
            .next_back()
            .unwrap();
    assert!(last.contains("Read-only file system"), "{last}");
    assert!(last.contains("add two"), "{last}");
    assert_eq!(tree(&n.ws), before);
    for f in n.child_folders(sid) {
        assert_eq!(f.canonicalize().unwrap(), n.ws.canonicalize().unwrap());
    }
}
