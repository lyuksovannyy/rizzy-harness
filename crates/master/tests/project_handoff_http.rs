//! Earlier chats in a project (docs/projects.md, "Earlier chats"): the
//! first turn of a new project chat reads what its owner's earlier chats in
//! that project did (their summaries, else their last replies), and only
//! the owner's own. Later turns, and chats outside a project, get nothing.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const SECTION: &str = "Earlier chats in this project";

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
    rec: Arc<Rec>,
    _dir: tempfile::TempDir,
}

/// The master's chat node, summarising every turn it can (the two newest
/// turns are kept whole, so a chat's third turn writes a summary).
async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
    });
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            rec.clone(),
            vec![],
            NativeConfig {
                summarize_after_tokens: Some(1),
                ..Default::default()
            },
        )),
        default_model: "mock".into(),
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
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    master.set_memory(Arc::new(SqliteMemory::open_in_memory().unwrap()));
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        rec,
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
    let r = rb.login(c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

/// The admin "alex" and a member "bob".
async fn users(t: &T) -> (String, String) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = common::signed_in(r).await;
    let pw = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        &alex,
    )
    .await
    .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "bob", "password": pw}))
        .send()
        .await
        .unwrap();
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .login(&common::signed_in(r).await)
        .same_origin()
        .json(&json!({"current": pw, "new": "bobs own password"}))
        .send()
        .await
        .unwrap();
    (alex, common::signed_in(r).await)
}

async fn new_project(t: &T, c: &str, body: Value) -> String {
    let (st, v) = req(t, Method::POST, "/api/projects", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["project_id"].as_str().unwrap().to_string()
}

/// A chat titled `title`, in `project` (and its workspace `ws`) if given.
async fn new_chat(t: &T, c: &str, title: &str, project: Option<&str>, ws: Option<&str>) -> String {
    let mut body = json!({"profile": "chat", "title": title, "project_id": project});
    if let Some(w) = ws {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn events(t: &T, c: &str, sid: &str) -> Vec<Value> {
    let (_, v) = req(
        t,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        c,
    )
    .await;
    v["events"].as_array().cloned().unwrap_or_default()
}

/// Send `text` and wait for that turn to end (in the master's mirror).
async fn say(t: &T, c: &str, sid: &str, text: &str) {
    let ended = |evs: &[Value]| evs.iter().filter(|e| e["kind"] == "turn_ended").count();
    let before = ended(&events(t, c, sid).await);
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..500 {
        if ended(&events(t, c, sid).await) > before {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("turn did not end");
}

/// The newest user message of the last request: where the per-turn
/// context goes.
fn last_context(t: &T) -> String {
    let reqs = t.rec.reqs.lock().unwrap();
    reqs.last()
        .unwrap()
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMsg::User(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn at(ctx: &str, needle: &str) -> usize {
    ctx.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} missing from {ctx}"))
}

#[tokio::test]
async fn a_new_project_chat_starts_with_the_owners_earlier_chats() {
    let t = start().await;
    let (alex, _) = users(&t).await;
    let pid = new_project(&t, &alex, json!({"name": "Harness"})).await;
    // Turns until one summarises the older ones (the two newest stay
    // whole); its last reply then is not what the section gives.
    let a = new_chat(&t, &alex, "Alpha work", Some(&pid), None).await;
    let mut summarised = false;
    for i in 1..=6 {
        say(&t, &alex, &a, &format!("alpha turn {i}")).await;
        summarised = events(&t, &alex, &a)
            .await
            .iter()
            .any(|e| e["kind"] == "history_summarized");
        if summarised {
            break;
        }
    }
    assert!(summarised, "no summary was written");
    tokio::time::sleep(Duration::from_millis(5)).await;
    let b = new_chat(&t, &alex, "Bravo work", Some(&pid), None).await;
    say(&t, &alex, &b, "bravo plan").await;
    let x = new_chat(&t, &alex, "Xray elsewhere", None, None).await;
    say(&t, &alex, &x, "xray notes").await;

    let c = new_chat(&t, &alex, "Charlie", Some(&pid), None).await;
    say(&t, &alex, &c, "carry on").await;
    let ctx = last_context(&t);
    assert!(ctx.contains(SECTION), "{ctx}");
    assert!(ctx.contains("ask the person before relying"), "{ctx}");
    // Newest first; a summary where there is one, else the last reply.
    assert!(
        at(&ctx, "\"Bravo work\"") < at(&ctx, "\"Alpha work\""),
        "{ctx}"
    );
    assert!(ctx.contains("You said: bravo plan"), "{ctx}");
    assert!(ctx.contains("Summary of"), "{ctx}");
    assert!(
        !ctx.contains("You said: alpha turn"),
        "the summary, not the reply: {ctx}"
    );
    // Not a chat outside the project, nor this one.
    assert!(!ctx.contains("Xray"), "{ctx}");
    assert!(!ctx.contains("\"Charlie\""), "{ctx}");

    // The second turn of the chat does not carry it again.
    say(&t, &alex, &c, "and then").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("Project \"Harness\""), "{ctx}");
    assert!(!ctx.contains(SECTION), "{ctx}");
}

#[tokio::test]
async fn another_members_chats_in_a_team_project_never_appear() {
    let t = start().await;
    let (alex, bob) = users(&t).await;
    let (st, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pid = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let mine = new_chat(&t, &alex, "Alex plan", Some(&pid), Some(&acme)).await;
    say(&t, &alex, &mine, "alex roadmap").await;
    let theirs = new_chat(&t, &bob, "Bob plan", Some(&pid), Some(&acme)).await;
    say(&t, &bob, &theirs, "bob salary").await;

    let next = new_chat(&t, &alex, "Alex next", Some(&pid), Some(&acme)).await;
    say(&t, &alex, &next, "carry on").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("alex roadmap"), "{ctx}");
    assert!(!ctx.contains("bob salary"), "{ctx}");
    assert!(!ctx.contains("Bob plan"), "{ctx}");

    let next = new_chat(&t, &bob, "Bob next", Some(&pid), Some(&acme)).await;
    say(&t, &bob, &next, "carry on").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("bob salary"), "{ctx}");
    assert!(!ctx.contains("alex roadmap"), "{ctx}");
}

#[tokio::test]
async fn a_chat_without_a_project_gets_no_earlier_chats() {
    let t = start().await;
    let (alex, _) = users(&t).await;
    let pid = new_project(&t, &alex, json!({"name": "Harness"})).await;
    let a = new_chat(&t, &alex, "Alpha work", Some(&pid), None).await;
    say(&t, &alex, &a, "alpha one").await;
    let b = new_chat(&t, &alex, "Loose", None, None).await;
    say(&t, &alex, &b, "carry on").await;
    let ctx = last_context(&t);
    assert!(!ctx.contains(SECTION), "{ctx}");
    assert!(!ctx.contains("alpha one"), "{ctx}");
}
