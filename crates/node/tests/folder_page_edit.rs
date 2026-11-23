//! A person's text changes to a Word or Excel file of a chat's folder,
//! through the node (PLAN.md T100, ADOPTION_PLAN X.60): `session.doc_page_edit`
//! is resolved by the node under its own rules and answers with one
//! `session.doc_page_edit_result`, never an ack, never an event, and writes
//! nothing unless node.toml lets the agent's own editing tools run. The paths
//! here all end before the helper (the test binary is not the shipped helper);
//! the helper itself runs in `crates/cli/tests/doc_edit.rs` and the whole path
//! in `crates/tools/src/folder_edit.rs`.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{
    CAP_DOC_PAGE_EDIT, MasterToNode, NodeToMaster, PageChange, PreviewRefusal,
};
use harness_store::{Durability, Store};

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
            let has = capabilities.contains(&CAP_DOC_PAGE_EDIT.to_string());
            let tools_on = !denied.contains(&"doc_edit") || !denied.contains(&"sheet_edit");
            assert_eq!(has, profile == Profile::Agentic && tools_on, "{denied:?}");
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

    /// The answer to one `session.doc_page_edit`; no ack comes with it.
    async fn ask(&mut self, sid: SessionId, path: &str) -> Option<PreviewRefusal> {
        self.ask_with(sid, path, vec![change()]).await.1
    }

    async fn ask_with(
        &mut self,
        sid: SessionId,
        path: &str,
        changes: Vec<PageChange>,
    ) -> (Option<String>, Option<PreviewRefusal>) {
        self.link
            .to_node
            .try_send(MasterToNode::DocPageEdit {
                req_id: "p".into(),
                session_id: sid,
                path: path.into(),
                sheet: None,
                changes,
            })
            .unwrap();
        loop {
            match self.recv().await {
                NodeToMaster::DocPageEditResult {
                    req_id,
                    saved,
                    refusal,
                    ..
                } => {
                    assert_eq!(req_id, "p");
                    return (saved, refusal);
                }
                NodeToMaster::Ack { .. } => panic!("a page edit is never acked"),
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

fn change() -> PageChange {
    PageChange {
        at: "p1#0000".into(),
        text: "x".into(),
    }
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// Every way of not reaching a file of the folder is the one answer a missing
/// file gets, whether the node lets the agent read outside or not; nothing is
/// written and the request appends no event.
#[tokio::test]
async fn paths_that_leave_the_folder_or_are_not_files_are_one_answer_and_write_nothing() {
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
    let snap = (names(&n.ws), names(&n.base), names(&n.data), names(&n.ws.join(".git")));
    let gone = Some(PreviewRefusal::NotFound);
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
    assert_eq!(n.events(sid), before, "a page edit appends no event");
    assert_eq!(
        snap,
        (names(&n.ws), names(&n.base), names(&n.data), names(&n.ws.join(".git")))
    );
    // An unknown chat is the same answer.
    assert_eq!(n.ask(SessionId::new(), "a.docx").await, gone);
}

/// The node decides: where node.toml denies `read`, or the editing tool of
/// the file's kind, nothing is written whatever the page asks, and the
/// capability is announced only where one of the editing tools is on.
#[tokio::test]
async fn a_node_that_denies_the_editing_tools_or_read_writes_nothing() {
    for (denied, name, want) in [
        (vec!["doc_edit"], "a.docx", PreviewRefusal::NoWrite),
        (vec!["sheet_edit"], "a.xlsx", PreviewRefusal::NoWrite),
        (vec!["read"], "a.docx", PreviewRefusal::NotFound),
    ] {
        let mut n = start(&denied, Profile::Agentic).await;
        std::fs::write(n.ws.join(name), docx()).unwrap();
        let sid = n.create().await;
        assert_eq!(n.ask(sid, name).await, Some(want), "{denied:?}");
        assert_eq!(names(&n.ws), [name], "{denied:?}");
    }
    // Both denied: the capability is not announced (checked in `start`), and
    // a request anyway is refused.
    let mut n = start(&["doc_edit", "sheet_edit"], Profile::Agentic).await;
    std::fs::write(n.ws.join("a.docx"), docx()).unwrap();
    let sid = n.create().await;
    assert_eq!(n.ask(sid, "a.docx").await, Some(PreviewRefusal::NoWrite));
    assert_eq!(names(&n.ws), ["a.docx"]);
}

/// Bounds are checked again on the node: none, too many, an oversized place or
/// text.
#[tokio::test]
async fn the_node_checks_the_bounds_again() {
    let mut n = start(&[], Profile::Agentic).await;
    std::fs::write(n.ws.join("a.docx"), docx()).unwrap();
    let sid = n.create().await;
    let nf = Some(PreviewRefusal::NotFound);
    assert_eq!(n.ask_with(sid, "a.docx", vec![]).await.1, nf);
    let many = vec![change(); harness_protocol::MAX_PAGE_EDIT_CHANGES + 1];
    assert_eq!(n.ask_with(sid, "a.docx", many).await.1, nf);
    let long_at = PageChange {
        at: "p".repeat(harness_protocol::MAX_PAGE_EDIT_AT_BYTES + 1),
        text: "x".into(),
    };
    assert_eq!(n.ask_with(sid, "a.docx", vec![long_at]).await.1, nf);
    let long_text = PageChange {
        at: "p1#0000".into(),
        text: "x".repeat(harness_protocol::MAX_PAGE_EDIT_TEXT_BYTES + 1),
    };
    assert_eq!(n.ask_with(sid, "a.docx", vec![long_text]).await.1, nf);
    let long_path = "a".repeat(harness_protocol::MAX_PREVIEW_PATH_BYTES + 1);
    assert_eq!(n.ask_with(sid, &long_path, vec![change()]).await.1, nf);
    assert_eq!(names(&n.ws), ["a.docx"]);
}

/// A node without a folder announces nothing and answers nothing.
#[tokio::test]
async fn a_chat_node_does_not_edit_folder_files() {
    let mut n = start(&[], Profile::Chat).await;
    std::fs::write(n.ws.join("e.docx"), docx()).unwrap();
    let sid = n.create_as(Some(Profile::Chat)).await;
    assert_eq!(n.ask(sid, "e.docx").await, Some(PreviewRefusal::NotFound));
    assert_eq!(names(&n.ws), ["e.docx"]);
}
