//! The header's git chip through the node (PLAN.md T19, ADOPTION_PLAN X.64):
//! `session.git_state` answers with one `session.git_state_result`, never an
//! ack, never an event. The reading itself is `crates/node/src/git/state.rs`
//! (its own tests: branches, counts, a hostile repository); here is what the
//! session adds: which chats are asked, what node.toml's `read` denial hides,
//! the capability, and that nothing is appended to the log.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_core::{Secret, SecretSet, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_GIT_STATE, GitState, MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

const KEY: &str = "sk-made-up-0123456789abcdef";

#[allow(dead_code)]
struct Node {
    _dir: tempfile::TempDir,
    base: std::path::PathBuf,
    ws: std::path::PathBuf,
    data: std::path::PathBuf,
    node: NodeRuntime,
    link: NodeLink,
}

fn config(
    base: &std::path::Path,
    data: &std::path::Path,
    denied: &[&str],
    profile: Profile,
    exec_off: bool,
) -> NodeConfig {
    let mut policy = NodePolicy {
        workspace_roots: vec![base.to_path_buf()],
        max_approval_mode: ApprovalMode::Auto,
        denied_tools: denied.iter().map(|s| s.to_string()).collect(),
        // A node that lets the agent read outside does not let the page.
        allow_outside_workspace: true,
        ..Default::default()
    };
    if exec_off {
        let why = || Err("narrowed in node.toml".to_string());
        policy.exec = harness_node::exec::ExecOffers::new(why(), why(), vec![]);
    }
    policy.protect(data);
    NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig {
                protected_dirs: Arc::new(policy.protected_dirs.clone()),
                ..Default::default()
            },
        )),
        policy,
        store: Store::open(&data.join("node.db"), Durability::Full).unwrap(),
        default_model: "mock".into(),
        secrets: SecretSet::new(vec![Secret::new(KEY.to_string())]),
        profile,
        incognito_ttl: None,
        checkpoints_dir: Some(data.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    }
}

async fn start(denied: &[&str], profile: Profile) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let ws = base.join("ws");
    let data = base.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let node = NodeRuntime::start(config(&base, &data, denied, profile, false))
        .await
        .unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert_eq!(
                capabilities.contains(&CAP_GIT_STATE.to_string()),
                profile == Profile::Agentic
            );
        }
        other => panic!("expected hello, got {other:?}"),
    }
    Node {
        _dir: dir,
        base,
        ws,
        data,
        node,
        link,
    }
}

impl Node {
    async fn create(&mut self, profile: Option<Profile>) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                exec_mode: None,
                no_callbacks: false,
                profile,
                incognito: false,
                agent: None,
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
            })
            .unwrap();
        loop {
            if let NodeToMaster::Ack { ok, error, .. } = self.recv().await {
                assert!(ok, "{error:?}");
                return sid;
            }
        }
    }

    async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
            .await
            .expect("no message")
            .expect("link closed")
    }

    async fn ask(&mut self, sid: SessionId) -> (Option<GitState>, bool) {
        self.link
            .to_node
            .try_send(MasterToNode::GitState {
                req_id: "g".into(),
                session_id: sid,
            })
            .unwrap();
        loop {
            match self.recv().await {
                NodeToMaster::GitStateResult {
                    req_id,
                    state,
                    busy,
                } => {
                    assert_eq!(req_id, "g");
                    return (state, busy);
                }
                NodeToMaster::Ack { .. } => panic!("a git state is never acked"),
                _ => {}
            }
        }
    }

    fn events(&self, sid: SessionId) -> usize {
        self.node
            .store()
            .events_after(sid, 0, 10_000)
            .unwrap()
            .len()
    }

    fn git(&self, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.ws)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo(&self) {
        self.git(&["init", "-q", "-b", "feature/chip"]);
        std::fs::write(self.ws.join("a.txt"), "a\n").unwrap();
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "init"]);
    }
}

/// A coding chat in a repository gets its branch and count, as a view: no
/// event is appended and no ack comes with it.
#[tokio::test]
async fn a_coding_chat_gets_branch_and_count_and_appends_no_event() {
    let mut n = start(&[], Profile::Agentic).await;
    n.repo();
    let sid = n.create(None).await;
    let before = n.events(sid);
    let (state, busy) = n.ask(sid).await;
    let s = state.unwrap();
    assert!(!busy);
    assert_eq!(s.branch.as_deref(), Some("feature/chip"));
    assert_eq!(s.changed, Some(0));
    std::fs::write(n.ws.join("new.txt"), "n").unwrap();
    assert_eq!(n.ask(sid).await.0.unwrap().changed, Some(1));
    assert_eq!(n.events(sid), before, "a view appends no event");
}

/// A folder that is no repository shows nothing, and so does a chat on a node
/// that denies the `read` tool: the branch is text of the folder.
#[tokio::test]
async fn no_repository_or_a_denied_read_shows_nothing() {
    let mut n = start(&[], Profile::Agentic).await;
    let sid = n.create(None).await;
    assert_eq!(n.ask(sid).await, (None, false));
    n.repo();
    assert!(n.ask(sid).await.0.is_some());

    let mut d = start(&["read"], Profile::Agentic).await;
    d.repo();
    let sid = d.create(None).await;
    assert_eq!(
        d.ask(sid).await,
        (None, false),
        "read denied: nothing shown"
    );
}

/// A plain chat (no folder) and an unknown session are asked of no repository.
#[tokio::test]
async fn a_plain_chat_and_an_unknown_session_show_nothing() {
    // A plain chat lives on a chat node, whose folder is a repository too.
    let mut c = start(&[], Profile::Chat).await;
    c.repo();
    let plain = c.create(Some(Profile::Chat)).await;
    assert_eq!(c.ask(plain).await, (None, false));
    let mut n = start(&[], Profile::Agentic).await;
    n.repo();
    assert_eq!(n.ask(SessionId::new()).await, (None, false));
}

/// A node that is not a coding node does not announce the request (`start`
/// checks the hello).
#[tokio::test]
async fn a_chat_node_does_not_announce_it() {
    let _ = start(&[], Profile::Chat).await;
}
