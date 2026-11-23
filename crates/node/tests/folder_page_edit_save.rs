//! A person's text changes saved through the node, with a helper that
//! answers (X.60, T100): the successful save writes the copy beside the
//! original and leaves the original as it was; the same VALID request
//! (the tag is the file's own) writes nothing where node.toml denies the
//! editing tool of the file's kind or its approval ceiling is plan, so the
//! gate and not a stale tag is what refused it. The helper is a stand-in
//! script (`testing` feature) that records the request it was given and
//! answers a canned reply and file.
#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_docs::edit::preview::{Block, Preview, preview};
use harness_docs::edit::serve::{Item, Reply};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_DOC_PAGE_EDIT, MasterToNode, NodeToMaster, PageChange, PreviewRefusal};
use harness_store::{Durability, Store};

const NEW: &[u8] = b"PK-the-new-file-the-helper-made";

struct Node {
    _dir: tempfile::TempDir,
    ws: std::path::PathBuf,
    link: harness_node::NodeLink,
    _node: NodeRuntime,
    announced: bool,
    sid: SessionId,
}

async fn start(denied: &[&str], ceiling: ApprovalMode) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let ws = base.join("ws");
    let data = base.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = NodePolicy {
        workspace_roots: vec![base.clone()],
        max_approval_mode: ceiling,
        denied_tools: denied.iter().map(|s| s.to_string()).collect(),
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
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let announced = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => {
            capabilities.contains(&CAP_DOC_PAGE_EDIT.to_string())
        }
        other => panic!("{other:?}"),
    };
    let sid = SessionId::new();
    link.to_node
        .try_send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: "c".into(),
            session_id: sid,
            title: "t".into(),
            cwd: ws.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        })
        .unwrap();
    loop {
        if let NodeToMaster::Ack { ok, .. } = link.from_node.recv().await.unwrap() {
            assert!(ok);
            break;
        }
    }
    Node {
        _dir: dir,
        ws,
        link,
        _node: node,
        announced,
        sid,
    }
}

