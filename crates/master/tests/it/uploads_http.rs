//! Attaching any file (PLAN.md T94, docs/plan/X.53.md) through the master:
//! a file the model cannot read as text is stored as the owner's blob (the
//! master never holds it whole, runs no file tool), and at send is pushed
//! to the chat's node in acked chunks ahead of the prompt that names it.
//! Text, PDFs and images stay as they were. Refused where it cannot go: a
//! chat without tools, a node that cannot save files, a file over the
//! node's limit; another person's upload, an id used twice; and a send the
//! node refuses gives the upload back, while one whose answer is lost does
//! not (the node may have the file).

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_UPLOAD_MAX_PREFIX, CAP_UPLOADS, CAP_UPLOADS_PER_MESSAGE_PREFIX, MAX_CHUNK, MasterToNode,
    NodeToMaster, PROTOCOL_VERSION, UploadRef,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

const TOKEN: &str = "api-secret";

/// What a fake node does, changed by a test while it runs.
#[derive(Default)]
struct Knobs {
    /// Refuse the n-th `session.upload` chunk it gets (0-based).
    fail_chunk: Mutex<Option<usize>>,
    /// Refuse a prompt with this error.
    refuse_prompt: Mutex<Option<String>>,
    /// Never answer a prompt (the master's wait runs out).
    silent_prompt: Mutex<bool>,
    /// Wait this long before acking each chunk (a slow link).
    slow_chunk_ms: Mutex<u64>,
}

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    /// The bytes of each pushed upload, reassembled in offset order.
    bytes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    chunks: Arc<AtomicUsize>,
    knobs: Arc<Knobs>,
    _leave: mpsc::UnboundedSender<()>,
}

impl Fake {
    fn chunks(&self) -> Vec<(String, u64, u64, usize)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::UploadChunk {
                    upload_id,
                    offset,
                    size,
                    data,
                    ..
                } => Some((
                    upload_id.clone(),
                    *offset,
                    *size,
                    base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .unwrap()
                        .len(),
                )),
                _ => None,
            })
            .collect()
    }

    fn prompts(&self) -> Vec<Vec<UploadRef>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::Prompt(p) => Some(p.uploads.clone()),
                _ => None,
            })
            .collect()
    }

    /// The order the node got uploads and prompts in: `u` for a chunk, `p`
    /// for a prompt.
    fn order(&self) -> String {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::UploadChunk { .. } => Some('u'),
                MasterToNode::Prompt(_) => Some('p'),
                _ => None,
            })
            .collect()
    }
}

/// An agentic and chat node `id` announcing `caps`, which acks every
/// command (unless a knob says otherwise), records every command and the
/// bytes of every upload pushed to it.
fn fake(id: &str, caps: &[String], chat_runtime: bool) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec!["native".to_string()];
    if chat_runtime {
        capabilities.push("profile:chat".to_string());
    } else {
        capabilities.push("profile:agentic".to_string());
        capabilities.push("exec:sandbox".to_string());
    }
    capabilities.extend(caps.iter().cloned());
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "1".into(),
        capabilities,
        sessions: vec![],
    })
    .unwrap();
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let bytes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
    let chunks = Arc::new(AtomicUsize::new(0));
    let knobs = Arc::new(Knobs::default());
    let (leave, mut left) = mpsc::unbounded_channel::<()>();
    let (node, keep, got, n, k) = (
        id.to_string(),
        seen.clone(),
        bytes.clone(),
        chunks.clone(),
        knobs.clone(),
    );
    tokio::spawn(async move {
        // The chat created before this one: a "child" chat is its sub-agent's.
        let mut last: Option<harness_core::SessionId> = None;
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    let mut error: Option<String> = None;
                    match &msg {
                        MasterToNode::Create { session_id, profile, exec_mode, title, .. } => {
                            let parent_of_this = last;
                            last = Some(*session_id);
                            let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                                forked: None,
                                exec_mode: *exec_mode,
                                no_callbacks: false,
                                title: "t".into(),
                                cwd: "/w".into(),
                                model: "demo::mock".into(),
                                backend: "native".into(),
                                node_id: node.clone(),
                                approval_mode: ApprovalMode::Ask,
                                profile: profile.unwrap_or(Profile::Agentic),
                                agent_id: None,
                                incognito: false,
                                parent_session: if title.as_str() == "child" {
                                    parent_of_this
                                } else {
                                    None
                                },
                                sub_folder: None,
                            })
                            .into_parts();
                            let _ = tx.try_send(NodeToMaster::Event {
                                event: RawEvent {
                                    session_id: *session_id,
                                    seq: 1,
                                    ts_ms: harness_core::now_ms(),
                                    kind,
                                    payload,
                                },
                            });
                        }
                        MasterToNode::UploadChunk { upload_id, offset, data, .. } => {
                            let ms = *k.slow_chunk_ms.lock().unwrap();
                            if ms > 0 {
                                tokio::time::sleep(Duration::from_millis(ms)).await;
                            }
                            let i = n.fetch_add(1, Ordering::SeqCst);
                            if *k.fail_chunk.lock().unwrap() == Some(i) {
                                error = Some("this node's disk is full".into());
                            } else {
                                let mut b = got.lock().unwrap();
                                let v = b.entry(upload_id.clone()).or_default();
                                v.truncate(*offset as usize);
                                v.extend(
                                    base64::engine::general_purpose::STANDARD
                                        .decode(data)
                                        .unwrap(),
                                );
                            }
                        }
                        MasterToNode::Prompt(_) => {
                            if *k.silent_prompt.lock().unwrap() {
                                continue;
                            }
                            error = k.refuse_prompt.lock().unwrap().clone();
                        }
                        _ => {}
                    }
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack {
                            req_id: req_id.into(),
                            ok: error.is_none(),
                            error,
                        });
                    }
                }
                _ = left.recv() => return,
            }
        }
    });
    (
        NodeLink { to_node, from_node },
        Fake {
            seen,
            bytes,
            chunks,
            knobs,
            _leave: leave,
        },
    )
}

