//! `history` on a node (PLAN.md T91): the call reads the log of the session
//! it runs in, from the node's store, with nothing sent to the master; a
//! node restart between two calls changes no answer; a sub-agent's chat
//! reads its own log and nothing of its parent's; `denied_tools` turns it
//! off.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::history::History;
use harness_agent_native::provider::{ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::agent::{AgentSpec, ToolSelection};
use harness_core::event::{EventKind, Profile};
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

type Requests = Arc<Mutex<Vec<ChatRequest>>>;

struct Recording {
    inner: Mock,
    seen: Requests,
}

#[async_trait::async_trait]
impl Provider for Recording {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        self.inner.complete(req, d).await
    }
}

struct N {
    node: Option<NodeRuntime>,
    link: NodeLink,
    dir: tempfile::TempDir,
    seen: Requests,
    denied: Vec<String>,
    n: u32,
}

fn config(dir: &std::path::Path, seen: &Requests, denied: &[String]) -> NodeConfig {
    let mut policy = NodePolicy {
        workspace_roots: vec![dir.into()],
        max_approval_mode: ApprovalMode::Auto,
        denied_tools: denied.to_vec(),
        ..Default::default()
    };
    // Not about skills: the developer's own stay out.
    policy.skills.roots.clear();
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(History));
    NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Recording {
                inner: Mock {
                    delay: Duration::from_millis(5),
                },
                seen: seen.clone(),
            }),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    }
}

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => link,
        other => panic!("expected hello, got {other:?}"),
    }
}

async fn start(denied: &[&str]) -> N {
    let dir = tempfile::tempdir().unwrap();
    let seen: Requests = Arc::default();
    let denied: Vec<String> = denied.iter().map(|d| d.to_string()).collect();
    let node = NodeRuntime::start(config(dir.path(), &seen, &denied))
        .await
        .unwrap();
    let link = attach(&node).await;
    N {
        node: Some(node),
        link,
        dir,
        seen,
        denied,
        n: 0,
    }
}

/// An agent as the master resolves it.
fn agent(id: &str, tools: &[&str], delegates: Vec<AgentSpec>) -> AgentSpec {
    AgentSpec {
        agent_id: id.into(),
        name: format!("Agent {id}"),
        system_prompt: format!("You are @{id}."),
        tools: ToolSelection::List(tools.iter().map(|t| t.to_string()).collect()),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        prompt_hash: format!("h-{id}"),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
        delegates,
        caps: None,
        no_personalisation: false,
    }
}

impl N {
    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    fn req(&mut self) -> String {
        self.n += 1;
        format!("r{}", self.n)
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    fn store(&self) -> &Store {
        self.node.as_ref().unwrap().store()
    }

    async fn restart(&mut self) {
        drop(self.node.take());
        tokio::time::sleep(Duration::from_millis(200)).await;
        let node = NodeRuntime::start(config(self.dir.path(), &self.seen, &self.denied))
            .await
            .unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    async fn create(&mut self, a: &AgentSpec) -> SessionId {
        let sid = SessionId::new();
        let req_id = self.req();
        self.send(MasterToNode::Create {
            req_id: req_id.clone(),
            session_id: sid,
            title: "t".into(),
            cwd: self.dir.path().display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
            profile: None,
            agent: Some(a.clone()),
            incognito: false,
            no_callbacks: false,
            exec_mode: None,
        });
        loop {
            if let NodeToMaster::Ack {
                req_id: r,
                ok,
                error,
                ..
            } = self.next().await
                && r == req_id
            {
                assert!(ok, "{error:?}");
                return sid;
            }
        }
    }

    /// One turn under `a`, to its end.
    async fn turn(&mut self, sid: SessionId, a: &AgentSpec, text: &str) {
        let req_id = self.req();
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id,
            agent: Some(a.clone()),
            ..PromptMsg::test(sid, text)
        }));
        loop {
            if let NodeToMaster::Event { event } = self.next().await
                && event.session_id == sid
                && matches!(event.typed(), Ok(EventKind::TurnEnded(_)))
            {
                return;
            }
        }
    }

    /// The finished calls of `tool` in `sid`'s log: (ok, output).
    fn finished(&self, sid: SessionId, tool: &str) -> Vec<(bool, String)> {
        let evs = self.store().events_after(sid, 0, 100_000).unwrap();
        let ids: Vec<String> = evs
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallStarted(c)) if c.tool == tool => Some(c.call_id),
                _ => None,
            })
            .collect();
        evs.iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(f)) if ids.contains(&f.call_id) => {
                    Some((f.ok, f.output))
                }
                _ => None,
            })
            .collect()
    }
}

