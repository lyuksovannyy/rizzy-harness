//! Images in chats end to end (docs/media.md): the composer uploads to
//! Media, the prompt names the items, the master checks and decrypts them,
//! the node keeps the bytes with the chat and the model gets them as binary
//! parts. Refused where they cannot go: incognito chats, models that cannot
//! see, nodes that cannot take them, too much in one message (the link's
//! 8 MiB frame is never reached). A Media item reports the chats that still
//! carry it.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret};
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, ProviderConfig};
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

/// Records every request, answers as the mock does.
struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.reqs.lock().unwrap().push(req.clone());
        Mock {
            delay: Duration::from_millis(0),
        }
        .complete(req, d)
        .await
    }
}

struct T {
    base: String,
    master: Master,
    rec: Arc<Rec>,
    /// The chat node's own database (its `session_images`).
    node_store: Store,
    alex: String,
    _node: NodeRuntime,
    _dir: tempfile::TempDir,
}

/// A chat node that sees nothing of images: it announces no `images`
/// capability, opens chats and acks, and keeps what it was sent.
fn old_node(id: &str) -> (NodeLink, Arc<Mutex<Vec<MasterToNode>>>) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "old".into(),
        version: "0".into(),
        capabilities: vec!["native".into(), "profile:chat".into()],
        sessions: vec![],
    })
    .unwrap();
    let seen: Arc<Mutex<Vec<MasterToNode>>> = Arc::default();
    let (node, keep) = (id.to_string(), seen.clone());
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            keep.lock().unwrap().push(msg.clone());
            if let MasterToNode::Create { session_id, .. } = &msg {
                let (kind, payload) = EventKind::SessionCreated(SessionCreated {
                    exec_mode: None,
                    no_callbacks: false,
                    title: "t".into(),
                    cwd: String::new(),
                    model: "demo::mock".into(),
                    backend: "native".into(),
                    node_id: node.clone(),
                    approval_mode: ApprovalMode::Ask,
                    profile: Profile::Chat,
                    agent_id: None,
                    incognito: false,
                    parent_session: None,
                })
                .into_parts();
                let event = RawEvent {
                    session_id: *session_id,
                    seq: 1,
                    ts_ms: harness_core::now_ms(),
                    kind,
                    payload,
                };
                let _ = tx.try_send(NodeToMaster::Event { event });
            }
            if let Some(req_id) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: req_id.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (NodeLink { to_node, from_node }, seen)
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
    });
    let node_store = Store::open_in_memory().unwrap();
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: node_store.clone(),
        backend: Arc::new(NativeBackend::new(
            rec.clone(),
            vec![],
            NativeConfig::default(),
        )),
        default_model: "demo::mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    // Media is on by default; one model sees images, one does not.
    // The default config already lists the demo provider: change it in place.
    let mut cfg = Config::default();
    let mut demo = ProviderConfig::demo(0);
    demo.models[0].vision = Some(true);
    demo.models.push(harness_master::config::ModelConfig {
        id: "blind".into(),
        ..Default::default()
    });
    cfg.providers = vec![demo];
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    master.set_provider_runtime(Arc::new(rt));
    harness_master::storage::open(&master, dir.path()).unwrap();
    master.add_link(node.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = cookie(r).await;
    T {
        base,
        master,
        rec,
        node_store,
        alex,
        _node: node,
        _dir: dir,
    }
}

