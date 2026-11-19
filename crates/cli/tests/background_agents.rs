//! Background sub-agents (ADOPTION_PLAN X.24, PLAN.md T72): `delegate` with
//! `background: true` returns at once; the child runs as the same child
//! session a foreground `delegate` makes, held as an entry of its chat's
//! background table; its end reaches the lead once, at a step boundary of
//! a running turn or as a callback turn, with a writer's changes merged
//! back when the end is delivered.
//!
//! An in-process node over the protocol, with the real contained shell
//! (bubblewrap; skipped with a printed reason where it does not work, and
//! failing with `HARNESS_REQUIRE_BWRAP=1`), the scripted agent and a
//! recording provider. Children are held on files the test creates (no
//! time margins on this slow disk). The real binary (a `kill -9` mid-child,
//! the master's own callback) is at the end of this file.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("bg-agents-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// The contained sandbox under `data`, or `None` (and a printed reason)
/// where it cannot run here; with `HARNESS_REQUIRE_BWRAP=1` it fails.
fn contained(data: &Path) -> Option<Arc<Sandbox>> {
    std::fs::create_dir_all(data.join("sandbox-tmp")).unwrap();
    let sb = Sandbox::new(
        PathBuf::from(EXE),
        &Settings::default(),
        Some(data.join("sandbox-tmp")),
        std::slice::from_ref(&data.to_path_buf()),
    )
    .unwrap();
    if let Err(e) = sb.probe(&data.join("sandbox-tmp/probe"), data) {
        assert!(!bwrap::required(), "Landlock does not work here: {e}");
        eprintln!("SKIPPED: Landlock does not work here: {e}");
        return None;
    }
    let sb = bwrap::contained(sb, &data.join("sandbox-tmp/probe-contain")).ok()?;
    Some(Arc::new(sb))
}

/// A command held until `file` exists in the chat's folder.
fn held(file: &str, then: &str) -> String {
    format!("while [ ! -e {file} ]; do sleep 0.05; done; {then}")
}

/// The lead: may delegate to `look` (reads: `read`, a read-only `bash`)
/// and `dev` (changes files: `read`, `write`), in the background when
/// `background`.
fn lead(background: bool) -> Value {
    json!({"agent_id": "lead", "name": "Lead", "system_prompt": "You are @lead.",
           "tools": ["bash", "bash_output", "bash_stop", "read", "write"], "prompt_hash": "h1",
           "caps": {"max_subagents": 4, "max_tokens": 200000, "background": background},
           "delegates": [
               {"agent_id": "look", "name": "Look", "system_prompt": "You are @look.",
                "tools": ["read", "bash"], "prompt_hash": "h2"},
               {"agent_id": "dev", "name": "Dev", "system_prompt": "You are @dev.",
                "tools": ["read", "write"], "prompt_hash": "h3"},
               {"agent_id": "dev2", "name": "Dev2", "system_prompt": "You are @dev2.",
                "tools": ["read", "write", "bash"], "prompt_hash": "h4"}]})
}

fn bg_delegate(agent: &str, task: &str) -> Value {
    json!({"tool": "delegate", "input": {"agent": agent, "task": task, "background": true}})
}

fn bash(cmd: &str) -> Value {
    json!({"tool": "bash", "input": {"command": cmd}})
}

fn write(path: &str, content: &str) -> Value {
    json!({"tool": "write", "input": {"path": path, "content": content}})
}

/// Our own processes whose command line holds `marker`.
fn procs(marker: &str) -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .map(|c| String::from_utf8_lossy(&c).replace('\0', " "))
                .is_ok_and(|c| c.contains(marker))
        })
        .filter(|p| {
            std::fs::read_to_string(format!("/proc/{p}/stat"))
                .map(|s| {
                    let st = s.rsplit(')').next().unwrap_or("").trim_start();
                    !(st.starts_with('Z') || st.starts_with('X'))
                })
                .unwrap_or(false)
        })
        .collect()
}

