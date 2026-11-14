//! Skills (ADOPTION_PLAN X.2) through the node: the catalogue reaches the
//! model in the turn's context block after the instruction files, only
//! for an agent that has the `skill` tool and only from the roots
//! node.toml allows; the tool loads a body the node found; `/name`
//! invokes a skill for the turn; a sub-agent with the tool sees it too.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{ChatMsg, NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, ToolCallRef};
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime, SkillsPolicy};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use serde_json::json;

/// The mock, keeping the last user text of every request; a message
/// `load:<name>` or `/<name> ...` makes it call `skill` with that name once.
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
        let name = theirs
            .strip_prefix("load:")
            .or_else(|| theirs.strip_prefix('/'))
            .and_then(|r| r.split_whitespace().next());
        if let (Some(name), false) = (name, answered) {
            return Ok(harness_agent_native::Completion {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: "skill".into(),
                    input: json!({ "name": name }),
                }],
                usage: None,
                stop: Default::default(),
            });
        }
        self.0.complete(req, deltas).await
    }
}

struct Node {
    _dir: tempfile::TempDir,
    /// A repository with `AGENTS.md`, a `sub` folder and skills.
    repo: PathBuf,
    node: NodeRuntime,
    link: NodeLink,
    seen: Arc<Mutex<Vec<String>>>,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill_md(name: &str, description: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n{body}\n")
}

async fn start(policy: impl FnOnce(&Path) -> NodePolicy) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let repo = base.join("ws/repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    put(&repo.join("AGENTS.md"), "Run cargo fmt.\n");
    put(&repo.join("sub/keep.txt"), "kept\n");
    put(
        &repo.join(".agents/skills/release/SKILL.md"),
        &skill_md("release", "Cut a release", "Tag it, then write notes."),
    );
    put(
        &repo.join(".claude/skills/deploy/SKILL.md"),
        &skill_md(
            "deploy",
            "The repository's deploy",
            "repository deploy body",
        ),
    );
    let home = base.join("home/skills");
    put(
        &home.join("deploy/SKILL.md"),
        &skill_md("deploy", "The owner's deploy", "owner deploy body"),
    );
    put(
        &home.join("manual/SKILL.md"),
        "---\ndescription: Only by hand\ndisable-model-invocation: true\n---\nmanual body\n",
    );
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let spy = Spy(
        Mock {
            delay: Duration::from_millis(1),
        },
        seen.clone(),
    );
    let mut policy = policy(&home);
    policy.workspace_roots = vec![base.join("ws")];
    policy.max_approval_mode = ApprovalMode::Auto;
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store: Store::open(&base.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(spy),
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

/// Both kinds of root, as node.toml would name them.
fn both(home: &Path) -> NodePolicy {
    NodePolicy {
        skills: SkillsPolicy {
            repository: true,
            roots: vec![home.to_path_buf()],
        },
        ..Default::default()
    }
}

fn spec(
    id: &str,
    tools: &[&str],
    delegates: Vec<harness_core::agent::AgentSpec>,
) -> harness_core::agent::AgentSpec {
    harness_core::agent::AgentSpec {
        agent_id: id.into(),
        name: id.into(),
        system_prompt: "work".into(),
        tools: harness_core::agent::ToolSelection::List(
            tools.iter().map(|t| t.to_string()).collect(),
        ),
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
    }
}

impl Node {
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

    async fn create(&mut self, cwd: &Path) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
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
            agent_named: false,
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

    /// The outputs of `skill` calls in `sid`, in order.
    fn loaded(&self, sid: SessionId) -> Vec<(bool, String)> {
        self.node
            .store()
            .events_after(sid, 0, 10_000)
            .unwrap()
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::ToolCallFinished(f)) => Some((f.ok, f.output)),
                _ => None,
            })
            .collect()
    }
}

/// The catalogue rides in the turn's context after the instruction files
/// and before the master's own context, quoted; the owner's `deploy`
/// stands, the repository's is not listed; the manual skill is not
/// listed; `skill` returns the body the node found.
#[tokio::test]
async fn the_catalogue_is_in_the_context_and_the_tool_loads_a_body() {
    let mut n = start(both).await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    n.turn(sid, "load:deploy", None).await;
    let first = n.requests()[0].clone();
    let (context, rest) = harness_agent_native::provider::split_context(&first).unwrap();
    assert_eq!(rest, "load:deploy");
    let files = context.find("Run cargo fmt.").expect(&context);
    let lead = context.find(harness_tools::skills::LEAD).expect(&context);
    let master = context.find("Person: Ann").expect(&context);
    assert!(files < lead && lead < master, "{context}");
    assert!(
        context.contains("\n- deploy: The owner's deploy\n- release: Cut a release\n"),
        "{context}"
    );
    assert!(!context.contains("The repository's deploy"), "{context}");
    assert!(!context.contains("manual"), "{context}");
    assert!(first.contains("\n| - release: Cut a release\n"), "{first}");
    let loaded = n.loaded(sid);
    assert_eq!(loaded.len(), 1, "{loaded:?}");
    assert!(loaded[0].0, "{loaded:?}");
    assert!(loaded[0].1.ends_with("owner deploy body"), "{loaded:?}");
    assert!(
        loaded[0].1.contains("from this machine's skill folder"),
        "{loaded:?}"
    );
}

