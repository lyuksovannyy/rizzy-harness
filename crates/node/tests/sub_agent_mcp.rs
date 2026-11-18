//! MCP tools for sub-agents (ADOPTION_PLAN X.34 step C, PLAN.md T47): a
//! foreground sub-agent whose OWN agent may use MCP is offered its chat's
//! servers, the same table and processes, started in the chat's folder
//! whoever starts them; one whose switch is off, a denied server, the
//! node's `denied_tools`, a read-only agent, plan mode and a background
//! run get none. Calls are asked about in the sub-agent's own chat as the
//! parent's are; a stop and a deletion leave nothing running; parallel
//! sub-agents get their own answers; a sub-agent's turn has the chat's
//! caps. Unconfined (no bubblewrap), with the scripted agent; the
//! contained path is `cli/tests/mcp_sandbox.rs`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::agent::AgentSpec;
use harness_core::event::EventKind;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use harness_tools::mcp::ServerConfig;
use serde_json::{Value, json};

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    _rt: NodeRuntime,
    link: NodeLink,
    /// Every event the node sent, of every session.
    seen: Vec<RawEvent>,
}

fn started(root: &Path) -> PathBuf {
    root.join("started-fake")
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

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A node with the fake server `fake` (more arguments `extra`) in its
/// node.toml, a committed repository `ws`, and `policy` changed by `edit`.
async fn node_with(extra: &[&str], edit: impl FnOnce(&mut NodePolicy)) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let ws = root.join("ws");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("a.txt"), "alpha\n").unwrap();
    git(&ws, &["init", "-q"]);
    git(&ws, &["add", "."]);
    git(&ws, &["commit", "-qm", "init"]);
    let started = started(&root);
    let mut args = vec![FAKE, "--started", started.to_str().unwrap()];
    args.extend_from_slice(extra);
    let mut servers = BTreeMap::new();
    servers.insert("fake".to_string(), ServerConfig::new("python3", &args));
    let mut policy = NodePolicy {
        workspace_roots: vec![root.clone()],
        max_approval_mode: ApprovalMode::Auto,
        mcp_servers: servers,
        skills: harness_node::SkillsPolicy {
            roots: vec![],
            ..Default::default()
        },
        ..Default::default()
    };
    edit(&mut policy);
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
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
        checkpoints_dir: Some(root.join("checkpoints")),
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
    Node {
        dir,
        ws,
        _rt: rt,
        link,
        seen: vec![],
    }
}

async fn node() -> Node {
    node_with(&[], |_| {}).await
}

/// An agent spec: `id` with `tools`, `delegates` and `more` fields.
fn spec(id: &str, tools: &[&str], delegates: Vec<AgentSpec>, more: Value) -> AgentSpec {
    let mut v = json!({
        "agent_id": id, "name": id, "system_prompt": "work", "prompt_hash": "h",
        "tools": tools, "delegates": delegates,
    });
    if let Value::Object(m) = more {
        for (k, x) in m {
            v[k] = x;
        }
    }
    serde_json::from_value(v).unwrap()
}

/// A scripted task: one call per step (a step that is an array is those
/// calls at once).
fn script(steps: Value) -> String {
    format!("script: {steps}")
}

fn call(tool: &str, input: Value) -> Value {
    json!({"tool": tool, "input": input})
}

/// The lead's own tools: no MCP unless asked for.
fn lead(tools: &[&str], delegates: Vec<AgentSpec>) -> AgentSpec {
    spec("lead", tools, delegates, json!({}))
}

impl Node {
    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    async fn next(&mut self) -> NodeToMaster {
        let m = tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        if let NodeToMaster::Event { event } = &m {
            self.seen.push(event.clone());
        }
        m
    }

