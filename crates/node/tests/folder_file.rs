//! The page's view of a text file in a chat's folder, through the node
//! (PLAN.md T99, ADOPTION_PLAN X.63): `session.folder_file` is resolved by the
//! node under its own read rules and answers with one
//! `session.folder_file_result`, never an ack, never an event. The reading
//! itself is in `crates/tools/src/folder_file.rs`; here is what the node adds:
//! its policy, its scrubber, its capability, and the one place.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_core::{Secret, SecretSet, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_FOLDER_FILE, FileRefusal, FolderFileText, MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

const KEY: &str = "sk-made-up-0123456789abcdef";

struct Node {
    _dir: tempfile::TempDir,
    base: std::path::PathBuf,
    ws: std::path::PathBuf,
    data: std::path::PathBuf,
    node: NodeRuntime,
    link: NodeLink,
}

async fn start(denied: &[&str], profile: Profile) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let ws = base.join("ws");
    let data = base.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = NodePolicy {
        workspace_roots: vec![base.clone()],
        max_approval_mode: ApprovalMode::Auto,
        denied_tools: denied.iter().map(|s| s.to_string()).collect(),
        // A node that lets the agent read outside does not let the page.
        allow_outside_workspace: true,
        ..Default::default()
    };
    policy.protect(&data);
    let cfg = NodeConfig {
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
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            assert_eq!(
                capabilities.contains(&CAP_FOLDER_FILE.to_string()),
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

type Got = (Option<FolderFileText>, Option<FileRefusal>);

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

    fn send(&mut self, req: &str, sid: SessionId, path: &str) {
        self.link
            .to_node
            .try_send(MasterToNode::FolderFile {
                req_id: req.into(),
                session_id: sid,
                path: path.into(),
            })
            .unwrap();
    }

    async fn answer(&mut self, req: &str) -> Got {
        loop {
            match self.recv().await {
                NodeToMaster::FolderFileResult {
                    req_id,
                    file,
                    refusal,
                } => {
                    assert_eq!(req_id, req);
                    return (file, refusal);
                }
                NodeToMaster::Ack { .. } => panic!("a file view is never acked"),
                _ => {}
            }
        }
    }

    /// The answer to one `session.folder_file`; no ack comes with it.
    async fn ask(&mut self, sid: SessionId, path: &str) -> Got {
        self.send("p", sid, path);
        self.answer("p").await
    }

    fn events(&self, sid: SessionId) -> usize {
        self.node
            .store()
            .events_after(sid, 0, 10_000)
            .unwrap()
            .len()
    }
}

/// The text of a file of the folder comes back with its size, scrubbed of the
/// node's secrets, and the request appends no event.
#[tokio::test]
async fn a_file_is_read_scrubbed_and_appends_no_event() {
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::create_dir_all(n.ws.join("src")).unwrap();
    std::fs::write(n.ws.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(n.ws.join(".env"), format!("KEY={KEY}\n")).unwrap();
    let sid = n.create(None).await;
    let before = n.events(sid);
    let (file, refusal) = n.ask(sid, "src/main.rs").await;
    assert_eq!(refusal, None);
    let f = file.unwrap();
    assert_eq!(
        (f.text.as_str(), f.size, f.truncated),
        ("fn main() {}\n", 13, false)
    );
    let (file, _) = n.ask(sid, ".env").await;
    let text = file.unwrap().text;
    assert!(!text.contains("sk-made-up"), "{text}");
    assert!(text.contains("[REDACTED]"), "{text}");
    assert_eq!(n.events(sid), before, "a view appends no event");
}

/// Every way of not reaching a file of the folder is the one answer a missing
/// file gets, whether the node lets the agent read outside or not.
#[tokio::test]
async fn paths_that_leave_the_folder_or_are_not_files_are_one_answer() {
    use std::os::unix::fs::symlink;
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::write(n.base.join("secret.txt"), "outside").unwrap();
    std::fs::write(n.data.join("key.txt"), "data").unwrap();
    symlink(n.base.join("secret.txt"), n.ws.join("link.txt")).unwrap();
    symlink(n.data.join("key.txt"), n.ws.join("key.txt")).unwrap();
    std::fs::create_dir_all(n.ws.join(".git")).unwrap();
    std::fs::write(n.ws.join(".git/config"), "[core]").unwrap();
    std::fs::create_dir_all(n.ws.join("dir")).unwrap();
    nix::unistd::mkfifo(&n.ws.join("pipe"), nix::sys::stat::Mode::S_IRWXU).unwrap();
    symlink("/dev/zero", n.ws.join("zero")).unwrap();
    let sid = n.create(None).await;
    let gone = (None, Some(FileRefusal::NotFound));
    assert_eq!(n.ask(sid, "missing.txt").await, gone);
    let abs = n.base.join("secret.txt").display().to_string();
    let abs_data = n.data.join("key.txt").display().to_string();
    for p in [
        "../secret.txt",
        &abs,
        &abs_data,
        "link.txt",
        "key.txt",
        ".git/config",
        "dir",
        "pipe",
        "zero",
        "",
        "a\nb.txt",
    ] {
        assert_eq!(n.ask(sid, p).await, gone, "{p:?}");
    }
    // An unknown chat is the same answer.
    assert_eq!(n.ask(SessionId::new(), "a.txt").await, gone);
}

/// A file that is not text is said so; node.toml's denial of `read` holds for
/// the page too, and nothing is read.
#[tokio::test]
async fn binary_is_said_and_denied_read_shows_nothing() {
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::write(n.ws.join("a.bin"), b"\x00\x01\x02").unwrap();
    let sid = n.create(None).await;
    assert_eq!(n.ask(sid, "a.bin").await, (None, Some(FileRefusal::Binary)));
    let mut d = start(&["read"], Profile::Agentic).await;
    std::fs::write(d.ws.join("a.txt"), "hi").unwrap();
    let sid = d.create(None).await;
    assert_eq!(d.ask(sid, "a.txt").await, (None, Some(FileRefusal::Denied)));
}

/// A node without a folder announces nothing and answers nothing.
#[tokio::test]
async fn a_chat_node_does_not_view_folder_files() {
    let mut n = start(&[], Profile::Chat).await;
    std::fs::write(n.ws.join("a.txt"), b"hi").unwrap();
    let sid = n.create(Some(Profile::Chat)).await;
    assert_eq!(
        n.ask(sid, "a.txt").await,
        (None, Some(FileRefusal::NotFound))
    );
}

/// One place on the node: requests that arrive while one is read are told to
/// try again, never queued; every answer is the file or `busy`, and each
/// request is answered once.
#[tokio::test]
async fn requests_beyond_the_one_place_are_answered_a_file_or_busy() {
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::write(n.ws.join("a.txt"), "x".repeat(200_000)).unwrap();
    let sid = n.create(None).await;
    for i in 0..40 {
        n.send(&format!("r{i}"), sid, "a.txt");
    }
    let (mut ok, mut busy) = (0, 0);
    let mut seen = std::collections::HashSet::new();
    while seen.len() < 40 {
        match n.recv().await {
            NodeToMaster::FolderFileResult {
                req_id,
                file,
                refusal,
            } => {
                assert!(seen.insert(req_id), "answered twice");
                match (file, refusal) {
                    (Some(_), None) => ok += 1,
                    (None, Some(FileRefusal::Busy)) => busy += 1,
                    other => panic!("{other:?}"),
                }
            }
            NodeToMaster::Ack { .. } => panic!("a file view is never acked"),
            _ => {}
        }
    }
    assert!(ok >= 1 && ok + busy == 40, "{ok} {busy}");
}
