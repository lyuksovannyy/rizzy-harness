//! A session's execution mode (N.10a; docs/execution-modes.md): chosen by
//! the node at create inside what it offers, whatever a (test) master asks,
//! recorded in `session_created`, never changed by a later command, and
//! checked again after a restart; a sub-agent's is its parent's.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ExecMode;
use harness_node::exec::ExecOffers;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

fn offers(sandbox: bool, host: bool) -> ExecOffers {
    let off = |what: &str| Err(format!("{what} is off in this test"));
    ExecOffers::new(
        if sandbox { Ok(()) } else { off("sandbox") },
        if host { Ok(()) } else { off("host") },
        vec![],
    )
}

fn policy(root: &std::path::Path, o: ExecOffers) -> NodePolicy {
    NodePolicy {
        workspace_roots: vec![root.into()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        exec: o,
        ..Default::default()
    }
}

/// The mock, keeping the tools of every request it is sent.
struct Spy(Mock, Arc<std::sync::Mutex<Vec<Vec<String>>>>);

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
        self.1
            .lock()
            .unwrap()
            .push(req.tools.iter().map(|t| t.name.clone()).collect());
        self.0.complete(req, deltas).await
    }
}

type Seen = Arc<std::sync::Mutex<Vec<Vec<String>>>>;

fn spied() -> (Arc<Spy>, Seen) {
    let seen: Seen = Arc::default();
    let spy = Spy(
        Mock {
            delay: Duration::from_millis(5),
        },
        seen.clone(),
    );
    (Arc::new(spy), seen)
}

fn config(store: Store, policy: NodePolicy, profile: Profile) -> NodeConfig {
    config_with(store, policy, profile, spied().0, None)
}

fn config_with(
    store: Store,
    policy: NodePolicy,
    profile: Profile,
    provider: Arc<Spy>,
    artifacts: Option<std::path::PathBuf>,
) -> NodeConfig {
    NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy,
        store,
        backend: Arc::new(NativeBackend::new(
            provider,
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: artifacts.map(harness_node::artifacts::StageConfig::new),
    }
}

struct Node {
    node: NodeRuntime,
    link: NodeLink,
    capabilities: Vec<String>,
}

async fn start(db: &std::path::Path, policy: NodePolicy, profile: Profile) -> Node {
    start_cfg(config(
        Store::open(db, Durability::Full).unwrap(),
        policy,
        profile,
    ))
    .await
}

async fn start_cfg(cfg: NodeConfig) -> Node {
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let capabilities = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    Node {
        node,
        link,
        capabilities,
    }
}

fn create(sid: SessionId, cwd: &std::path::Path, mode: Option<ExecMode>) -> MasterToNode {
    MasterToNode::Create {
        req_id: format!("c-{sid}"),
        session_id: sid,
        title: "t".into(),
        cwd: cwd.display().to_string(),
        model: None,
        approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        profile: None,
        agent: None,
        incognito: false,
        no_callbacks: false,
        exec_mode: mode,
    }
}

fn prompt(sid: SessionId, text: &str) -> MasterToNode {
    MasterToNode::Prompt {
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
    }
}

/// The ack of the next command, skipping events.
async fn ack(n: &mut Node) -> Result<(), String> {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), n.link.from_node.recv())
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

fn created(n: &Node, sid: SessionId) -> Option<SessionCreated> {
    n.node
        .store()
        .events_after(sid, 0, 1000)
        .unwrap()
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) => Some(c),
            _ => None,
        })
}

/// No bubblewrap and `host = false`: the node offers nothing, announces no
/// mode, and an agentic create is refused with the plain reason.
#[tokio::test]
async fn a_node_offering_nothing_refuses_an_agentic_create_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let o = ExecOffers::new(
        Err("bubblewrap not found".into()),
        Err(harness_node::exec::host_off()),
        vec![],
    );
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), o),
        Profile::Agentic,
    )
    .await;
    assert!(
        !n.capabilities.iter().any(|c| c.starts_with("exec:")),
        "{:?}",
        n.capabilities
    );
    for mode in [None, Some(ExecMode::Sandbox), Some(ExecMode::Host)] {
        let sid = SessionId::new();
        n.link
            .to_node
            .try_send(create(sid, dir.path(), mode))
            .unwrap();
        let e = ack(&mut n).await.unwrap_err();
        assert!(
            e.contains("this node offers no way to run commands: install bubblewrap"),
            "{mode:?}: {e}"
        );
        assert!(e.contains("[execution] host = true"), "{e}");
        assert!(created(&n, sid).is_none());
    }
}