    async fn ack(&mut self) -> Result<(), String> {
        loop {
            if let NodeToMaster::Ack { ok, error, .. } = self.next().await {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    async fn create(&mut self, mode: ApprovalMode) -> SessionId {
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
                exec_mode: None,
            })
            .unwrap();
        self.ack().await.unwrap();
        sid
    }

    fn prompt(&mut self, sid: SessionId, text: &str, agent: AgentSpec) {
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
            })
            .unwrap();
    }

    fn answer(&self, session: SessionId, approval_id: String, decision: Decision) {
        self.link
            .to_node
            .try_send(MasterToNode::ApprovalRespond {
                req_id: format!("a-{approval_id}"),
                session_id: session,
                approval_id,
                decision,
                note: None,
            })
            .unwrap();
    }

    /// One prompt to the end of `sid`'s turn, allowing every card of every
    /// session.
    async fn turn(&mut self, sid: SessionId, text: &str, agent: AgentSpec) {
        let from = self.seen.len();
        self.prompt(sid, text, agent);
        loop {
            if let NodeToMaster::Event { event } = self.next().await {
                match event.typed() {
                    Ok(EventKind::ApprovalRequested(a)) => {
                        self.answer(event.session_id, a.approval_id, Decision::Allow)
                    }
                    Ok(EventKind::TurnEnded(_))
                        if event.session_id == sid
                            && self.seen[from..].iter().any(|e| e.kind == "turn_started") =>
                    {
                        return;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Wait until `pred` holds for an event; cards are allowed meanwhile.
    async fn until(&mut self, pred: impl Fn(&RawEvent) -> bool) -> RawEvent {
        loop {
            if let NodeToMaster::Event { event } = self.next().await {
                if let Ok(EventKind::ApprovalRequested(a)) = event.typed() {
                    self.answer(event.session_id, a.approval_id, Decision::Allow);
                }
                if pred(&event) {
                    return event;
                }
            }
        }
    }

    /// Finished tool calls of sessions other than `sid` (its sub-agents):
    /// (session, tool, ok, output), in order.
    fn child_results(&self, sid: SessionId) -> Vec<(SessionId, String, bool, String)> {
        let mut tools: HashMap<String, String> = HashMap::new();
        let mut out = vec![];
        for e in self.seen.iter().filter(|e| e.session_id != sid) {
            match e.typed() {
                Ok(EventKind::ToolCallStarted(c)) => {
                    tools.insert(c.call_id, c.tool);
                }
                Ok(EventKind::ToolCallFinished(f)) => out.push((
                    e.session_id,
                    tools.get(&f.call_id).cloned().unwrap_or_default(),
                    f.ok,
                    f.output,
                )),
                _ => {}
            }
        }
        out
    }

    /// Finished tool calls of `sid` itself: (tool, ok, output).
    fn results(&self, sid: SessionId) -> Vec<(String, bool, String)> {
        let mut tools: HashMap<String, String> = HashMap::new();
        let mut out = vec![];
        for e in self.seen.iter().filter(|e| e.session_id == sid) {
            match e.typed() {
                Ok(EventKind::ToolCallStarted(c)) => {
                    tools.insert(c.call_id, c.tool);
                }
                Ok(EventKind::ToolCallFinished(f)) => out.push((
                    tools.get(&f.call_id).cloned().unwrap_or_default(),
                    f.ok,
                    f.output,
                )),
                _ => {}
            }
        }
        out
    }

    /// The cards asked: (session, tool, reason).
    fn asked(&self) -> Vec<(SessionId, String, String)> {
        self.seen
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ApprovalRequested(a)) => Some((e.session_id, a.tool, a.reason)),
                _ => None,
            })
            .collect()
    }

    fn started(&self) -> Vec<i32> {
        pids(&started(&self.root()))
    }
}

/// The helper sub-agent: it can change files (so it works in its own copy
/// of the folder) and has `more` tools.
fn helper(more: &[&str], fields: Value) -> AgentSpec {
    let mut tools = vec!["read", "write"];
    tools.extend_from_slice(more);
    spec("helper", &tools, vec![], fields)
}

/// The step: a call no tool of the turn may be offered under that name.
fn unknown(out: &str, tool: &str) -> bool {
    out.contains(&format!("unknown tool `{tool}`"))
}

/// Owner's decision (2026-11-17, X.34): a sub-agent follows its OWN
/// Connectors switch. With it on, it is offered its chat's servers although
/// the chat's agent has the switch off; the server it starts runs in the
/// CHAT's folder (never the sub-agent's copy, which is where its own calls
/// run); its calls are asked about in its own chat with the MCP wording,
/// as the parent's are; and the chat's own agent, switched on later, uses
/// the same process (one per chat).
#[tokio::test]
async fn a_sub_agent_with_its_switch_on_uses_the_chats_server_in_the_chats_folder() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Ask).await;
    let task = script(json!([
        call("mcp__fake__cwd", json!({})),
        call("mcp__fake__pid", json!({}))
    ]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![helper(&["mcp"], json!({}))]),
    )
    .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 2, "{res:#?}");
    let child = res[0].0;
    assert!(res.iter().all(|r| r.2), "{res:#?}");
    let ws = n.ws.canonicalize().unwrap();
    assert_eq!(res[0].3, format!("cwd={}", ws.display()), "{res:#?}");
    // Its own folder is elsewhere: the server is the chat's, not its copy's.
    let cwd_of_child = n
        .seen
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) if e.session_id == child => Some(c.cwd),
            _ => None,
        })
        .unwrap();
    assert_ne!(
        Path::new(&cwd_of_child),
        ws,
        "the helper had its own folder"
    );
    // Asked in the sub-agent's chat, as a program the harness did not write.
    let asked = n.asked();
    assert!(
        asked.iter().any(|(s, t, r)| *s == child
            && t == "mcp__fake__cwd"
            && r.contains("MCP server")
            && r.contains("server `fake`, its tool `cwd`")),
        "{asked:#?}"
    );
    assert_eq!(n.started().len(), 1);
    let child_pid = res[1].3.clone();
    // The chat's agent, now with the switch on, uses the same process.
    n.turn(
        sid,
        &script(json!([call("mcp__fake__pid", json!({}))])),
        lead(&["read", "mcp"], vec![]),
    )
    .await;
    let own = n.results(sid);
    let last = own.last().unwrap();
    assert_eq!(last.0, "mcp__fake__pid");
    assert_eq!(last.2, child_pid, "one process per chat");
    assert_eq!(n.started().len(), 1);
    assert!(
        n.asked()
            .iter()
            .any(|(s, t, _)| *s == sid && t == "mcp__fake__pid"),
        "the parent's call is asked about too"
    );
}

