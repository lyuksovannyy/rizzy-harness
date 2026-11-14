//! The repository's own instruction files (`AGENTS.md`, `CLAUDE.md`;
//! ADOPTION_PLAN X.2) reach the model through the node: in the request
//! the provider gets, quoted in the turn's context block, read again on
//! every turn, given to a sub-agent too, and never URL provenance.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::provider::CONTEXT_LEAD;
use harness_agent_native::{ChatMsg, NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, ToolCallRef};
use harness_core::policy::{ApprovalMode, ToolRisk};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use serde_json::{Value, json};

const U: &str = "https://evil.example/from-agents-md";

/// The mock, keeping the last user text of every request it is sent; a
/// message that is just `probe` makes it call `web_fetch` with [`U`] once
/// (the person never typed the URL).
struct Spy(Mock, Arc<Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl harness_agent_native::Provider for Spy {
    fn name(&self) -> &'static str {
        "spy"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::ChatRequest,
        deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<harness_agent_native::Completion, harness_agent_native::ProviderError> {
        let at = req
            .messages
            .iter()
            .rposition(|m| m.user_text().is_some())
            .unwrap();
        let user = req.messages[at].user_text().unwrap().to_string();
        self.1.lock().unwrap().push(user.clone());
        let theirs = harness_agent_native::provider::split_context(&user)
            .map_or(user.as_str(), |(_, rest)| rest);
        let answered = req.messages[at + 1..]
            .iter()
            .any(|m| matches!(m, ChatMsg::Tool { .. }));
        if theirs == "probe" && !answered {
            return Ok(harness_agent_native::Completion {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: "web_fetch".into(),
                    input: json!({ "url": U }),
                }],
                usage: None,
                stop: Default::default(),
            });
        }
        self.0.complete(req, deltas).await
    }
}

/// Stands in for `web_fetch`: says whether `url` is URL provenance.
struct Probe;

#[async_trait::async_trait]
impl harness_tools::Tool for Probe {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        "probe"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &harness_tools::ToolCtx, input: Value) -> harness_tools::ToolOutcome {
        let url = input["url"].as_str().unwrap_or("");
        harness_tools::ToolOutcome::ok(format!(
            "seen={}",
            ctx.observed.iter().any(|t| t.contains(url))
        ))
    }
}

struct Node {
    _dir: tempfile::TempDir,
    /// A repository: `.git`, `AGENTS.md`, `sub/CLAUDE.md`, `sub/keep.txt`.
    repo: std::path::PathBuf,
    node: NodeRuntime,
    link: NodeLink,
    seen: Arc<Mutex<Vec<String>>>,
}

async fn start() -> Node {
    start_as(harness_core::event::Profile::Agentic).await
}

async fn start_as(profile: harness_core::event::Profile) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap().join("ws/repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    std::fs::write(
        repo.join("AGENTS.md"),
        format!("Run cargo fmt.\nDocs live at {U}\n"),
    )
    .unwrap();
    std::fs::write(repo.join("sub/CLAUDE.md"), "Keep sub small.\n").unwrap();
    std::fs::write(repo.join("sub/keep.txt"), "kept\n").unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let spy = Spy(
        Mock {
            delay: Duration::from_millis(1),
        },
        seen.clone(),
    );
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(Probe));
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().join("ws")],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(spy),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        other => panic!("expected hello, got {other:?}"),
    }
    Node {
        _dir: dir,
        repo,
        node,
        link,
        seen,
    }
}