impl Node {
    async fn save(
        &mut self,
        path: &str,
        changes: Vec<PageChange>,
    ) -> (Option<String>, Option<PreviewRefusal>) {
        self.link
            .to_node
            .try_send(MasterToNode::DocPageEdit {
                req_id: "p".into(),
                session_id: self.sid,
                path: path.into(),
                sheet: None,
                changes,
            })
            .unwrap();
        loop {
            let m = tokio::time::timeout(Duration::from_secs(10), self.link.from_node.recv())
                .await
                .expect("no answer")
                .unwrap();
            if let NodeToMaster::DocPageEditResult { saved, refusal, .. } = m {
                return (saved, refusal);
            }
        }
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

#[tokio::test]
async fn a_valid_save_writes_a_copy_and_the_gates_write_nothing() {
    // The stand-in helper: records what it was asked, answers one ok result
    // and a file.
    let h = tempfile::tempdir().unwrap();
    let hp = h.path().to_path_buf();
    Box::leak(Box::new(h));
    let reply = Reply {
        results: vec![Item {
            ok: true,
            msg: String::new(),
            why: None,
        }],
        file: true,
        ..Default::default()
    };
    let head = serde_json::to_vec(&reply).unwrap();
    let mut frame = (head.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&head);
    frame.extend_from_slice(NEW);
    std::fs::write(hp.join("reply"), &frame).unwrap();
    harness_node::testing::doc_helper_script(&format!(
        "cat >'{0}/stdin'; cat '{0}/reply'",
        hp.display()
    ));
    let original = harness_docs::docx::build("# T\n\nBody text\n", &Default::default())
        .unwrap()
        .bytes;
    let Preview::Doc { blocks, .. } = preview(&original, None).unwrap() else {
        panic!()
    };
    // The file's own tag: a request that is valid in every respect.
    let at = blocks
        .iter()
        .find_map(|b| match b {
            Block::P { at, .. } if !at.is_empty() => Some(at.clone()),
            _ => None,
        })
        .unwrap();
    let ask = || {
        vec![PageChange {
            at: at.clone(),
            text: "New".into(),
        }]
    };

    // A node that allows it: the copy is written beside the original with the
    // helper's bytes; the original is as it was; the helper got the kind.
    let mut n = start(&[], ApprovalMode::Auto).await;
    assert!(n.announced);
    std::fs::create_dir_all(n.ws.join("d")).unwrap();
    std::fs::write(n.ws.join("d/a.docx"), &original).unwrap();
    let (saved, refusal) = n.save("d/a.docx", ask()).await;
    assert_eq!(
        (saved.as_deref(), refusal),
        (Some("d/a (edited).docx"), None)
    );
    assert_eq!(std::fs::read(n.ws.join("d/a (edited).docx")).unwrap(), NEW);
    assert_eq!(std::fs::read(n.ws.join("d/a.docx")).unwrap(), original);
    let raw = std::fs::read(hp.join("stdin")).unwrap();
    let n_head = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
    let req: serde_json::Value = serde_json::from_slice(&raw[4..4 + n_head]).unwrap();
    assert_eq!(req["tool"], "page_edit");
    assert_eq!(req["kind"], "doc");
    assert_eq!(&raw[4 + n_head..], original.as_slice());
    // Again: a second copy, never over the first.
    let (saved, _) = n.save("d/a.docx", ask()).await;
    assert_eq!(saved.as_deref(), Some("d/a (edited) (2).docx"));
    assert_eq!(std::fs::read(n.ws.join("d/a (edited).docx")).unwrap(), NEW);

    // The same request where the gate says no: nothing is written, and the
    // helper is never run (its record is removed first).
    let book = harness_docs::xlsx::build(
        &serde_json::json!({"sheets": [{"name": "S", "columns": [{"header": "A"}], "rows": [["x"]]}]}),
        0,
    )
    .unwrap()
    .bytes;
    for (denied, ceiling, name, bytes) in [
        (vec!["doc_edit"], ApprovalMode::Auto, "a.docx", &original),
        (vec!["sheet_edit"], ApprovalMode::Auto, "b.xlsx", &book),
        (vec![], ApprovalMode::Plan, "a.docx", &original),
        (vec![], ApprovalMode::Plan, "b.xlsx", &book),
    ] {
        let _ = std::fs::remove_file(hp.join("stdin"));
        let mut g = start(&denied, ceiling).await;
        std::fs::write(g.ws.join(name), bytes).unwrap();
        let (saved, refusal) = g.save(name, ask()).await;
        assert_eq!(
            (saved, refusal),
            (None, Some(PreviewRefusal::NoWrite)),
            "{denied:?} {ceiling:?} {name}"
        );
        assert_eq!(names(&g.ws), [name], "{denied:?} {ceiling:?}");
        assert!(!hp.join("stdin").exists(), "the helper never ran");
    }
    // The plan ceiling: not announced; one tool denied: still announced (the
    // other kind of file can be saved); both denied: not announced.
    assert!(!start(&[], ApprovalMode::Plan).await.announced);
    assert!(start(&["doc_edit"], ApprovalMode::Auto).await.announced);
    assert!(
        !start(&["doc_edit", "sheet_edit"], ApprovalMode::Auto)
            .await
            .announced
    );
    // `ask` and `accept edits` ceilings do not gate it: the save is the
    // person's act, not the agent's call.
    for c in [
        ApprovalMode::AskAll,
        ApprovalMode::Ask,
        ApprovalMode::AcceptEdits,
    ] {
        let mut g = start(&[], c).await;
        assert!(g.announced, "{c:?}");
        std::fs::write(g.ws.join("a.docx"), &original).unwrap();
        let (saved, _) = g.save("a.docx", ask()).await;
        assert_eq!(saved.as_deref(), Some("a (edited).docx"), "{c:?}");
    }
}
