//! Remote nodes over the real link: join tokens, pinned TLS, Ed25519 auth,
//! a link cut mid-turn (logs must converge), remote cancel, revocation.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::Profile;
use harness_core::{Secret, SessionId};
use harness_link::client::{Remote, join, run};
use harness_link::{LinkError, MasterCert, NodeKey};
use harness_master::auth::Auth;
use harness_master::{CreateSpec, Master, PromptOpts};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

struct Env {
    master: Master,
    url: String,
    fp: String,
    dir: tempfile::TempDir,
}

async fn master() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![], false),
    );
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    let cert = Arc::new(MasterCert::load_or_create(dir.path()).unwrap());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let fp = cert.fingerprint();
    master.set_node_listener(port, fp.clone());
    master.serve_nodes(l, cert).unwrap();
    Env {
        master,
        url: format!("wss://127.0.0.1:{port}"),
        fp,
        dir,
    }
}

async fn node(dir: &std::path::Path, id: &str) -> NodeRuntime {
    std::fs::create_dir_all(dir.join("ws")).unwrap();
    NodeRuntime::start(NodeConfig {
        node_id: id.into(),
        hostname: "box".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.join("ws")],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(15),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
    })
    .await
    .unwrap()
}

/// A TCP proxy whose live connections can be cut.
async fn proxy(target: String) -> (String, Arc<Mutex<Vec<tokio::task::AbortHandle>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let conns: Arc<Mutex<Vec<tokio::task::AbortHandle>>> = Arc::default();
    let c2 = conns.clone();
    tokio::spawn(async move {
        loop {
            let (mut a, _) = l.accept().await.unwrap();
            let t = target.clone();
            let h = tokio::spawn(async move {
                let mut b = TcpStream::connect(t).await.unwrap();
                let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
            });
            c2.lock().unwrap().push(h.abort_handle());
        }
    });
    (format!("wss://127.0.0.1:{port}"), conns)
}

fn start_client(
    url: String,
    fp: String,
    node_id: String,
    key: NodeKey,
    rt: NodeRuntime,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), LinkError>>,
) {
    let stop = CancellationToken::new();
    let s = stop.clone();
    let h = tokio::spawn(async move {
        run(
            Remote {
                url,
                fingerprint: fp,
                node_id,
            },
            key,
            s,
            || {
                let rt = rt.clone();
                async move {
                    let l = rt.attach().await;
                    (l.to_node, l.from_node)
                }
            },
        )
        .await
    });
    (stop, h)
}

async fn wait_connected(m: &Master, id: &str, want: bool) {
    for _ in 0..300 {
        if m.nodes().iter().any(|n| n.node_id == id && n.connected) == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("node {id} connected != {want}");
}

async fn turn_done(store: &Store, sid: SessionId) -> Vec<harness_core::RawEvent> {
    for _ in 0..500 {
        let evs = store.events_after(sid, 0, 10_000).unwrap();
        if evs.iter().any(|e| e.kind == "turn_ended") {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("turn did not end");
}

#[tokio::test]
async fn join_rules() {
    let env = master().await;
    let key = NodeKey::load_or_create(&env.dir.path().join("n1.key")).unwrap();
    let token = env.master.create_join_token(None).await.unwrap();
    // Wrong pinned fingerprint: the TLS handshake fails before any secret is sent.
    let bad_fp = format!("sha256:{}", "0".repeat(64));
    assert!(matches!(
        join(&env.url, &bad_fp, &token, "box", &key).await,
        Err(LinkError::Io(_) | LinkError::Tls(_) | LinkError::Ws(_))
    ));
    let id = join(&env.url, &env.fp, &token, "My Box!", &key)
        .await
        .unwrap();
    assert_eq!(id, "my-box");
    // Single use.
    let key2 = NodeKey::load_or_create(&env.dir.path().join("n2.key")).unwrap();
    assert!(matches!(
        join(&env.url, &env.fp, &token, "box2", &key2).await,
        Err(LinkError::Rejected(_))
    ));
    // A registered key cannot join twice.
    let t2 = env.master.create_join_token(None).await.unwrap();
    assert!(matches!(
        join(&env.url, &env.fp, &t2, "again", &key).await,
        Err(LinkError::Rejected(_))
    ));
    // Reserved ids are never handed out.
    let t3 = env.master.create_join_token(None).await.unwrap();
    assert_eq!(
        join(&env.url, &env.fp, &t3, "local", &key2).await.unwrap(),
        "node-local"
    );
    // Auth with another node's id fails (key mismatch).
    let rt = node(env.dir.path(), "my-box").await;
    let (stop, h) = start_client(
        env.url.clone(),
        env.fp.clone(),
        "node-local".into(),
        NodeKey::load_or_create(&env.dir.path().join("n1.key")).unwrap(),
        rt,
    );
    assert!(matches!(h.await.unwrap(), Err(LinkError::Rejected(_))));
    stop.cancel();
}

#[tokio::test]
async fn link_cut_mid_turn_converges_and_cancel_and_revoke_work() {
    let env = master().await;
    let key_path = env.dir.path().join("node.key");
    let key = NodeKey::load_or_create(&key_path).unwrap();
    let token = env.master.create_join_token(None).await.unwrap();
    let id = join(&env.url, &env.fp, &token, "worker", &key)
        .await
        .unwrap();
    let rt = node(env.dir.path(), &id).await;
    let target = env.url.trim_start_matches("wss://").to_string();
    let (purl, conns) = proxy(target).await;
    let (stop, _h) = start_client(purl, env.fp.clone(), id.clone(), key, rt.clone());
    wait_connected(&env.master, &id, true).await;

    let sid = env
        .master
        .create_session(CreateSpec {
            profile: Some(Profile::Agentic),
            node_id: Some(id.clone()),
            title: "remote".into(),
            cwd: env.dir.path().join("ws").display().to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    env.master
        .prompt(sid, "slow: 60".into(), PromptOpts::default())
        .await
        .unwrap();
    // Cut the link while the turn streams; the node keeps working.
    tokio::time::sleep(Duration::from_millis(300)).await;
    for c in conns.lock().unwrap().drain(..) {
        c.abort();
    }
    let node_log = turn_done(rt.store(), sid).await;
    // After reconnecting, the master resumes from its cursor: identical logs.
    let mirror = env.master.inner_mirror();
    let mut mirrored = vec![];
    for _ in 0..500 {
        mirrored = mirror.events_after(sid, 0, 10_000).unwrap();
        if mirrored.len() == node_log.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(mirrored, node_log, "mirror converged after the cut");

    // Remote cancel propagates over the link.
    env.master
        .prompt(sid, "slow: 400".into(), PromptOpts::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    env.master.cancel(sid).await.unwrap();
    let mut reason = None;
    for _ in 0..300 {
        let evs = mirror.events_after(sid, 0, 10_000).unwrap();
        if let Some(e) = evs.iter().rev().find(|e| e.kind == "turn_ended")
            && evs.iter().filter(|e| e.kind == "turn_ended").count() == 2
        {
            reason = e.payload["reason"].as_str().map(str::to_string);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(reason.as_deref(), Some("cancelled"));

    // Revoke: the live link drops and the key no longer works.
    assert!(env.master.revoke_node(&id).await.unwrap());
    wait_connected(&env.master, &id, false).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !env.master
            .nodes()
            .iter()
            .any(|n| n.node_id == id && n.connected)
    );
    stop.cancel();
}