fn history(input: Value) -> String {
    format!("script: {}", json!([{"tool": "history", "input": input}]))
}

/// The call reads its own chat's earlier turns from the node's store.
#[tokio::test]
async fn a_call_reads_the_chats_own_log_on_the_node() {
    let mut n = start(&[]).await;
    let a = agent("a", &["read", "history"], vec![]);
    let sid = n.create(&a).await;
    n.turn(sid, &a, "remember the word kumquat").await;
    n.turn(sid, &a, &history(json!({}))).await;
    n.turn(sid, &a, &history(json!({"search": "KUMQUAT"})))
        .await;
    let out = n.finished(sid, "history");
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].0, "{out:?}");
    assert!(out[0].1.contains("turns 1-2 of 2"), "{}", out[0].1);
    assert!(
        out[0].1.contains("remember the word kumquat"),
        "{}",
        out[0].1
    );
    assert!(out[1].0 && out[1].1.contains("turn 1 |"), "{}", out[1].1);
    // Offered to the model, in the request.
    assert!(
        n.seen
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.tools.iter().any(|t| t.name == "history"))
    );
}

/// A node restart between two calls: the same answer, since everything
/// comes from the log.
#[tokio::test]
async fn a_restart_between_two_calls_changes_no_answer() {
    let mut n = start(&[]).await;
    let a = agent("a", &["read", "history"], vec![]);
    let sid = n.create(&a).await;
    n.turn(sid, &a, "first question about quokkas").await;
    n.turn(sid, &a, &history(json!({"turn": 1}))).await;
    n.restart().await;
    n.turn(sid, &a, &history(json!({"turn": 1}))).await;
    let out = n.finished(sid, "history");
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].0 && out[0].1.contains("quokkas"));
    // The header counts the turns the chat has by then (one more); the
    // turn itself is the same, line for line.
    let body = |o: &str| o.split_once('\n').unwrap().1.to_string();
    assert_eq!(body(&out[0].1), body(&out[1].1));
}

/// A sub-agent's chat reads its own log: its task, nothing of its
/// parent's conversation.
#[tokio::test]
async fn a_sub_agents_chat_reads_only_its_own_log() {
    let mut n = start(&[]).await;
    let reader = agent("reader", &["read", "history"], vec![]);
    let lead = agent("lead", &["read"], vec![reader]);
    let sid = n.create(&lead).await;
    n.turn(sid, &lead, "the parent's private word is PARENTSECRET")
        .await;
    let task = history(json!({}));
    n.turn(
        sid,
        &lead,
        &format!(
            "script: {}",
            json!([{"tool": "delegate", "input": {"agent": "reader", "task": task}}])
        ),
    )
    .await;
    let delegated = n.finished(sid, "delegate");
    let child: SessionId = delegated[0]
        .1
        .strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no child in {delegated:?}"));
    assert_ne!(child, sid);
    let out = n.finished(child, "history");
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].0, "{out:?}");
    assert!(out[0].1.contains("turns 1-1 of 1"), "{}", out[0].1);
    assert!(!out[0].1.contains("PARENTSECRET"), "{}", out[0].1);
    // The parent's own call sees the parent's chat, not the child's.
    n.turn(sid, &lead, &history(json!({}))).await;
}

/// `denied_tools` refuses it: the call is not run, so the log is not read.
#[tokio::test]
async fn denied_tools_turns_it_off() {
    let mut n = start(&["history"]).await;
    let a = agent("a", &["read", "history"], vec![]);
    let sid = n.create(&a).await;
    n.turn(sid, &a, "private words").await;
    n.turn(sid, &a, &history(json!({}))).await;
    let out = n.finished(sid, "history");
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(!out[0].0, "{out:?}");
    assert!(!out[0].1.contains("private words"), "{}", out[0].1);
}