struct T {
    base: String,
    master: Master,
    alex: String,
    /// Announces uploads: 1 MiB per file, 2 per message.
    node: Fake,
    /// Announces uploads with the default limits.
    big: Fake,
    /// A node from before uploads.
    old: Fake,
    /// Kept so the chat runtime's task is not told to leave.
    _plain: Fake,
    dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    harness_master::storage::open(&master, dir.path()).unwrap();
    let caps = |mb: u64, per: usize| {
        vec![
            CAP_UPLOADS.to_string(),
            format!("{CAP_UPLOAD_MAX_PREFIX}{mb}"),
            format!("{CAP_UPLOADS_PER_MESSAGE_PREFIX}{per}"),
        ]
    };
    let (l, node) = fake("small", &caps(1, 2), false);
    master.add_link(l, true).await.unwrap();
    let (l, big) = fake("big", &caps(10, 10), false);
    master.add_link(l, false).await.unwrap();
    let (l, old) = fake("old", &[], false);
    master.add_link(l, false).await.unwrap();
    // The master's own chat runtime: chats without tools.
    let (l, plain) = fake("plain", &[], true);
    master.add_link(l, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let alex = common::signed_in(r).await;
    T {
        base,
        master,
        alex,
        node,
        big,
        old,
        _plain: plain,
        dir,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match c {
        TOKEN => rb.bearer_auth(TOKEN),
        c => rb.login(c).same_origin(),
    };
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn account(t: &T, name: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "exec_ceiling": "host"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": pw}))
        .send()
        .await
        .unwrap();
    let temp = common::signed_in(r).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .json(&json!({"current": pw, "new": "a password of my own"}))
        .login(&temp)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    common::signed_in(r).await
}

async fn chat(t: &T, who: &str, node: &str, profile: &str) -> String {
    let body = json!({"cwd": "/w", "title": "t", "node_id": node, "profile": profile});
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

/// Upload `bytes` named `name` as `who`; `query` is added after the name.
async fn upload(t: &T, who: &str, name: &str, query: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!(
            "{}/api/attachments?name={}{query}",
            t.base,
            name.replace(' ', "%20")
        ))
        .login(who)
        .same_origin()
        .body(bytes)
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn send(t: &T, who: &str, sid: &str, ids: &[&str]) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "unpack it", "attachments": ids})),
        who,
    )
    .await
}

fn zip(n: usize) -> Vec<u8> {
    let mut v = b"PK\x03\x04\0\0".to_vec();
    v.extend((0..n).map(|i| (i % 251) as u8 ^ 0x5a));
    v
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn blob_files(t: &T) -> usize {
    let mut n = 0;
    let root = t.dir.path().join("blobs");
    for shard in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        if shard.file_name() == "tmp" {
            continue;
        }
        n += std::fs::read_dir(shard.path()).map_or(0, |d| d.count());
    }
    n
}