/// Abuse: the parent's switch gives a sub-agent nothing. The chat's agent
/// uses the server; its sub-agent, whose own switch is off, calls the
/// tool by name and is refused as for any tool it was not offered.
#[tokio::test]
async fn a_sub_agent_whose_switch_is_off_gets_nothing_from_its_parents() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
    let lead_script = script(json!([
        call("mcp__fake__echo", json!({"text": "lead"})),
        call("delegate", json!({"agent": "helper", "task": task})),
    ]));
    n.turn(
        sid,
        &lead_script,
        lead(&["read", "mcp"], vec![helper(&[], json!({}))]),
    )
    .await;
    let own = n.results(sid);
    assert_eq!((own[0].1, own[0].2.as_str()), (true, "lead"), "{own:#?}");
    let res = n.child_results(sid);
    assert_eq!(res.len(), 1, "{res:#?}");
    assert!(
        !res[0].2 && unknown(&res[0].3, "mcp__fake__echo"),
        "{res:#?}"
    );
}

/// Abuse: a server denied to the sub-agent's own agent (step A's list)
/// is neither started for its turn nor callable; nor is a tool listed in
/// its own `deny_tools`.
#[tokio::test]
async fn a_server_denied_to_the_sub_agent_is_not_started_nor_callable() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(
            &["read"],
            vec![helper(&["mcp"], json!({"deny_mcp_servers": ["fake"]}))],
        ),
    )
    .await;
    let res = n.child_results(sid);
    assert!(
        !res[0].2 && unknown(&res[0].3, "mcp__fake__echo"),
        "{res:#?}"
    );
    assert!(n.started().is_empty(), "a denied server started");
    // Its own `deny_tools` takes the tool too.
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(
            &["read"],
            vec![helper(&["mcp"], json!({"deny_tools": ["mcp__fake__echo"]}))],
        ),
    )
    .await;
    let res = n.child_results(sid);
    let last = res.last().unwrap();
    assert!(!last.2 && unknown(&last.3, "mcp__fake__echo"), "{res:#?}");
}

