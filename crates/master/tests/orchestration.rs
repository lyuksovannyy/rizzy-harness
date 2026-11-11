//! Teamlead delegation: parallel child sessions, caps, depth one, owner
//! inheritance, cancel cascading to children.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::Auth;
use harness_master::{CreateSpec, Master, PromptOpts};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;

async fn setup() -> (Master, String, tempfile::TempDir) {
    setup_with(Arc::new(Mock {
        delay: Duration::from_millis(10),
    }))
    .await
}

/// Passes requests to the mock and keeps the `delegate` tool each one offered.
struct Seen {
    inner: Mock,
    delegate_tools: std::sync::Mutex<Vec<harness_agent_native::provider::ToolSpec>>,
}

#[async_trait::async_trait]
impl harness_agent_native::provider::Provider for Seen {
    fn name(&self) -> &'static str {
        "seen"
    }
    async fn complete(
        &self,
        req: &harness_agent_native::provider::ChatRequest,
        deltas: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<
        harness_agent_native::provider::Completion,
        harness_agent_native::provider::ProviderError,
    > {
        if let Some(t) = req.tools.iter().find(|t| t.name == "delegate") {
            self.delegate_tools.lock().unwrap().push(t.clone());
        }
        self.inner.complete(req, deltas).await
    }
}

async fn setup_with(
    provider: Arc<dyn harness_agent_native::provider::Provider>,
) -> (Master, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![], false),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![ws.clone()],
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            provider,
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap();
    master.add_link(node.attach().await, true).await.unwrap();
    (master, ws.display().to_string(), dir)
}

async fn session(m: &Master, ws: &str, agent: &str) -> SessionId {
    let sid = m
        .create_session(CreateSpec {
            profile: Some(Profile::Agentic),
            title: "orchestrate".into(),
            cwd: ws.into(),
            agent_id: Some(agent.into()),
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
            ..Default::default()
        })
        .await
        .unwrap();
    m.inner_mirror()
        .claim_session(sid, Some("alex"), None)
        .unwrap();
    sid
}

