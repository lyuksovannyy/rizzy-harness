//! The Agents page over HTTP: built-ins take a summary and additional
//! instructions only, custom agents are made, changed and deleted, edits
//! survive a restart, memory off keeps an agent away from memory, chats of
//! a deleted agent go on, and a summary can be written by the model.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::Auth;
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// The chat runtime's provider: records each request, answers like the mock.
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
    store: Store,
    rec: Arc<Rec>,
    _dir: tempfile::TempDir,
}

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
            vec![Arc::new(harness_memory::Remember)],
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let store = Store::open_in_memory().unwrap();
    let master = Master::new(
        store.clone(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false),
    );
    master.set_memory(Arc::new(SqliteMemory::open_in_memory().unwrap()));
    let (rt, _) = harness_master::providers::ProviderRuntime::start(
        harness_master::config::Config::default(),
        dir.path(),
        Default::default(),
    )
    .unwrap();
    master.set_provider_runtime(Arc::new(rt));
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        store,
        rec,
        _dir: dir,
    }
}

async fn call(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .bearer_auth("tok");
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn features() -> Value {
    json!({
        "memory": true, "web": true, "read_files": true, "edit_files": false,
        "commands": false, "delegate": false, "as_agent": true, "as_subagent": true
    })
}

async fn listed(t: &T, id: &str) -> Value {
    let (_, v) = call(t, Method::GET, "/api/agents", None).await;
    v["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

/// One chat turn with `agent`; the session id.
async fn chat(t: &T, agent: &str, text: &str) -> String {
    let (st, v) = call(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "agent_id": agent})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    say(t, &sid, text).await;
    sid
}

async fn say(t: &T, sid: &str, text: &str) {
    let before = t.rec.reqs.lock().unwrap().len();
    let (st, v) = call(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({ "text": text })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..300 {
        if t.rec.reqs.lock().unwrap().len() > before {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no model request");
}

fn last_request(t: &T) -> (String, Vec<String>) {
    let reqs = t.rec.reqs.lock().unwrap();
    let r = reqs.last().unwrap();
    let system = match r.messages.first() {
        Some(ChatMsg::System(s)) => s.clone(),
        _ => String::new(),
    };
    (system, r.tools.iter().map(|x| x.name.clone()).collect())
}

#[tokio::test]
async fn built_ins_take_a_summary_and_additional_instructions_only() {
    let t = start().await;
    let coder = listed(&t, "coder").await;
    assert_eq!(coder["origin"], "builtin");
    assert_eq!(coder["features"]["edit_files"], true);
    assert!(coder.get("prompt").is_none(), "the list leaves prompts out");
    let (st, d) = call(&t, Method::GET, "/api/agents/coder", None).await;
    assert_eq!(st, StatusCode::OK);
    let prompt = d["prompt"].as_str().unwrap().to_string();
    assert!(prompt.starts_with("You are a careful software engineer"));
    assert_eq!(d["instructions"], "");
    assert_eq!(d["summary"], "");
    assert_eq!(d["description_default"], coder["description"]);
    // The page reads in the list's scope, so its model line agrees with it.
    assert_eq!(d["main_model"], coder["main_model"]);
    let (st, d) = call(&t, Method::GET, "/api/agents/coder?scope=server", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(d["main_model"], coder["main_model"]);
    let (st, _) = call(&t, Method::GET, "/api/agents/coder?scope=nope", None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, d) = call(
        &t,
        Method::PATCH,
        "/api/agents/coder",
        Some(json!({"summary": " Our engineer. ", "instructions": "Use pnpm."})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["summary"], "Our engineer.");
    assert_eq!(d["description"], "Our engineer.");
    assert_eq!(d["prompt"], prompt.as_str(), "the built-in prompt stays");
    let spec = t.master.agents().get("coder").unwrap().spec.clone();
    assert!(
        spec.system_prompt
            .ends_with("# Additional instructions\n\nUse pnpm.")
    );
    // The instructions reach the model.
    chat(&t, "coder", "hello").await;
    assert!(last_request(&t).0.contains("Use pnpm."));

    for change in [
        json!({"name": "Mine"}),
        json!({"prompt": "Do other things."}),
        json!({"features": features()}),
    ] {
        let (st, v) = call(&t, Method::PATCH, "/api/agents/coder", Some(change)).await;
        assert_eq!(st, StatusCode::CONFLICT, "{v}");
        assert!(v["error"].as_str().unwrap().contains("built in"), "{v}");
    }
    let (st, _) = call(&t, Method::DELETE, "/api/agents/coder", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&t, Method::GET, "/api/agents/nobody", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(&t, Method::DELETE, "/api/agents/nobody", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Clearing both fields drops the edit.
    let (st, d) = call(
        &t,
        Method::PATCH,
        "/api/agents/coder",
        Some(json!({"summary": "", "instructions": ""})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(d["description"], coder["description"]);
    assert!(t.master.agent_edits().overlays.is_empty());
}

#[tokio::test]
async fn custom_agents_are_made_changed_kept_and_deleted() {
    let t = start().await;
    let body = |id: &str| json!({"id": id, "name": "Reviewer", "summary": "Reviews.", "prompt": "Review it.", "features": features()});
    let (st, d) = call(&t, Method::POST, "/api/agents", Some(body("reviewer"))).await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["origin"], "custom");
    assert_eq!(d["prompt"], "Review it.");
    assert_eq!(listed(&t, "reviewer").await["description"], "Reviews.");
    for (id, code) in [
        ("reviewer", StatusCode::CONFLICT),
        ("coder", StatusCode::CONFLICT),
        ("new", StatusCode::CONFLICT),
        ("Bad Id", StatusCode::BAD_REQUEST),
    ] {
        let (st, v) = call(&t, Method::POST, "/api/agents", Some(body(id))).await;
        assert_eq!(st, code, "{id}: {v}");
    }
    let mut nowhere = body("ghost");
    nowhere["features"]["as_agent"] = json!(false);
    nowhere["features"]["as_subagent"] = json!(false);
    let (st, _) = call(&t, Method::POST, "/api/agents", Some(nowhere)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // A lead that delegates gets every sub-agent.
    let mut lead = body("lead");
    lead["name"] = json!("Lead");
    lead["features"]["delegate"] = json!(true);
    let (st, _) = call(&t, Method::POST, "/api/agents", Some(lead)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.master.agents().get("lead").unwrap().delegates,
        ["general", "coder", "writer", "scout", "reviewer"]
    );

    let (st, d) = call(
        &t,
        Method::PATCH,
        "/api/agents/reviewer",
        Some(json!({"summary": "Reviews code.", "prompt": "Review carefully."})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(
        (d["summary"].as_str(), d["name"].as_str()),
        (Some("Reviews code."), Some("Reviewer"))
    );
    let (st, _) = call(
        &t,
        Method::PATCH,
        "/api/agents/reviewer",
        Some(json!({"instructions": "More."})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Edits are stored: a restarted master has them.
    let again = Master::new(
        t.store.clone(),
        Auth::new(Secret::new("tok".into()), vec![], false),
    );
    again.load_agent_edits().await.unwrap();
    let kept = again.agents();
    assert_eq!(
        kept.get("reviewer").unwrap().spec.system_prompt,
        "Review carefully."
    );
    assert!(kept.get("lead").is_some());

    // A chat of a deleted agent goes on with the default one.
    let sid = chat(&t, "reviewer", "first").await;
    assert!(last_request(&t).0.starts_with("Review carefully."));
    let (st, _) = call(&t, Method::DELETE, "/api/agents/reviewer", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(listed(&t, "reviewer").await.is_null());
    assert_eq!(
        t.master.agents().get("lead").unwrap().delegates,
        ["general", "coder", "writer", "scout"]
    );
    say(&t, &sid, "second").await;
    assert!(
        last_request(&t)
            .0
            .starts_with("You are a helpful, concise assistant")
    );
}

#[tokio::test]
async fn memory_off_means_no_recall_and_no_remember() {
    let t = start().await;
    let (st, _) = call(
        &t,
        Method::POST,
        "/api/memory",
        Some(json!({"text": "I take the number 7 tram"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let mut quiet = features();
    quiet["memory"] = json!(false);
    for (id, f) in [("keeper", features()), ("quiet", quiet)] {
        let (st, v) = call(
            &t,
            Method::POST,
            "/api/agents",
            Some(json!({"id": id, "name": id, "prompt": "Help.", "features": f})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    chat(&t, "keeper", "which tram do I take?").await;
    let (system, tools) = last_request(&t);
    assert!(tools.contains(&"remember".to_string()), "{tools:?}");
    let all: String = t
        .rec
        .reqs
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .messages
        .iter()
        .map(|m| format!("{m:?}"))
        .collect();
    assert!(all.contains("number 7 tram") || system.contains("number 7 tram"));

    chat(&t, "quiet", "which tram do I take?").await;
    let (_, tools) = last_request(&t);
    assert!(!tools.contains(&"remember".to_string()), "{tools:?}");
    let all: String = t
        .rec
        .reqs
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .messages
        .iter()
        .map(|m| format!("{m:?}"))
        .collect();
    assert!(!all.contains("number 7 tram"), "{all}");
}

/// Aliases and categories: listed, one namespace with ids, resolved to the
/// id wherever an agent is named, fixed on built-ins.
#[tokio::test]
async fn aliases_and_categories() {
    let t = start().await;
    let body = |id: &str, aliases: Value| {
        json!({"id": id, "name": "Critic", "prompt": "Critique it.", "features": features(),
               "aliases": aliases, "category": "Quality"})
    };
    let (st, d) = call(
        &t,
        Method::POST,
        "/api/agents",
        Some(body("critic", json!(["crit"]))),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    let listed_critic = listed(&t, "critic").await;
    assert_eq!(listed_critic["aliases"], json!(["crit"]));
    assert_eq!(listed_critic["category"], "Quality");
    assert_eq!(listed(&t, "writer").await["category"], "General");
    // One namespace: an alias or id someone has is refused, saying which.
    for (id, aliases, code) in [
        ("other", json!(["crit"]), StatusCode::CONFLICT),
        ("other", json!(["coder"]), StatusCode::CONFLICT),
        ("other", json!(["new"]), StatusCode::CONFLICT),
        ("crit", json!([]), StatusCode::CONFLICT),
        ("other", json!(["Bad"]), StatusCode::BAD_REQUEST),
        ("other", json!(["other"]), StatusCode::BAD_REQUEST),
    ] {
        let (st, v) = call(&t, Method::POST, "/api/agents", Some(body(id, aliases))).await;
        assert_eq!(st, code, "{id}: {v}");
    }
    let mut long = body("other", json!([]));
    long["category"] = json!("c".repeat(40));
    let (st, _) = call(&t, Method::POST, "/api/agents", Some(long)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Its own aliases can change; another's cannot be taken.
    let (st, d) = call(
        &t,
        Method::PATCH,
        "/api/agents/critic",
        Some(json!({"aliases": ["crit", "nitpick"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["aliases"], json!(["crit", "nitpick"]));
    assert_eq!(d["category"], "Quality");

    // A built-in's aliases and category are fixed; sending them unchanged
    // is fine.
    let coder = listed(&t, "coder").await;
    for change in [json!({"aliases": ["mine"]}), json!({"category": "Other"})] {
        let (st, v) = call(&t, Method::PATCH, "/api/agents/coder", Some(change)).await;
        assert_eq!(st, StatusCode::CONFLICT, "{v}");
    }
    let (st, v) = call(
        &t,
        Method::PATCH,
        "/api/agents/coder",
        Some(json!({"aliases": coder["aliases"], "category": coder["category"], "summary": "S."})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // An alias names the agent; what is stored is its id.
    let (st, v) = call(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "agent_id": "crit"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (_, row) = call(&t, Method::GET, &format!("/api/sessions/{sid}"), None).await;
    assert_eq!(row["agent_id"], "critic");
    let other = chat(&t, "general", "hello").await;
    say(&t, &other, "@nitpick look").await;
    assert!(last_request(&t).0.starts_with("Critique it."));

    // A custom agent saved before aliases existed still loads.
    t.store
        .kv_set(
            "agent-custom:server:oldie",
            &json!({"name": "Oldie", "summary": "", "prompt": "Old.", "features": features()})
                .to_string(),
        )
        .unwrap();
    t.master.load_agent_edits().await.unwrap();
    let oldie = listed(&t, "oldie").await;
    assert_eq!(oldie["aliases"], json!([]));
    assert_eq!(oldie["category"], "General");
}

#[tokio::test]
async fn a_summary_is_written_from_the_instructions() {
    let t = start().await;
    let (st, v) = call(
        &t,
        Method::POST,
        "/api/agent-summary",
        Some(json!({"name": "Reviewer", "prompt": "Review code."})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let s = v["summary"].as_str().unwrap();
    assert!(
        !s.is_empty() && s.chars().count() <= 300 && !s.contains('\n'),
        "{s}"
    );
    let (st, _) = call(
        &t,
        Method::POST,
        "/api/agent-summary",
        Some(json!({"prompt": "  "})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}