/// Abuse: node.toml's `denied_tools` wins for sub-agents, whether it
/// names every MCP tool or one server.
#[tokio::test]
async fn the_nodes_denied_tools_hold_for_sub_agents() {
    for denied in ["mcp", "mcp__fake"] {
        let mut n = node_with(&[], |p| p.denied_tools = vec![denied.into()]).await;
        let sid = n.create(ApprovalMode::Auto).await;
        let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
        n.turn(
            sid,
            &format!("delegate: helper={task}"),
            lead(&["read"], vec![helper(&["mcp"], json!({}))]),
        )
        .await;
        let res = n.child_results(sid);
        assert_eq!(res.len(), 1, "{denied}: {res:#?}");
        assert!(!res[0].2 && res[0].3 != "x", "{denied}: {res:#?}");
        // A server node.toml denies whole does not even start.
        assert!(n.started().is_empty(), "{denied}: a denied server started");
        // Nor for the chat's own agent.
        n.turn(sid, &task, lead(&["read", "mcp"], vec![])).await;
        let own = n.results(sid);
        assert!(!own.last().unwrap().1, "{denied}: {own:#?}");
        assert!(n.started().is_empty(), "{denied}: a denied server started");
    }
}

/// Abuse (point 1): an agent the harness treats as read-only (PLAN.md
/// T70: read-only shell, its parent's folder, the small model, allowed in
/// plan mode) gets no server, even when its spec names an MCP tool: here
/// its only one is withheld by its own `deny_tools`, which makes it
/// read-only, and nothing of the server may start for it.
#[tokio::test]
async fn a_read_only_sub_agent_starts_and_gets_no_server() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let reader = spec(
        "reader",
        &["read", "mcp__fake__echo"],
        vec![],
        json!({"deny_tools": ["mcp__fake__echo"]}),
    );
    assert!(reader.allows_mcp() && !reader.changes_things(|_| true));
    let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
    n.turn(
        sid,
        &format!("delegate: reader={task}"),
        lead(&["read"], vec![reader]),
    )
    .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 1, "{res:#?}");
    assert!(!res[0].2, "{res:#?}");
    assert!(
        n.started().is_empty(),
        "a read-only agent's turn started a server"
    );
}

/// Abuse: plan mode. A sub-agent with Connectors is one that changes
/// things, so a plan-mode chat does not start it; and a sub-agent working
/// when its chat enters plan mode has its next server call refused.
#[tokio::test]
async fn plan_mode_holds_for_a_sub_agents_server_calls() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Plan).await;
    let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![helper(&["mcp"], json!({}))]),
    )
    .await;
    assert!(
        n.child_results(sid).is_empty(),
        "{:#?}",
        n.child_results(sid)
    );
    let own = n.results(sid);
    assert!(own.iter().all(|r| !r.1), "{own:#?}");
    assert!(n.started().is_empty());

    // Entering plan mode while a sub-agent works.
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([
        call("mcp__fake__slow", json!({"text": "first", "ms": 1500})),
        call("mcp__fake__echo", json!({"text": "after plan"})),
    ]));
    n.prompt(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![helper(&["mcp"], json!({}))]),
    );
    n.until(|e| {
        e.session_id != sid
            && matches!(e.typed(), Ok(EventKind::ToolCallStarted(c)) if c.tool == "mcp__fake__slow")
    })
    .await;
    n.link
        .to_node
        .try_send(MasterToNode::SetApprovalMode {
            lowering: true,
            req_id: "plan".into(),
            session_id: sid,
            mode: ApprovalMode::Plan,
        })
        .unwrap();
    n.until(|e| e.session_id == sid && e.kind == "turn_ended")
        .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 2, "{res:#?}");
    assert_eq!((res[0].2, res[0].3.as_str()), (true, "first"), "{res:#?}");
    assert!(!res[1].2 && !res[1].3.contains("after plan"), "{res:#?}");
}