// ------------------------------------------------------------------

/// The whole path: stored, pushed in chunks of at most 256 KiB in order
/// ahead of the prompt, named in the prompt with the size and sha256 the
/// master computed while it wrote the file; the bytes the node got are the
/// bytes sent.
#[tokio::test]
async fn a_zip_is_stored_pushed_in_chunks_and_named_in_the_prompt() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let bytes = zip(600_000);
    let (st, v) = upload(
        &t,
        &t.alex,
        "Q3 report.zip",
        &format!("&session={sid}"),
        bytes.clone(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["saved"], json!(true), "{v}");
    assert_eq!(v["name"], "Q3 report.zip");
    assert_eq!(v["bytes"], json!(bytes.len()));
    assert_eq!(blob_files(&t), 1, "the bytes are a stored file, not a row");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let chunks = t.big.chunks();
    assert!(chunks.len() >= 3, "{chunks:?}");
    let mut at = 0u64;
    for (up, offset, size, n) in &chunks {
        assert_eq!(up, &id);
        assert_eq!((*offset, *size), (at, bytes.len() as u64));
        assert!(*n <= MAX_CHUNK as usize && *n > 0);
        at += *n as u64;
    }
    assert_eq!(at, bytes.len() as u64);
    assert_eq!(t.big.order(), "u".repeat(chunks.len()) + "p");
    assert_eq!(t.big.bytes.lock().unwrap()[&id], bytes);
    let sent = &t.big.prompts()[0];
    assert_eq!(sent.len(), 1);
    assert_eq!(
        (
            sent[0].upload_id.as_str(),
            sent[0].name.as_str(),
            sent[0].size
        ),
        (id.as_str(), "Q3 report.zip", bytes.len() as u64)
    );
    assert_eq!(sent[0].sha256, sha(&bytes));
    // Taken: the same id is not offered again.
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("already used"), "{v}");
}

/// Text, Markdown and the like are read as text exactly as before, in a
/// coding chat too; a text file is never saved to the folder.
#[tokio::test]
async fn text_is_read_as_text_in_a_coding_chat_too() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (st, v) = upload(
        &t,
        &t.alex,
        "notes.md",
        &format!("&session={sid}"),
        b"# hi\nthere".to_vec(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v.get("saved").is_none() && v["chars"] == json!(10), "{v}");
    let id = v["attachment_id"].as_str().unwrap();
    let (st, v) = send(&t, &t.alex, &sid, &[id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(t.big.chunks().is_empty() && t.big.prompts()[0].is_empty());
}

/// A text file and a zip in one message: each goes its way.
#[tokio::test]
async fn text_and_a_file_travel_together() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let q = format!("&session={sid}");
    let (_, a) = upload(&t, &t.alex, "a.txt", &q, b"plain".to_vec()).await;
    let (_, b) = upload(&t, &t.alex, "b.zip", &q, zip(1000)).await;
    let (ia, ib) = (
        a["attachment_id"].as_str().unwrap(),
        b["attachment_id"].as_str().unwrap(),
    );
    let (st, v) = send(&t, &t.alex, &sid, &[ia, ib]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.big.prompts()[0].len(), 1);
    assert_eq!(t.big.prompts()[0][0].upload_id, ib);
}

/// A chat without tools refuses such a file at attach time, with the
/// reason; so does an upload that names no chat and claims no folder; and
/// nothing is stored.
#[tokio::test]
async fn a_plain_chat_refuses_the_file_with_the_reason() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "plain", "chat").await;
    for q in [format!("&session={sid}"), String::new()] {
        let (st, v) = upload(&t, &t.alex, "x.zip", &q, zip(500)).await;
        assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        let e = v["error"].as_str().unwrap();
        assert!(
            e.contains("cannot open files")
                && e.contains("switch to Agentic")
                && e.contains("PDF or an image"),
            "{e}"
        );
    }
    assert_eq!(blob_files(&t), 0);
    // A chat that is not the caller's: not found, whatever the file.
    let bob = account(&t, "bob").await;
    let (st, _) = upload(&t, &bob, "x.zip", &format!("&session={sid}"), zip(500)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

/// The send checks again: an upload made before the chat (the home
/// composer) that ends up in a plain chat is refused at send, and kept.
#[tokio::test]
async fn a_home_upload_sent_to_a_plain_chat_is_refused_at_send_and_kept() {
    let t = start().await;
    let (st, v) = upload(&t, &t.alex, "x.zip", "&agentic=1", zip(500)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let plain = chat(&t, &t.alex, "plain", "chat").await;
    let (st, v) = send(&t, &t.alex, &plain, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("cannot open files"),
        "{v}"
    );
    assert!(t.big.chunks().is_empty());
    // Kept: a coding chat takes it.
    let coding = chat(&t, &t.alex, "big", "agentic").await;
    let (st, v) = send(&t, &t.alex, &coding, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Over the node's number: refused with the number, before and while the
/// body arrives, and nothing stays on disk. The default number holds
/// where the chat is not known yet.
#[tokio::test]
async fn a_file_over_the_limit_is_refused_with_the_number() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "small", "agentic").await;
    let q = format!("&session={sid}");
    let (st, v) = upload(&t, &t.alex, "big.zip", &q, zip(1024 * 1024 + 1)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("1 MB"), "{v}");
    // No length announced: the cap holds as the bytes arrive.
    let stream =
        futures::stream::iter((0..16).map(|_| Ok::<_, std::io::Error>(vec![0u8; 256 * 1024])));
    let r = reqwest::Client::new()
        .post(format!("{}/api/attachments?name=s.zip{q}", t.base))
        .login(&t.alex)
        .same_origin()
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(r.text().await.unwrap().contains("1 MB"));
    assert_eq!(blob_files(&t), 0, "nothing stays");
    assert_eq!(
        std::fs::read_dir(t.dir.path().join("blobs/tmp")).map_or(0, |d| d.count()),
        0,
        "no temporary file stays"
    );
    // Exactly the limit is taken.
    let (st, v) = upload(&t, &t.alex, "ok.zip", &q, zip(1024 * 1024 - 6)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Unknown chat: the default, 10 MB.
    let (st, v) = upload(&t, &t.alex, "huge.zip", "&agentic=1", zip(11 * 1024 * 1024)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("10 MB"), "{v}");
}

/// A file over the limit of text (10 MiB) in a plain chat says the text
/// limit's number: nothing about folders it has not got.
#[tokio::test]
async fn a_big_file_in_a_plain_chat_names_the_text_limit() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "plain", "chat").await;
    let (st, v) = upload(
        &t,
        &t.alex,
        "x.bin",
        &format!("&session={sid}"),
        vec![1u8; 11 * 1024 * 1024],
    )
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("10 MB"), "{v}");
}

