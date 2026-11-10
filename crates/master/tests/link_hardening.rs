//! What the master accepts from a remote node's hello (docs/security-node-link.md).

use harness_core::Secret;
use harness_master::Master;
use harness_master::auth::Auth;
use harness_node::NodeLink;
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::Store;
use tokio::sync::mpsc;

fn link(id: &str, hostname: &str, caps: &[&str]) -> (NodeLink, mpsc::Receiver<MasterToNode>) {
    let (to_node, commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: hostname.into(),
        version: "1".repeat(200),
        capabilities: caps.iter().map(|c| c.to_string()).collect(),
        sessions: vec![],
    })
    .unwrap();
    // Keep the sender alive so the link stays open.
    std::mem::forget(tx);
    (NodeLink { to_node, from_node }, commands)
}

fn master() -> Master {
    Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("t".into()), vec![], false),
    )
}

#[tokio::test]
async fn a_remote_node_cannot_be_the_chat_runtime() {
    let m = master();
    let (l, _c) = link("evil", "h", &["profile:chat"]);
    let err = m.add_link(l, false).await.unwrap_err();
    assert!(err.contains("chat profile"), "{err}");
    assert!(m.nodes().iter().all(|n| n.node_id != "evil"));
    // The master's own runtime (embedded) still may.
    let (l, _c2) = link("master", "h", &["profile:chat"]);
    m.add_link(l, true).await.unwrap();
    assert_eq!(
        m.default_node(harness_core::event::Profile::Chat)
            .as_deref(),
        Some("master")
    );
}

#[tokio::test]
async fn what_a_remote_node_says_about_itself_is_cleaned() {
    let m = master();
    let (l, _c) = link("n1", &format!("box\u{1b}[31m{}", "x".repeat(100)), &[]);
    m.add_link(l, false).await.unwrap();
    let n = m.nodes().into_iter().find(|n| n.node_id == "n1").unwrap();
    assert!(
        !n.hostname.chars().any(char::is_control),
        "{:?}",
        n.hostname
    );
    assert_eq!(n.hostname.chars().count(), 64);
    assert_eq!(n.version.chars().count(), 64);
}