/// Abuse: a stopped turn leaves no sub-agent's call waiting, and the
/// chat's deletion leaves no server process, though the sub-agent started
/// it.
#[tokio::test]
async fn a_stop_and_a_deletion_leave_no_sub_agent_call_nor_server() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([call("mcp__fake__hang", json!({}))]));
    n.prompt(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![helper(&["mcp"], json!({}))]),
    );
    n.until(|e| {
        e.session_id != sid
            && matches!(e.typed(), Ok(EventKind::ToolCallStarted(c)) if c.tool == "mcp__fake__hang")
    })
    .await;
    n.link
        .to_node
        .try_send(MasterToNode::Cancel {
            req_id: "x".into(),
            session_id: sid,
        })
        .unwrap();
    n.until(|e| e.session_id == sid && e.kind == "turn_ended")
        .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 1, "{res:#?}");
    assert!(!res[0].2, "the hung call was not ended: {res:#?}");
    let pid = n.started()[0];
    assert!(
        alive(pid),
        "the chat's server stays until it idles or the chat goes"
    );
    n.link
        .to_node
        .try_send(MasterToNode::Delete {
            req_id: "d".into(),
            session_id: sid,
        })
        .unwrap();
    n.ack().await.unwrap();
    assert!(gone(pid).await, "the deleted chat's server still runs");
}

/// Point 4: sub-agents running at once, and their parent beside them, call
/// one server process; each gets its own answer though the server answers
/// in another order than it was asked (the client matches answers by
/// request id).
#[tokio::test]
async fn parallel_sub_agents_get_their_own_answers() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = |who: &str, ms: u64| {
        script(json!([
            call(
                "mcp__fake__slow",
                json!({"text": format!("{who}-1"), "ms": ms})
            ),
            call(
                "mcp__fake__slow",
                json!({"text": format!("{who}-2"), "ms": ms / 2})
            ),
        ]))
    };
    let delegates: Vec<AgentSpec> = ["a", "b", "c"]
        .iter()
        .map(|id| spec(id, &["read", "write", "mcp"], vec![], json!({})))
        .collect();
    let lead_script = script(json!([[
        call("delegate", json!({"agent": "a", "task": task("a", 1800)})),
        call("delegate", json!({"agent": "b", "task": task("b", 1200)})),
        call("delegate", json!({"agent": "c", "task": task("c", 600)})),
        call("mcp__fake__slow", json!({"text": "lead", "ms": 900})),
    ]]));
    n.turn(sid, &lead_script, lead(&["read", "mcp"], delegates))
        .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 6, "{res:#?}");
    let mut by: HashMap<SessionId, Vec<String>> = HashMap::new();
    for (s, tool, ok, out) in &res {
        assert!(*ok && tool == "mcp__fake__slow", "{res:#?}");
        by.entry(*s).or_default().push(out.clone());
    }
    let mut got: Vec<Vec<String>> = by.into_values().collect();
    got.sort();
    let want: Vec<Vec<String>> = ["a", "b", "c"]
        .iter()
        .map(|w| vec![format!("{w}-1"), format!("{w}-2")])
        .collect();
    assert_eq!(got, want);
    let own = n.results(sid);
    assert!(
        own.iter()
            .any(|(t, ok, out)| t == "mcp__fake__slow" && *ok && out == "lead"),
        "{own:#?}"
    );
    assert_eq!(n.started().len(), 1, "one process for the chat");
    // They ran at once: the answers came back in another order than the
    // calls (c's first answer before a's).
    let finished: Vec<String> = res.iter().map(|r| r.3.clone()).collect();
    let pos = |s: &str| finished.iter().position(|f| f == s).unwrap();
    assert!(pos("c-1") < pos("a-1"), "{finished:?}");
}

/// Abuse: the chat's caps hold for each turn: a sub-agent is offered at
/// most `MAX_TOOLS` of a server's tools, the same ones its chat would get,
/// and a tool past the cap is not callable by it.
#[tokio::test]
async fn a_sub_agent_gets_the_chats_caps() {
    let mut n = node_with(&["--tools", "45"], |_| {}).await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([
        call("mcp__fake__pid", json!({})),
        call("mcp__fake__t44", json!({})),
    ]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![helper(&["mcp"], json!({}))]),
    )
    .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 2, "{res:#?}");
    assert!(res[0].2, "{res:#?}");
    assert!(
        !res[1].2 && unknown(&res[1].3, "mcp__fake__t44"),
        "{res:#?}"
    );
    let offered = res[1].3.matches("mcp__fake__").count() - 1;
    assert!(
        offered <= harness_tools::mcp::MAX_TOOLS,
        "{offered}: {}",
        res[1].3
    );
}