/// The cookie a login answer set; its login key is recorded (P-1).
async fn cookie(r: reqwest::Response) -> String {
    common::signed_in(r).await
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
    let r = rb.login(c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// A member account, signed in with its own password.
async fn account(t: &T, name: &str) -> String {
    let body = json!({"username": name});
    let (st, v) = req(t, Method::POST, "/api/users", Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let client = reqwest::Client::new();
    let first = client
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": name, "password": pw}))
        .send()
        .await
        .unwrap();
    let r = client
        .post(format!("{}/api/me/password", t.base))
        .login(&cookie(first).await)
        .same_origin()
        .json(&json!({"current": pw, "new": format!("{name}s own password")}))
        .send()
        .await
        .unwrap();
    cookie(r).await
}

/// A PNG header of `w`×`h` and then `tail` (the master reads headers only).
fn png(w: u32, h: u32, tail: &[u8]) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(tail);
    b
}

/// An image `n` bytes long, different for every `seed`.
fn big_png(seed: u8, n: usize) -> Vec<u8> {
    let tail: Vec<u8> = (0..n.saturating_sub(33))
        .map(|i| (i as u8).wrapping_mul(31) ^ seed)
        .collect();
    png(64, 48, &tail)
}

/// Upload from a chat's composer (`session`: the chat, when it exists).
async fn upload(
    t: &T,
    c: &str,
    name: &str,
    bytes: Vec<u8>,
    session: Option<&str>,
) -> (StatusCode, Value) {
    let mut url = format!("{}/api/media?origin=chat", t.base);
    if let Some(s) = session {
        url.push_str(&format!("&session={s}"));
    }
    let r = reqwest::Client::new()
        .post(url)
        .login(c)
        .same_origin()
        .header("x-file-name", name)
        .body(bytes)
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn image(t: &T, name: &str, bytes: Vec<u8>, session: Option<&str>) -> String {
    let (st, v) = upload(t, &t.alex, name, bytes, session).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["origin"], "chat");
    v["media_id"].as_str().unwrap().to_string()
}

async fn chat(t: &T, extra: Value) -> String {
    let mut body = json!({"profile": "chat"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn prompt(t: &T, sid: &str, body: Value) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(body),
        &t.alex,
    )
    .await
}

async fn events(t: &T, sid: &str) -> Vec<Value> {
    let (_, v) = req(
        t,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        &t.alex,
    )
    .await;
    v["events"].as_array().cloned().unwrap_or_default()
}

/// The chat's events once `n` turns ended.
async fn turns(t: &T, sid: &str, n: usize) -> Vec<Value> {
    for _ in 0..400 {
        let evs = events(t, sid).await;
        if evs.iter().filter(|e| e["kind"] == "turn_ended").count() >= n {
            return evs;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{n} turns never ended in {sid}");
}

/// Send `text` with `images` and wait for the turn.
async fn send(t: &T, sid: &str, text: &str, images: &[&str], turn: usize) -> Vec<Value> {
    let (st, v) = prompt(t, sid, json!({"text": text, "images": images})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    turns(t, sid, turn).await
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// The last request's user messages, as (text, images with bytes).
fn last_users(t: &T) -> Vec<(String, Vec<String>)> {
    let reqs = t.rec.reqs.lock().unwrap();
    reqs.last()
        .unwrap()
        .messages
        .iter()
        .filter_map(|m| match m {
            ChatMsg::User(u) => Some((u.clone(), vec![])),
            ChatMsg::UserImages { text, images } => Some((
                text.clone(),
                images
                    .iter()
                    .map(|i| i.data.as_deref().unwrap_or("").to_string())
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

async fn item(t: &T, id: &str) -> Value {
    let (st, v) = req(t, Method::GET, &format!("/api/media/{id}"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

/// How many of `images` the chat node keeps for chat `sid`.
async fn kept(t: &T, sid: &str, images: &[&[u8]]) -> usize {
    let sid = sid.to_string();
    let shas: Vec<String> = images
        .iter()
        .map(|b| hex::encode(Sha256::digest(b)))
        .collect();
    t.node_store
        .call(move |s| s.session_images(&sid, &shas))
        .await
        .unwrap()
        .len()
}

async fn wait_until<F, Fut>(f: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("never happened");
}

#[tokio::test]
async fn the_model_gets_the_images_as_binary_parts() {
    let t = start().await;
    let (_, f) = req(&t, Method::GET, "/api/features", None, &t.alex).await;
    assert_eq!(f["media"]["vision_images"], true, "{f}");
    let sid = chat(&t, json!({})).await;
    let (a, b) = (png(1280, 720, b"first"), png(10, 20, b"second"));
    let ia = image(&t, "a.png", a.clone(), Some(&sid)).await;
    let ib = image(&t, "b.png", b.clone(), None).await;
    let evs = send(&t, &sid, "what are these?", &[&ia, &ib], 1).await;

    // The provider saw both images' bytes on the user message.
    let users = last_users(&t);
    let (text, data) = users.last().unwrap();
    assert!(text.ends_with("what are these?"), "{text}");
    assert_eq!(data, &vec![b64(&a), b64(&b)]);
    // The log names them (no bytes), with their header sizes.
    let msg = evs.iter().find(|e| e["kind"] == "user_message").unwrap();
    let refs = &msg["payload"]["images"];
    assert_eq!(refs[0]["media_id"], ia.as_str());
    assert_eq!(refs[0]["sha256"], hex::encode(Sha256::digest(&a)));
    assert_eq!(
        (refs[0]["width"].clone(), refs[0]["height"].clone()),
        (json!(1280), json!(720))
    );
    assert_eq!(refs[1]["name"], "b.png");
    assert!(refs[0].get("data").is_none());
    let answer = evs
        .iter()
        .find(|e| e["kind"] == "assistant_message")
        .unwrap();
    assert_eq!(answer["payload"]["text"], "I see 2 image(s).");
    // The chat carries both; the one uploaded before the chat existed took
    // it as its source.
    for id in [&ia, &ib] {
        let v = item(&t, id).await;
        assert_eq!(v["chats"], 1, "{v}");
        assert_eq!(v["source"]["session_id"], sid.as_str(), "{v}");
    }
    assert_eq!(kept(&t, &sid, &[&a, &b]).await, 2);

    // Later turns show only the newest three image-bearing messages again.
    for n in 2..=4u8 {
        let id = image(&t, &format!("p{n}.png"), png(8, 8, &[n; 9]), Some(&sid)).await;
        send(&t, &sid, &format!("and {n}?"), &[&id], n as usize).await;
    }
    send(&t, &sid, "thanks", &[], 5).await;
    let users = last_users(&t);
    assert_eq!(users.len(), 5);
    assert!(users[0].1.is_empty(), "{:?}", users[0].0);
    assert!(
        users[0].0.contains("[image: a.png] [image: b.png]"),
        "{}",
        users[0].0
    );
    assert!(users[1..4].iter().all(|(_, d)| d.len() == 1));
    // The newest message (after its per-turn context) has no images.
    assert!(
        users[4].0.ends_with("thanks") && users[4].1.is_empty(),
        "{:?}",
        users[4]
    );
    let answer = turns(&t, &sid, 5).await;
    let last = answer
        .iter()
        .rev()
        .find(|e| e["kind"] == "assistant_message")
        .unwrap();
    // Without images the mock echoes as usual.
    assert_eq!(last["payload"]["text"], "You said: thanks");
}

#[tokio::test]
async fn images_are_refused_where_they_cannot_go() {
    let t = start().await;
    let sid = chat(&t, json!({})).await;
    let img = image(&t, "a.png", png(4, 4, b"x"), Some(&sid)).await;

    // Incognito: neither the upload nor the prompt.
    let secret = chat(&t, json!({"incognito": true})).await;
    let (st, v) = upload(&t, &t.alex, "c.png", png(4, 4, b"c"), Some(&secret)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, v) = prompt(&t, &secret, json!({"text": "hi", "images": [img]})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");

    // A model that cannot see them: the chat's own (422), or one an
    // answer is compared with.
    let blind = chat(&t, json!({"model": "demo::blind"})).await;
    let (st, v) = prompt(&t, &blind, json!({"text": "hi", "images": [img]})).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("demo::blind"), "{v}");
    let (st, v) = prompt(
        &t,
        &sid,
        json!({"text": "hi", "images": [img], "compare": ["demo::mock", "demo::blind"]}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");

    // Too many, the same twice, too large together.
    let five: Vec<String> = {
        let mut v = Vec::new();
        for n in 0..5u8 {
            v.push(image(&t, &format!("n{n}.png"), png(4, 4, &[n; 3]), Some(&sid)).await);
        }
        v
    };
    let (st, v) = prompt(&t, &sid, json!({"text": "hi", "images": five})).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    let (st, v) = prompt(&t, &sid, json!({"text": "hi", "images": [img, img]})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let huge = [
        image(&t, "h1.png", big_png(1, 2_700_000), Some(&sid)).await,
        image(&t, "h2.png", big_png(2, 2_700_000), Some(&sid)).await,
    ];
    let (st, v) = prompt(&t, &sid, json!({"text": "hi", "images": huge})).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");

    // Not an image, or not one's own: never sent.
    let wav = [b"RIFF\x24\0\0\0WAVEfmt ".as_slice(), &[0u8; 64]].concat();
    let (st, v) = upload(&t, &t.alex, "a.wav", wav, Some(&sid)).await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{v}");
    let bob = account(&t, "bob").await;
    let (st, v) = upload(&t, &bob, "own.png", png(2, 2, b"bob"), None).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let bobs = v["media_id"].as_str().unwrap().to_string();
    let (st, v) = prompt(&t, &sid, json!({"text": "hi", "images": [bobs]})).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    let (st, v) = upload(&t, &bob, "x.png", png(2, 2, b"x"), Some(&sid)).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");

    // Nothing was sent, so nothing is carried.
    assert!(
        events(&t, &sid)
            .await
            .iter()
            .all(|e| e["kind"] != "user_message")
    );
    assert_eq!(item(&t, &img).await["chats"], 0);
    assert!(t.rec.reqs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_node_without_the_capability_gets_no_images() {
    let t = start().await;
    let (link, seen) = old_node("old");
    // An older build of the master's own chat runtime (always embedded).
    t.master.add_link(link, true).await.unwrap();
    let sid = chat(&t, json!({"node_id": "old"})).await;
    let img = image(&t, "a.png", png(4, 4, b"x"), Some(&sid)).await;
    let (st, v) = prompt(&t, &sid, json!({"text": "hi", "images": [img]})).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("update it"), "{v}");
    // Text still goes; images never did.
    let (st, v) = prompt(&t, &sid, json!({"text": "hi"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let seen = seen.lock().unwrap();
    let prompts: Vec<&MasterToNode> = seen
        .iter()
        .filter(|m| matches!(m, MasterToNode::Prompt { .. }))
        .collect();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts
            .iter()
            .all(|m| matches!(m, MasterToNode::Prompt { images, .. } if images.is_empty()))
    );
}

#[tokio::test]
async fn a_message_over_7_mib_is_refused_before_the_link() {
    let t = start().await;
    let sid = chat(&t, json!({})).await;
    // Four images just under 5 MiB together: base64 makes them 6.7 MiB.
    let mut ids = Vec::new();
    for n in 0..4u8 {
        ids.push(
            image(
                &t,
                &format!("big{n}.png"),
                big_png(n, 1_310_000),
                Some(&sid),
            )
            .await,
        );
    }
    // Five attachments of 100,000 characters take it past 7 MiB.
    let mut attachments = Vec::new();
    for n in 0..5 {
        let r = reqwest::Client::new()
            .post(format!("{}/api/attachments?name=n{n}.txt", t.base))
            .login(&t.alex)
            .same_origin()
            .body("x".repeat(100_000))
            .send()
            .await
            .unwrap();
        let v: Value = r.json().await.unwrap();
        attachments.push(v["attachment_id"].as_str().unwrap().to_string());
    }
    let (st, v) = prompt(
        &t,
        &sid,
        json!({"text": "all of it", "images": ids, "attachments": attachments}),
    )
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert_eq!(v["error"], "Too much attached to one message");
    assert!(
        events(&t, &sid)
            .await
            .iter()
            .all(|e| e["kind"] != "user_message")
    );
    assert_eq!(item(&t, &ids[0]).await["chats"], 0);
    // The images alone fit, and arrive whole.
    let evs = send(
        &t,
        &sid,
        "just the pictures",
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
        1,
    )
    .await;
    let answer = evs
        .iter()
        .find(|e| e["kind"] == "assistant_message")
        .unwrap();
    assert_eq!(answer["payload"]["text"], "I see 4 image(s).");
    let users = last_users(&t);
    assert_eq!(users.last().unwrap().1.len(), 4);
}

#[tokio::test]
async fn media_items_report_the_chats_that_carry_them() {
    let t = start().await;
    let (a, b) = (chat(&t, json!({})).await, chat(&t, json!({})).await);
    let (shared_png, b_png) = (png(4, 4, b"shared"), png(4, 4, b"b"));
    let shared = image(&t, "shared.png", shared_png.clone(), Some(&a)).await;
    let only_b = image(&t, "b.png", b_png.clone(), None).await;
    send(&t, &a, "one", &[&shared], 1).await;
    send(&t, &b, "two", &[&shared, &only_b], 1).await;
    // Sending the same image again does not count twice.
    send(&t, &b, "again", &[&shared], 2).await;
    assert_eq!(item(&t, &shared).await["chats"], 2);
    assert_eq!(item(&t, &only_b).await["chats"], 1);
    let (_, from_b) = req(
        &t,
        Method::GET,
        &format!("/api/media?session={b}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(from_b["items"].as_array().unwrap().len(), 1, "{from_b}");
    assert_eq!(from_b["items"][0]["media_id"], only_b.as_str());
    assert_eq!(kept(&t, &b, &[&shared_png, &b_png]).await, 2);

    // A deleted chat no longer carries it, and its copies are gone.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{a}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    wait_until(|| async { item(&t, &shared).await["chats"] == 1 }).await;
    assert_eq!(item(&t, &shared).await["source"], "deleted");
    wait_until(|| async { kept(&t, &a, &[&shared_png]).await == 0 }).await;
    assert_eq!(kept(&t, &b, &[&shared_png, &b_png]).await, 2);

    // `?media=1` also deletes the items that came from the chat; the one
    // that came from elsewhere stays, carried by nothing.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{b}?media=1"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/media/{only_b}"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    wait_until(|| async { item(&t, &shared).await["chats"] == 0 }).await;
    wait_until(|| async { kept(&t, &b, &[&shared_png, &b_png]).await == 0 }).await;
}
