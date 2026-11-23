//! The node reads one preview at a time (X.59, T99): while one runs, a second
//! is told `busy` at once, so a person's previews never take both of the
//! helper's places. The helper is a stand-in script (`testing` feature).
#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PreviewRefusal};
use harness_store::{Durability, Store};

#[tokio::test]
async fn a_second_preview_while_one_runs_is_busy_at_once() {
    harness_node::testing::doc_helper_script("cat >/dev/null; sleep 2");
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let ws = base.join("ws");
    let data = base.join("data");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(ws.join("a.docx"), b"not empty").unwrap();
    let policy = NodePolicy {
        workspace_roots: vec![base.clone()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    let cfg = NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
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
    let _hello = link.from_node.recv().await.unwrap();
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
    for id in ["first", "second"] {
        link.to_node
            .try_send(MasterToNode::DocPreview {
                req_id: id.into(),
                session_id: sid,
                path: "a.docx".into(),
                sheet: None,
            })
            .unwrap();
    }
    let mut answers = Vec::new();
    while answers.len() < 2 {
        let m = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("no answer")
            .unwrap();
        if let NodeToMaster::DocPreviewResult {
            req_id, refusal, ..
        } = m
        {
            answers.push((req_id, refusal));
        }
    }
    // The second came back first, busy; the first ends when its helper does
    // (the stand-in says nothing, which is a damaged file).
    assert_eq!(answers[0], ("second".into(), Some(PreviewRefusal::Busy)));
    assert_eq!(answers[1], ("first".into(), Some(PreviewRefusal::Damaged)));
}