/// More files than the node takes per message: refused at send with the
/// number, nothing pushed, every upload kept.
#[tokio::test]
async fn too_many_files_for_the_node_are_refused_at_send() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "small", "agentic").await;
    let q = format!("&session={sid}");
    let mut ids = vec![];
    for i in 0..3 {
        let (st, v) = upload(&t, &t.alex, &format!("f{i}.zip"), &q, zip(100 + i)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        ids.push(v["attachment_id"].as_str().unwrap().to_string());
    }
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let (st, v) = send(&t, &t.alex, &sid, &refs).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("at most 2"), "{v}");
    assert!(t.node.chunks().is_empty());
    let (st, v) = send(&t, &t.alex, &sid, &refs[..2]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// A node from before uploads: the send is refused with "update the node"
/// and the upload stays in the composer's list (it can be sent once the
/// node is current).
#[tokio::test]
async fn an_old_node_refuses_the_send_and_nothing_is_lost() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "old", "agentic").await;
    let (st, v) = upload(&t, &t.alex, "x.zip", &format!("&session={sid}"), zip(500)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("update the node"),
        "{v}"
    );
    assert!(t.old.chunks().is_empty() && t.old.prompts().is_empty());
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(
        st,
        StatusCode::CONFLICT,
        "still known, not 'already used': {v}"
    );
}

/// Another account's upload id is no one's but its owner's, and a send
/// that cannot use it consumes nothing.
#[tokio::test]
async fn another_persons_upload_is_not_theirs_to_send() {
    let t = start().await;
    let bob = account(&t, "bob").await;
    let sid_alex = chat(&t, &t.alex, "big", "agentic").await;
    let sid_bob = chat(&t, &bob, "big", "agentic").await;
    let (st, v) = upload(
        &t,
        &t.alex,
        "mine.zip",
        &format!("&session={sid_alex}"),
        zip(300),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &bob, &sid_bob, &[&id]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(t.big.chunks().is_empty(), "nothing was pushed for bob");
    // Alex still has it.
    let (st, v) = send(&t, &t.alex, &sid_alex, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.big.bytes.lock().unwrap()[&id], zip(300));
}

/// The node refuses the prompt: the upload is given back, and the retry
/// pushes it again and goes through.
#[tokio::test]
async fn a_refused_prompt_gives_the_upload_back_and_a_retry_pushes_again() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(
        &t,
        &t.alex,
        "x.zip",
        &format!("&session={sid}"),
        zip(300_000),
    )
    .await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    *t.big.knobs.refuse_prompt.lock().unwrap() = Some("a turn is running".into());
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let first = t.big.chunks().len();
    assert!(first >= 2);
    *t.big.knobs.refuse_prompt.lock().unwrap() = None;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        t.big.chunks().len(),
        first * 2,
        "the retry pushes the file again"
    );
    assert_eq!(t.big.bytes.lock().unwrap()[&id], zip(300_000));
}

