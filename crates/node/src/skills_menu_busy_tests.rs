//! The node answers two `session.skills_menu` requests at a time (X.65): with
//! both places taken (by work that has outlived its wait, see
//! `session_skills::bounded_holding`) a third is `busy` at once, and the page
//! keeps the menu it has.

use super::*;
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_protocol::{MasterToNode, NodeToMaster};

#[tokio::test]
async fn a_request_with_both_places_taken_is_busy() {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let data = base.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let mut policy = crate::NodePolicy {
        workspace_roots: vec![base.clone()],
        max_approval_mode: ApprovalMode::Auto,
        ..Default::default()
    };
    policy.protect(&data);
    let cfg = crate::NodeConfig {
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
        store: harness_store::Store::open(&data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = crate::NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let _hello = link.from_node.recv().await.unwrap();
    let held = [
        node.inner.skills_menus.clone().try_acquire_owned().unwrap(),
        node.inner.skills_menus.clone().try_acquire_owned().unwrap(),
    ];
    let agent: harness_core::agent::AgentSpec = serde_json::from_value(serde_json::json!({
        "agent_id": "a", "name": "A", "system_prompt": "", "tools": "all", "prompt_hash": "h"
    }))
    .unwrap();
    let ask = |id: &str| MasterToNode::SkillsMenu {
        req_id: id.into(),
        session_id: SessionId::new(),
        agent: agent.clone(),
    };
    link.to_node.try_send(ask("3")).unwrap();
    let busy = loop {
        let m = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("no answer")
            .unwrap();
        if let NodeToMaster::SkillsMenuResult { req_id, busy, .. } = m {
            break (req_id, busy);
        }
    };
    assert_eq!(busy, ("3".to_string(), true));
    // A place that comes back is used again (an unknown chat: an empty list).
    drop(held);
    link.to_node.try_send(ask("4")).unwrap();
    let again = loop {
        let m = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("no answer")
            .unwrap();
        if let NodeToMaster::SkillsMenuResult {
            req_id,
            busy,
            skills,
        } = m
        {
            break (req_id, busy, skills.len());
        }
    };
    assert_eq!(again, ("4".to_string(), false, 0));
}
