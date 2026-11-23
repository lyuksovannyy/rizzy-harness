//! The page's preview of a Word or Excel file in a chat's folder, through
//! the node (PLAN.md T99, ADOPTION_PLAN X.59): `session.doc_preview` is
//! resolved by the node under its own read rules and answers with one
//! `session.doc_preview_result`, never an ack, never an event. The paths here
//! all end before the helper (the test binary is not the shipped helper); the
//! helper itself runs in `crates/cli/tests/doc_edit.rs` and the whole path in
//! `crates/tools/src/folder_preview.rs`.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_DOC_PREVIEW, MasterToNode, NodeToMaster, PreviewRefusal};
use harness_store::{Durability, Store};
use serde_json::Value;

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
        NodeToMaster::Hello { capabilities, .. } => {
            assert_eq!(
                capabilities.contains(&CAP_DOC_PREVIEW.to_string()),
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
    async fn create(&mut self) -> SessionId {
        self.create_as(None).await
    }

    async fn create_as(&mut self, profile: Option<Profile>) -> SessionId {
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

    /// The answer to one `session.doc_preview`; no ack comes with it.
    async fn ask(&mut self, sid: SessionId, path: &str) -> (Option<Value>, Option<PreviewRefusal>) {
        self.link
            .to_node
            .try_send(MasterToNode::DocPreview {
                req_id: "p".into(),
                session_id: sid,
                path: path.into(),
                sheet: None,
            })
            .unwrap();
        loop {
            match self.recv().await {
                NodeToMaster::DocPreviewResult {
                    req_id,
                    preview,
                    refusal,
                } => {
                    assert_eq!(req_id, "p");
                    return (preview, refusal);
                }
                NodeToMaster::Ack { .. } => panic!("a preview is never acked"),
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
}

fn docx() -> Vec<u8> {
    harness_docs::docx::build("# T\n\nx\n", &Default::default())
        .unwrap()
        .bytes
}

/// Every way of not reaching a file of the folder is the one answer a missing
/// file gets, whether the node lets the agent read outside or not; and the
/// request appends no event.
#[tokio::test]
async fn paths_that_leave_the_folder_or_are_not_files_are_one_answer() {
    use std::os::unix::fs::symlink;
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::write(n.base.join("secret.docx"), docx()).unwrap();
    std::fs::write(n.data.join("key.docx"), docx()).unwrap();
    symlink(n.base.join("secret.docx"), n.ws.join("link.docx")).unwrap();
    symlink(n.data.join("key.docx"), n.ws.join("key.docx")).unwrap();
    std::fs::create_dir_all(n.ws.join(".git")).unwrap();
    std::fs::write(n.ws.join(".git/h.docx"), docx()).unwrap();
    std::fs::create_dir_all(n.ws.join("dir.docx")).unwrap();
    nix::unistd::mkfifo(&n.ws.join("pipe.docx"), nix::sys::stat::Mode::S_IRWXU).unwrap();
    let sid = n.create().await;
    let before = n.events(sid);
    let gone = (None, Some(PreviewRefusal::NotFound));
    assert_eq!(n.ask(sid, "missing.docx").await, gone);
    let abs = n.base.join("secret.docx").display().to_string();
    let abs_data = n.data.join("key.docx").display().to_string();
    for p in [
        "../secret.docx",
        &abs,
        &abs_data,
        "link.docx",
        "key.docx",
        ".git/h.docx",
        "dir.docx",
        "pipe.docx",
        "",
        "a\nb.docx",
    ] {
        assert_eq!(n.ask(sid, p).await, gone, "{p:?}");
    }
    assert_eq!(n.events(sid), before, "a preview appends no event");
    // An unknown chat is the same answer.
    assert_eq!(n.ask(SessionId::new(), "a.docx").await, gone);
}

/// The size cap, the empty file and the wrong kind answer before any helper
/// is started; node.toml's denial of `read` holds for the page too.
#[tokio::test]
async fn the_cap_empty_and_kind_answer_first_and_denied_read_shows_nothing() {
    let mut n = start(&[], Profile::Agentic).await;
    let big = std::fs::File::create(n.ws.join("big.xlsx")).unwrap();
    big.set_len(10 * 1024 * 1024 + 1).unwrap();
    std::fs::write(n.ws.join("e.docx"), b"").unwrap();
    std::fs::write(n.ws.join("a.txt"), b"hi").unwrap();
    let sid = n.create().await;
    assert_eq!(
        n.ask(sid, "big.xlsx").await,
        (None, Some(PreviewRefusal::FileTooLarge))
    );
    assert_eq!(
        n.ask(sid, "e.docx").await,
        (None, Some(PreviewRefusal::Empty))
    );
    assert_eq!(
        n.ask(sid, "a.txt").await,
        (None, Some(PreviewRefusal::NotOffice))
    );
    let mut d = start(&["read"], Profile::Agentic).await;
    std::fs::write(d.ws.join("e.docx"), b"").unwrap();
    let sid = d.create().await;
    assert_eq!(
        d.ask(sid, "e.docx").await,
        (None, Some(PreviewRefusal::NotFound))
    );
}

/// A node without a folder announces nothing and answers nothing.
#[tokio::test]
async fn a_chat_node_does_not_preview_folder_files() {
    let mut n = start(&[], Profile::Chat).await;
    std::fs::write(n.ws.join("e.docx"), b"").unwrap();
    let sid = n.create_as(Some(Profile::Chat)).await;
    assert_eq!(
        n.ask(sid, "e.docx").await,
        (None, Some(PreviewRefusal::NotFound))
    );
}