/// A test master that asks anyway: `host` of a node with `host = false`,
/// `sandbox` of a node that cannot contain. The node refuses both, and
/// creates nothing; what it offers it announces at hello.
#[tokio::test]
async fn the_node_refuses_a_mode_it_does_not_offer_whatever_the_master_asks() {
    for (sandbox, host, ask, word) in [
        (true, false, ExecMode::Host, "host"),
        (false, true, ExecMode::Sandbox, "sandbox"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut n = start(
            &dir.path().join("node.db"),
            policy(dir.path(), offers(sandbox, host)),
            Profile::Agentic,
        )
        .await;
        let caps: Vec<&str> = n
            .capabilities
            .iter()
            .filter(|c| c.starts_with("exec:"))
            .map(String::as_str)
            .collect();
        assert_eq!(caps, [if sandbox { "exec:sandbox" } else { "exec:host" }]);
        let sid = SessionId::new();
        n.link
            .to_node
            .try_send(create(sid, dir.path(), Some(ask)))
            .unwrap();
        let e = ack(&mut n).await.unwrap_err();
        assert!(
            e.contains(&format!("does not offer the {word} mode")),
            "{e}"
        );
        assert!(created(&n, sid).is_none(), "{word}: created anyway");
    }
}

/// Without a request the node takes `sandbox` (never `host`); the
/// mode is in `session_created`, and no later command changes it (a second
/// create for the same session asking for another mode is refused, and the
/// session runs on in its own).
#[tokio::test]
async fn the_mode_is_recorded_at_create_and_a_later_command_cannot_change_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    assert!(n.capabilities.contains(&"exec:sandbox".to_string()));
    assert!(n.capabilities.contains(&"exec:host".to_string()));
    let sid = SessionId::new();
    n.link
        .to_node
        .try_send(create(sid, dir.path(), None))
        .unwrap();
    ack(&mut n).await.unwrap();
    assert_eq!(created(&n, sid).unwrap().exec_mode, Some(ExecMode::Sandbox));
    n.link
        .to_node
        .try_send(create(sid, dir.path(), Some(ExecMode::Host)))
        .unwrap();
    assert!(ack(&mut n).await.unwrap_err().contains("already exists"));
    n.link.to_node.try_send(prompt(sid, "hello")).unwrap();
    ack(&mut n).await.unwrap();
    let log = n.node.store().events_after(sid, 0, 1000).unwrap();
    let firsts = log.iter().filter(|e| e.kind == "session_created").count();
    assert_eq!(firsts, 1);
    assert_eq!(created(&n, sid).unwrap().exec_mode, Some(ExecMode::Sandbox));

    // Asked for: recorded as asked.
    let host = SessionId::new();
    n.link
        .to_node
        .try_send(create(host, dir.path(), Some(ExecMode::Host)))
        .unwrap();
    ack(&mut n).await.unwrap();
    assert_eq!(created(&n, host).unwrap().exec_mode, Some(ExecMode::Host));
}

