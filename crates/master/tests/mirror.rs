//! Master-side replication: a mirror that is empty or behind catches up from
//! the node on attach (hello -> resume -> snapshot), with no gaps or dups.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_master::{Master, auth::Auth};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;

async fn node(dir: &std::path::Path) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

fn master(mirror: Store) -> Master {
    Master::new(
        mirror,
        Auth::new(Secret::new("t".into()), vec!["localhost:1".into()], false),
    )
}

async fn wait_for(mut f: impl FnMut() -> bool) {
    for _ in 0..500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached");
}

#[tokio::test]
async fn empty_and_stale_mirrors_catch_up_on_attach() {
    let dir = tempfile::tempdir().unwrap();
    let n = node(dir.path()).await;

    // Produce history through a first master.
    let m1 = master(Store::open_in_memory().unwrap());
    m1.add_link(n.attach().await, true).await.unwrap();
    let sid = m1
        .create_session(harness_master::CreateSpec {
            title: "t".into(),
            cwd: dir.path().display().to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let ended = |st: &Store| {
        st.events_after(sid, 0, 10_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "turn_ended")
            .count()
    };
    for (i, text) in ["one two three", "four five"].into_iter().enumerate() {
        m1.prompt(sid, text.into(), Default::default())
            .await
            .unwrap();
        let st = n.store().clone();
        wait_for(|| ended(&st) == i + 1).await;
    }
    let node_log = n.store().events_after(sid, 0, 10_000).unwrap();
    assert!(node_log.len() > 8);

    // A brand-new master with an empty mirror.
    let fresh = Store::open_in_memory().unwrap();
    let m2 = master(fresh.clone());
    m2.add_link(n.attach().await, true).await.unwrap();
    wait_for(|| fresh.head_seq(sid).unwrap() == node_log.len() as u64).await;
    assert_eq!(fresh.events_after(sid, 0, 10_000).unwrap(), node_log);

    // A stale mirror holding only a prefix.
    let stale = Store::open_in_memory().unwrap();
    for e in &node_log[..4] {
        stale.append("local", e).unwrap();
    }
    let m3 = master(stale.clone());
    m3.add_link(n.attach().await, true).await.unwrap();
    wait_for(|| stale.head_seq(sid).unwrap() == node_log.len() as u64).await;
    assert_eq!(stale.events_after(sid, 0, 10_000).unwrap(), node_log);

    // Live events keep flowing to the current master after catch-up.
    m3.prompt(sid, "six".into(), Default::default())
        .await
        .unwrap();
    let st = n.store().clone();
    wait_for(|| ended(&st) == 3).await;
    let full = n.store().events_after(sid, 0, 10_000).unwrap();
    wait_for(|| stale.head_seq(sid).unwrap() == full.len() as u64).await;
    assert_eq!(stale.events_after(sid, 0, 10_000).unwrap(), full);
}

#[tokio::test]
async fn commands_to_unknown_sessions_and_offline_nodes_fail_cleanly() {
    let m = master(Store::open_in_memory().unwrap());
    let err = m
        .prompt(
            harness_core::SessionId::new(),
            "x".into(),
            Default::default(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("unknown session"));
    let err = m
        .create_session(harness_master::CreateSpec {
            title: "t".into(),
            cwd: "/".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not connected"), "{err}");
}
