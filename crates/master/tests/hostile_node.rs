//! Artifact transfer against nodes that lie (T18): a protocol-level fake
//! node publishes files and answers the master's fetches honestly, with
//! extra bytes, other bytes, someone else's request ids, or not at all.
//! Every lie fails cleanly: the version is failed with a plain reason, no
//! blob or temp file is left, and the stage is released. Ownership comes
//! from the publishing session's own row only; nodes without the
//! capability are never sent `artifact.*`.

mod common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_core::artifacts::{ArtifactMeta, StagedBy};
use harness_core::event::{ArtifactStaged, EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{CreateSpec, Master, router};
use harness_node::NodeLink;
use harness_protocol::{CAP_ARTIFACTS, MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::artifacts::VersionRow;
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

/// How the fake node answers fetches.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Honest,
    /// One byte more than asked for.
    Extra,
    /// Other bytes than the ones it declared.
    Other,
    /// Answers under request ids the master never used.
    Foreign,
    /// Never answers.
    Stall,
    /// `busy` to the first fetch, then honest.
    BusyOnce,
}

#[derive(Clone)]
struct Fake {
    id: String,
    /// `None` once the link is closed.
    tx: Arc<Mutex<Option<mpsc::Sender<NodeToMaster>>>>,
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    seqs: Arc<Mutex<HashMap<SessionId, u64>>>,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    mode: Arc<Mutex<Mode>>,
    /// What its `session_created` names as parent when it answers a create.
    parent: Arc<Mutex<Option<SessionId>>>,
}

/// A node that announces `caps` and keeps its files in `files` (shared
/// with a later link of the same node, as a restarted node keeps its
/// stage).
fn fake_with(
    id: &str,
    caps: &[&str],
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "evil".into(),
        version: "0".into(),
        capabilities: caps
            .iter()
            .map(|c| c.to_string())
            .chain(["exec:sandbox".to_string()])
            .collect(),
        sessions: vec![],
    })
    .unwrap();
    let f = Fake {
        id: id.into(),
        tx: Arc::new(Mutex::new(Some(tx))),
        seen: Arc::default(),
        seqs: Arc::default(),
        files,
        mode: Arc::new(Mutex::new(Mode::Honest)),
        parent: Arc::default(),
    };
    let me = f.clone();
    tokio::spawn(async move {
        let mut busy_sent = false;
        while let Some(msg) = commands.recv().await {
            me.seen.lock().unwrap().push(msg.clone());
            match &msg {
                MasterToNode::Create {
                    session_id, cwd, ..
                } => {
                    let parent = *me.parent.lock().unwrap();
                    me.emit(*session_id, created(&me.id, cwd, parent))
                }
                MasterToNode::ArtifactFetch {
                    req_id,
                    stage_id,
                    offset,
                    len,
                    ..
                } => {
                    let mode = *me.mode.lock().unwrap();
                    if mode == Mode::BusyOnce && !busy_sent {
                        busy_sent = true;
                        me.chunk(req_id, stage_id, *offset, vec![], false, Some("busy"));
                        continue;
                    }
                    me.answer(mode, req_id, stage_id, *offset, *len);
                }
                _ => {}
            }
            if let Some(req_id) = msg.req_id() {
                me.send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, f)
}

fn fake(id: &str, caps: &[&str]) -> (NodeLink, Fake) {
    fake_with(id, caps, Arc::default())
}

fn created(node: &str, cwd: &str, parent: Option<SessionId>) -> EventKind {
    EventKind::SessionCreated(SessionCreated {
        exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
        no_callbacks: false,
        title: "t".into(),
        cwd: cwd.into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: node.into(),
        approval_mode: ApprovalMode::Ask,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: parent,
    })
}

impl Fake {
    fn send(&self, m: NodeToMaster) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.try_send(m);
        }
    }

    /// The node's process goes away: its link closes.
    fn close(&self) {
        self.tx.lock().unwrap().take();
    }

    fn emit(&self, sid: SessionId, kind: EventKind) {
        let seq = {
            let mut s = self.seqs.lock().unwrap();
            let n = s.entry(sid).or_default();
            *n += 1;
            *n
        };
        let (kind, payload) = kind.into_parts();
        let event = RawEvent {
            session_id: sid,
            seq,
            ts_ms: harness_core::now_ms(),
            kind,
            payload,
        };
        self.send(NodeToMaster::Event { event });
    }

    fn chunk(
        &self,
        req: &str,
        stage: &str,
        offset: u64,
        data: Vec<u8>,
        eof: bool,
        error: Option<&str>,
    ) {
        self.send(NodeToMaster::ArtifactChunk {
            req_id: req.into(),
            stage_id: stage.into(),
            offset,
            data: base64::engine::general_purpose::STANDARD.encode(data),
            eof,
            error: error.map(str::to_string),
        });
    }

    fn answer(&self, mode: Mode, req: &str, stage: &str, offset: u64, len: u32) {
        let mut bytes = self
            .files
            .lock()
            .unwrap()
            .get(stage)
            .cloned()
            .unwrap_or_default();
        if mode == Mode::Other {
            bytes.iter_mut().for_each(|b| *b ^= 0xff);
        }
        let from = (offset as usize).min(bytes.len());
        let to = (from + len as usize).min(bytes.len());
        let mut data = bytes[from..to].to_vec();
        let eof = to == bytes.len();
        match mode {
            Mode::Stall => {}
            Mode::Extra => {
                data.push(0);
                self.chunk(req, stage, offset, data, eof, None);
            }
            Mode::Foreign => self.chunk(&harness_core::new_id(), stage, offset, data, eof, None),
            _ => self.chunk(req, stage, offset, data, eof, None),
        }
    }

    /// Stage `bytes` and record it in `sid`'s log, as a node does.
    fn publish(&self, sid: SessionId, name: &str, bytes: &[u8]) -> String {
        let stage_id = harness_core::new_id();
        self.files
            .lock()
            .unwrap()
            .insert(stage_id.clone(), bytes.to_vec());
        self.emit(
            sid,
            EventKind::ArtifactStaged(Box::new(ArtifactStaged {
                stage_id: stage_id.clone(),
                call_id: Some("c1".into()),
                by: StagedBy::Agent,
                name: name.into(),
                filename: format!("{name}.bin"),
                packed: None,
                size: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(bytes)),
                kind_hint: None,
                source_path: format!("/w/{name}.bin"),
                notes: None,
                meta: ArtifactMeta::default(),
            })),
        );
        stage_id
    }

    fn set(&self, m: Mode) {
        *self.mode.lock().unwrap() = m;
    }

    fn saw(&self, pred: impl Fn(&MasterToNode) -> bool) -> usize {
        self.seen.lock().unwrap().iter().filter(|m| pred(m)).count()
    }

    fn artifact_messages(&self) -> usize {
        self.saw(|m| {
            matches!(
                m,
                MasterToNode::ArtifactFetch { .. }
                    | MasterToNode::ArtifactRelease { .. }
                    | MasterToNode::ArtifactStage { .. }
            )
        })
    }

    /// Whether the master told this node to delete `stage`. The master sends
    /// that after it has recorded the version's final status, so a test that
    /// has just seen the status must wait for the message, not look once.
    async fn released(&self, stage: &str) -> bool {
        for _ in 0..400 {
            let seen = self.saw(
                |m| matches!(m, MasterToNode::ArtifactRelease { stage_id, .. } if stage_id == stage),
            );
            if seen > 0 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    }
}

