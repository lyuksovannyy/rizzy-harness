//! `project.create` through the node (ADOPTION_PLAN X.26, PLAN.md T77): a
//! node announces it only where node.toml names a usable projects folder,
//! never the path; it answers with one `project.created`, never an ack; a
//! node that did not offer it (turned off, a chat node) refuses with the
//! reason, and makes nothing. The folder rules themselves are tested in
//! `src/projects.rs`.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_PROJECTS, MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};

struct Node {
    _dir: tempfile::TempDir,
    projects: std::path::PathBuf,
    _node: NodeRuntime,
    link: NodeLink,
    caps: Vec<String>,
}

async fn start(dir_key: Option<&str>, per_hour: usize, profile: Profile) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (ws, data) = (base.join("ws"), base.join("data"));
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let projects = ws.join("projects");
    let mut policy = NodePolicy {
        workspace_roots: vec![ws.clone()],
        ..Default::default()
    };
    policy.skills.roots.clear();
    policy.limits.projects_per_hour = per_hour;
    policy.projects.dir = Some(match dir_key {
        Some(k) => k.into(),
        None => projects.clone(),
    });
    policy.protect(&data);
    policy.locate_projects().unwrap();
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
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let node = NodeRuntime::start(cfg).await.unwrap();
    let mut link = node.attach().await;
    let caps = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    Node {
        _dir: dir,
        projects,
        _node: node,
        link,
        caps,
    }
}

impl Node {
    async fn create(&mut self, name: &str) -> Result<String, String> {
        self.link
            .to_node
            .try_send(MasterToNode::ProjectCreate {
                req_id: "r".into(),
                name: name.into(),
            })
            .unwrap();
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
                .await
                .expect("no answer")
                .expect("link closed");
            match msg {
                NodeToMaster::ProjectCreated { req_id, path, error } => {
                    assert_eq!(req_id, "r");
                    return match (path, error) {
                        (Some(p), None) => Ok(p),
                        (None, Some(e)) => Err(e),
                        other => panic!("{other:?}"),
                    };
                }
                NodeToMaster::Ack { req_id, .. } => panic!("acked {req_id}"),
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn a_node_with_a_projects_folder_offers_and_makes_one() {
    let mut n = start(None, 2, Profile::Agentic).await;
    assert!(n.caps.contains(&CAP_PROJECTS.to_string()));
    assert!(
        !n.caps.iter().any(|c| c.contains('/')),
        "no path in the hello: {:?}",
        n.caps
    );
    let p = n.create("Project Name Example").await.unwrap();
    let p = std::path::Path::new(&p);
    assert_eq!(p.parent(), Some(n.projects.as_path()));
    assert!(
        p.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("project-name-example-")
    );
    assert!(p.join(".git").is_dir());
    // The hour's allowance (2 here) holds through the link.
    n.create("b").await.unwrap();
    let e = n.create("c").await.unwrap_err();
    assert!(e.contains("projects_per_hour"), "{e}");
}

/// A hostile master asking a node that did not offer it: turned off in
/// node.toml (`dir = ""`), `projects_per_hour = 0`, or a chat node. Each
/// refuses with the reason; nothing is made.
#[tokio::test]
async fn a_node_that_did_not_offer_it_refuses() {
    for (key, per_hour, profile) in [
        (Some(""), 5, Profile::Agentic),
        (None, 0, Profile::Agentic),
        (None, 5, Profile::Chat),
    ] {
        let mut n = start(key, per_hour, profile).await;
        if profile == Profile::Agentic {
            assert!(!n.caps.contains(&CAP_PROJECTS.to_string()), "{key:?} {per_hour}");
        }
        let e = n.create("x").await.unwrap_err();
        assert!(e.contains("makes no project folders"), "{e}");
        assert!(!n.projects.exists(), "{key:?} {per_hour} {profile:?}");
    }
}