impl Node {
    /// Send `m` and wait for its ack, which must be ok.
    async fn send(&mut self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
                .await
                .expect("no ack")
                .expect("link closed");
            if let NodeToMaster::Ack { ok, error, .. } = msg {
                assert!(ok, "nack: {error:?}");
                return;
            }
        }
    }

    async fn create(&mut self, cwd: &std::path::Path) -> SessionId {
        self.create_as(cwd, None).await
    }

    async fn create_as(
        &mut self,
        cwd: &std::path::Path,
        profile: Option<harness_core::event::Profile>,
    ) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        })
        .await;
        sid
    }

    /// Prompt `sid` (as `agent`, when given) and wait for its turn to end.
    async fn turn(
        &mut self,
        sid: SessionId,
        text: &str,
        agent: Option<harness_core::agent::AgentSpec>,
    ) {
        let before = self.ended(sid);
        self.send(MasterToNode::Prompt {
            savings: None,
            context: Some("Person: Ann".into()),
            reply: None,
            agent,
            attachments: vec![],
            compare: vec![],
            req_id: "p".into(),
            session_id: sid,
            text: text.into(),
            images: vec![],
        })
        .await;
        let by = std::time::Instant::now() + Duration::from_secs(20);
        while self.ended(sid) == before {
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn ended(&self, sid: SessionId) -> usize {
        self.node
            .store()
            .events_after(sid, 0, 10_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "turn_ended")
            .count()
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

/// The request the provider gets carries the files in the turn's context
/// block, quoted under the harness's line, root first, each under its path
/// from the session folder, before the master's own context; an edit to a
/// file is in the next turn's request.
#[tokio::test]
async fn the_provider_gets_the_files_in_the_context_block_and_edits_next_turn() {
    let mut n = start().await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    n.turn(sid, "hello", None).await;
    let first = n.requests().pop().expect("a request");
    assert!(first.starts_with(CONTEXT_LEAD), "{first}");
    let (context, rest) = harness_agent_native::provider::split_context(&first).unwrap();
    assert_eq!(rest, "hello");
    let root = context
        .find("==> ../AGENTS.md <==\nRun cargo fmt.")
        .expect(&context);
    let sub = context
        .find("==> CLAUDE.md <==\nKeep sub small.")
        .expect(&context);
    let master = context.find("Person: Ann").expect(&context);
    assert!(root < sub && sub < master, "{context}");
    // Each file line is quoted in what the provider gets.
    assert!(first.contains("\n| Run cargo fmt.\n"), "{first}");

    std::fs::write(n.repo.join("sub/CLAUDE.md"), "Now split sub up.\n").unwrap();
    n.turn(sid, "again", None).await;
    let second = n.requests().pop().unwrap();
    assert!(second.contains("| Now split sub up."), "{second}");
    assert!(!second.contains("Keep sub small."), "{second}");
}

/// A sub-agent `delegate` starts gets the same instruction files as its
/// parent (it gets no project context from the master).
#[tokio::test]
async fn a_sub_agent_gets_the_files_too() {
    let mut n = start().await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let spec = |id: &str, tools: Vec<String>, delegates| harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(tools),
        model: None,
        fallbacks: vec![],
        unavailable: vec![],
        no_effort: vec![],
        context_windows: Default::default(),
        effort: None,
        budget: Default::default(),
        ui: Default::default(),
        prompt_hash: "h".into(),
        deny_tools: vec![],
        delegates,
        caps: None,
    };
    let scout = spec("scout", vec!["read".into()], vec![]);
    let lead = spec("lead", vec!["read".into()], vec![scout]);
    n.turn(sid, "delegate: scout=read: keep.txt", Some(lead))
        .await;
    let child = n
        .requests()
        .into_iter()
        .find(|r| r.ends_with("\n\nread: keep.txt"))
        .unwrap_or_else(|| panic!("no request for the sub-agent: {:?}", n.requests()));
    let (context, _) = harness_agent_native::provider::split_context(&child).unwrap();
    assert!(
        context.contains("==> ../AGENTS.md <==\nRun cargo fmt."),
        "{context}"
    );
    assert!(
        context.contains("==> CLAUDE.md <==\nKeep sub small."),
        "{context}"
    );
    // The master's context is the parent's, not the sub-agent's.
    assert!(!context.contains("Person: Ann"), "{context}");
}

/// A URL that is only in `AGENTS.md` is not URL provenance: `web_fetch`
/// does not take it.
#[tokio::test]
async fn a_url_only_in_agents_md_is_not_fetchable() {
    let mut n = start().await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    n.turn(sid, "probe", None).await;
    assert!(
        n.requests()[0].contains(U),
        "the file reached the model: {:?}",
        n.requests()
    );
    let out = n
        .node
        .store()
        .events_after(sid, 0, 10_000)
        .unwrap()
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) => Some(f.output),
            _ => None,
        })
        .expect("the probe ran");
    assert_eq!(out, "seen=false");
}

/// A chat session (no folder of its own) gets no instruction files, even
/// when the folder it was asked for holds some.
#[tokio::test]
async fn a_chat_session_gets_none() {
    let mut n = start_as(harness_core::event::Profile::Chat).await;
    let repo = n.repo.clone();
    let sid = n
        .create_as(&repo, Some(harness_core::event::Profile::Chat))
        .await;
    n.turn(sid, "hello", None).await;
    let sent = n.requests().pop().expect("a request");
    assert!(sent.contains("Person: Ann"), "{sent}");
    assert!(!sent.contains("AGENTS.md"), "{sent}");
    assert!(!sent.contains("Run cargo fmt"), "{sent}");
}