struct T {
    base: String,
    master: Master,
    dir: tempfile::TempDir,
    alex: String,
    alex_id: String,
}

impl T {
    fn blobs(&self) -> PathBuf {
        self.dir.path().join("blobs")
    }

    fn store(&self) -> Store {
        self.master.inner_mirror()
    }
}

async fn master_on(dir: &std::path::Path, edit: impl FnOnce(&mut Config)) -> (Master, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let store = Store::open(&dir.join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut cfg = Config::default();
    edit(&mut cfg);
    let (rt, _) = ProviderRuntime::start(cfg, dir, Default::default()).unwrap();
    master.set_provider_runtime(Arc::new(rt));
    harness_master::storage::open(&master, dir).unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    (master, format!("http://{addr}"))
}

async fn start_with(edit: impl FnOnce(&mut Config)) -> T {
    let dir = tempfile::tempdir().unwrap();
    let (master, base) = master_on(dir.path(), edit).await;
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = common::signed_in(r).await;
    let mut t = T {
        base,
        master,
        dir,
        alex,
        alex_id: String::new(),
    };
    let (_, me) = req(&t, Method::GET, "/api/me", None).await;
    t.alex_id = me["user"]["user_id"].as_str().unwrap().to_string();
    harness_master::artifact_pull::set_timeouts(
        &t.master,
        Duration::from_millis(400),
        Duration::from_secs(20),
    );
    t
}

async fn start() -> T {
    start_with(|_| {}).await
}

async fn req(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .login(&t.alex)
        .same_origin();
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// Link a fake node and open an agentic chat of alex's on it.
async fn chat(t: &T, link: NodeLink, node: &str) -> SessionId {
    t.master.add_link(link, false).await.unwrap();
    t.master
        .create_session(CreateSpec {
            profile: Some(Profile::Agentic),
            node_id: Some(node.into()),
            cwd: "/w".into(),
            owner_id: Some(t.alex_id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
}

async fn versions(t: &T, sid: SessionId) -> Vec<VersionRow> {
    let s = sid.to_string();
    t.store()
        .call(move |st| st.artifact_session_versions(&s))
        .await
        .unwrap()
}

/// The chat's first version, once the master recorded it.
async fn recorded(t: &T, sid: SessionId) -> VersionRow {
    for _ in 0..400 {
        if let Some(v) = versions(t, sid).await.into_iter().next() {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never recorded");
}

/// The chat's versions once none is pending.
async fn settled(t: &T, sid: SessionId, n: usize) -> Vec<VersionRow> {
    for _ in 0..400 {
        let v = versions(t, sid).await;
        if v.len() >= n && v.iter().all(|v| v.status != "pending") {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("still pending: {:?}", versions(t, sid).await);
}

/// Bytes that differ per position, `n` long.
fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + i / 300) as u8).collect()
}

fn tmp_files(t: &T) -> usize {
    std::fs::read_dir(t.blobs().join("tmp")).unwrap().count()
}

#[tokio::test]
async fn an_honest_transfer_arrives_byte_for_byte() {
    let t = start().await;
    let (link, node) = fake("evil", &["profile:agentic", CAP_ARTIFACTS]);
    let sid = chat(&t, link, "evil").await;
    node.set(Mode::BusyOnce);
    // Three chunks: 256 KiB, 256 KiB and the rest.
    let bytes = data(600_000);
    let stage = node.publish(sid, "app", &bytes);
    let v = settled(&t, sid, 1).await;
    assert_eq!(v[0].status, "ready", "{:?}", v[0].error);
    assert!(node.released(&stage).await);
    assert!(node.saw(|m| matches!(m, MasterToNode::ArtifactFetch { len, .. } if *len <= harness_protocol::MAX_CHUNK)) >= 4,
        "three chunks and the busy retry");
    let path = format!(
        "/api/artifacts/{}/versions/{}/download",
        v[0].artifact_id, v[0].version_id
    );
    let r = reqwest::Client::new()
        .get(format!("{}{path}", t.base))
        .login(&t.alex)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let sha = hex::encode(Sha256::digest(&bytes));
    assert_eq!(r.headers()["x-content-sha256"], sha.as_str());
    assert_eq!(r.headers()["content-type"], "application/octet-stream");
    assert!(
        r.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment")
    );
    assert_eq!(r.bytes().await.unwrap().as_ref(), &bytes[..]);
    // Latest download redirects there, with the API token too.
    let r = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(format!(
            "{}/api/artifacts/{}/latest/download",
            t.base, v[0].artifact_id
        ))
        .header("authorization", "Bearer tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FOUND);
    assert_eq!(r.headers()["location"], path.as_str());
    assert_eq!(tmp_files(&t), 0);
}

#[tokio::test]
async fn lying_chunks_fail_cleanly() {
    let t = start().await;
    let (link, node) = fake("evil", &["profile:agentic", CAP_ARTIFACTS]);
    let sid = chat(&t, link, "evil").await;
    let cases = [
        (Mode::Extra, "different amount of data"),
        (Mode::Other, "does not match its checksum"),
        (Mode::Foreign, "stopped sending"),
        (Mode::Stall, "stopped sending"),
    ];
    for (i, (mode, why)) in cases.into_iter().enumerate() {
        node.set(mode);
        let stage = node.publish(sid, &format!("app{i}"), &data(300_000));
        let v = settled(&t, sid, i + 1).await;
        let v = v.iter().find(|v| v.stage_id == stage).unwrap();
        assert_eq!(v.status, "failed", "{mode:?}");
        assert!(
            v.error.as_deref().unwrap().contains(why),
            "{mode:?}: {:?}",
            v.error
        );
        assert!(node.released(&stage).await, "{mode:?}");
        assert!(t.store().known_blob_ids().unwrap().is_empty(), "{mode:?}");
        assert_eq!(tmp_files(&t), 0, "{mode:?}");
    }
}

#[tokio::test]
async fn ownership_comes_from_the_publishing_sessions_own_row() {
    let t = start().await;
    let (a_link, a) = fake("a", &["profile:agentic", CAP_ARTIFACTS]);
    let mine = chat(&t, a_link, "a").await;
    let (b_link, b) = fake("b", &["profile:agentic", CAP_ARTIFACTS]);
    t.master.add_link(b_link, false).await.unwrap();
    let audits = t.store().audit_count().unwrap();
    // Node b claims a file in node a's chat: the mirror refuses the event.
    b.seqs.lock().unwrap().insert(mine, 1);
    b.publish(mine, "planted", b"x");
    // A sub-session of b's whose parent is a's chat gets no owner.
    let sub = SessionId::new();
    b.emit(sub, created("b", "/w", Some(mine)));
    b.publish(sub, "planted", b"y");
    // The refusal is audited (and arrives after the planted event).
    for _ in 0..400 {
        if t.store().audit_count().unwrap() > audits {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(versions(&t, mine).await.is_empty());
    assert!(versions(&t, sub).await.is_empty());
    let row = t.store().session(sub).unwrap().unwrap();
    assert_eq!((row.owner_id, row.workspace_id), (None, None));
    assert!(
        t.store().audit_count().unwrap() > audits,
        "the refusal is audited"
    );
    assert_eq!(b.artifact_messages(), 0);
    let (_, list) = req(&t, Method::GET, "/api/artifacts", None).await;
    assert_eq!(list["artifacts"], json!([]));
    // a's own publish still works.
    a.publish(mine, "real", b"real bytes");
    assert_eq!(settled(&t, mine, 1).await[0].status, "ready");
}

#[tokio::test]
async fn a_31st_staged_file_in_an_hour_is_ignored() {
    let t = start().await;
    let (link, node) = fake("evil", &["profile:agentic", CAP_ARTIFACTS]);
    let sid = chat(&t, link, "evil").await;
    for i in 0..31 {
        node.publish(sid, &format!("f{i}"), format!("bytes {i}").as_bytes());
    }
    let v = settled(&t, sid, 30).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(versions(&t, sid).await.len(), 30, "no row for the 31st");
    assert!(v.iter().any(|v| v.status == "ready"));
}

#[tokio::test]
async fn nodes_without_the_capability_are_never_contacted() {
    let t = start().await;
    let (link, old) = fake("old", &["profile:agentic"]);
    let sid = chat(&t, link, "old").await;
    old.publish(sid, "app", b"bytes");
    let v = settled(&t, sid, 1).await;
    assert_eq!(v[0].status, "failed");
    assert_eq!(
        v[0].error.as_deref(),
        Some(harness_master::artifacts::TOO_OLD)
    );
    let (st, body) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/artifacts"),
        Some(json!({"path": "app"})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Update the worker node")
    );
    assert_eq!(old.artifact_messages(), 0);

    // A capable node goes away mid-transfer: the version waits. It comes
    // back without the capability: the version fails, still unasked.
    let files = Arc::new(Mutex::new(HashMap::new()));
    let (link, flaky) = fake_with("flaky", &["profile:agentic", CAP_ARTIFACTS], files.clone());
    let sid = chat(&t, link, "flaky").await;
    flaky.set(Mode::Stall);
    flaky.publish(sid, "app", &data(1000));
    for _ in 0..100 {
        if flaky.artifact_messages() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    flaky.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        recorded(&t, sid).await.status,
        "pending",
        "waits for the node"
    );
    let (again, back) = fake_with("flaky", &["profile:agentic"], files);
    t.master.add_link(again, false).await.unwrap();
    let v = settled(&t, sid, 1).await;
    assert_eq!(
        v[0].error.as_deref(),
        Some(harness_master::artifacts::TOO_OLD)
    );
    assert_eq!(back.artifact_messages(), 0);
}

#[tokio::test]
async fn a_revoked_node_fails_its_pending_files() {
    let t = start().await;
    let (link, node) = fake("evil", &["profile:agentic", CAP_ARTIFACTS]);
    let sid = chat(&t, link, "evil").await;
    node.set(Mode::Stall);
    harness_master::artifact_pull::set_timeouts(
        &t.master,
        Duration::from_secs(30),
        Duration::from_secs(60),
    );
    node.publish(sid, "app", &data(1000));
    assert_eq!(recorded(&t, sid).await.status, "pending");
    harness_master::artifacts::on_node_revoked(&t.master, "evil").await;
    let v = settled(&t, sid, 1).await;
    assert_eq!(v[0].error.as_deref(), Some("the worker node was removed"));
}

#[tokio::test]
async fn limits_refuse_before_any_byte_moves() {
    let t = start_with(|c| {
        c.storage.max_file_mb = 1;
        c.storage.account_quota_mb = 1;
    })
    .await;
    let (link, node) = fake("evil", &["profile:agentic", CAP_ARTIFACTS]);
    let sid = chat(&t, link, "evil").await;
    node.publish(sid, "big", &data(1024 * 1024 + 1));
    let v = settled(&t, sid, 1).await;
    assert!(
        v[0].error.as_deref().unwrap().contains("1 MB limit"),
        "{:?}",
        v[0].error
    );
    node.publish(sid, "a", &data(700_000));
    settled(&t, sid, 2).await;
    node.publish(sid, "b", &data(700_001));
    let v = settled(&t, sid, 3).await;
    let full = v.iter().find(|v| v.filename == "b.bin").unwrap();
    assert!(
        full.error
            .as_deref()
            .unwrap()
            .starts_with("Your storage is full"),
        "{:?}",
        full.error
    );
    // Only the file that fit was ever fetched.
    let fetched: Vec<String> = node
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|m| match m {
            MasterToNode::ArtifactFetch { stage_id, .. } => Some(stage_id.clone()),
            _ => None,
        })
        .collect();
    let a = &v.iter().find(|v| v.filename == "a.bin").unwrap().stage_id;
    assert!(fetched.iter().all(|s| s == a), "{fetched:?}");
}

/// The master restarts mid-transfer: the version is still pending in the
/// database, and the next master pulls it when the node says hello.
#[tokio::test]
async fn a_restart_mid_transfer_resumes_at_the_next_hello() {
    let t = start().await;
    let files = Arc::new(Mutex::new(HashMap::new()));
    let (link, node) = fake_with("evil", &["profile:agentic", CAP_ARTIFACTS], files.clone());
    let sid = chat(&t, link, "evil").await;
    node.set(Mode::Stall);
    let bytes = data(400_000);
    node.publish(sid, "app", &bytes);
    for _ in 0..100 {
        if node.artifact_messages() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(recorded(&t, sid).await.status, "pending");
    // The first master's link goes down (its process is gone).
    node.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let dir = t.dir.path().to_path_buf();
    let (second, _) = master_on(&dir, |_| {}).await;
    let (link, _node) = fake_with("evil", &["profile:agentic", CAP_ARTIFACTS], files);
    second.add_link(link, false).await.unwrap();
    let mut done = None;
    for _ in 0..400 {
        let s = sid.to_string();
        let v = second
            .inner_mirror()
            .call(move |st| st.artifact_session_versions(&s))
            .await
            .unwrap();
        if v[0].status != "pending" {
            done = Some(v);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let v = done.expect("pulled by the second master");
    assert_eq!(v[0].status, "ready", "{:?}", v[0].error);
    assert_eq!(v[0].sha256, hex::encode(Sha256::digest(&bytes)));
}

/// A user on the master, with their Personal workspace.
fn user(t: &T, name: &str) -> String {
    let id = format!("u-{name}");
    t.store()
        .create_user(&harness_store::NewUser {
            user_id: id.clone(),
            username: name.into(),
            password_hash: "x".into(),
            role: harness_store::Role::Member,
            can_agentic: true,
            must_change_password: false,
        })
        .unwrap();
    t.store().personal_workspace(&id).unwrap();
    id
}

/// The context of the last prompt the fake node got for `sid`.
fn prompt_context(node: &Fake, sid: SessionId) -> Option<String> {
    node.seen
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find_map(|m| match m {
            MasterToNode::Prompt {
                session_id,
                context,
                ..
            } if *session_id == sid => Some(context.clone().unwrap_or_default()),
            _ => None,
        })
}

/// L-1 (Gate 3 review): a node answers a create with `session_created`
/// naming another user's chat as parent, to get that chat's project
/// context on the next prompt. The parent is dropped (another owner, even
/// on the same node), the prompt carries no project, and a sub-agent
/// session the node creates on its own under the same owner keeps its
/// parent and its parent's project.
#[tokio::test]
async fn a_node_cannot_borrow_another_users_chat_as_parent() {
    let t = start().await;
    let (link, node) = fake("evil", &["profile:agentic"]);
    let victim = chat(&t, link, "evil").await;
    let ws = t.store().personal_workspace(&t.alex_id).unwrap();
    let project = t
        .store()
        .create_project(
            &harness_store::NewProject {
                project_id: harness_core::new_id(),
                workspace_id: ws,
                name: "Plans".into(),
                description: String::new(),
                icon: String::new(),
                accent: String::new(),
                instructions: "SECRET-PLAN-INSTRUCTIONS".into(),
                settings: "{}".into(),
                created_by: Some(t.alex_id.clone()),
                columns: vec![],
            },
            10,
        )
        .unwrap();
    t.store()
        .set_session_project(victim, Some(&project.project_id))
        .unwrap();
    // The control: the victim's own prompt carries its project.
    t.master
        .prompt(victim, "hi".into(), Default::default())
        .await
        .unwrap();
    assert!(
        prompt_context(&node, victim)
            .unwrap()
            .contains("SECRET-PLAN-INSTRUCTIONS")
    );

    let mallory = user(&t, "mallory");
    *node.parent.lock().unwrap() = Some(victim);
    let sid = t
        .master
        .create_session(CreateSpec {
            profile: Some(Profile::Agentic),
            node_id: Some("evil".into()),
            cwd: "/w".into(),
            owner_id: Some(mallory.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    *node.parent.lock().unwrap() = None;
    let row = t.store().session(sid).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some(mallory.as_str()));
    assert_eq!(row.parent_id, None, "another owner's chat is no parent");
    assert_ne!(
        row.workspace_id,
        t.store().session(victim).unwrap().unwrap().workspace_id
    );
    assert!(
        harness_master::projects::project_of(&t.master, &row)
            .await
            .is_none()
    );
    t.master
        .prompt(sid, "hi".into(), Default::default())
        .await
        .unwrap();
    let context = prompt_context(&node, sid).unwrap();
    assert!(!context.contains("SECRET-PLAN"), "{context}");

    // A sub-agent session the node starts itself under the victim's chat
    // (a delegation) is the victim's, with the victim's project.
    let child = SessionId::new();
    node.emit(child, created("evil", "/w", Some(victim)));
    let mut row = None;
    for _ in 0..400 {
        row = t.store().session(child).unwrap();
        if row.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let row = row.expect("the child is mirrored");
    assert_eq!(row.parent_id, Some(victim.to_string()));
    assert_eq!(row.owner_id.as_deref(), Some(t.alex_id.as_str()));
    let p = harness_master::projects::project_of(&t.master, &row).await;
    assert_eq!(p.map(|p| p.project_id), Some(project.project_id));
}
