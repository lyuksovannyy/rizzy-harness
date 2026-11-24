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
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
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
        if theirs == "grants" && !answered {
            return Ok(harness_agent_native::Completion {
                text: String::new(),
                tool_calls: vec![ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: "grants".into(),
                    input: json!({}),
                }],
                usage: None,
                stop: Default::default(),
                thinking: None,
            });
        }
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
                thinking: None,
            });
        }
        self.0.complete(req, deltas).await
    }
}

/// A test tool: the node owner's skill folders this call's shell would be
/// granted read-only (`skills::shell_reads`, what `bash` binds), one per
/// line.
struct Grants;

#[async_trait::async_trait]
impl harness_tools::Tool for Grants {
    fn name(&self) -> &str {
        "grants"
    }
    fn description(&self) -> &str {
        "the shell's skill grants"
    }
    fn schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }
    fn risk(&self) -> harness_core::policy::ToolRisk {
        harness_core::policy::ToolRisk::ReadOnly
    }
    async fn run(
        &self,
        ctx: &harness_tools::ToolCtx,
        _input: serde_json::Value,
    ) -> harness_tools::ToolOutcome {
        let paths: Vec<String> = harness_tools::skills::shell_reads(ctx)
            .into_iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        harness_tools::ToolOutcome::ok(format!("GRANTS\n{}", paths.join("\n")))
    }
}

struct Node {
    _dir: tempfile::TempDir,
    /// A repository with `AGENTS.md`, a `sub` folder and skills.
    repo: PathBuf,
    node: NodeRuntime,
    link: NodeLink,
    seen: Arc<Mutex<Vec<String>>>,
    /// What its `node.hello` announced.
    capabilities: Vec<String>,
    /// Its `node.plugins` (X.34).
    plugins: harness_protocol::NodePlugins,
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
            {
                let mut tools = harness_tools::builtin();
                tools.push(Arc::new(Grants));
                tools
            },
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
    let capabilities = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    let plugins = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Plugins { plugins } => plugins,
        other => panic!("expected node.plugins, got {other:?}"),
    };
    Node {
        _dir: dir,
        repo,
        node,
        link,
        seen,
        capabilities,
        plugins,
    }
}

