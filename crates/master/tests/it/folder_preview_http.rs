//! Previewing a Word or Excel file of a coding chat's folder (PLAN.md T99,
//! ADOPTION_PLAN X.59): `GET /api/sessions/{id}/folder-preview?path=&sheet=`.
//! The master reads nothing: it asks the owning node (a fake here; the real
//! node's rules are in `crates/node/tests/folder_preview.rs`, the real helper
//! in `crates/cli/tests/doc_edit.rs`), owner only, one at a time.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::policy::ApprovalMode;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_DOC_PREVIEW, MasterToNode, NodeToMaster, PROTOCOL_VERSION, PreviewRefusal,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};

const TOKEN: &str = "api-secret";

struct Fake {
    seen: Arc<Mutex<Vec<MasterToNode>>>,
    /// Answers to `slow.docx` wait for `true`.
    release: watch::Sender<bool>,
    tx: mpsc::Sender<NodeToMaster>,
    _leave: mpsc::UnboundedSender<()>,
}

impl Fake {
    /// The `req_id` of every `session.doc_preview` this node was sent.
    fn req_ids(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::DocPreview { req_id, .. } => Some(req_id.clone()),
                _ => None,
            })
            .collect()
    }

    /// This node says something unasked.
    fn say(&self, m: NodeToMaster) {
        self.tx.try_send(m).unwrap();
    }

    /// The node's link closes.
    fn go_away(&self) {
        let _ = self._leave.send(());
    }

    /// `(path, sheet)` of every `session.doc_preview` this node was sent.
    fn asked(&self) -> Vec<(String, Option<u32>)> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|m| match m {
                MasterToNode::DocPreview { path, sheet, .. } => Some((path.clone(), *sheet)),
                _ => None,
            })
            .collect()
    }
}

fn doc_json() -> Value {
    json!({"kind": "doc", "blocks": [{"k": "p", "r": [{"t": "Hello"}]}], "more": false, "notes": []})
}

/// What the fake node answers to a path: `ok.docx` a document,
/// `gone.docx` a refusal, `odd.docx` a value that is no preview,
/// `huge.docx` one far past the bound, `slow.docx` waits.
async fn answer(
    path: &str,
    mut rel: watch::Receiver<bool>,
) -> (Option<Value>, Option<PreviewRefusal>) {
    match path.rsplit('/').next().unwrap_or(path) {
        "ok.docx" => (Some(doc_json()), None),
        "odd.docx" => (Some(json!({"kind": "nope"})), None),
        "huge.docx" => {
            let mut v = doc_json();
            v["notes"] = json!(["x".repeat(600 * 1024)]);
            (Some(v), None)
        }
        "slow.docx" => {
            let _ = rel.wait_for(|r| *r).await;
            (Some(doc_json()), None)
        }
        "big.docx" => (None, Some(PreviewRefusal::FileTooLarge)),
        "toolarge.docx" => (None, Some(PreviewRefusal::TooLarge)),
        "pass.docx" => (None, Some(PreviewRefusal::Protected)),
        "hurt.docx" => (None, Some(PreviewRefusal::Damaged)),
        "full.docx" => (None, Some(PreviewRefusal::Busy)),
        _ => (None, Some(PreviewRefusal::NotFound)),
    }
}

fn fake(id: &str, caps: &[&str], profile: Profile) -> (NodeLink, Fake) {
    fake_with(id, caps, profile, false)
}

/// `incognito`: the chats this node opens say they are temporary.
fn fake_with(id: &str, caps: &[&str], profile: Profile, incognito: bool) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (tx, from_node) = mpsc::channel::<NodeToMaster>(1024);
    let mut capabilities = vec![
        "native".to_string(),
        "profile:agentic".to_string(),
        "exec:sandbox".to_string(),
    ];
    capabilities.extend(caps.iter().map(|c| c.to_string()));
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
    let (leave, mut left) = mpsc::unbounded_channel::<()>();
    let (release, rel) = watch::channel(false);
    let (node, keep) = (id.to_string(), seen.clone());
    let sender = tx.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = commands.recv() => {
                    let Some(msg) = msg else { return };
                    keep.lock().unwrap().push(msg.clone());
                    match &msg {
                        MasterToNode::Create { session_id, exec_mode, .. } => {
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
                                profile,
                                agent_id: None,
                                incognito,
                                parent_session: None,
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
                        MasterToNode::DocPreview { req_id, path, .. } => {
                            let (req_id, path, rel, tx) =
                                (req_id.clone(), path.clone(), rel.clone(), tx.clone());
                            tokio::spawn(async move {
                                let (preview, refusal) = answer(&path, rel).await;
                                let _ = tx
                                    .send(NodeToMaster::DocPreviewResult {
                                        req_id,
                                        preview,
                                        refusal,
                                    })
                                    .await;
                            });
                        }
                        _ => {}
                    }
                    if let Some(req_id) = msg.req_id() {
                        let _ = tx.try_send(NodeToMaster::Ack { req_id: req_id.into(), ok: true, error: None });
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
            release,
            tx: sender,
            _leave: leave,
        },
    )
}