/// A chunk the node refuses (its disk is full) stops the send before any
/// prompt, with the node's words; the upload is kept.
#[tokio::test]
async fn a_chunk_the_node_refuses_stops_the_send() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(
        &t,
        &t.alex,
        "x.zip",
        &format!("&session={sid}"),
        zip(600_000),
    )
    .await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    *t.big.knobs.fail_chunk.lock().unwrap() = Some(1);
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("disk is full"), "{v}");
    assert!(
        t.big.prompts().is_empty(),
        "no prompt follows a failed push"
    );
    assert_eq!(t.big.chunks().len(), 2, "the push stops at the refusal");
    *t.big.knobs.fail_chunk.lock().unwrap() = None;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// The prompt's answer is lost: the node may have taken the file, so the
/// upload is not offered again (it could go twice); it expires.
#[tokio::test]
async fn a_lost_answer_does_not_give_the_upload_back() {
    let t = start().await;
    t.master
        .set_ack_timeout_for_test(Duration::from_millis(400));
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(&t, &t.alex, "x.zip", &format!("&session={sid}"), zip(1000)).await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    *t.big.knobs.silent_prompt.lock().unwrap() = true;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::GATEWAY_TIMEOUT, "{v}");
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("already used"), "{v}");
}

/// An upload never sent goes with the others unsent after the TTL, and
/// nothing can send it then.
#[tokio::test]
async fn an_abandoned_upload_is_purged() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(&t, &t.alex, "x.zip", &format!("&session={sid}"), zip(1000)).await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    t.master.purge_unsent(harness_core::now_ms() + 1000).await;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(t.big.chunks().is_empty());
}

/// A file attached to an answer, parallel answers or a retry is refused
/// before anything is taken.
#[tokio::test]
async fn an_answer_takes_no_files() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(&t, &t.alex, "x.zip", &format!("&session={sid}"), zip(1000)).await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "", "attachments": [id], "reply": {"message_id": "m", "block_id": "b", "values": {}}})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("cannot carry attachments"),
        "{v}"
    );
}

/// The count of unsent files an account may hold.
#[tokio::test]
async fn an_account_holds_a_bounded_number_of_unsent_files() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let q = format!("&session={sid}");
    let mut last = StatusCode::OK;
    for i in 0..25 {
        let (st, _) = upload(&t, &t.alex, &format!("f{i}.zip"), &q, zip(50 + i)).await;
        last = st;
        if st != StatusCode::OK {
            break;
        }
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    let _ = t.node.chunks.load(Ordering::SeqCst);
}

/// A sub-agent's chat takes no file: no composer there, and the API says
/// so too (a person could still call it).
#[tokio::test]
async fn a_sub_agents_chat_takes_no_files() {
    let t = start().await;
    let _parent = chat(&t, &t.alex, "big", "agentic").await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "child", "node_id": "big", "profile": "agentic"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    let (st, v) = upload(&t, &t.alex, "x.zip", &format!("&session={sid}"), zip(500)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    assert_eq!(blob_files(&t), 0);
}

/// A send the client gives up on (its request is dropped while the push is
/// still running) is settled all the same: the bookkeeping runs in a task of
/// its own. Here the node refuses the prompt after the client left, so the
/// upload must be given back, not stranded "already used" (reviewer's probe,
/// flipped).
#[tokio::test]
async fn a_dropped_send_does_not_strand_the_upload() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(
        &t,
        &t.alex,
        "x.zip",
        &format!("&session={sid}"),
        zip(1_000_000),
    )
    .await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    *t.big.knobs.slow_chunk_ms.lock().unwrap() = 300;
    *t.big.knobs.refuse_prompt.lock().unwrap() = Some("a turn is running".into());
    let gone = reqwest::Client::new()
        .post(format!("{}/api/sessions/{sid}/prompt", t.base))
        .login(&t.alex)
        .same_origin()
        .timeout(Duration::from_millis(500))
        .json(&json!({"text": "unpack it", "attachments": [id.clone()]}))
        .send()
        .await;
    assert!(gone.is_err(), "the client gave up while the push ran");
    // The settled send: all chunks pushed, the prompt refused, the row back.
    for _ in 0..100 {
        if !t.big.prompts().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(t.big.prompts().len(), 1, "the send ran to its end");
    tokio::time::sleep(Duration::from_millis(200)).await;
    *t.big.knobs.slow_chunk_ms.lock().unwrap() = 0;
    *t.big.knobs.refuse_prompt.lock().unwrap() = None;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::OK, "the upload was given back: {v}");
}