/// Both kinds of root, as node.toml would name them.
fn both(home: &Path) -> NodePolicy {
    NodePolicy {
        skills: SkillsPolicy {
            repository: true,
            roots: vec![home.to_path_buf()],
            ..Default::default()
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
        vision: vec![],
        context_windows: Default::default(),
        effort: None,
        ui: Default::default(),
        no_personalisation: false,
        prompt_hash: "h".into(),
        deny_tools: vec![],
        deny_skills: vec![],
        deny_mcp_servers: vec![],
        stored_skills: vec![],
        connector_tools: vec![],
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
        self.send(MasterToNode::Prompt(PromptMsg {
            context: Some(Box::new(harness_core::backend::MasterContext {
                turn: Some("Person: Ann".into()),
                standing: vec![],
                unattended: false,
                push: None,
            })),
            agent,
            ..PromptMsg::test(sid, text)
        }))
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
            ..Default::default()
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

/// X.34 (PLAN.md T80): a skill the turn's agent may not use in its
/// workspace is not listed, the tool refuses it with the reason, and a
/// person's `/name` of it is answered with a `[harness]` line instead of
/// an invitation to load it; by name, so the repository's skill of that
/// name does not stand in; the other skills stay.
#[tokio::test]
async fn a_withheld_skill_is_not_listed_loaded_or_invoked() {
    let mut n = start(both).await;
    let repo = n.repo.clone();
    let mut coder = spec("coder", &["read", "skill"], vec![]);
    coder.deny_skills = vec!["deploy".into()];
    let sid = n.create(&repo).await;
    n.turn(sid, "load:deploy", Some(coder.clone())).await;
    let first = n.requests()[0].clone();
    assert!(first.contains("| - release: Cut a release"), "{first}");
    assert!(!first.contains("deploy:"), "{first}");
    assert!(!first.contains("deploy body"), "{first}");
    let loaded = n.loaded(sid);
    assert!(!loaded[0].0, "{loaded:?}");
    assert!(
        loaded[0].1.contains("is not available to this agent"),
        "{loaded:?}"
    );
    assert!(!loaded[0].1.contains("deploy body"), "{loaded:?}");
    n.turn(sid, "/deploy now", Some(coder)).await;
    let reqs = n.requests();
    let slash = reqs
        .iter()
        .rev()
        .find(|r| r.contains("/deploy now"))
        .unwrap();
    assert!(
        slash.contains(
            "[harness] The person's message starts with /deploy: they ask for the \
             skill `deploy`, which this agent may not use here"
        ),
        "{slash}"
    );
    assert!(
        !slash.contains("Load it with the `skill` tool first"),
        "{slash}"
    );
    let loaded = n.loaded(sid);
    assert!(
        loaded
            .iter()
            .all(|(ok, out)| !ok && !out.contains("deploy body")),
        "{loaded:?}"
    );
}

/// A sub-agent follows its own spec's list, not its parent's: denied to the
/// parent, the child still sees it; denied to the child, the parent's
/// catalogue does not reach it.
#[tokio::test]
async fn a_sub_agent_follows_its_own_list() {
    let mut n = start(both).await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let scout = spec("scout", &["read", "skill"], vec![]);
    let mut lead = spec("lead", &["read", "skill"], vec![scout]);
    lead.deny_skills = vec!["release".into()];
    n.turn(sid, "delegate: scout=read: keep.txt", Some(lead))
        .await;
    let reqs = n.requests();
    assert!(!reqs[0].contains("release: Cut a release"), "{}", reqs[0]);
    let child = reqs
        .iter()
        .find(|r| r.ends_with("\n\nread: keep.txt"))
        .unwrap_or_else(|| panic!("no request for the sub-agent: {reqs:?}"));
    assert!(child.contains("| - release: Cut a release"), "{child}");

    let mut n = start(both).await;
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let mut scout = spec("scout", &["read", "skill"], vec![]);
    scout.deny_skills = vec!["release".into()];
    let lead = spec("lead", &["read", "skill"], vec![scout]);
    n.turn(sid, "delegate: scout=read: keep.txt", Some(lead))
        .await;
    let reqs = n.requests();
    assert!(reqs[0].contains("release: Cut a release"), "{}", reqs[0]);
    let child = reqs
        .iter()
        .find(|r| r.ends_with("\n\nread: keep.txt"))
        .unwrap_or_else(|| panic!("no request for the sub-agent: {reqs:?}"));
    assert!(!child.contains("release: Cut a release"), "{child}");
}

/// The node says it enforces the lists and announces its own roots'
/// skills (not a repository's), names and one-line descriptions only.
#[tokio::test]
async fn the_node_announces_its_own_skills_and_enforces_the_lists() {
    let n = start(both).await;
    assert!(
        n.capabilities
            .iter()
            .any(|c| c == harness_protocol::CAP_PLUGIN_ACCESS)
    );
    let names: Vec<&str> = n.plugins.skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["deploy", "manual"], "{:?}", n.plugins);
    assert_eq!(n.plugins.skills[0].description, "The owner's deploy");
    assert!(n.plugins.mcp_servers.is_empty());
    let text = serde_json::to_string(&n.plugins).unwrap();
    assert!(
        !text.contains("/home") && !text.contains("SKILL.md"),
        "{text}"
    );
    // node.toml without the tool: nothing announced.
    let n = start(|home| NodePolicy {
        denied_tools: vec!["skill".into()],
        ..both(home)
    })
    .await;
    assert!(n.plugins.skills.is_empty(), "{:?}", n.plugins);
}

/// Review of PR 147 (M1, M4): through the node, a turn whose agent may not
/// use `deploy` gives its shell the kept skills' own folders and never a
/// whole root: not the first root holding `deploy`, nor a second root
/// whose own `deploy` discovery did not use; the same agent without the
/// denial gets both roots.
#[tokio::test]
async fn the_shell_is_not_granted_a_withheld_skills_folder() {
    let mut n = start(|home| {
        let second = home.parent().unwrap().join("second");
        put(
            &second.join("deploy/SKILL.md"),
            &skill_md("deploy", "Second deploy", "second deploy body"),
        );
        put(
            &second.join("other/SKILL.md"),
            &skill_md("other", "Other", "o"),
        );
        NodePolicy {
            skills: SkillsPolicy {
                repository: true,
                roots: vec![home.to_path_buf(), second],
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .await;
    let repo = n.repo.clone();
    let home = n
        .repo
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("home/skills");
    let second = home.parent().unwrap().join("second");
    let sid = n.create(&repo).await;
    let mut coder = spec("coder", &["read", "skill", "grants"], vec![]);
    n.turn(sid, "grants", Some(coder.clone())).await;
    coder.deny_skills = vec!["deploy".into()];
    n.turn(sid, "grants", Some(coder)).await;
    let grants: Vec<String> = n
        .loaded(sid)
        .into_iter()
        .filter(|(_, out)| out.starts_with("GRANTS"))
        .map(|(_, out)| out)
        .collect();
    assert_eq!(grants.len(), 2, "{grants:?}");
    let lines = |g: &str| g.lines().skip(1).map(str::to_string).collect::<Vec<_>>();
    let shown = |p: &std::path::Path| p.display().to_string();
    assert_eq!(lines(&grants[0]), [shown(&home), shown(&second)]);
    let narrowed = lines(&grants[1]);
    assert_eq!(
        narrowed,
        [shown(&home.join("manual")), shown(&second.join("other"))],
        "{narrowed:?}"
    );
}

/// Review of PR 147 (L4): the announcement is read once per runtime and
/// never holds hello up: a later link gets hello first and the same list
/// (a skill added since is announced after a restart), so a slow or hung
/// folder costs at most one read, not one per reconnect.
#[tokio::test]
async fn the_announcement_is_read_once_and_follows_hello() {
    let n = start(both).await;
    let home = n
        .repo
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("home/skills");
    put(
        &home.join("later/SKILL.md"),
        &skill_md("later", "Added later", "l"),
    );
    let mut link = n.node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        other => panic!("expected hello first, got {other:?}"),
    }
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Plugins { plugins } => assert_eq!(plugins, n.plugins),
        other => panic!("expected node.plugins, got {other:?}"),
    }
}

// ----- Stored skills (X.34 step B, PLAN.md T81) -----

fn stored(names: &[(&str, &str)]) -> Vec<harness_core::skills::StoredSkillEntry> {
    names
        .iter()
        .map(|(n, d)| harness_core::skills::StoredSkillEntry {
            name: n.to_string(),
            description: d.to_string(),
            scope: harness_core::skills::StoredScope::Workspace,
        })
        .collect()
}

impl Node {
    /// A turn during which the test is the master: each `skill.request`
    /// is recorded (session, name) and answered with `body(name)`.
    async fn turn_answering(
        &mut self,
        sid: SessionId,
        text: &str,
        agent: Option<harness_core::agent::AgentSpec>,
        body: impl Fn(&str) -> Result<String, String>,
    ) -> Vec<(SessionId, String)> {
        let before = self.ended(sid);
        let mut asked = Vec::new();
        self.link
            .to_node
            .try_send(MasterToNode::Prompt(PromptMsg {
                context: Some(Box::new(harness_core::backend::MasterContext {
                    turn: Some("Person: Ann".into()),
                    standing: vec![],
                    unattended: false,
                    push: None,
                })),
                agent,
                ..PromptMsg::test(sid, text)
            }))
            .unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(20);
        while self.ended(sid) == before {
            assert!(std::time::Instant::now() < by, "the turn did not end");
            let Ok(Some(msg)) =
                tokio::time::timeout(Duration::from_millis(20), self.link.from_node.recv()).await
            else {
                continue;
            };
            if let NodeToMaster::Skill {
                req_id,
                session_id,
                name,
            } = msg
            {
                let (body, error) = match body(&name) {
                    Ok(b) => (Some(b), None),
                    Err(e) => (None, Some(e)),
                };
                asked.push((session_id, name));
                self.link
                    .to_node
                    .try_send(MasterToNode::SkillResult {
                        req_id,
                        body,
                        error,
                    })
                    .unwrap();
            }
        }
        asked
    }
}

/// The master's stored skills are listed beside the node's own, whose of a
/// name wins (its body, no request); loading a stored one asks the master
/// for that name and session only and returns its text; the node says it
/// takes them.
#[tokio::test]
async fn stored_skills_list_under_the_nodes_own_and_load_from_the_master() {
    let mut n = start(both).await;
    assert!(
        n.capabilities
            .iter()
            .any(|c| c == harness_protocol::CAP_STORED_SKILLS),
        "{:?}",
        n.capabilities
    );
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let mut coder = spec("coder", &["read", "skill"], vec![]);
    coder.stored_skills = stored(&[("deploy", "The master's deploy"), ("notes", "Take notes")]);
    let asked = n
        .turn_answering(sid, "load:notes", Some(coder.clone()), |name| {
            Ok(format!("STORED {name} BODY"))
        })
        .await;
    assert_eq!(asked, [(sid, "notes".to_string())]);
    let first = n.requests()[0].clone();
    assert!(
        first.contains(harness_tools::skills::STORED_LEAD),
        "{first}"
    );
    assert!(first.contains("| - notes: Take notes"), "{first}");
    assert!(first.contains("| - deploy: The owner's deploy"), "{first}");
    assert!(!first.contains("master's deploy"), "{first}");
    let loaded = n.loaded(sid);
    assert!(loaded[0].0, "{loaded:?}");
    assert!(loaded[0].1.ends_with("STORED notes BODY"), "{loaded:?}");
    assert!(loaded[0].1.contains("kept on the server"), "{loaded:?}");
    // The clash: the node's own body, and the master is not asked.
    let asked = n
        .turn_answering(sid, "load:deploy", Some(coder), |_| Ok("NOT THIS".into()))
        .await;
    assert!(asked.is_empty(), "{asked:?}");
    let loaded = n.loaded(sid);
    assert!(loaded[1].1.ends_with("owner deploy body"), "{loaded:?}");
}

/// `[skills] from_master = false`: no capability, no list entry, no
/// request, for the turn's agent and for its sub-agents.
#[tokio::test]
async fn from_master_off_ignores_the_list_and_never_asks() {
    let mut n = start(|home| {
        let mut p = both(home);
        p.skills.from_master = false;
        p
    })
    .await;
    assert!(
        !n.capabilities
            .iter()
            .any(|c| c == harness_protocol::CAP_STORED_SKILLS)
    );
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let mut scout = spec("scout", &["read", "skill"], vec![]);
    scout.stored_skills = stored(&[("notes", "Take notes")]);
    let mut lead = spec("lead", &["read", "skill"], vec![scout]);
    lead.stored_skills = stored(&[("notes", "Take notes")]);
    let asked = n
        .turn_answering(
            sid,
            "delegate: scout=read: keep.txt",
            Some(lead.clone()),
            |_| Ok("NO".into()),
        )
        .await;
    assert!(asked.is_empty());
    for r in n.requests() {
        assert!(!r.contains("notes: Take notes"), "{r}");
    }
    let asked = n
        .turn_answering(sid, "load:notes", Some(lead), |_| Ok("NO".into()))
        .await;
    assert!(asked.is_empty());
    let loaded = n.loaded(sid);
    let last = loaded.last().unwrap();
    assert!(!last.0 && last.1.contains("no skill `notes`"), "{loaded:?}");
}

/// An agent that neither works on files nor runs commands is offered the
/// stored skills only: no skill folder of the node or the repository is
/// listed or loaded for it.
#[tokio::test]
async fn an_agent_without_file_access_gets_stored_skills_only() {
    let mut n = start(both).await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let mut chat = spec("general", &["skill", "web_fetch"], vec![]);
    chat.stored_skills = stored(&[("notes", "Take notes")]);
    let asked = n
        .turn_answering(sid, "load:release", Some(chat.clone()), |_| Ok("NO".into()))
        .await;
    assert!(asked.is_empty());
    let first = n.requests()[0].clone();
    assert!(first.contains("| - notes: Take notes"), "{first}");
    assert!(!first.contains("release: Cut a release"), "{first}");
    assert!(!first.contains("owner's deploy"), "{first}");
    let loaded = n.loaded(sid);
    assert!(
        !loaded[0].0 && !loaded[0].1.contains("Tag it"),
        "{loaded:?}"
    );
    let asked = n
        .turn_answering(sid, "load:notes", Some(chat), |_| Ok("STORED".into()))
        .await;
    assert_eq!(asked, [(sid, "notes".to_string())]);
}

// ---- the composer's `/` menu (X.65, PLAN.md T46) ---------------------------

impl Node {
    /// Ask `session.skills_menu` for `agent` and wait for its one answer;
    /// `None` when `busy`. The requests the model got stay as they were:
    /// the menu starts no turn, and appends no event.
    async fn menu(
        &mut self,
        sid: SessionId,
        agent: harness_core::agent::AgentSpec,
    ) -> Option<Vec<(String, String)>> {
        let events = self
            .node
            .store()
            .events_after(sid, 0, 10_000)
            .unwrap()
            .len();
        let asked = self.requests().len();
        self.link
            .to_node
            .try_send(MasterToNode::SkillsMenu {
                req_id: "m".into(),
                session_id: sid,
                agent,
            })
            .unwrap();
        let answer = loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
                .await
                .expect("no answer")
                .expect("link closed");
            match msg {
                NodeToMaster::SkillsMenuResult {
                    req_id,
                    skills,
                    busy,
                } => {
                    assert_eq!(req_id, "m");
                    break (!busy).then(|| {
                        skills
                            .into_iter()
                            .map(|s| (s.name, s.description))
                            .collect::<Vec<_>>()
                    });
                }
                NodeToMaster::Ack { .. } => panic!("the menu is never acked"),
                _ => {}
            }
        };
        assert_eq!(self.requests().len(), asked, "no model request");
        assert_eq!(
            self.node
                .store()
                .events_after(sid, 0, 10_000)
                .unwrap()
                .len(),
            events,
            "no event"
        );
        answer
    }
}

fn menu_names(m: &[(String, String)]) -> Vec<&str> {
    m.iter().map(|(n, _)| n.as_str()).collect()
}

/// The menu is the turn's catalogue as a person may ask for it: the owner's
/// `deploy` (not the repository's), the repository's `release`, and the
/// manual skill, which the model is not told of but a person may ask for;
/// the node announces the capability.
#[tokio::test]
async fn the_menu_lists_the_turns_skills_and_the_manual_one() {
    let mut n = start(both).await;
    assert!(
        n.capabilities
            .contains(&harness_protocol::CAP_SKILLS_MENU.to_string()),
        "{:?}",
        n.capabilities
    );
    let sub = n.repo.join("sub");
    let sid = n.create(&sub).await;
    let m = n
        .menu(sid, spec("a", &["read", "skill"], vec![]))
        .await
        .unwrap();
    assert_eq!(menu_names(&m), ["deploy", "manual", "release"], "{m:?}");
    assert_eq!(m[0].1, "The owner's deploy");
    assert_eq!(m[1].1, "Only by hand");
    // The list the model gets is the same minus the manual one.
    n.turn(sid, "hello", None).await;
    let first = n.requests()[0].clone();
    assert!(first.contains("| - release: Cut a release"), "{first}");
    assert!(!first.contains("manual"), "{first}");
}

/// node.toml decides and the agent's own switch holds: `repository = false`
/// lists the owner's roots only; `denied_tools` naming `skill`, an agent
/// without the tool, and a skill the agent may not use list nothing of it.
#[tokio::test]
async fn the_menu_follows_node_toml_and_the_agents_switches() {
    let mut n = start(|home| NodePolicy {
        skills: SkillsPolicy {
            repository: false,
            roots: vec![home.to_path_buf()],
            ..Default::default()
        },
        ..Default::default()
    })
    .await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let m = n
        .menu(sid, spec("a", &["read", "skill"], vec![]))
        .await
        .unwrap();
    assert_eq!(menu_names(&m), ["deploy", "manual"], "{m:?}");

    let mut n = start(|home| NodePolicy {
        denied_tools: vec!["skill".into()],
        ..both(home)
    })
    .await;
    assert!(
        !n.capabilities
            .contains(&harness_protocol::CAP_SKILLS_MENU.to_string()),
        "{:?}",
        n.capabilities
    );
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let m = n
        .menu(sid, spec("a", &["read", "skill"], vec![]))
        .await
        .unwrap();
    assert!(m.is_empty(), "{m:?}");

    let mut n = start(both).await;
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let m = n.menu(sid, spec("a", &["read"], vec![])).await.unwrap();
    assert!(m.is_empty(), "no skill tool: {m:?}");
    let mut denying = spec("a", &["read", "skill"], vec![]);
    denying.deny_skills = vec!["deploy".into(), "manual".into()];
    let m = n.menu(sid, denying).await.unwrap();
    assert_eq!(menu_names(&m), ["release"], "{m:?}");
}

/// A hostile repository: hundreds of skills, a megabyte description, names
/// with spaces, slashes, control and bidirectional characters and markup, a
/// folder named like a built-in, a `SKILL.md` that is a link out of its
/// folder, a skills folder that is a link out, a hidden-from-the-menu one:
/// the menu holds at most 150 entries of bounded, valid, inert text.
#[tokio::test]
async fn a_hostile_repository_gets_a_bounded_inert_menu() {
    let mut n = start(|_| NodePolicy {
        skills: SkillsPolicy {
            repository: true,
            roots: vec![],
            ..Default::default()
        },
        ..Default::default()
    })
    .await;
    let repo = n.repo.clone();
    let skills = repo.join(".agents/skills");
    for i in 0..900 {
        put(
            &skills.join(format!("bulk{i:05}/SKILL.md")),
            &skill_md(&format!("bulk{i:05}"), "d", "b"),
        );
    }
    put(
        &skills.join("big/SKILL.md"),
        &skill_md("big", &"x".repeat(1_000_000), "b"),
    );
    put(
        &skills.join("bigish/SKILL.md"),
        &skill_md("bigish", &"x".repeat(5_000), "b"),
    );
    put(
        &skills.join("Bad Name/SKILL.md"),
        &skill_md("my skill", "spaces", "b"),
    );
    put(
        &skills.join("sl/SKILL.md"),
        "---\nname: a/b\ndescription: slash\n---\nb\n",
    );
    put(
        &skills.join("ctl/SKILL.md"),
        "---\nname: \"x\\u202ey\"\ndescription: \"bidi\\u202e and \\u0007 bell <script>alert(1)</script>\"\n---\nb\n",
    );
    put(
        &skills.join("compact/SKILL.md"),
        &skill_md("compact", "shadow", "b"),
    );
    put(
        &skills.join("quiet/SKILL.md"),
        "---\nname: quiet\ndescription: Not for people\nuser-invocable: false\n---\nb\n",
    );
    // A SKILL.md that links out of its folder, and a skills folder that does.
    let outside = repo.parent().unwrap().join("outside");
    put(
        &outside.join("secret.md"),
        &skill_md("leak", "outside", "b"),
    );
    std::fs::create_dir_all(skills.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.md"), skills.join("linked/SKILL.md")).unwrap();
    put(
        &outside.join("farskill/SKILL.md"),
        &skill_md("far", "outside", "b"),
    );
    std::os::unix::fs::symlink(outside.join("farskill"), skills.join("far")).unwrap();
    let sid = n.create(&repo).await;
    let m = n
        .menu(sid, spec("a", &["skill", "read"], vec![]))
        .await
        .unwrap();
    assert!(m.len() <= harness_protocol::MAX_MENU_SKILLS, "{}", m.len());
    for (name, d) in &m {
        assert!(
            harness_tools::skills::menu(&harness_core::skills::Catalogue {
                skills: vec![harness_core::skills::Skill {
                    name: name.clone(),
                    description: d.clone(),
                    dir: Default::default(),
                    origin: harness_core::skills::SkillOrigin::Repository,
                    manual: false,
                }],
                ..Default::default()
            })
            .len()
                == 1,
            "{name}"
        );
        assert!(d.chars().count() <= harness_protocol::MAX_MENU_DESCRIPTION_CHARS);
        assert!(
            !d.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{d:?}"
        );
    }
    let names = menu_names(&m);
    for gone in [
        "compact", "quiet", "my skill", "a/b", "leak", "far", "linked",
    ] {
        assert!(!names.contains(&gone), "{gone} listed: {names:?}");
    }
    // A front matter past the reader's 16 KB is refused, as for a turn.
    assert!(!names.contains(&"big"), "{names:?}");
    assert!(names.contains(&"bigish"), "{names:?}");
    let big = m.iter().find(|(n, _)| n == "bigish").unwrap();
    assert_eq!(
        big.1.chars().count(),
        harness_protocol::MAX_MENU_DESCRIPTION_CHARS
    );
}

/// A chat the node does not know gets an empty menu, not an error; a plain
/// chat (no folder) lists only what the master stored for its agent.
#[tokio::test]
async fn an_unknown_chat_has_an_empty_menu_and_a_stored_skill_is_listed() {
    let mut n = start(|home| NodePolicy {
        skills: SkillsPolicy {
            from_master: true,
            ..both(home).skills
        },
        ..Default::default()
    })
    .await;
    let m = n
        .menu(SessionId::new(), spec("a", &["read", "skill"], vec![]))
        .await
        .unwrap();
    assert!(m.is_empty());
    let repo = n.repo.clone();
    let sid = n.create(&repo).await;
    let mut a = spec("a", &["read", "skill"], vec![]);
    a.stored_skills = vec![
        harness_core::skills::StoredSkillEntry {
            name: "from-server".into(),
            description: "Kept on the server".into(),
            scope: harness_core::skills::StoredScope::Server,
        },
        // The node's own folder wins a name.
        harness_core::skills::StoredSkillEntry {
            name: "deploy".into(),
            description: "stored deploy".into(),
            scope: harness_core::skills::StoredScope::Server,
        },
    ];
    let m = n.menu(sid, a).await.unwrap();
    assert_eq!(
        menu_names(&m),
        ["deploy", "from-server", "manual", "release"],
        "{m:?}"
    );
    assert_eq!(m[0].1, "The owner's deploy");
}