async fn gone(marker: &str, how: &str) {
    let by = Instant::now() + Duration::from_secs(20);
    while !procs(marker).is_empty() {
        assert!(Instant::now() < by, "{how}: {marker} still runs");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn appears(marker: &str) {
    let by = Instant::now() + Duration::from_secs(60);
    while procs(marker).is_empty() {
        assert!(Instant::now() < by, "{marker} never started");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// ---- the node over the protocol ----

struct Node {
    _dir: tempfile::TempDir,
    data: PathBuf,
    ws: PathBuf,
    _rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
    seen: Vec<RawEvent>,
    n: u32,
    requests: Arc<std::sync::Mutex<Vec<harness_agent_native::ChatRequest>>>,
}

struct Spy(
    harness_agent_native::mock::Mock,
    Arc<std::sync::Mutex<Vec<harness_agent_native::ChatRequest>>>,
);

#[async_trait::async_trait]
impl harness_agent_native::Provider for Spy {
    fn name(&self) -> &'static str {
        "spy"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::ChatRequest,
        deltas: mpsc::UnboundedSender<String>,
    ) -> Result<harness_agent_native::Completion, harness_agent_native::ProviderError> {
        self.1.lock().unwrap().push(req.clone());
        self.0.complete(req, deltas).await
    }
}

async fn node() -> Option<Node> {
    node_with(|_| {}).await
}

async fn node_with(edit: impl FnOnce(&mut harness_node::NodePolicy)) -> Option<Node> {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    let dir = tempdir();
    let root = dir.path().canonicalize().unwrap();
    let (data, ws) = (root.join("data"), root.join("home/proj"));
    std::fs::create_dir_all(&ws).unwrap();
    let sb = contained(&data)?;
    let store =
        harness_store::Store::open(&data.join("node.db"), harness_store::Durability::Full).unwrap();
    let mut policy = harness_node::NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        skills: harness_node::SkillsPolicy {
            roots: vec![],
            ..Default::default()
        },
        ..Default::default()
    };
    edit(&mut policy);
    // A test's own short wall clock (below node.toml's 60 s floor, which
    // `validate` keeps for every real configuration).
    if policy.limits.background_agent_timeout_s >= 60 {
        policy.validate().unwrap();
    }
    let requests: Arc<std::sync::Mutex<Vec<harness_agent_native::ChatRequest>>> = Arc::default();
    let backend = NativeBackend::new(
        Arc::new(Spy(
            Mock {
                delay: Duration::from_millis(1),
            },
            requests.clone(),
        )),
        harness_tools::builtin(),
        NativeConfig {
            sandbox: Confinement::Confined {
                sandbox: sb,
                tmp: None,
            },
            ..Default::default()
        },
    );
    let rt = harness_node::NodeRuntime::start(harness_node::NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store,
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
    match link.from_node.recv().await.unwrap() {
        harness_protocol::NodeToMaster::Hello { .. } => {}
        other => panic!("{other:?}"),
    }
    Some(Node {
        _dir: dir,
        data,
        ws,
        _rt: rt,
        link,
        seen: vec![],
        n: 0,
        requests,
    })
}

impl Node {
    fn send(&self, m: Value) {
        let m: harness_protocol::MasterToNode = serde_json::from_value(m).unwrap();
        self.link.to_node.try_send(m).unwrap();
    }

    async fn create(&mut self, mode: &str) -> SessionId {
        let sid = SessionId::new();
        self.n += 1;
        self.send(
            json!({"type": "session.create", "req_id": format!("c{}", self.n),
            "session_id": sid, "title": "t", "cwd": self.ws.display().to_string(),
            "approval_mode": mode}),
        );
        self.until(move |e, k| e.session_id == sid && matches!(k, EventKind::SessionCreated(_)))
            .await;
        sid
    }

    /// One turn of `agent` running the scripted `steps`; does not wait.
    fn prompt_as(&mut self, sid: SessionId, agent: Value, steps: &[Value]) {
        self.n += 1;
        self.send(
            json!({"type": "session.prompt", "req_id": format!("p{}", self.n),
            "session_id": sid, "text": format!("script: {}", json!(steps)), "agent": agent}),
        );
    }

    fn prompt(&mut self, sid: SessionId, steps: &[Value]) {
        self.prompt_as(sid, lead(true), steps);
    }

    async fn callback(&mut self, sid: SessionId, ids: &[&str]) -> Result<(), String> {
        self.n += 1;
        let req = format!("cb{}", self.n);
        self.send(
            json!({"type": "session.callback", "req_id": req, "session_id": sid,
            "bg_ids": ids, "agent": lead(true)}),
        );
        loop {
            match self.next().await {
                harness_protocol::NodeToMaster::Ack { req_id, ok, error } if req_id == req => {
                    return if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
                _ => {}
            }
        }
    }

    async fn next(&mut self) -> harness_protocol::NodeToMaster {
        let m = tokio::time::timeout(Duration::from_secs(120), self.link.from_node.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "timed out; the last events: {:#?}",
                    self.seen.iter().rev().take(8).collect::<Vec<_>>()
                )
            })
            .expect("link closed");
        if let harness_protocol::NodeToMaster::Event { event } = &m {
            self.seen.push(event.clone());
        }
        m
    }

    async fn until(&mut self, pred: impl Fn(&RawEvent, &EventKind) -> bool) {
        if self
            .seen
            .iter()
            .any(|e| e.typed().is_ok_and(|k| pred(e, &k)))
        {
            return;
        }
        loop {
            if let harness_protocol::NodeToMaster::Event { event } = self.next().await {
                let k = event.typed().unwrap();
                if pred(&event, &k) {
                    return;
                }
            }
        }
    }

    /// Wait for the `n`-th `turn_ended` of `sid`.
    async fn turns_ended(&mut self, sid: SessionId, n: usize) {
        loop {
            let have = self
                .of(sid)
                .iter()
                .filter(|k| matches!(k, EventKind::TurnEnded(_)))
                .count();
            if have >= n {
                return;
            }
            self.next().await;
        }
    }

    async fn ended(&mut self, sid: SessionId, bg: &str) -> BackgroundEnded {
        let bg = bg.to_string();
        let b2 = bg.clone();
        self.until(move |e, k| {
            e.session_id == sid && matches!(k, EventKind::BackgroundEnded(b) if b.bg_id == b2)
        })
        .await;
        self.ends(sid).into_iter().find(|b| b.bg_id == bg).unwrap()
    }

    fn of(&self, sid: SessionId) -> Vec<EventKind> {
        self.seen
            .iter()
            .filter(|e| e.session_id == sid)
            .map(|e| e.typed().unwrap())
            .collect()
    }

    fn ends(&self, sid: SessionId) -> Vec<BackgroundEnded> {
        self.of(sid)
            .into_iter()
            .filter_map(|k| match k {
                EventKind::BackgroundEnded(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    fn starts(&self, sid: SessionId) -> Vec<BackgroundStarted> {
        self.of(sid)
            .into_iter()
            .filter_map(|k| match k {
                EventKind::BackgroundStarted(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    /// The outputs of `sid`'s `delegate` and `bash_stop` calls, in order.
    fn outputs(&self, sid: SessionId, tool: &str) -> Vec<String> {
        let evs = self.of(sid);
        let calls: Vec<String> = evs
            .iter()
            .filter_map(|k| match k {
                EventKind::ToolCallStarted(c) if c.tool == tool => Some(c.call_id.clone()),
                _ => None,
            })
            .collect();
        evs.iter()
            .filter_map(|k| match k {
                EventKind::ToolCallFinished(f) if calls.contains(&f.call_id) => {
                    Some(f.output.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The child the `i`-th background start of `sid` made.
    fn child(&self, sid: SessionId, i: usize) -> SessionId {
        self.starts(sid)[i].child.as_ref().unwrap().session
    }

    async fn drain(&mut self) {
        while let Ok(Some(m)) =
            tokio::time::timeout(Duration::from_millis(500), self.link.from_node.recv()).await
        {
            if let harness_protocol::NodeToMaster::Event { event } = m {
                self.seen.push(event);
            }
        }
    }

    fn git(&self, args: &[&str]) {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.ws)
            .args([
                "-c",
                "commit.gpgSign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            // Nor the global ignore file (`~/.config/git/ignore`).
            .env("XDG_CONFIG_HOME", "/nonexistent")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "user")
            .env("GIT_AUTHOR_EMAIL", "user@example.com")
            .env("GIT_COMMITTER_NAME", "user")
            .env("GIT_COMMITTER_EMAIL", "user@example.com")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    /// The folder a repository of `a.txt` and `b.txt`.
    fn repo(&self) {
        self.git(&["init", "-q"]);
        std::fs::write(self.ws.join("a.txt"), "alpha\n").unwrap();
        std::fs::write(self.ws.join("b.txt"), "beta\n").unwrap();
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "init"]);
    }

    /// Sub-agents' own folders the node still holds:
    /// `<data>/sessions/<child>/files` (review F5: this looked in
    /// `checkpoints` before and could never find one).
    fn child_folders(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.data.join("sessions"))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path().join("files"))
                    .filter(|p| p.exists())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Until no sub-agent's own folder is left (their removal follows the
    /// end), at most 30 s.
    async fn no_child_folders(&self) {
        let by = Instant::now() + Duration::from_secs(30);
        while !self.child_folders().is_empty() {
            assert!(
                Instant::now() < by,
                "left behind: {:?}",
                self.child_folders()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn texts(m: &[harness_agent_native::ChatMsg]) -> Vec<String> {
    use harness_agent_native::ChatMsg;
    m.iter()
        .map(|m| match m {
            ChatMsg::Tool { content, .. } => content.clone(),
            ChatMsg::Assistant { text, .. } => text.clone(),
            ChatMsg::Note(t) => t.clone(),
            other => other.user_text().unwrap_or_default().to_string(),
        })
        .collect()
}

/// The call returns at once and the lead's turn goes on and ends while the
/// child still works; the child's end, while the chat is idle, is recorded
/// once with its result and waits for a callback turn; the callback turn's
/// opening carries the result, which the person's next turn rebuilds byte
/// for byte from the log (T59).
#[tokio::test(flavor = "multi_thread")]
async fn the_call_returns_at_once_and_the_result_calls_back_once() {
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    std::fs::write(n.ws.join("notes.txt"), "the answer is 42\n").unwrap();
    n.prompt(
        sid,
        &[
            bg_delegate("look", &format!("run: {}", held("go", "cat notes.txt"))),
            bash("echo lead-goes-on"),
        ],
    );
    n.turns_ended(sid, 1).await;
    // The turn ended while the child is held: it ran on.
    assert!(n.ends(sid).is_empty(), "{:?}", n.ends(sid));
    let starts = n.starts(sid);
    assert_eq!(starts.len(), 1, "{starts:?}");
    let child = n.child(sid, 0);
    let out = n.outputs(sid, "delegate");
    assert!(
        out[0].starts_with(&format!("session: {child}\n")),
        "{out:?}"
    );
    assert!(
        out[0].contains("started in the background as bg-1"),
        "{out:?}"
    );
    let bash_out = n.outputs(sid, "bash");
    assert!(bash_out[0].contains("lead-goes-on"), "{bash_out:?}");
    // The child is the same kind of child session.
    assert!(n.of(child).iter().any(|k| matches!(k,
        EventKind::SessionCreated(c) if c.parent_session == Some(sid) && c.no_callbacks)));
    std::fs::write(n.ws.join("go"), "").unwrap();
    let end = n.ended(sid, "bg-1").await;
    assert_eq!(end.reason, BackgroundReason::Finished, "{end:?}");
    assert!(end.calls_back() && !end.delivered);
    let c = end.child.as_ref().unwrap();
    assert_eq!((c.session, c.agent_id.as_str()), (child, "look"));
    assert!(
        c.result
            .starts_with(&format!("session: {child}\nSub-agent @look finished: done")),
        "{}",
        c.result
    );
    assert!(c.ok && c.usage.is_some());
    let note = end.model_note().unwrap();
    assert!(
        note.starts_with("[harness] Background sub-agent bg-1 (@look) finished after"),
        "{note}"
    );
    let before = n.requests.lock().unwrap().len();
    n.callback(sid, &["bg-1"]).await.unwrap();
    // Asked again (a master that retries): refused, the turn has it.
    assert!(n.callback(sid, &["bg-1"]).await.is_err());
    n.turns_ended(sid, 2).await;
    let reqs = n.requests.lock().unwrap().clone();
    let opening = texts(&reqs[before].messages).last().unwrap().clone();
    assert!(opening.starts_with(CALLBACK_LEAD), "{opening}");
    assert!(opening.contains(&note), "{opening}");
    // The person's next turn: the same opening, rebuilt from the log.
    n.prompt(sid, &[bash("true")]);
    n.turns_ended(sid, 3).await;
    let reqs = n.requests.lock().unwrap().clone();
    let later = texts(&reqs.last().unwrap().messages);
    assert!(later.contains(&opening), "not rebuilt the same: {later:#?}");
    assert_eq!(n.ends(sid).len(), 1, "one end in all");
    let turns = n
        .of(sid)
        .iter()
        .filter(|k| {
            matches!(
                k,
                EventKind::TurnStarted(TurnStarted { cause: Some(_), .. })
            )
        })
        .count();
    assert_eq!(turns, 1, "one callback turn");
}

/// A lead that is busy reads the end at its next step boundary, once
/// (`delivered`), and no callback is due for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_busy_lead_reads_the_end_at_a_step_boundary_once() {
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    n.prompt(
        sid,
        &[
            bg_delegate("look", &format!("run: {}", held("go", "echo child-saw-it"))),
            bash(&format!("touch go; {}", held("release", "echo released"))),
            bash("sleep 0.5"),
            bash("true"),
        ],
    );
    n.until(move |e, k| e.session_id == sid && matches!(k, EventKind::BackgroundStarted(_)))
        .await;
    let child = n.child(sid, 0);
    n.turns_ended(child, 1).await;
    std::fs::write(n.ws.join("release"), "").unwrap();
    n.turns_ended(sid, 1).await;
    let ends = n.ends(sid);
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert!(ends[0].delivered && !ends[0].calls_back(), "{:?}", ends[0]);
    // In the log before the lead's end, and in its next request once.
    let evs = n.of(sid);
    let at_end = evs
        .iter()
        .position(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .unwrap();
    let turn_end = evs
        .iter()
        .rposition(|k| matches!(k, EventKind::TurnEnded(_)))
        .unwrap();
    assert!(at_end < turn_end);
    let note = ends[0].model_note().unwrap();
    let reqs = n.requests.lock().unwrap().clone();
    let last_lead = reqs
        .iter()
        .rfind(|r| matches!(r.messages.first(), Some(harness_agent_native::ChatMsg::System(s)) if s.contains("You are @lead.")))
        .unwrap();
    let seen = texts(&last_lead.messages)
        .iter()
        .filter(|t| t.contains(&note))
        .count();
    assert_eq!(seen, 1, "{:#?}", texts(&last_lead.messages));
    assert!(
        n.callback(sid, &["bg-1"]).await.is_err(),
        "a delivered end calls nothing back"
    );
}

/// Two children at once: each end is recorded once, and one callback turn
/// names both.
#[tokio::test(flavor = "multi_thread")]
async fn two_children_end_together_into_one_callback_turn() {
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    n.prompt(
        sid,
        &[
            bg_delegate("look", &format!("run: {}", held("go", "echo one"))),
            bg_delegate("look", &format!("run: {}", held("go", "echo two"))),
        ],
    );
    n.turns_ended(sid, 1).await;
    assert_eq!(n.starts(sid).len(), 2);
    std::fs::write(n.ws.join("go"), "").unwrap();
    n.ended(sid, "bg-1").await;
    n.ended(sid, "bg-2").await;
    n.callback(sid, &["bg-1", "bg-2"]).await.unwrap();
    n.turns_ended(sid, 2).await;
    n.drain().await;
    assert_eq!(n.ends(sid).len(), 2);
    let cause = n.of(sid).into_iter().find_map(|k| match k {
        EventKind::TurnStarted(TurnStarted { cause: Some(c), .. }) => Some(c),
        _ => None,
    });
    assert_eq!(
        cause,
        Some(TurnCause::Callback(vec!["bg-1".into(), "bg-2".into()]))
    );
}

/// The node's cap on running children per chat: the next start is refused
/// naming the running ones and the node.toml key; an agent spec asking
/// for more sub-agents per turn does not raise it; the ended ones free
/// their places.
#[tokio::test(flavor = "multi_thread")]
async fn the_nodes_cap_refuses_the_next_start_and_the_spec_cannot_raise_it() {
    let Some(mut n) = node_with(|p| p.limits.background_agents_per_chat = 2).await else {
        return;
    };
    let sid = n.create("auto").await;
    let mut greedy = lead(true);
    greedy["caps"]["max_subagents"] = json!(16);
    greedy["caps"]["background_agents_per_chat"] = json!(99);
    let task = format!("run: {}", held("go", "true"));
    n.prompt_as(
        sid,
        greedy,
        &[
            bg_delegate("look", &task),
            bg_delegate("look", &task),
            bg_delegate("look", &task),
        ],
    );
    n.turns_ended(sid, 1).await;
    let out = n.outputs(sid, "delegate");
    assert_eq!(out.len(), 3, "{out:#?}");
    let refused: Vec<&String> = out
        .iter()
        .filter(|o| o.contains("background_agents_per_chat = 2"))
        .collect();
    assert_eq!(refused.len(), 1, "{out:#?}");
    assert!(
        refused[0].contains("bg-"),
        "names the running ones: {}",
        refused[0]
    );
    assert_eq!(n.starts(sid).len(), 2);
    std::fs::write(n.ws.join("go"), "").unwrap();
    n.ended(sid, "bg-1").await;
    n.ended(sid, "bg-2").await;
    // Room again.
    std::fs::remove_file(n.ws.join("go")).unwrap();
    n.prompt(sid, &[bg_delegate("look", &task)]);
    n.turns_ended(sid, 2).await;
    assert_eq!(n.starts(sid).len(), 3);
    std::fs::write(n.ws.join("go"), "").unwrap();
    n.ended(sid, "bg-3").await;
}

/// The agent's switch off, or the node's key at 0: `delegate` offers no
/// `background`, and a forged one is refused with the reason before any
/// child exists.
#[tokio::test(flavor = "multi_thread")]
async fn without_the_switch_or_the_nodes_yes_nothing_is_offered_and_a_forged_start_is_refused() {
    for (key, switched, want) in [
        (4, false, "\"Background sub-agents\" switch is off"),
        (0, true, "background_agents_per_chat = 0"),
    ] {
        let Some(mut n) = node_with(|p| p.limits.background_agents_per_chat = key).await else {
            return;
        };
        let sid = n.create("auto").await;
        n.prompt_as(sid, lead(switched), &[bg_delegate("look", "run: true")]);
        n.turns_ended(sid, 1).await;
        let out = n.outputs(sid, "delegate");
        assert!(out[0].contains(want), "{key}/{switched}: {out:?}");
        assert!(n.starts(sid).is_empty());
        assert_eq!(
            n.seen
                .iter()
                .filter(|e| e.kind == "session_created")
                .count(),
            1,
            "a child was made"
        );
        let reqs = n.requests.lock().unwrap().clone();
        let delegate = reqs[0].tools.iter().find(|t| t.name == "delegate").unwrap();
        assert!(
            delegate.schema["properties"].get("background").is_none(),
            "{:?}",
            delegate.schema
        );
        assert!(!delegate.description.contains("background: true"));
    }
    // And with both: it is offered.
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    n.prompt(sid, &[bash("true")]);
    n.turns_ended(sid, 1).await;
    let reqs = n.requests.lock().unwrap().clone();
    let delegate = reqs[0].tools.iter().find(|t| t.name == "delegate").unwrap();
    assert!(delegate.schema["properties"].get("background").is_some());
}

/// The person's Stop of one, the agent's `bash_stop`, and the chat's
/// deletion each end the child and what it runs; the stops are recorded
/// with their reason and the child's result, and call nothing back.
#[tokio::test(flavor = "multi_thread")]
async fn stops_and_a_delete_end_them_and_leave_nothing_running() {
    let Some(mut n) = node().await else { return };
    n.repo();
    let sid = n.create("auto").await;
    n.prompt(sid, &[bg_delegate("look", "run: sleep 977301")]);
    n.turns_ended(sid, 1).await;
    appears("sleep 977301").await;
    n.send(
        json!({"type": "session.stop_background", "req_id": "s1", "session_id": sid,
        "bg_id": "bg-1", "by": "user"}),
    );
    let end = n.ended(sid, "bg-1").await;
    assert_eq!(end.reason, BackgroundReason::StoppedByUser);
    assert!(!end.calls_back());
    assert!(
        end.child
            .as_ref()
            .unwrap()
            .result
            .contains("Sub-agent @look finished: failed"),
        "{end:?}"
    );
    gone("sleep 977301", "the person's stop").await;
    // The agent's own stop, in the same turn: its result says how, and the
    // end reaches it at the next step.
    n.prompt(
        sid,
        &[
            bg_delegate("look", "run: sleep 977302"),
            bash("sleep 1"),
            json!({"tool": "bash_stop", "input": {"id": "bg-2"}}),
            bash("true"),
        ],
    );
    n.turns_ended(sid, 2).await;
    let stop = n.outputs(sid, "bash_stop");
    assert!(
        stop[0].contains("bg-2 was stopped by the agent"),
        "{stop:?}"
    );
    let end = n.ended(sid, "bg-2").await;
    assert_eq!(end.reason, BackgroundReason::StoppedByAgent);
    assert!(end.model_note().unwrap().contains("was stopped by you"));
    gone("sleep 977302", "bash_stop").await;
    // The chat's deletion, with a writer that has its own folder.
    n.prompt(sid, &[bg_delegate("dev2", "run: sleep 977303")]);
    n.turns_ended(sid, 3).await;
    appears("sleep 977303").await;
    let child = n.child(sid, 2);
    assert_eq!(
        n.child_folders(),
        vec![
            n.data
                .join("sessions")
                .join(child.to_string())
                .join("files")
        ]
    );
    n.send(json!({"type": "session.delete", "req_id": "d1", "session_id": sid}));
    gone("sleep 977303", "the chat's deletion").await;
    n.drain().await;
    n.no_child_folders().await;
}

/// A child that changes files works in its own copy and its changes are
/// merged back when its end is delivered, over the lead's own change of
/// another file; one whose change meets the lead's change of the same
/// line is reported as not applied (the lead's file stays), with its diff,
/// and its folder is not left behind.
#[tokio::test(flavor = "multi_thread")]
async fn a_writers_changes_merge_back_at_delivery_and_a_conflict_is_reported() {
    let Some(mut n) = node().await else { return };
    n.repo();
    let sid = n.create("auto").await;
    let child_task = format!("script: {}", json!([write("c.txt", "from the child\n")]));
    n.prompt(
        sid,
        &[
            bg_delegate("dev", &child_task),
            bash("echo beta-lead > b.txt"),
        ],
    );
    n.turns_ended(sid, 1).await;
    let end = n.ended(sid, "bg-1").await;
    n.drain().await;
    let c = end.child.as_ref().unwrap();
    assert!(c.ok, "{}", c.result);
    assert_eq!(
        std::fs::read_to_string(n.ws.join("c.txt")).unwrap(),
        "from the child\n"
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("b.txt")).unwrap(),
        "beta-lead\n"
    );
    // The same line on both sides.
    let child_task = format!(
        "script: {}",
        json!([{"tool": "read", "input": {"path": "a.txt"}}, write("a.txt", "alpha-child\n")])
    );
    n.prompt(
        sid,
        &[
            bg_delegate("dev", &child_task),
            bash("echo alpha-lead > a.txt"),
        ],
    );
    n.turns_ended(sid, 2).await;
    let end = n.ended(sid, "bg-2").await;
    n.drain().await;
    let c = end.child.as_ref().unwrap();
    assert!(!c.ok, "{}", c.result);
    assert!(
        c.result.ends_with(harness_core::subagent::NOT_APPLIED_LAST),
        "{}",
        c.result
    );
    assert!(
        c.result.contains("| +alpha-child"),
        "the diff, quoted: {}",
        c.result
    );
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "alpha-lead\n"
    );
    n.no_child_folders().await;
}

/// Plan mode: a child that can write is not started in the background
/// either; one that only reads is, made in plan mode for good.
#[tokio::test(flavor = "multi_thread")]
async fn plan_mode_starts_only_readers_and_they_stay_in_it() {
    let Some(mut n) = node().await else { return };
    n.repo();
    let sid = n.create("plan").await;
    n.prompt(
        sid,
        &[
            bg_delegate("dev", "script: []"),
            bg_delegate("look", &format!("run: {}", held("go", "true"))),
        ],
    );
    n.turns_ended(sid, 1).await;
    let out = n.outputs(sid, "delegate");
    assert!(
        out[0].contains("plan mode: @dev was not started"),
        "{out:?}"
    );
    assert!(out[1].contains("started in the background"), "{out:?}");
    let child = n.child(sid, 0);
    assert!(n.of(child).iter().any(|k| matches!(k,
        EventKind::SessionCreated(c) if c.approval_mode == harness_core::policy::ApprovalMode::Plan)));
    std::fs::write(n.ws.join("go"), "").unwrap();
    n.ended(sid, "bg-1").await;
}

/// The same child given a follow-up while it still runs is refused, and
/// once its end is in, the follow-up reaches it (T68).
#[tokio::test(flavor = "multi_thread")]
async fn a_follow_up_while_it_runs_is_refused_and_after_its_end_reaches_it() {
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    n.prompt(
        sid,
        &[bg_delegate("look", &format!("run: {}", held("go", "true")))],
    );
    n.turns_ended(sid, 1).await;
    let child = n.child(sid, 0);
    let follow = json!({"tool": "delegate", "input": {"agent": "look", "session": child.to_string(), "task": "read: notes.txt"}});
    n.prompt(sid, std::slice::from_ref(&follow));
    n.turns_ended(sid, 2).await;
    let out = n.outputs(sid, "delegate");
    assert!(
        out[1].contains("is still running in the background as bg-1"),
        "{out:?}"
    );
    std::fs::write(n.ws.join("go"), "").unwrap();
    std::fs::write(n.ws.join("notes.txt"), "noted\n").unwrap();
    n.ended(sid, "bg-1").await;
    n.prompt(sid, &[follow]);
    n.turns_ended(sid, 3).await;
    let out = n.outputs(sid, "delegate");
    assert!(
        out[2].starts_with(&format!("session: {child}\nSub-agent @look finished: done")),
        "{out:?}"
    );
}

/// The token caps a turn grants its background sub-agents (review F3, the
/// reviewer's R4 turned around): each start takes an equal share of what the
/// turn's sub-agent budget had left when its step began, reserved at once,
/// so the caps granted in a turn never add up to more than the budget; a
/// start with nothing left is refused with the reason, and no child is made.
#[tokio::test(flavor = "multi_thread")]
async fn the_caps_granted_in_a_turn_never_exceed_its_token_budget() {
    let Some(mut n) = node().await else { return };
    std::fs::write(n.ws.join("a.txt"), "alpha\n").unwrap();
    let sid = n.create("auto").await;
    let mut tight = lead(true);
    tight["caps"]["max_tokens"] = json!(60);
    let reads = format!(
        "script: {}",
        json!([
            {"tool": "read", "input": {"path": "a.txt"}},
            {"tool": "read", "input": {"path": "a.txt"}},
            {"tool": "read", "input": {"path": "a.txt"}}
        ])
    );
    n.prompt_as(
        sid,
        tight,
        &[
            bg_delegate("look", &reads),
            bg_delegate("look", &reads),
            bash("sleep 3"),
            bg_delegate("look", &reads),
            bash("true"),
        ],
    );
    n.turns_ended(sid, 1).await;
    let out = n.outputs(sid, "delegate");
    assert_eq!(out.len(), 3, "{out:#?}");
    let granted: Vec<u64> = out
        .iter()
        .filter_map(|o| {
            o.split("or at ")
                .nth(1)?
                .split(" tokens")
                .next()?
                .parse()
                .ok()
        })
        .collect();
    // 60 over 4 places, 45 over 3, 30 over 2.
    assert_eq!(granted, vec![15, 15, 15], "{out:#?}");
    assert!(granted.iter().sum::<u64>() <= 60);
    for id in ["bg-1", "bg-2", "bg-3"] {
        let e = n.ended(sid, id).await;
        let r = &e.child.as_ref().unwrap().result;
        assert!(
            r.contains("share of its parent turn's sub-agent budget (15)"),
            "{r}"
        );
    }
    // A budget too small to share: refused, nothing made.
    let mut tiny = lead(true);
    tiny["caps"]["max_tokens"] = json!(3);
    n.prompt_as(sid, tiny, &[bg_delegate("look", &reads)]);
    n.turns_ended(sid, 2).await;
    let out = n.outputs(sid, "delegate");
    assert!(
        out[3].contains("[harness] not started") && out[3].contains("token budget (3 of 3)"),
        "{out:#?}"
    );
    assert_eq!(
        n.starts(sid).len(),
        3,
        "a child was made for the refused start"
    );
}

/// A background start beside a foreground sub-agent of the same step is in
/// the log (its row, its Stop) while the foreground one still works (review
/// F1: it was logged only after every delegation of the step returned).
#[tokio::test(flavor = "multi_thread")]
async fn a_background_start_is_logged_while_a_foreground_one_of_its_step_runs() {
    let Some(mut n) = node().await else { return };
    let sid = n.create("auto").await;
    // One step: the foreground call first, the background one beside it.
    n.prompt(
        sid,
        &[json!([
            {"tool": "delegate", "input": {"agent": "look", "task": format!("run: {}", held("release", "echo fg"))}},
            bg_delegate("look", &format!("run: {}", held("go", "echo bg"))),
        ])],
    );
    let seen = tokio::time::timeout(
        Duration::from_secs(60),
        n.until(move |e, k| e.session_id == sid && matches!(k, EventKind::BackgroundStarted(_))),
    )
    .await;
    // Whatever happened, the foreground one is let go.
    std::fs::write(n.ws.join("release"), "").unwrap();
    assert!(
        seen.is_ok(),
        "no background_started while the foreground sub-agent ran"
    );
    let evs = n.of(sid);
    let at = evs
        .iter()
        .position(|k| matches!(k, EventKind::BackgroundStarted(_)))
        .unwrap();
    assert!(
        !evs[..at]
            .iter()
            .any(|k| matches!(k, EventKind::ToolCallFinished(_))),
        "the start came after a delegate result: {evs:#?}"
    );
    // The side panel's row: the fold has it running.
    let raw: Vec<RawEvent> = n
        .seen
        .iter()
        .filter(|e| e.session_id == sid)
        .cloned()
        .collect();
    let v = harness_core::view::SessionView::fold(&raw).unwrap();
    assert!(
        v.background
            .get("bg-1")
            .is_some_and(|b| b.child.is_some() && !b.ended)
    );
    n.turns_ended(sid, 1).await;
    std::fs::write(n.ws.join("go"), "").unwrap();
    n.ended(sid, "bg-1").await;
}

/// The end of a writer child whose append the log refuses once (review F4):
/// its merge ran once, the end is kept built and recorded once at the next
/// try (the person's next turn reads it at its step), carrying the result;
/// nothing is merged a second time.
#[tokio::test(flavor = "multi_thread")]
async fn an_end_the_log_refuses_once_is_merged_once_and_recorded_once() {
    let Some(mut n) = node().await else { return };
    n.repo();
    let sid = n.create("auto").await;
    harness_node::testing::fail_next_end_append(sid);
    // Held until the lead's turn is over, so its end is recorded between
    // turns (where the refusal is injected), not at a step of that turn.
    let child_task = format!(
        "script: {}",
        json!([
            bash(&held("go", "true")),
            {"tool": "read", "input": {"path": "a.txt"}},
            write("a.txt", "alpha-child\n")
        ])
    );
    n.prompt(sid, &[bg_delegate("dev2", &child_task)]);
    n.turns_ended(sid, 1).await;
    let child = n.child(sid, 0);
    // Its own copy of the folder has no `go`: written there.
    let own = n
        .data
        .join("sessions")
        .join(child.to_string())
        .join("files");
    let by = Instant::now() + Duration::from_secs(60);
    while !own.exists() {
        assert!(Instant::now() < by, "no own folder");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    std::fs::write(own.join("go"), "").unwrap();
    n.turns_ended(child, 1).await;
    // Merged (into the folder) although the end is not in the log.
    let by = Instant::now() + Duration::from_secs(30);
    while std::fs::read_to_string(n.ws.join("a.txt")).unwrap() != "alpha-child\n" {
        assert!(Instant::now() < by, "never merged");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    n.drain().await;
    assert!(n.ends(sid).is_empty(), "the refused end is in the log");
    // The person changes the file after the merge: a second merge would
    // meet it (and report it).
    std::fs::write(n.ws.join("a.txt"), "alpha-person\n").unwrap();
    n.prompt(sid, &[bash("true"), bash("true")]);
    n.turns_ended(sid, 2).await;
    n.drain().await;
    let ends = n.ends(sid);
    assert_eq!(ends.len(), 1, "{ends:#?}");
    let c = ends[0]
        .child
        .as_ref()
        .expect("the kept end lost its result");
    assert!(c.ok, "{}", c.result);
    assert!(!c.result.contains("NOT applied"), "{}", c.result);
    assert_eq!(
        std::fs::read_to_string(n.ws.join("a.txt")).unwrap(),
        "alpha-person\n",
        "merged a second time"
    );
    n.no_child_folders().await;
}

/// The wall-clock limit (the reviewer's R3), with a short limit set on the
/// policy the test builds (node.toml's floor is 60 s: `validate` refuses
/// less, and nothing over the protocol sets it): the child's running
/// command is killed, its end is `timed_out` and calls back; a card the
/// child waits on is resolved, and nothing is merged without a yes.
#[tokio::test(flavor = "multi_thread")]
async fn the_wall_clock_limit_ends_the_child_its_command_and_its_card() {
    // 30 s, not 10: the limit runs from the moment the child's task is
    // spawned, so it also covers the child's first model call, the tool
    // dispatch and the start of the sandboxed shell. On a loaded machine
    // that took longer than 10 s: the limit ended the child before its
    // command had started, and `appears` then waited for a process that
    // never came (it passed when run alone). Both halves below need the
    // child to get as far as its command or its card before the limit.
    let Some(mut n) = node_with(|p| p.limits.background_agent_timeout_s = 30).await else {
        return;
    };
    let sid = n.create("auto").await;
    n.prompt(sid, &[bg_delegate("look", "run: sleep 977531")]);
    n.turns_ended(sid, 1).await;
    appears("sleep 977531").await;
    let end = n.ended(sid, "bg-1").await;
    assert_eq!(end.reason, BackgroundReason::TimedOut);
    assert!(end.calls_back());
    gone("sleep 977531", "the wall clock").await;
    // A card: an `ask` chat whose writer child wants to write.
    n.repo();
    let sid2 = n.create("ask").await;
    n.prompt(
        sid2,
        &[bg_delegate(
            "dev2",
            &format!("script: {}", json!([write("z.txt", "z\n")])),
        )],
    );
    n.turns_ended(sid2, 1).await;
    let child = n.child(sid2, 0);
    n.until(move |e, k| e.session_id == child && matches!(k, EventKind::ApprovalRequested(_)))
        .await;
    let end = n.ended(sid2, "bg-1").await;
    assert_eq!(end.reason, BackgroundReason::TimedOut, "{end:?}");
    n.drain().await;
    let raw: Vec<RawEvent> = n
        .seen
        .iter()
        .filter(|e| e.session_id == child)
        .cloned()
        .collect();
    let v = harness_core::view::SessionView::fold(&raw).unwrap();
    assert!(v.pending_approvals.is_empty(), "{:?}", v.pending_approvals);
    assert!(v.running_turn.is_none());
    assert!(!n.ws.join("z.txt").exists(), "merged without the yes");
    n.no_child_folders().await;
}

// ---- the real binary: a `kill -9` mid-child, the master's callback ----

mod real {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    pub struct Serve {
        child: Option<Child>,
        pub addr: String,
        token: String,
        log: Arc<std::sync::Mutex<Vec<String>>>,
        /// A member's login, when the test acts as one: (cookie, key).
        /// Chats made with the API token have no owner and never call back.
        owner: Option<(String, String)>,
    }

    impl Drop for Serve {
        fn drop(&mut self) {
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    pub fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir();
        let root = dir.path().canonicalize().unwrap();
        let (data, ws) = (root.join("data"), root.join("ws"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            data.join("node.toml"),
            format!(
                "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n\n[skills]\nroots = []\n",
                ws.display().to_string()
            ),
        )
        .unwrap();
        std::fs::write(
            data.join("config.toml"),
            "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
             mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
        )
        .unwrap();
        (dir, data, ws)
    }

    pub fn serve(data: &Path) -> Serve {
        let mut child = Command::new(EXE)
            .args([
                "--data-dir",
                data.to_str().unwrap(),
                "serve",
                "--bind",
                "127.0.0.1:0",
            ])
            .current_dir(data.parent().unwrap())
            .env("HARNESS_LOG", "info")
            .env("NO_COLOR", "1")
            .env_remove("HARNESS_JOIN_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let err = child.stderr.take().unwrap();
        let l2 = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                l2.lock().unwrap().push(line);
            }
        });
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let addr = loop {
            let line = lines.next().expect("serve ended").unwrap();
            if let Some(a) = line.split("listening on http://").nth(1) {
                break a.trim().to_string();
            }
        };
        std::thread::spawn(move || for _ in lines {});
        let token = std::fs::read_to_string(data.join("api-token"))
            .unwrap()
            .trim()
            .to_string();
        Serve {
            child: Some(child),
            addr,
            token,
            log,
            owner: None,
        }
    }

    impl Serve {
        /// Act as a new member `name` with a password of its own from now on.
        pub async fn as_member(&mut self, name: &str) {
            let c = reqwest::Client::new();
            let v = self
                .post(
                    "/api/users",
                    json!({"username": name, "role": "admin", "exec_ceiling": "host"}),
                )
                .await;
            let pw = v["password"].as_str().unwrap().to_string();
            let r = c
                .post(format!("http://{}/api/login", self.addr))
                .json(&json!({"username": name, "password": pw}))
                .send()
                .await
                .unwrap();
            let cookie = |r: &reqwest::Response| {
                r.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string()
            };
            let ck = cookie(&r);
            let key = r.json::<Value>().await.unwrap()["login_key"]
                .as_str()
                .unwrap()
                .to_string();
            let r = c
                .post(format!("http://{}/api/me/password", self.addr))
                .header("cookie", &ck)
                .header("x-harness-login", &key)
                .header("origin", format!("http://{}", self.addr))
                .json(&json!({"current": pw, "new": "a password of my own"}))
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success(), "{}", r.status());
            let ck = cookie(&r);
            let key = r.json::<Value>().await.unwrap()["login_key"]
                .as_str()
                .unwrap()
                .to_string();
            self.owner = Some((ck, key));
        }

        fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
            let rb = reqwest::Client::new().request(method, format!("http://{}{path}", self.addr));
            match &self.owner {
                Some((ck, key)) => rb
                    .header("cookie", ck)
                    .header("x-harness-login", key)
                    .header("origin", format!("http://{}", self.addr)),
                None => rb.bearer_auth(&self.token),
            }
        }

        pub fn kill9(&mut self) {
            let mut c = self.child.take().unwrap();
            let _ = c.kill();
            let _ = c.wait();
        }

        pub fn log(&self) -> String {
            self.log.lock().unwrap().join("\n")
        }

        pub async fn post(&self, path: &str, body: Value) -> Value {
            let r = self
                .req(reqwest::Method::POST, path)
                .json(&body)
                .send()
                .await
                .unwrap();
            let st = r.status();
            let v = r.json().await.unwrap_or(Value::Null);
            assert!(st.is_success(), "{path}: {st} {v}");
            v
        }

        pub async fn events(&self, sid: &str) -> Vec<Value> {
            let r = self
                .req(
                    reqwest::Method::GET,
                    &format!("/api/sessions/{sid}/events?limit=5000"),
                )
                .send()
                .await
                .unwrap();
            let v: Value = r.json().await.unwrap();
            v["events"].as_array().cloned().unwrap_or_default()
        }

        pub async fn wait_for(&self, sid: &str, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
            let by = Instant::now() + Duration::from_secs(90);
            loop {
                let evs = self.events(sid).await;
                if pred(&evs) {
                    return evs;
                }
                assert!(
                    Instant::now() < by,
                    "not found in {sid}: {evs:#?}\n{}",
                    self.log()
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        /// A chat of the built-in Team Lead (`background_subagents: true`)
        /// whose first turn runs the scripted `steps`.
        pub async fn lead_chat(&self, ws: &Path, steps: &[Value]) -> String {
            let sid = self
                .post(
                    "/api/sessions",
                    json!({"cwd": ws.display().to_string(), "approval_mode": "auto"}),
                )
                .await["session_id"]
                .as_str()
                .unwrap()
                .to_string();
            self.prompt(&sid, steps).await;
            sid
        }

        pub async fn prompt(&self, sid: &str, steps: &[Value]) {
            self.post(
                &format!("/api/sessions/{sid}/prompt"),
                json!({"text": format!("script: {}", json!(steps)), "agent_id": "teamlead"}),
            )
            .await;
        }
    }

    pub fn count(evs: &[Value], kind: &str) -> usize {
        evs.iter().filter(|e| e["kind"] == kind).count()
    }
}

/// The master's own watcher resumes the lead when its background child
/// ends while the chat is idle: one callback turn, its opening the child's
/// result (the real binary, its demo provider and the built-in Team Lead).
#[tokio::test(flavor = "multi_thread")]
async fn the_master_calls_the_lead_back_with_the_childs_result() {
    let (_dir, data, ws) = real::setup();
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let sid = s
        .lead_chat(
            &ws,
            &[bg_delegate(
                "scout",
                &format!("run: {}", held("go", "echo found")),
            )],
        )
        .await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s
        .wait_for(&sid, |e| real::count(e, "turn_ended") == 2)
        .await;
    let cause = evs
        .iter()
        .find(|e| e["kind"] == "turn_started" && !e["payload"]["cause"].is_null())
        .unwrap_or_else(|| panic!("no callback turn: {evs:#?}"));
    assert_eq!(cause["payload"]["cause"]["callback"], json!(["bg-1"]));
    let end = evs
        .iter()
        .find(|e| e["kind"] == "background_ended")
        .unwrap();
    assert_eq!(end["payload"]["reason"], "finished");
    assert!(
        end["payload"]["child"]["result"]
            .as_str()
            .unwrap()
            .contains("Sub-agent @scout finished: done")
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        real::count(&s.events(&sid).await, "turn_started"),
        2,
        "one callback turn"
    );
}

/// A `kill -9` of the server mid-child: nothing it ran is left, and at the
/// next start its end says the restart cut it (once, no callback turn);
/// the lead can give the child a follow-up, which keeps its conversation.
#[tokio::test(flavor = "multi_thread")]
async fn kill_9_mid_child_tells_the_truth_once_and_the_child_can_be_continued() {
    let (_dir, data, ws) = real::setup();
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let mut s = real::serve(&data);
    let sid = s
        .lead_chat(&ws, &[bg_delegate("scout", "run: sleep 977401")])
        .await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    appears("sleep 977401").await;
    let evs = s.events(&sid).await;
    let start = evs
        .iter()
        .find(|e| e["kind"] == "background_started")
        .unwrap();
    let child = start["payload"]["child"]["session"]
        .as_str()
        .unwrap()
        .to_string();
    s.kill9();
    gone("sleep 977401", "kill -9 of serve").await;
    let s2 = real::serve(&data);
    let evs = s2
        .wait_for(&sid, |e| real::count(e, "background_ended") == 1)
        .await;
    let end = evs
        .iter()
        .find(|e| e["kind"] == "background_ended")
        .unwrap();
    assert_eq!(end["payload"]["reason"], "node_restarted", "{end}");
    assert_eq!(end["payload"]["child"]["session"], json!(child));
    let ends: BackgroundEnded = serde_json::from_value(end["payload"].clone()).unwrap();
    let note = ends.model_note().unwrap();
    assert!(note.contains("cut off") && note.contains(&child), "{note}");
    // The child's own turn is closed as interrupted.
    let theirs = s2
        .wait_for(&child, |e| real::count(e, "turn_ended") == 1)
        .await;
    let te = theirs.iter().find(|e| e["kind"] == "turn_ended").unwrap();
    assert_eq!(te["payload"]["reason"], "interrupted");
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        real::count(&s2.events(&sid).await, "turn_started"),
        1,
        "no callback turn"
    );
    // A follow-up reaches it.
    s2.prompt(
        &sid,
        &[json!({"tool": "delegate", "input": {"agent": "scout", "session": child, "task": "run: echo again"}})],
    )
    .await;
    s2.wait_for(&child, |e| real::count(e, "turn_ended") == 2)
        .await;
    let evs = s2
        .wait_for(&sid, |e| real::count(e, "turn_ended") == 2)
        .await;
    let out = evs
        .iter()
        .rev()
        .find(|e| e["kind"] == "tool_call_finished")
        .unwrap();
    assert!(
        out["payload"]["output"]
            .as_str()
            .unwrap()
            .starts_with(&format!("session: {child}\nSub-agent @scout finished")),
        "{out}"
    );
    assert_eq!(real::count(&evs, "background_ended"), 1, "told once");
}