/// A background sub-agent (X.24) gets no server: its calls could land in
/// the chat's folder between the chat's turns, where no checkpoint covers
/// them (PLAN.md T72's residual), so Undo could not take them back.
#[tokio::test]
async fn a_background_sub_agent_gets_no_server() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([call("mcp__fake__echo", json!({"text": "x"}))]));
    let mut l = lead(&["read"], vec![helper(&["mcp"], json!({}))]);
    l.caps = Some(harness_core::agent::DelegateCaps {
        max_subagents: 4,
        max_tokens: 200_000,
        background: true,
    });
    n.turn(
        sid,
        &script(json!([call(
            "delegate",
            json!({"agent": "helper", "task": task, "background": true})
        )])),
        l,
    )
    .await;
    let own = n.results(sid);
    assert!(own[0].1, "started in the background: {own:#?}");
    n.until(|e| e.session_id == sid && e.kind == "background_ended")
        .await;
    let res = n.child_results(sid);
    assert_eq!(res.len(), 1, "{res:#?}");
    assert!(
        !res[0].2 && unknown(&res[0].3, "mcp__fake__echo"),
        "{res:#?}"
    );
    assert!(n.started().is_empty());
}

impl Node {
    /// The sub-agent sessions `parent` started, in order.
    fn children(&self, parent: SessionId) -> Vec<SessionId> {
        self.seen
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::SessionCreated(c)) if c.parent_session == Some(parent) => {
                    Some(e.session_id)
                }
                _ => None,
            })
            .collect()
    }

    /// A person's message to session `sid` (a sub-agent's chat), to its
    /// turn's end: its last finished call, or the node's refusal of the
    /// message.
    async fn person_turn_in(
        &mut self,
        sid: SessionId,
        text: &str,
        agent: AgentSpec,
    ) -> Result<(String, bool, String), String> {
        self.prompt(sid, text, agent);
        self.ack().await?;
        self.until(|e| e.session_id == sid && e.kind == "turn_ended")
            .await;
        Ok(self.results(sid).last().cloned().expect("a call"))
    }
}

/// Review of PR 149, HIGH 1 (R2): a person's message to a sub-agent's chat
/// after its parent's turn ended is not a run of any turn of the chat: it
/// gets no server (an MCP-only sub-agent, which works in the chat's
/// folder; a writer's chat refuses the message, its own folder is gone).
#[tokio::test]
async fn a_person_prompting_an_idle_sub_agent_gets_no_server() {
    for (writer, h) in [
        (true, helper(&["mcp"], json!({}))),
        (false, spec("helper", &["read", "mcp"], vec![], json!({}))),
    ] {
        let mut n = node().await;
        let sid = n.create(ApprovalMode::Auto).await;
        let task = script(json!([call("mcp__fake__echo", json!({"text": "first"}))]));
        n.turn(
            sid,
            &format!("delegate: helper={task}"),
            lead(&["read"], vec![h.clone()]),
        )
        .await;
        let first = n.child_results(sid);
        assert!(first[0].2 && first[0].3 == "first", "{first:#?}");
        let child = n.children(sid)[0];
        let last = n
            .person_turn_in(
                child,
                &script(json!([call("mcp__fake__echo", json!({"text": "later"}))])),
                h,
            )
            .await;
        if writer {
            assert!(last.is_err(), "{last:?}");
            continue;
        }
        let last = last.unwrap();
        assert!(!last.1 && unknown(&last.2, "mcp__fake__echo"), "{last:?}");
    }
}

