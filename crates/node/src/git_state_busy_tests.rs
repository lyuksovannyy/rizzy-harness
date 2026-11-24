//! The node answers two `session.git_state` requests at a time (X.64): a
//! third is `busy` at once, and the page keeps what it shows. The slow work
//! is a stand-in `git` whose `diff-index` sleeps (the test thread's
//! `TEST_GIT`; the runtime is single-threaded, so its tasks see it).

use super::*;
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_protocol::{MasterToNode, NodeToMaster};

#[tokio::test]
async fn the_third_concurrent_request_is_busy() {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let (ws, data) = (base.join("ws"), base.join("data"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(&ws)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap()
            .success();
        assert!(ok);
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(ws.join("a"), "a").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "i"]);
    let script = base.join("slow-git.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in\nupdate-index) exit 0;;\n\
         diff-index) exec sleep 30;;\nesac; done\nexit 0\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
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
        checkpoints_dir: Some(data.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    };
    let node = crate::NodeRuntime::start(cfg).await.unwrap();
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
        if let NodeToMaster::Ack { ok, error, .. } = link.from_node.recv().await.unwrap() {
            assert!(ok, "{error:?}");
            break;
        }
    }
    super::git::use_test_git(Some(script));
    for id in ["1", "2", "3"] {
        link.to_node
            .try_send(MasterToNode::GitState {
                req_id: id.into(),
                session_id: sid,
            })
            .unwrap();
    }
    // The first answer to come is the third's: busy, with no state.
    let first = loop {
        let m = tokio::time::timeout(Duration::from_secs(10), link.from_node.recv())
            .await
            .expect("no answer")
            .unwrap();
        if let NodeToMaster::GitStateResult {
            req_id,
            state,
            busy,
        } = m
        {
            break (req_id, state, busy);
        }
    };
    super::git::use_test_git(None);
    assert_eq!(first, ("3".to_string(), None, true));
}