/// The other end of the same send: the node takes the prompt after the
/// client left, so the upload is done and cannot go twice.
#[tokio::test]
async fn a_dropped_send_the_node_took_is_finished() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let (_, v) = upload(
        &t,
        &t.alex,
        "x.zip",
        &format!("&session={sid}"),
        zip(1_000_000),
    )
    .await;
    let id = v["attachment_id"].as_str().unwrap().to_string();
    *t.big.knobs.slow_chunk_ms.lock().unwrap() = 300;
    let gone = reqwest::Client::new()
        .post(format!("{}/api/sessions/{sid}/prompt", t.base))
        .login(&t.alex)
        .same_origin()
        .timeout(Duration::from_millis(500))
        .json(&json!({"text": "unpack it", "attachments": [id.clone()]}))
        .send()
        .await;
    assert!(gone.is_err());
    for _ in 0..100 {
        if !t.big.prompts().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(t.big.prompts().len(), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    *t.big.knobs.slow_chunk_ms.lock().unwrap() = 0;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

/// A file uploaded for a coding chat and sent to a sub-agent's chat is
/// refused at send too, nothing pushed (reviewer's probe).
#[tokio::test]
async fn a_home_upload_sent_to_a_sub_agents_chat_is_refused() {
    let t = start().await;
    let (st, v) = upload(&t, &t.alex, "x.zip", "&agentic=1", zip(500)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let _parent = chat(&t, &t.alex, "big", "agentic").await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": "/w", "title": "child", "node_id": "big", "profile": "agentic"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let child = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &t.alex, &child, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("sub-agent"), "{v}");
    assert!(t.big.chunks().is_empty(), "nothing was pushed");
}

/// The master's own sentence for a file over the node's number at send: a
/// file made from the home composer (default 10 MB) sent to a chat whose
/// node takes 1 MB.
#[tokio::test]
async fn a_home_upload_over_the_nodes_number_is_refused_at_send() {
    let t = start().await;
    let (st, v) = upload(&t, &t.alex, "big.zip", "&agentic=1", zip(2 * 1024 * 1024)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let sid = chat(&t, &t.alex, "small", "agentic").await;
    let (st, v) = send(&t, &t.alex, &sid, &[&id]).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("up to 1 MB"), "{v}");
    assert!(t.node.chunks().is_empty());
}

/// One count for everything a message carries: text, saved files and
/// images together are at most 10 (refused before anything is taken).
#[tokio::test]
async fn a_message_carries_at_most_ten_files_and_images_together() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "big", "agentic").await;
    let q = format!("&session={sid}");
    let mut ids = vec![];
    for i in 0..10 {
        let (name, body) = if i % 2 == 0 {
            (format!("f{i}.zip"), zip(100 + i))
        } else {
            (format!("f{i}.txt"), format!("text {i}").into_bytes())
        };
        let (st, v) = upload(&t, &t.alex, &name, &q, body).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        ids.push(v["attachment_id"].as_str().unwrap().to_string());
    }
    // 6 attachments and 5 images: 11.
    let mut body = json!({"text": "all of it", "attachments": &ids[..6], "images": vec!["a"; 5]});
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(body.clone()),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("at most 10 files and images"),
        "{v}"
    );
    assert!(t.big.chunks().is_empty(), "nothing was taken or pushed");
    // 11 attachments, no image.
    body["attachments"] = json!([&ids[..], &ids[..1]].concat());
    body["images"] = json!([]);
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(body),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Exactly 10 (five files and five texts) go.
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let (st, v) = send(&t, &t.alex, &sid, &refs).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.big.prompts()[0].len(), 5);
}