/// A node restarted with fewer modes (bubblewrap gone, or `host` turned
/// off): the sessions whose mode it no longer offers run nothing and say
/// why; none is moved to the mode that is left, in either direction.
#[tokio::test]
async fn a_session_whose_mode_is_no_longer_offered_runs_nothing_after_a_restart() {
    for (lost, kept) in [
        (ExecMode::Host, ExecMode::Sandbox),
        (ExecMode::Sandbox, ExecMode::Host),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("node.db");
        let (gone, stays) = (SessionId::new(), SessionId::new());
        {
            let mut n = start(
                &db,
                policy(dir.path(), offers(true, true)),
                Profile::Agentic,
            )
            .await;
            for (sid, m) in [(gone, lost), (stays, kept)] {
                n.link
                    .to_node
                    .try_send(create(sid, dir.path(), Some(m)))
                    .unwrap();
                ack(&mut n).await.unwrap();
            }
        }
        let after = offers(kept == ExecMode::Sandbox, kept == ExecMode::Host);
        // With publishing on, so "Save as artifact" reaches its check.
        let mut n = start_cfg(config_with(
            Store::open(&db, Durability::Full).unwrap(),
            policy(dir.path(), after),
            Profile::Agentic,
            spied().0,
            Some(dir.path().join("data")),
        ))
        .await;
        let marker = dir.path().join("ran.txt");
        n.link
            .to_node
            .try_send(prompt(
                gone,
                &format!("run: echo ran > {}", marker.display()),
            ))
            .unwrap();
        let e = ack(&mut n).await.unwrap_err();
        assert!(
            e.contains(&format!("runs in the {} mode", lost.id())) && e.contains("no longer"),
            "{lost:?}: {e}"
        );
        assert_eq!(created(&n, gone).unwrap().exec_mode, Some(lost));
        assert!(
            !n.node
                .store()
                .events_after(gone, 0, 1000)
                .unwrap()
                .iter()
                .any(|e| e.kind == "turn_started"),
            "{lost:?}: a turn started"
        );
        assert!(!marker.exists());
        // Review N-b: and nothing else that does work: an undo, a
        // callback, "Save as artifact".
        for (what, m) in [
            (
                "restore",
                MasterToNode::Restore {
                    req_id: "r".into(),
                    session_id: gone,
                    turn_id: "t1".into(),
                    force: true,
                    rewind: false,
                },
            ),
            (
                "callback",
                MasterToNode::Callback {
                    req_id: "cb".into(),
                    session_id: gone,
                    bg_ids: vec!["bg-1".into()],
                    agent: None,
                    savings: None,
                },
            ),
            (
                "stage",
                MasterToNode::ArtifactStage {
                    req_id: "s".into(),
                    session_id: gone,
                    path: "x".into(),
                    name: None,
                },
            ),
        ] {
            n.link.to_node.try_send(m).unwrap();
            let e = ack(&mut n).await.unwrap_err();
            // A host chat has no background table, so its callback is
            // refused for that before the mode is looked at.
            let ok = e.contains(&format!("runs in the {} mode", lost.id()))
                || (what == "callback" && lost == ExecMode::Host && e.contains("no background"));
            assert!(ok, "{lost:?} {what}: {e}");
        }
        n.link.to_node.try_send(prompt(stays, "hello")).unwrap();
        ack(&mut n).await.unwrap();
    }
}

/// Review N-b: an agentic session whose `session_created` names no mode
/// (a log written by hand, or by a node from before modes) runs nothing.
#[tokio::test]
async fn an_agentic_session_without_a_recorded_mode_runs_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let sid = SessionId::new();
    {
        let store = Store::open(&db, Durability::Full).unwrap();
        let (kind, payload) = EventKind::SessionCreated(SessionCreated {
            exec_mode: None,
            no_callbacks: false,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: None,
            title: "t".into(),
            cwd: dir.path().display().to_string(),
            model: "mock".into(),
            backend: "native".into(),
            node_id: "n".into(),
            approval_mode: harness_core::policy::ApprovalMode::Auto,
        })
        .into_parts();
        store
            .append(
                "n",
                &harness_core::RawEvent {
                    session_id: sid,
                    seq: 1,
                    ts_ms: 0,
                    kind,
                    payload,
                },
            )
            .unwrap();
    }
    let mut n = start(
        &db,
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    n.link.to_node.try_send(prompt(sid, "run: true")).unwrap();
    let e = ack(&mut n).await.unwrap_err();
    assert!(e.contains("no execution mode"), "{e}");
}

