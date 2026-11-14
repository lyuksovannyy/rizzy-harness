//! Project memory in coding chats (docs/projects.md, "Recall in a project
//! chat"): an agentic chat on a node proposes project facts with
//! `remember`, every turn of a project chat gets all of the project's
//! active facts (newest first, up to a cap; recall for the rest), and the
//! hint about `remember` appears only where the node offers the tool.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

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

/// How the node offers `remember`.
#[derive(Clone, Copy, Debug)]
enum Offer {
    /// Registered, as `rizzy-harness` registers it on agentic nodes.
    Yes,
    /// Not registered (a node built without it).
    NotRegistered,
    /// Registered, and refused by the node's `denied_tools`.
    Denied,
}

struct S {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    rec: Arc<Rec>,
    _master: Master,
    _dir: tempfile::TempDir,
}

/// One agentic node with the built-in tools (plus `remember` per `offer`),
/// built-in memory and a claimed admin.
async fn start(offer: Offer) -> S {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
    });
    let mut tools = harness_tools::builtin();
    if !matches!(offer, Offer::NotRegistered) {
        tools.push(Arc::new(harness_memory::Remember));
    }
    let node = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
            denied_tools: match offer {
                Offer::Denied => vec!["remember".into()],
                _ => vec![],
            },
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            rec.clone(),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let (rt, _) =
        ProviderRuntime::start(Config::default(), dir.path(), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    master.set_memory(Arc::new(SqliteMemory::open_in_memory().unwrap()));
    master.set_provider_runtime(Arc::new(rt));
    master.add_link(node.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let r = reqwest::Client::new()
        .post(format!("http://{addr}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let admin = common::signed_in(r).await;
    S {
        base: format!("http://{addr}"),
        ws,
        admin,
        rec,
        _master: master,
        _dir: dir,
    }
}

async fn call(s: &S, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.login(&s.admin).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn project(s: &S) -> String {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "Harness"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["project_id"].as_str().unwrap().to_string()
}

/// An active project fact, added by hand (newest so far: the clock moves
/// between two of them).
async fn fact(s: &S, pid: &str, text: &str) {
    tokio::time::sleep(Duration::from_millis(5)).await;
    let (st, v) = call(
        s,
        Method::POST,
        &format!("/api/projects/{pid}/memory"),
        Some(json!({ "text": text })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "active", "{v}");
}

/// A Software Developer (Coder) chat on the node, in `project` if given.
async fn coding_chat(s: &S, project: Option<&str>) -> String {
    let body = json!({
        "cwd": s.ws.display().to_string(),
        "agent_id": "coder",
        "approval_mode": "auto",
        "project_id": project,
    });
    let (st, v) = call(s, Method::POST, "/api/sessions", Some(body)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

/// Send `text` and wait for that turn to end.
async fn say(s: &S, sid: &str, text: &str) {
    let ended = |v: &Value| {
        v["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "turn_ended")
            .count()
    };
    let path = format!("/api/sessions/{sid}/events");
    let before = ended(&call(s, Method::GET, &path, None).await.1);
    let (st, v) = call(
        s,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..500 {
        if ended(&call(s, Method::GET, &path, None).await.1) > before {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("turn did not end");
}

/// The newest user message of the last request: where the per-turn
/// context goes.
fn last_context(s: &S) -> String {
    let reqs = s.rec.reqs.lock().unwrap();
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

/// The tools the model was offered in the last request.
fn last_tools(s: &S) -> Vec<String> {
    let reqs = s.rec.reqs.lock().unwrap();
    reqs.last()
        .unwrap()
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect()
}

async fn project_facts(s: &S, pid: &str) -> Vec<Value> {
    let (st, v) = call(s, Method::GET, &format!("/api/projects/{pid}/memory"), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["memories"].as_array().unwrap().clone()
}

#[tokio::test]
async fn a_coding_chat_on_a_node_proposes_project_facts() {
    let s = start(Offer::Yes).await;
    let pid = project(&s).await;
    let sid = coding_chat(&s, Some(&pid)).await;
    say(
        &s,
        &sid,
        "remember project: run the tests with CARGO_BUILD_JOBS=8",
    )
    .await;
    assert!(
        last_tools(&s).contains(&"remember".to_string()),
        "{:?}",
        last_tools(&s)
    );
    let ctx = last_context(&s);
    assert!(
        ctx.contains("call `remember` with scope \"project\""),
        "{ctx}"
    );
    // The Software Developer's own memory is off: the hint says so, with no
    // word about a workspace switch the person never set.
    assert!(ctx.contains("use no other scope"), "{ctx}");
    assert!(!ctx.contains("out of this workspace"), "{ctx}");
    let mut facts = vec![];
    for _ in 0..200 {
        facts = project_facts(&s, &pid).await;
        if !facts.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0]["status"], "proposed");
    assert_eq!(facts[0]["text"], "run the tests with CARGO_BUILD_JOBS=8");
    assert_eq!(facts[0]["source_session"], sid.as_str());
    let (_, personal) = call(&s, Method::GET, "/api/memory", None).await;
    assert!(
        personal["memories"].as_array().unwrap().is_empty(),
        "{personal}"
    );
}

#[tokio::test]
async fn all_active_project_facts_reach_a_turn_that_shares_no_words_with_them() {
    let s = start(Offer::Yes).await;
    let pid = project(&s).await;
    fact(
        &s,
        &pid,
        "The workspace builds with cargo on a slow USB disk.",
    )
    .await;
    fact(&s, &pid, "Sandbox suites need HARNESS_REQUIRE_BWRAP=1.").await;
    fact(&s, &pid, "Commit titles are imperative.").await;
    let sid = coding_chat(&s, Some(&pid)).await;
    // A proposal waiting for review is not part of it.
    say(&s, &sid, "remember project: an unconfirmed guess").await;
    say(&s, &sid, "continue").await;
    let ctx = last_context(&s);
    let at = |needle: &str| {
        ctx.find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from {ctx}"))
    };
    assert!(ctx.contains("What this project has established"), "{ctx}");
    // Newest first.
    assert!(at("Commit titles") < at("Sandbox suites"), "{ctx}");
    assert!(at("Sandbox suites") < at("slow USB disk"), "{ctx}");
    assert!(!ctx.contains("unconfirmed guess"), "{ctx}");
}

#[tokio::test]
async fn above_the_cap_the_newest_that_fit_and_recalled_ones() {
    let s = start(Offer::Yes).await;
    let pid = project(&s).await;
    // Twelve facts of about 450 characters: about 5,400 in all, above the
    // cap of about 4,000. Only the oldest names the zeppelin.
    let filler = "alpha bravo charlie delta echo foxtrot golf hotel ".repeat(9);
    for i in 1..=12 {
        let extra = if i == 1 { "zeppelin " } else { "" };
        fact(&s, &pid, format!("Fact {i:02} {extra}{filler}").trim()).await;
    }
    let sid = coding_chat(&s, Some(&pid)).await;
    say(&s, &sid, "what about the zeppelin?").await;
    let ctx = last_context(&s);
    for i in 5..=12 {
        assert!(
            ctx.contains(&format!("Fact {i:02} ")),
            "newest {i} missing: {ctx}"
        );
    }
    for i in 2..=4 {
        assert!(!ctx.contains(&format!("Fact {i:02} ")), "old {i} in: {ctx}");
    }
    assert!(
        ctx.contains("Fact 01 zeppelin"),
        "recalled one missing: {ctx}"
    );
    assert!(
        ctx.find("Fact 12 ").unwrap() < ctx.find("Fact 01 ").unwrap(),
        "{ctx}"
    );
    // The model is told the list is not complete.
    assert!(ctx.contains("3 older facts not shown"), "{ctx}");
}

#[tokio::test]
async fn the_hint_names_remember_only_where_the_node_offers_it() {
    for offer in [Offer::NotRegistered, Offer::Denied] {
        let s = start(offer).await;
        let pid = project(&s).await;
        fact(&s, &pid, "Commit titles are imperative.").await;
        let sid = coding_chat(&s, Some(&pid)).await;
        say(&s, &sid, "remember project: a fact nobody may file").await;
        let ctx = last_context(&s);
        assert!(ctx.contains("Project \"Harness\""), "{offer:?}: {ctx}");
        assert!(!ctx.contains("`remember`"), "{offer:?}: {ctx}");
        assert!(ctx.contains("Commit titles"), "{offer:?}: {ctx}");
        // The node decides: a call from a node that does not offer the tool
        // files nothing, whatever the model sends.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let facts = project_facts(&s, &pid).await;
        assert_eq!(facts.len(), 1, "{offer:?}: {facts:?}");
        assert_eq!(facts[0]["status"], "active", "{offer:?}");
    }
}

#[tokio::test]
async fn a_chat_without_a_project_gets_no_project_facts() {
    let s = start(Offer::Yes).await;
    let pid = project(&s).await;
    fact(&s, &pid, "Commit titles are imperative.").await;
    let sid = coding_chat(&s, None).await;
    say(&s, &sid, "continue").await;
    let ctx = last_context(&s);
    assert!(!ctx.contains("Commit titles"), "{ctx}");
    assert!(!ctx.contains("What this project has established"), "{ctx}");
    assert!(!ctx.contains("`remember`"), "{ctx}");
}