struct T {
    base: String,
    master: Master,
    alex: String,
    local: Fake,
    old: Fake,
    plain: Fake,
    remote: Fake,
    inc: Fake,
    _dir: tempfile::TempDir,
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
    let (l, local) = fake("local", &[CAP_DOC_PREVIEW], Profile::Agentic);
    master.add_link(l, true).await.unwrap();
    let (l, old) = fake("old", &[], Profile::Agentic);
    master.add_link(l, false).await.unwrap();
    let (l, plain) = fake("plain", &[CAP_DOC_PREVIEW], Profile::Chat);
    master.add_link(l, false).await.unwrap();
    // A member's own coding chats are on a remote node, not the server's.
    let (l, remote) = fake("remote", &[CAP_DOC_PREVIEW, "exec:host"], Profile::Agentic);
    master.add_link(l, false).await.unwrap();
    // A node whose coding chats say they are temporary.
    let (l, inc) = fake_with("inc", &[CAP_DOC_PREVIEW], Profile::Agentic, true);
    master.add_link(l, false).await.unwrap();
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
        local,
        old,
        plain,
        remote,
        inc,
        _dir: dir,
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
    let cache = r
        .headers()
        .get("cache-control")
        .map(|v| v.to_str().unwrap().to_string());
    let v = r.json().await.unwrap_or(Value::Null);
    if st == StatusCode::OK {
        assert_eq!(cache.as_deref(), Some("no-store"));
    }
    (st, v)
}

async fn account(t: &T, name: &str, role: &str) -> String {
    account_with_id(t, name, role).await.0
}

/// The account's cookie and its user id.
async fn account_with_id(t: &T, name: &str, role: &str) -> (String, String) {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "role": role, "exec_ceiling": "host"})),
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
    (
        common::signed_in(r).await,
        v["user_id"].as_str().unwrap().to_string(),
    )
}