/// HIGH 1 (R3): the same for a former background sub-agent's chat.
#[tokio::test]
async fn a_person_prompting_a_former_background_sub_agent_gets_no_server() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let h = spec("helper", &["read", "mcp"], vec![], json!({}));
    let mut l = lead(&["read"], vec![h.clone()]);
    l.caps = Some(harness_core::agent::DelegateCaps {
        max_subagents: 4,
        max_tokens: 200_000,
        background: true,
    });
    let task = script(json!([call("mcp__fake__echo", json!({"text": "bg"}))]));
    n.turn(
        sid,
        &script(json!([call(
            "delegate",
            json!({"agent": "helper", "task": task, "background": true})
        )])),
        l,
    )
    .await;
    n.until(|e| e.session_id == sid && e.kind == "background_ended")
        .await;
    let child = n.children(sid)[0];
    let last = n
        .person_turn_in(
            child,
            &script(json!([call("mcp__fake__echo", json!({"text": "later"}))])),
            h,
        )
        .await
        .unwrap();
    assert!(!last.1 && unknown(&last.2, "mcp__fake__echo"), "{last:?}");
    assert!(n.started().is_empty());
}

/// HIGH 1: while the chat's own turn runs, a person's turn in a sub-agent's
/// chat is still not part of it: no server.
#[tokio::test]
async fn a_person_turn_in_a_sub_agents_chat_beside_the_chats_turn_gets_no_server() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let h = spec("helper", &["read", "mcp"], vec![], json!({}));
    let task = script(json!([call("mcp__fake__echo", json!({"text": "first"}))]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(&["read"], vec![h.clone()]),
    )
    .await;
    let child = n.children(sid)[0];
    n.prompt(
        sid,
        &script(json!([call(
            "mcp__fake__slow",
            json!({"text": "chat", "ms": 3000})
        )])),
        lead(&["read", "mcp"], vec![]),
    );
    n.until(|e| {
        e.session_id == sid
            && matches!(e.typed(), Ok(EventKind::ToolCallStarted(c)) if c.tool == "mcp__fake__slow")
    })
    .await;
    let last = n
        .person_turn_in(
            child,
            &script(json!([call("mcp__fake__echo", json!({"text": "beside"}))])),
            h,
        )
        .await
        .unwrap();
    assert!(!last.1 && unknown(&last.2, "mcp__fake__echo"), "{last:?}");
    n.until(|e| e.session_id == sid && e.kind == "turn_ended")
        .await;
    let own = n.results(sid);
    assert_eq!(own.last().unwrap().2, "chat", "{own:#?}");
}

/// L3 (R4): a foreground grandchild under a background child gets no
/// server; under a foreground child (R5) it gets the chat's, in the chat's
/// folder.
#[tokio::test]
async fn a_grandchild_follows_every_turn_above_it() {
    let boss = || {
        spec(
            "boss",
            &["read", "write", "delegate"],
            vec![spec("helper", &["read", "mcp"], vec![], json!({}))],
            json!({}),
        )
    };
    let g_task = script(json!([call("mcp__fake__cwd", json!({}))]));
    let b_task = script(json!([call(
        "delegate",
        json!({"agent": "helper", "task": g_task})
    )]));
    // Under a background child.
    let mut n = node_with(&[], |p| p.limits.max_delegation_depth = 2).await;
    let sid = n.create(ApprovalMode::Auto).await;
    let mut l = lead(&["read"], vec![boss()]);
    l.caps = Some(harness_core::agent::DelegateCaps {
        max_subagents: 4,
        max_tokens: 200_000,
        background: true,
    });
    n.turn(
        sid,
        &script(json!([call(
            "delegate",
            json!({"agent": "boss", "task": b_task, "background": true})
        )])),
        l,
    )
    .await;
    n.until(|e| e.session_id == sid && e.kind == "background_ended")
        .await;
    let res = n.child_results(sid);
    let g = res
        .iter()
        .find(|r| r.1 == "mcp__fake__cwd")
        .expect("the grandchild called");
    assert!(!g.2 && unknown(&g.3, "mcp__fake__cwd"), "{res:#?}");
    assert!(n.started().is_empty());
    // Under a foreground child.
    let mut n = node_with(&[], |p| p.limits.max_delegation_depth = 2).await;
    let sid = n.create(ApprovalMode::Auto).await;
    n.turn(
        sid,
        &format!("delegate: boss={b_task}"),
        lead(&["read"], vec![boss()]),
    )
    .await;
    let res = n.child_results(sid);
    let g = res
        .iter()
        .find(|r| r.1 == "mcp__fake__cwd")
        .expect("the grandchild called");
    let ws = n.ws.canonicalize().unwrap();
    assert_eq!(
        (g.2, g.3.clone()),
        (true, format!("cwd={}", ws.display())),
        "{res:#?}"
    );
}