/// Review L-a: a create that names no mode is never given `host`: on a
/// node whose only mode is `host` it is refused, saying it must be asked
/// for by name.
#[tokio::test]
async fn host_is_never_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(false, true)),
        Profile::Agentic,
    )
    .await;
    let sid = SessionId::new();
    n.link
        .to_node
        .try_send(create(sid, dir.path(), None))
        .unwrap();
    let e = ack(&mut n).await.unwrap_err();
    assert!(
        e.contains("only the host mode") && e.contains("by name"),
        "{e}"
    );
    assert!(created(&n, sid).is_none());
}

/// Review N-e: a `host` session never runs in `auto`, whatever the master
/// asks and whatever node.toml allows: recorded as `ask` at create, and a
/// log that says `auto` loads as `ask` (its next command waits for a card).
#[tokio::test]
async fn a_host_session_is_never_in_auto() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("node.db");
    let (made, on_disk) = (SessionId::new(), SessionId::new());
    {
        let mut n = start(
            &db,
            policy(dir.path(), offers(true, true)),
            Profile::Agentic,
        )
        .await;
        // `create` asks for `auto`; node.toml allows `auto`.
        n.link
            .to_node
            .try_send(create(made, dir.path(), Some(ExecMode::Host)))
            .unwrap();
        ack(&mut n).await.unwrap();
        assert_eq!(
            created(&n, made).unwrap().approval_mode,
            harness_core::policy::ApprovalMode::Ask
        );
        // A sandbox session keeps `auto`.
        let sb = SessionId::new();
        n.link
            .to_node
            .try_send(create(sb, dir.path(), Some(ExecMode::Sandbox)))
            .unwrap();
        ack(&mut n).await.unwrap();
        assert_eq!(
            created(&n, sb).unwrap().approval_mode,
            harness_core::policy::ApprovalMode::Auto
        );
    }
    {
        let store = Store::open(&db, Durability::Full).unwrap();
        let (kind, payload) = EventKind::SessionCreated(SessionCreated {
            exec_mode: Some(ExecMode::Host),
            no_callbacks: false,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: None,
            title: "t".into(),
            cwd: dir.path().display().to_string(),
            model: "mock".into(),
            backend: "native".into(),
            node_id: "n".into(),
            approval_mode: harness_core::policy::ApprovalMode::Auto,
        })
        .into_parts();
        store
            .append(
                "n",
                &harness_core::RawEvent {
                    session_id: on_disk,
                    seq: 1,
                    ts_ms: 0,
                    kind,
                    payload,
                },
            )
            .unwrap();
    }
    let mut n = start(
        &db,
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let marker = dir.path().join("ran-auto.txt");
    n.link
        .to_node
        .try_send(prompt(
            on_disk,
            &format!("run: echo x > {}", marker.display()),
        ))
        .unwrap();
    ack(&mut n).await.unwrap();
    let by = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let log = n.node.store().events_after(on_disk, 0, 1000).unwrap();
        if let Some(a) = log.iter().find_map(|e| match e.typed() {
            Ok(EventKind::ApprovalRequested(a)) => Some(a),
            _ => None,
        }) {
            assert!(a.reason.contains("in the host shell"), "{}", a.reason);
            break;
        }
        assert!(
            !log.iter().any(|e| e.kind == "turn_ended"),
            "the turn ended without a card: {log:?}"
        );
        assert!(std::time::Instant::now() < by, "no approval card");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!marker.exists(), "ran without the card");
}