async fn chat(t: &T, who: &str, node: &str, extra: Value) -> SessionId {
    let mut body = json!({"cwd": "/w", "title": "t", "node_id": node});
    if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
        b.extend(e.clone());
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

fn url(sid: SessionId, query: &str) -> String {
    format!("/api/sessions/{sid}/folder-preview{query}")
}

#[tokio::test]
async fn the_owner_previews_a_folder_file_through_the_node() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (st, v) = req(
        &t,
        Method::GET,
        &url(sid, "?path=docs/ok.docx"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The shape of the preview of a chat without a folder.
    assert_eq!(v["preview"], doc_json());
    assert_eq!(v["name"], "ok.docx");
    let (st, _) = req(
        &t,
        Method::GET,
        &url(sid, "?path=ok.docx&sheet=2"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.local.asked(),
        [
            ("docs/ok.docx".to_string(), None),
            ("ok.docx".to_string(), Some(2))
        ]
    );
    // The API token reads any chat, as it can everywhere else.
    let (st, _) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, TOKEN).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn nobody_else_previews_not_even_an_admin_and_the_node_is_never_asked() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for (who, name) in [(&bob, "a member"), (&carol, "a server admin")] {
        let (st, v) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    let (st, _) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, "nope").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(t.local.asked().is_empty());
}

#[tokio::test]
async fn a_plain_chat_and_a_bad_path_are_404_and_ask_nothing() {
    let t = start().await;
    let plain = chat(&t, &t.alex, "plain", json!({"profile": "chat"})).await;
    let (st, _) = req(&t, Method::GET, &url(plain, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for q in ["", "?path=", &format!("?path={}", "a".repeat(1025))] {
        let (st, _) = req(&t, Method::GET, &url(sid, q), None, &t.alex).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{q:.30}");
    }
    // Not a number: refused by the query's own type.
    let (st, _) = req(
        &t,
        Method::GET,
        &url(sid, "?path=ok.docx&sheet=x"),
        None,
        &t.alex,
    )
    .await;
    assert!(st.is_client_error());
    assert!(t.local.asked().is_empty() && t.plain.asked().is_empty());
}

/// The node's refusals are the sentences of the preview of a chat without a
/// folder; a path it will not show is the same 404 as any other missing one.
#[tokio::test]
async fn each_refusal_has_the_sentence_of_the_other_route() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for (path, code, has) in [
        ("../etc/x.docx", 404, "unknown file"),
        ("gone.docx", 404, "unknown file"),
        // A folder file has no download: Word or Excel, never "download".
        (
            "big.docx",
            413,
            "too large to preview; open it in Word or Excel",
        ),
        (
            "toolarge.docx",
            422,
            "too large to preview; open it in Word or Excel",
        ),
        ("pass.docx", 422, "protected with a password"),
        ("hurt.docx", 422, "damaged or signed"),
        ("full.docx", 503, "busy"),
    ] {
        let (st, v) = req(
            &t,
            Method::GET,
            &url(sid, &format!("?path={path}")),
            None,
            &t.alex,
        )
        .await;
        assert_eq!(st.as_u16(), code, "{path}: {v}");
        assert!(v["error"].as_str().unwrap().contains(has), "{path}: {v}");
        if path.ends_with("large.docx") || path == "big.docx" {
            assert!(
                !v["error"].as_str().unwrap().contains("download"),
                "{path}: {v}"
            );
        }
    }
    // A node that sends what is no preview, or too much of it, is not
    // forwarded.
    for path in ["odd.docx", "huge.docx"] {
        let (st, v) = req(
            &t,
            Method::GET,
            &url(sid, &format!("?path={path}")),
            None,
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{path}: {v}");
    }
}

#[tokio::test]
async fn an_old_node_and_an_away_node_get_a_sentence_not_a_hang() {
    let t = start().await;
    let old = chat(&t, &t.alex, "old", json!({})).await;
    let (st, v) = req(&t, Method::GET, &url(old, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("cannot preview files; update"),
        "{v}"
    );
    assert!(t.old.asked().is_empty());
    // Away: the sentence the `@` completion gives.
    let away = chat(&t, &t.alex, "local", json!({})).await;
    t.local.go_away();
    for _ in 0..100 {
        if !t.master.is_connected("local") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (st, v) = req(&t, Method::GET, &url(away, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("is away"), "{v}");
}

/// One account's previews run one at a time: a second is told to wait while
/// the first is with the node, so a person cannot fill the node's helper
/// places the agent's own calls use; another account is not held up.
#[tokio::test]
async fn one_account_previews_one_file_at_a_time() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let bsid = chat(&t, &bob, "remote", json!({})).await;
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let slow = tokio::spawn(async move {
        reqwest::Client::new()
            .get(format!(
                "{base}/api/sessions/{sid}/folder-preview?path=slow.docx"
            ))
            .login(&alex)
            .same_origin()
            .send()
            .await
            .unwrap()
            .status()
    });
    for _ in 0..100 {
        if !t.local.asked().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (st, v) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    let (st, v) = req(&t, Method::GET, &url(bsid, "?path=ok.docx"), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.remote.asked().len(), 1, "bob's chat is on his own node");
    t.local.release.send(true).unwrap();
    assert_eq!(slow.await.unwrap(), StatusCode::OK);
    let (st, _) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::OK);
}

/// A coding chat that says it is temporary is not previewed, and its node is
/// never asked.
#[tokio::test]
async fn a_temporary_coding_chat_is_404_and_asks_nothing() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "inc", json!({})).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert!(t.inc.asked().is_empty());
}

/// An owner whose level no longer allows agentic chats gets what the `@`
/// completion gives, and the node is never asked.
#[tokio::test]
async fn an_owner_who_lost_the_level_is_refused_and_the_node_is_never_asked() {
    let t = start().await;
    let (bob, bob_id) = account_with_id(&t, "bob", "member").await;
    // A chat on the host, while the account's level allows the host.
    let sid = chat(&t, &bob, "remote", json!({"exec_mode": "host"})).await;
    let (st, v) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/users/{bob_id}/exec-ceiling"),
        Some(json!({"level": "sandbox"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(t.remote.asked().len(), 1, "only the first was asked");
}

/// An answer from a node that was not asked is ignored; the asked node's own
/// answer is the one the page gets.
#[tokio::test]
async fn another_nodes_answer_to_a_request_is_ignored() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let slow = tokio::spawn(async move {
        let r = reqwest::Client::new()
            .get(format!(
                "{base}/api/sessions/{sid}/folder-preview?path=slow.docx"
            ))
            .login(&alex)
            .same_origin()
            .send()
            .await
            .unwrap();
        (r.status(), r.text().await.unwrap())
    });
    for _ in 0..100 {
        if !t.local.req_ids().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let forged = json!({"kind": "doc", "blocks": [{"k": "p", "r": [{"t": "FORGED"}]}], "more": false, "notes": []});
    t.old.say(NodeToMaster::DocPreviewResult {
        req_id: t.local.req_ids()[0].clone(),
        preview: Some(forged),
        refusal: None,
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!slow.is_finished(), "the forged answer ended the request");
    t.local.release.send(true).unwrap();
    let (st, body) = slow.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("Hello") && !body.contains("FORGED"), "{body}");
}

/// A person who closes the page while the node is still reading gives the
/// account's place back at once.
#[tokio::test]
async fn a_request_the_page_gave_up_frees_the_accounts_place() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let slow = tokio::spawn(async move {
        reqwest::Client::new()
            .get(format!(
                "{base}/api/sessions/{sid}/folder-preview?path=slow.docx"
            ))
            .login(&alex)
            .same_origin()
            .send()
            .await
    });
    for _ in 0..100 {
        if !t.local.asked().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (st, _) = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "the place is taken");
    slow.abort();
    let _ = slow.await;
    let mut got = StatusCode::SERVICE_UNAVAILABLE;
    for _ in 0..60 {
        got = req(&t, Method::GET, &url(sid, "?path=ok.docx"), None, &t.alex)
            .await
            .0;
        if got == StatusCode::OK {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(got, StatusCode::OK);
}