/// Review of PR 149, MEDIUM 2 (R8): one crash with four sub-agents' calls in
/// flight on the process counts as one failure, and the next call starts
/// the server again.
#[tokio::test]
async fn one_crash_under_many_calls_counts_once() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Auto).await;
    let slow = |who: &str| {
        script(json!([call(
            "mcp__fake__slow",
            json!({"text": who, "ms": 4000})
        )]))
    };
    let dier = script(json!([
        call("mcp__fake__slow", json!({"text": "d", "ms": 1500})),
        call("mcp__fake__die", json!({}))
    ]));
    let delegates: Vec<AgentSpec> = ["a", "b", "c", "d"]
        .iter()
        .map(|id| spec(id, &["read", "mcp"], vec![], json!({})))
        .collect();
    let lead_script = script(json!([
        [
            call("delegate", json!({"agent": "a", "task": slow("a")})),
            call("delegate", json!({"agent": "b", "task": slow("b")})),
            call("delegate", json!({"agent": "c", "task": slow("c")})),
            call("delegate", json!({"agent": "d", "task": dier})),
        ],
        call("mcp__fake__echo", json!({"text": "after one crash"})),
    ]));
    n.turn(sid, &lead_script, lead(&["read", "mcp"], delegates))
        .await;
    let res = n.child_results(sid);
    let failed: Vec<&String> = res.iter().filter(|r| !r.2).map(|r| &r.3).collect();
    assert!(failed.len() >= 2, "calls were in flight: {res:#?}");
    assert!(
        failed
            .iter()
            .all(|o| o.contains("restart 1 of") && !o.contains("not started again")),
        "{failed:#?}"
    );
    let own = n.results(sid);
    let last = own.last().unwrap();
    assert_eq!(
        (last.1, last.2.as_str()),
        (true, "after one crash"),
        "{own:#?}"
    );
    assert_eq!(n.started().len(), 2, "started again after the crash");
}

/// L4: in plan mode the chat's own MCP agent starts no server.
#[tokio::test]
async fn plan_mode_starts_no_server_for_the_chats_agent() {
    let mut n = node().await;
    let sid = n.create(ApprovalMode::Plan).await;
    n.turn(
        sid,
        &script(json!([call("mcp__fake__echo", json!({"text": "x"}))])),
        lead(&["read", "mcp"], vec![]),
    )
    .await;
    let own = n.results(sid);
    assert!(!own[0].1, "{own:#?}");
    assert!(n.started().is_empty(), "a server started in plan mode");
}

/// L9: a tool node.toml denies is not offered (an MCP tool), and a call
/// the node refuses says so, not that the person denied it.
#[tokio::test]
async fn a_tool_the_node_denies_is_not_offered_and_its_refusal_names_the_node() {
    let mut n = node_with(&[], |p| {
        p.denied_tools = vec!["mcp__fake__echo".into(), "write".into()]
    })
    .await;
    let sid = n.create(ApprovalMode::Auto).await;
    let task = script(json!([
        call("mcp__fake__echo", json!({"text": "x"})),
        call("mcp__fake__pid", json!({})),
    ]));
    n.turn(
        sid,
        &format!("delegate: helper={task}"),
        lead(
            &["read"],
            vec![spec("helper", &["read", "mcp"], vec![], json!({}))],
        ),
    )
    .await;
    let res = n.child_results(sid);
    assert!(
        !res[0].2 && unknown(&res[0].3, "mcp__fake__echo"),
        "{res:#?}"
    );
    assert!(res[1].2, "{res:#?}");
    n.turn(
        sid,
        &script(json!([call(
            "write",
            json!({"path": "w.txt", "content": "x"})
        )])),
        lead(&["read", "write"], vec![]),
    )
    .await;
    let own = n.results(sid);
    let last = own.last().unwrap();
    assert!(
        !last.1 && last.2.contains("denied by this node's policy") && !last.2.contains("The user"),
        "{own:#?}"
    );
    assert!(!n.ws.join("w.txt").exists());
}