/// Review N-c: a `host` session has no background table, so its model is
/// not offered `bash_output` or `bash_stop` (nor `background` on bash); a
/// `sandbox` session is.
#[tokio::test]
async fn a_host_session_is_not_offered_background_commands() {
    let dir = tempfile::tempdir().unwrap();
    let (spy, seen) = spied();
    let mut n = start_cfg(config_with(
        Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
        spy,
        None,
    ))
    .await;
    for (mode, offered) in [(ExecMode::Host, false), (ExecMode::Sandbox, true)] {
        let sid = SessionId::new();
        n.link
            .to_node
            .try_send(create(sid, dir.path(), Some(mode)))
            .unwrap();
        ack(&mut n).await.unwrap();
        seen.lock().unwrap().clear();
        n.link.to_node.try_send(prompt(sid, "hello")).unwrap();
        ack(&mut n).await.unwrap();
        let by = std::time::Instant::now() + Duration::from_secs(20);
        while seen.lock().unwrap().is_empty() {
            assert!(std::time::Instant::now() < by, "no model call");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let tools = seen.lock().unwrap()[0].clone();
        assert!(tools.contains(&"bash".to_string()), "{tools:?}");
        assert_eq!(
            tools.contains(&"bash_output".to_string()),
            offered,
            "{mode:?}: {tools:?}"
        );
        assert_eq!(
            tools.contains(&"bash_stop".to_string()),
            offered,
            "{mode:?}: {tools:?}"
        );
        // Until the turn ends, so the next session's request is its own.
        let by = std::time::Instant::now() + Duration::from_secs(20);
        while !n
            .node
            .store()
            .events_after(sid, 0, 1000)
            .unwrap()
            .iter()
            .any(|e| e.kind == "turn_ended")
        {
            assert!(std::time::Instant::now() < by, "the turn did not end");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

/// A sub-agent's session gets its parent's mode, always: here `host`, on
/// a node whose most confined mode (what a child without one would get) is
/// `sandbox`.
#[tokio::test]
async fn a_sub_agents_mode_is_its_parents() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("keep.txt"), "hello\n").unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Agentic,
    )
    .await;
    let parent = SessionId::new();
    n.link
        .to_node
        .try_send(create(parent, &ws, Some(ExecMode::Host)))
        .unwrap();
    ack(&mut n).await.unwrap();
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
    let lead = spec("lead", vec!["bash".into()], vec![scout]);
    let MasterToNode::Prompt {
        req_id,
        session_id,
        text,
        attachments,
        compare,
        reply,
        savings,
        context,
        images,
        ..
    } = prompt(parent, "delegate: scout=read: keep.txt")
    else {
        unreachable!()
    };
    n.link
        .to_node
        .try_send(MasterToNode::Prompt {
            req_id,
            session_id,
            text,
            agent: Some(lead),
            attachments,
            compare,
            reply,
            savings,
            context,
            images,
            agent_named: false,
        })
        .unwrap();
    ack(&mut n).await.unwrap();
    // Until the parent's turn ends.
    let by = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let log = n.node.store().events_after(parent, 0, 10_000).unwrap();
        if log.iter().any(|e| e.kind == "turn_ended") {
            break;
        }
        assert!(
            std::time::Instant::now() < by,
            "the parent's turn did not end"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let sessions = n.node.store().sessions().unwrap();
    assert_eq!(sessions.len(), 2, "no child session: {sessions:?}");
    for s in sessions {
        let c = created(&n, s.session_id).unwrap();
        assert_eq!(c.exec_mode, Some(ExecMode::Host), "{:?}", c.parent_session);
    }
}

/// A chat-profile runtime runs no commands: its sessions have no mode, it
/// announces none, and a create that asks for one is refused.
#[tokio::test]
async fn a_chat_session_has_no_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = start(
        &dir.path().join("node.db"),
        policy(dir.path(), offers(true, true)),
        Profile::Chat,
    )
    .await;
    assert!(!n.capabilities.iter().any(|c| c.starts_with("exec:")));
    let chat = |sid, mode| MasterToNode::Create {
        req_id: "c".into(),
        session_id: sid,
        title: "t".into(),
        cwd: String::new(),
        model: None,
        approval_mode: None,
        profile: Some(Profile::Chat),
        agent: None,
        incognito: false,
        no_callbacks: false,
        exec_mode: mode,
    };
    let sid = SessionId::new();
    n.link
        .to_node
        .try_send(chat(sid, Some(ExecMode::Sandbox)))
        .unwrap();
    assert!(ack(&mut n).await.unwrap_err().contains("no execution mode"));
    n.link.to_node.try_send(chat(sid, None)).unwrap();
    ack(&mut n).await.unwrap();
    assert_eq!(created(&n, sid).unwrap().exec_mode, None);
}