async fn turns_ended(m: &Master, sid: SessionId, n: usize) -> Vec<RawEvent> {
    for _ in 0..500 {
        let evs = m.inner_mirror().events_after(sid, 0, 100_000).unwrap();
        if evs.iter().filter(|e| e.kind == "turn_ended").count() >= n {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("turn did not end");
}

fn outputs(evs: &[RawEvent]) -> Vec<String> {
    evs.iter()
        .filter(|e| e.kind == "tool_call_finished")
        .map(|e| e.payload["output"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn teamlead_delegates_in_parallel_within_caps() {
    let (m, ws, _d) = setup().await;
    // Coder and Writer can change files: they need a repository for their
    // worktrees, or they are not started.
    git(&ws, &["init", "-q"]);
    let sid = session(&m, &ws, "teamlead").await;
    m.prompt(
        sid,
        "delegate: coder=slow: 20 | scout=slow: 20 | writer=draft notes | coder=delegate: scout=nested | scout=fifth".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 1).await;
    let outs = outputs(&evs);
    assert_eq!(outs.len(), 5, "{outs:#?}");
    assert_eq!(
        outs.iter()
            .filter(|o| o.contains("cap reached: at most 4"))
            .count(),
        1,
        "{outs:#?}"
    );
    let done: Vec<&String> = outs
        .iter()
        .filter(|o| o.contains("ended (completed)"))
        .collect();
    assert_eq!(done.len(), 4, "{outs:#?}");
    assert!(
        outs.iter()
            .any(|o| o.contains("@writer ended (completed):\nYou said: draft notes"))
    );
    // Depth one: a sub-agent has no delegate tool.
    assert!(
        outs.iter().any(|o| o.contains("unknown tool `delegate`")),
        "{outs:#?}"
    );

    // Children: own sessions, linked to the parent, owned by the same user,
    // and they ran at the same time.
    let rows = m.inner_mirror().sessions().unwrap();
    let kids: Vec<_> = rows
        .iter()
        .filter(|r| r.parent_id.as_deref() == Some(&sid.to_string()))
        .collect();
    assert_eq!(kids.len(), 4);
    assert!(kids.iter().all(|k| k.owner_id.as_deref() == Some("alex")));
    let span = |id: SessionId| {
        let e = m.inner_mirror().events_after(id, 0, 10_000).unwrap();
        let t = |k: &str| e.iter().find(|x| x.kind == k).unwrap().ts_ms;
        (t("turn_started"), t("turn_ended"))
    };
    let spans: Vec<(i64, i64)> = kids.iter().map(|k| span(k.session_id)).collect();
    let latest_start = spans.iter().map(|s| s.0).max().unwrap();
    let earliest_end = spans.iter().map(|s| s.1).min().unwrap();
    assert!(
        latest_start < earliest_end,
        "sub-agents overlapped: {spans:?}"
    );
    // The parent's link to each child appeared while it ran.
    assert!(evs.iter().any(|e| e.kind == "tool_output_delta"
        && e.payload["text"].as_str().unwrap().starts_with("session: ")));

    // A plain agent gets no delegate tool at all.
    let plain = session(&m, &ws, "reviewer").await;
    m.prompt(plain, "delegate: scout=x".into(), PromptOpts::default())
        .await
        .unwrap();
    let evs = turns_ended(&m, plain, 1).await;
    assert!(outputs(&evs)[0].contains("unknown tool `delegate`"));

    // Agents that run only as agents are handed no sub-task, named by id
    // or by alias: the node refuses what is not in the roster.
    m.prompt(
        sid,
        "delegate: general=x | teamlead=y | ai=z".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 2).await;
    let outs: Vec<String> = outputs(&evs).into_iter().skip(5).collect();
    assert_eq!(outs.len(), 3, "{outs:#?}");
    assert!(
        outs.iter()
            .all(|o| o.contains("is not an agent this one can delegate to")),
        "{outs:#?}"
    );
}

/// The built-in roster against sub-agent isolation (T32): in a folder that
/// is not a git repository, the read-only delegates (Scout, Reviewer) still
/// run there, and the ones that can change files (Software Developer,
/// Writer) are refused with the reason. Researcher is refused for another
/// reason: its web and memory tools do not exist on a node.
#[tokio::test]
async fn builtin_delegates_outside_a_repository_by_role() {
    let (m, ws, _d) = setup().await;
    let sid = session(&m, &ws, "teamlead").await;
    // Two turns: a refused delegation still counts against the cap of 4.
    m.prompt(
        sid,
        "delegate: scout=look | reviewer=check | researcher=ask".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    turns_ended(&m, sid, 1).await;
    m.prompt(
        sid,
        "delegate: coder=build | writer=draft".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 2).await;
    let outs = outputs(&evs);
    assert_eq!(outs.len(), 5, "{outs:#?}");
    for (who, said) in [("scout", "look"), ("reviewer", "check")] {
        assert!(
            outs.iter()
                .any(|o| o.contains(&format!("@{who} ended (completed):\nYou said: {said}"))),
            "{who}: {outs:#?}"
        );
    }
    for who in ["coder", "writer"] {
        assert!(
            outs.iter()
                .any(|o| o.contains(&format!("@{who} was not started"))
                    && o.contains("not a git repository")),
            "{who}: {outs:#?}"
        );
    }
    assert!(
        outs.iter().any(|o| o.contains(
            "@researcher was not started: none of its tools (web search, web fetch, memory) \
             are available on this node. Do the work another way or tell the user."
        )),
        "researcher: {outs:#?}"
    );
    // Only the two read-only sub-agents with tools here got a session.
    let kids = m
        .inner_mirror()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|r| r.parent_id.as_deref() == Some(&sid.to_string()))
        .count();
    assert_eq!(kids, 2);
    assert_eq!(
        std::fs::read_dir(&ws).unwrap().count(),
        0,
        "folder unchanged"
    );
}

/// On an agentic node the web and memory tools are not registered, so
/// Researcher would run with no tools while the parent believes research
/// was done: it is not offered, and a call to it by name starts no session.
#[tokio::test]
async fn researcher_is_not_offered_on_a_node_without_its_tools() {
    let seen = Arc::new(Seen {
        inner: Mock {
            delay: Duration::from_millis(10),
        },
        delegate_tools: Default::default(),
    });
    let (m, ws, _d) = setup_with(seen.clone()).await;
    let sid = session(&m, &ws, "teamlead").await;
    m.prompt(sid, "hello".into(), PromptOpts::default())
        .await
        .unwrap();
    turns_ended(&m, sid, 1).await;
    let tool = seen.delegate_tools.lock().unwrap()[0].clone();
    let offered: Vec<&str> = tool.schema["properties"]["agent"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for who in ["scout", "reviewer", "coder", "writer"] {
        assert!(offered.contains(&who), "{who} missing from {offered:?}");
    }
    assert!(!offered.contains(&"researcher"), "{offered:?}");
    assert!(
        !tool.description.contains("researcher"),
        "{}",
        tool.description
    );

    m.prompt(
        sid,
        "delegate: researcher=ask".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 2).await;
    let last = outputs(&evs).pop().unwrap();
    assert!(
        last.starts_with("@researcher was not started: none of its tools"),
        "{last}"
    );
    let kids = m
        .inner_mirror()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|r| r.parent_id.as_deref() == Some(&sid.to_string()))
        .count();
    assert_eq!(kids, 0, "no child session");
}

#[tokio::test]
async fn cancelling_the_parent_cancels_children() {
    let (m, ws, _d) = setup().await;
    git(&ws, &["init", "-q"]);
    let sid = session(&m, &ws, "teamlead").await;
    m.prompt(
        sid,
        "delegate: coder=slow: 500".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    m.cancel(sid).await.unwrap();
    let evs = turns_ended(&m, sid, 1).await;
    let end = evs.iter().rev().find(|e| e.kind == "turn_ended").unwrap();
    assert_eq!(end.payload["reason"], "cancelled");
    let kid = m
        .inner_mirror()
        .sessions()
        .unwrap()
        .into_iter()
        .find(|r| r.parent_id.as_deref() == Some(&sid.to_string()))
        .unwrap();
    let kevs = turns_ended(&m, kid.session_id, 1).await;
    let kend = kevs.iter().rev().find(|e| e.kind == "turn_ended").unwrap();
    assert_eq!(kend.payload["reason"], "cancelled");
}

fn git(dir: &str, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn parallel_coders_get_worktrees_and_merge_back() {
    let (m, ws, _d) = setup().await;
    git(&ws, &["init", "-q"]);
    std::fs::write(format!("{ws}/shared.txt"), "base\n").unwrap();
    git(&ws, &["add", "."]);
    git(&ws, &["commit", "-qm", "init"]);
    // Uncommitted user work is visible to the sub-agents.
    std::fs::write(format!("{ws}/draft.txt"), "wip\n").unwrap();

    let sid = session(&m, &ws, "teamlead").await;
    m.prompt(
        sid,
        "delegate: coder=run: cat draft.txt > a.txt | coder=run: echo b > b.txt".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 1).await;
    let outs = outputs(&evs);
    assert!(
        outs.iter().all(|o| o.contains("1 changed file(s) applied")),
        "{outs:#?}"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{ws}/a.txt")).unwrap(),
        "wip\n"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{ws}/b.txt")).unwrap(),
        "b\n"
    );
    // The children worked elsewhere, and their worktrees are gone.
    let kids: Vec<_> = m
        .inner_mirror()
        .sessions()
        .unwrap()
        .into_iter()
        .filter(|r| r.parent_id.as_deref() == Some(&sid.to_string()))
        .collect();
    assert!(
        kids.iter().all(|k| k.cwd.contains("harness-worktrees")),
        "{kids:?}"
    );
    assert!(kids.iter().all(|k| !std::path::Path::new(&k.cwd).exists()));

    // Two sub-agents editing the same file: one applies, the other is kept aside.
    m.prompt(
        sid,
        "delegate: coder=run: echo one > shared.txt | coder=run: echo two > shared.txt".into(),
        PromptOpts::default(),
    )
    .await
    .unwrap();
    let evs = turns_ended(&m, sid, 2).await;
    let outs: Vec<String> = outputs(&evs).into_iter().skip(2).collect();
    assert_eq!(
        outs.iter()
            .filter(|o| o.contains("applied to the folder"))
            .count(),
        1,
        "{outs:#?}"
    );
    assert_eq!(
        outs.iter().filter(|o| o.contains("NOT applied")).count(),
        1,
        "{outs:#?}"
    );
    let now = std::fs::read_to_string(format!("{ws}/shared.txt")).unwrap();
    assert!(now == "one\n" || now == "two\n");
}