/// An agent without the `skill` tool gets no catalogue; a sub-agent it
/// delegates to that has the tool does.
#[tokio::test]
async fn only_agents_with_the_tool_see_the_catalogue() {
    let mut n = start(both).await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let scout = spec("scout", &["read", "skill"], vec![]);
    let lead = spec("lead", &["read"], vec![scout]);
    n.turn(sid, "delegate: scout=read: keep.txt", Some(lead))
        .await;
    let reqs = n.requests();
    let parent = &reqs[0];
    assert!(parent.contains("Run cargo fmt."), "{parent}");
    assert!(!parent.contains("Skills:"), "{parent}");
    let child = reqs
        .iter()
        .find(|r| r.ends_with("\n\nread: keep.txt"))
        .unwrap_or_else(|| panic!("no request for the sub-agent: {reqs:?}"));
    assert!(child.contains("| - release: Cut a release"), "{child}");
}

/// node.toml decides: `repository = false` lists only the owner's roots;
/// `denied_tools = ["skill"]` lists nothing and the tool is refused.
#[tokio::test]
async fn node_toml_turns_repository_skills_or_all_skills_off() {
    let mut n = start(|home| NodePolicy {
        skills: SkillsPolicy {
            repository: false,
            roots: vec![home.to_path_buf()],
        },
        ..Default::default()
    })
    .await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    n.turn(sid, "load:release", None).await;
    let first = n.requests()[0].clone();
    assert!(first.contains("| - deploy: The owner's deploy"), "{first}");
    assert!(!first.contains("release: Cut a release"), "{first}");
    let loaded = n.loaded(sid);
    assert!(!loaded[0].0, "{loaded:?}");
    assert!(
        loaded[0].1.contains("There is no skill `release` here"),
        "{loaded:?}"
    );

    let mut n = start(|home| NodePolicy {
        denied_tools: vec!["skill".into()],
        ..both(home)
    })
    .await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    n.turn(sid, "load:release", None).await;
    let first = n.requests()[0].clone();
    assert!(first.contains("Run cargo fmt."), "{first}");
    assert!(!first.contains("Skills:"), "{first}");
    assert!(!first.contains("Cut a release"), "{first}");
}

/// `/name` names a skill for this turn: the context says so, and a manual
/// skill loads in that turn, not in a later one.
#[tokio::test]
async fn a_slash_name_invokes_a_manual_skill_for_the_turn() {
    let mut n = start(both).await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    n.turn(sid, "/manual please", None).await;
    let first = n.requests()[0].clone();
    assert!(
        first.contains(
            "| [The person's message starts with /manual: they ask for the skill `manual`."
        ),
        "{first}"
    );
    n.turn(sid, "load:manual", None).await;
    let loaded = n.loaded(sid);
    assert_eq!(loaded.len(), 2, "{loaded:?}");
    assert!(loaded[0].0, "{loaded:?}");
    assert!(loaded[0].1.ends_with("manual body"), "{loaded:?}");
    assert!(!loaded[1].0, "{loaded:?}");
}

/// Review S5: only a person's own message invokes a skill; a delegated
/// task that starts with `/manual` does not (the sub-agent's context has no
/// invoked line, so the manual skill stays closed to it).
#[tokio::test]
async fn a_delegated_task_does_not_invoke_a_skill() {
    let mut n = start(both).await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let scout = spec("scout", &["read", "skill"], vec![]);
    let lead = spec("lead", &["read"], vec![scout]);
    n.turn(sid, "delegate: scout=/manual now", Some(lead)).await;
    let reqs = n.requests();
    let child = reqs
        .iter()
        .find(|r| r.ends_with("\n\n/manual now"))
        .unwrap_or_else(|| panic!("no request for the sub-agent: {reqs:?}"));
    assert!(child.contains("| - release: Cut a release"), "{child}");
    assert!(!child.contains("starts with /manual"), "{child}");
}
