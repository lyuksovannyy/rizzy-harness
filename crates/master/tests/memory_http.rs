//! Memory end to end: the model proposes, the user confirms, recall feeds the
//! next prompt; incognito neither writes nor reads; other users see nothing.

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
    // Supplies the admin settings (token savings) from a default config.
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
    let r = rb.header("cookie", c).send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn cookie(r: &reqwest::Response) -> String {
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn login(t: &T) -> String {
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    cookie(&r)
}

async fn turn(t: &T, c: &str, text: &str, incognito: bool) -> String {
    let (_, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "incognito": incognito})),
        c,
    )
    .await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..300 {
        let (_, row) = req(t, Method::GET, &format!("/api/sessions/{sid}"), None, c).await;
        if row["running"] == false && row["head_seq"].as_u64().unwrap_or(0) > 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    sid
}

/// What the model saw besides the history: the system prompt and the
/// per-turn context in front of the newest user message.
fn last_system(t: &T) -> String {
    let reqs = t.rec.reqs.lock().unwrap();
    let r = reqs.last().unwrap();
    let ChatMsg::System(sys) = &r.messages[0] else {
        panic!()
    };
    // Recall and the time-stamped context line change every turn: they must
    // stay out of the system prompt, or the prompt cache never hits.
    assert!(!sys.contains("What you remember"), "{sys}");
    assert!(!sys.contains("Context: the user's name"), "{sys}");
    let user = r
        .messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMsg::User(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap_or_default();
    format!("{sys}\n{user}")
}

async fn memories(t: &T, c: &str) -> Vec<Value> {
    let (_, v) = req(t, Method::GET, "/api/memory", None, c).await;
    v["memories"].as_array().unwrap().clone()
}

/// With caching off and the chat style set to `ultra` (clamped to `lite`).
async fn off_turn_checks(t: &T, c: &str) {
    turn(t, c, "hello again", false).await;
    let reqs = t.rec.reqs.lock().unwrap();
    let r = reqs.last().unwrap();
    assert!(!r.cache);
    let ChatMsg::System(sys) = &r.messages[0] else {
        panic!()
    };
    assert!(sys.contains("Answer style: lead with the answer"), "{sys}");
    assert!(!sys.contains("Articles optional"), "{sys}");
}

#[tokio::test]
async fn propose_confirm_recall() {
    let t = start().await;
    let c = login(&t).await;
    let sid = turn(
        &t,
        &c,
        "remember: Alex rides the number 7 tram to work",
        false,
    )
    .await;
    let mut m = vec![];
    for _ in 0..100 {
        m = memories(&t, &c).await;
        if !m.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(m.len(), 1);
    assert_eq!(m[0]["status"], "proposed");
    assert_eq!(m[0]["source_session"], sid.as_str());
    let id = m[0]["id"].as_str().unwrap().to_string();

    // Proposed facts are not recalled.
    turn(&t, &c, "which tram do I take?", false).await;
    assert!(!last_system(&t).contains("number 7 tram"));
    // Confirmed: recalled into the next relevant prompt.
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/memory/{id}"),
        Some(json!({"status": "active"})),
        &c,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    turn(&t, &c, "which tram do I take?", false).await;
    let sys = last_system(&t);
    assert!(
        sys.contains("What you remember about the user") && sys.contains("number 7 tram"),
        "{sys}"
    );
    // The context line is there too.
    assert!(sys.contains("Context: the user's name is alex"), "{sys}");

    // Token savings defaults reach the model: cache markers and, in a chat,
    // the lite style only.
    {
        let reqs = t.rec.reqs.lock().unwrap();
        let r = reqs.last().unwrap();
        assert!(r.cache);
        let ChatMsg::System(sys) = &r.messages[0] else {
            panic!()
        };
        assert!(sys.contains("Answer style: lead with the answer"), "{sys}");
        assert!(
            !sys.contains("Articles optional"),
            "chat never gets the terse style"
        );
    }
    // Every method can be switched off; the next turn follows.
    let (st, v) = req(&t, Method::GET, "/api/admin/savings", None, &c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let mut off = v.clone();
    for k in [
        "prompt_cache",
        "mask_old_outputs",
        "filter_shell_output",
        "bounded_reads",
        "web_readability",
        "repo_map",
        "outline_tool",
        "graphify",
    ] {
        off[k] = json!(false);
    }
    off["chat_style"] = json!("ultra");
    off["agentic_style"] = json!("off");
    let (st, v) = req(&t, Method::PUT, "/api/admin/savings", Some(off), &c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    off_turn_checks(&t, &c).await;
    // No built-in node here: graphify is decided by each node's node.toml.
    let (st, _) = req(
        &t,
        Method::PUT,
        "/api/admin/graphify",
        Some(json!({"allowed": true})),
        &c,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Incognito: no recall and no remember tool.
    turn(&t, &c, "which tram do I take?", true).await;
    assert!(!last_system(&t).contains("number 7 tram"));
    let tools: Vec<String> = t
        .rec
        .reqs
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .tools
        .iter()
        .map(|x| x.name.clone())
        .collect();
    assert!(!tools.contains(&"remember".to_string()), "{tools:?}");
    turn(&t, &c, "remember: incognito secret", true).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        memories(&t, &c).await.len(),
        1,
        "nothing filed from incognito"
    );

    // User-added facts are active at once; other users cannot touch them.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/memory",
        Some(json!({"text": "Alex is vegetarian", "kind": "preference"})),
        &c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "active");
    req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        &c,
    )
    .await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "bob", "password": "temporary pw"}))
        .send()
        .await
        .unwrap();
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .header("cookie", cookie(&r))
        .json(&json!({"current": "temporary pw", "new": "bobs own password"}))
        .send()
        .await
        .unwrap();
    let bob = cookie(&r);
    assert!(memories(&t, &bob).await.is_empty());
    let (st, _) = req(&t, Method::DELETE, &format!("/api/memory/{id}"), None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/memory/{id}"),
        Some(json!({"text": "hijack"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(&t, Method::DELETE, &format!("/api/memory/{id}"), None, &c).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(memories(&t, &c).await.len(), 1);
}

/// What a fake `/embeddings` endpoint was sent: auth headers and model.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Value>>>);

impl Seen {
    fn last(&self) -> Value {
        self.0
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or(Value::Null)
    }
    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// A fake embeddings server; without `embeddings` it has no such route
/// (like a chat-only provider). Model `missing` is answered the way
/// Ollama answers a model it does not have.
async fn fake_embeddings(embeddings: bool) -> (String, Seen) {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    let seen = Seen::default();
    let s = seen.clone();
    let mut app = axum::Router::new().route(
        "/v1/models",
        axum::routing::get(|| async { axum::Json(json!({"data": []})) }),
    );
    if embeddings {
        app = app.route(
            "/v1/embeddings",
            axum::routing::post(move |h: HeaderMap, axum::Json(b): axum::Json<Value>| {
                let s = s.clone();
                async move {
                    let hv = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
                    s.0.lock().unwrap().push(json!({
                        "authorization": hv("authorization"),
                        "x_api_key": hv("x-api-key"),
                        "anthropic_version": hv("anthropic-version"),
                        "model": b["model"],
                    }));
                    if b["model"] == "missing" {
                        let err = json!({"error": {"message": "model \"missing\" not found, try pulling it first\u{202E}"}});
                        return (StatusCode::NOT_FOUND, axum::Json(err)).into_response();
                    }
                    let n = b["input"].as_array().map_or(0, Vec::len);
                    let data: Vec<Value> = (0..n)
                        .map(|i| json!({"index": i, "embedding": [1.0, 0.0, 0.0]}))
                        .collect();
                    axum::Json(json!({ "data": data })).into_response()
                }
            }),
        );
    }
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (base, seen)
}

struct C {
    base: String,
    dir: tempfile::TempDir,
    admin: String,
    mem: Arc<SqliteMemory>,
}

/// A server wired like `harness serve`: config and memory from the data
/// dir, which `setup` prepares first.
async fn start_cfg(setup: impl FnOnce(&std::path::Path)) -> C {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let cfg = harness_master::config::Config::load(&dir.path().join("harness.toml")).unwrap();
    let secrets = harness_core::SecretSet::default();
    let (_, mem) = harness_master::providers::memory(&cfg, dir.path(), &secrets).unwrap();
    let mem = mem.unwrap();
    let (rt, _) =
        harness_master::providers::ProviderRuntime::start(cfg, dir.path(), secrets).unwrap();
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
    master.set_memory(mem.clone());
    master.set_builtin_memory(mem.clone());
    master.set_provider_runtime(Arc::new(rt));
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let base = format!("http://{addr}");
    let r = reqwest::Client::new()
        .post(format!("{base}/api/setup"))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    C {
        admin: cookie(&r),
        base,
        dir,
        mem,
    }
}

async fn call(c: &C, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", c.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.header("cookie", &c.admin).send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn put_mem(c: &C, body: Value) -> (StatusCode, Value) {
    call(c, Method::PUT, "/api/admin/memory", Some(body)).await
}

async fn put_provider(c: &C, id: &str, preset: &str, url: &str, key: &str) {
    let (st, v) = call(
        c,
        Method::PUT,
        &format!("/api/admin/providers/{id}"),
        Some(json!({
            "name": format!("Acme {id}"), "preset": preset, "base_url": url,
            "api_key": key, "models": [{"id": "chat-1"}],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

fn file(c: &C, name: &str) -> String {
    std::fs::read_to_string(c.dir.path().join(name)).unwrap_or_default()
}

/// Recall once, so the live embedder is called.
async fn recall(c: &C) {
    use harness_memory::MemoryStore;
    c.mem.recall(None, "anything", 3).await.unwrap();
}

#[tokio::test]
async fn embeddings_from_a_provider_follow_it() {
    let c = start_cfg(|_| {}).await;
    let (a, seen_a) = fake_embeddings(true).await;
    let (b, seen_b) = fake_embeddings(true).await;
    // Before anything is chosen, the GET suggests Ollama (no real provider).
    let (_, v) = call(&c, Method::GET, "/api/admin/memory", None).await;
    assert_eq!(
        (&v["recall"], &v["source"]),
        (&json!("keywords"), &json!("ollama"))
    );
    // Not the demo, not an unknown id.
    for id in ["demo", "nope", ""] {
        let (st, v) = put_mem(
            &c,
            json!({"recall": "meaning", "source": "provider", "provider_id": id, "embeddings_model": "e"}),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{id}: {v}");
    }
    put_provider(&c, "acme", "custom_openai", &a, "pk-1").await;
    let (_, v) = call(&c, Method::GET, "/api/admin/memory", None).await;
    assert_eq!(
        (&v["source"], &v["provider_id"]),
        (&json!("provider"), &json!("acme"))
    );
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "provider", "provider_id": "acme"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "a model is required: {v}");
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "provider", "provider_id": "acme", "embeddings_model": "emb-1"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["active_model"], "emb-1");
    assert_eq!(
        (&v["key"], &v["api_style"]),
        (&json!("none"), &json!("openai"))
    );
    assert_eq!(seen_a.last()["authorization"], "Bearer pk-1");
    // The provider's key is resolved live, never copied.
    assert!(!file(&c, "harness.toml").contains("pk-1"));
    assert!(!file(&c, "secrets.toml").contains("embeddings"));

    // The provider moves and gets a new key: the embedder follows at once.
    put_provider(&c, "acme", "custom_openai", &b, "pk-2").await;
    let before = seen_a.count();
    recall(&c).await;
    assert_eq!(seen_a.count(), before);
    assert_eq!(seen_b.last()["authorization"], "Bearer pk-2");
    assert_eq!(seen_b.last()["model"], "emb-1");

    // An Anthropic-kind provider: its key goes as x-api-key.
    put_provider(&c, "ant", "custom_anthropic", &a, "ak-1").await;
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "provider", "provider_id": "ant", "embeddings_model": "emb-1"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["api_style"], "anthropic");
    let last = seen_a.last();
    assert_eq!(last["x_api_key"], "ak-1");
    assert_eq!(last["anthropic_version"], "2023-06-01");
    assert_eq!(last["authorization"], Value::Null);

    // Deleting it falls back to keywords, live.
    let (st, _) = call(&c, Method::DELETE, "/api/admin/providers/ant", None).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&c, Method::GET, "/api/admin/memory", None).await;
    assert_eq!(v["recall"], "keywords");
    assert_eq!(v["active_model"], Value::Null);
    assert!(c.mem.embedder_model().is_none());
}

#[tokio::test]
async fn embeddings_failures_say_why() {
    let c = start_cfg(|_| {}).await;
    let (chat_only, _) = fake_embeddings(false).await;
    let (ollama, _) = fake_embeddings(true).await;
    put_provider(&c, "chat", "custom_openai", &chat_only, "pk-1").await;
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "provider", "provider_id": "chat", "embeddings_model": "e"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    let msg = v["error"].as_str().unwrap_or(&v.to_string()).to_string();
    assert!(
        msg.contains("Acme chat has no embeddings endpoint"),
        "{msg}"
    );
    // A missing model: the upstream's own words, cleaned, plus a hint.
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "ollama", "embeddings_url": ollama, "embeddings_model": "missing"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    let msg = v.to_string();
    assert!(msg.contains("model \\\"missing\\\" not found"), "{msg}");
    assert!(msg.contains("is the model `missing` available"), "{msg}");
    assert!(!msg.contains('\u{202E}'), "{msg}");
    // Nothing was saved.
    let (_, v) = call(&c, Method::GET, "/api/admin/memory", None).await;
    assert_eq!(v["recall"], "keywords");
}

#[tokio::test]
async fn ollama_and_custom_sources_and_their_key() {
    let c = start_cfg(|_| {}).await;
    let (a, seen_a) = fake_embeddings(true).await;
    let (b, seen_b) = fake_embeddings(true).await;
    // Ollama: the preset's model, no key.
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "ollama", "embeddings_url": a, "preset": "fast"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (&v["model"], &v["active_model"]),
        (&json!("all-minilm"), &json!("all-minilm"))
    );
    assert_eq!(seen_a.last()["authorization"], Value::Null);
    // Custom without a model, or with a key over plain http elsewhere: 400.
    let (st, _) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "custom", "embeddings_url": a}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "custom", "embeddings_url": "http://embed.example/v1",
               "embeddings_model": "e", "embeddings_key": "ek-1"}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    // Custom with its own key.
    let custom = |url: &str, style: &str, key: Option<&str>| {
        json!({"recall": "meaning", "source": "custom", "embeddings_url": url,
               "embeddings_model": "e", "api_style": style, "embeddings_key": key})
    };
    let (st, v) = put_mem(&c, custom(&a, "openai", Some("ek-1"))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "stored");
    assert_eq!(seen_a.last()["authorization"], "Bearer ek-1");
    assert!(file(&c, "secrets.toml").contains("ek-1"));
    assert!(!file(&c, "harness.toml").contains("ek-1"));
    // Omitted key, same URL: kept; Anthropic style sends it as x-api-key.
    let (st, v) = put_mem(&c, custom(&a, "anthropic", None)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "stored");
    let last = seen_a.last();
    assert_eq!(
        (&last["x_api_key"], &last["anthropic_version"]),
        (&json!("ek-1"), &json!("2023-06-01"))
    );
    assert_eq!(last["authorization"], Value::Null);
    // Keywords only: nothing is called, the custom settings stay.
    let (st, v) = put_mem(&c, json!({"recall": "keywords"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        (&v["source"], &v["key"]),
        (&json!("custom"), &json!("stored"))
    );
    assert_eq!(v["active_model"], Value::Null);
    // Another URL without a key: the stored key is not sent there, and goes.
    let (st, v) = put_mem(&c, custom(&b, "openai", None)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "none");
    assert_eq!(seen_b.last()["authorization"], Value::Null);
    assert!(!file(&c, "secrets.toml").contains("ek-1"));
    // Switching away from custom clears a new key too.
    put_mem(&c, custom(&b, "openai", Some("ek-2"))).await;
    assert!(file(&c, "secrets.toml").contains("ek-2"));
    let (st, v) = put_mem(
        &c,
        json!({"recall": "meaning", "source": "ollama", "embeddings_url": b}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["key"], "none");
    assert!(!file(&c, "secrets.toml").contains("ek-2"));
    recall(&c).await;
    assert_eq!(seen_b.last()["authorization"], Value::Null);
}

#[tokio::test]
async fn legacy_embeddings_config_loads() {
    let (a, seen) = fake_embeddings(true).await;
    let c = start_cfg(|dir| {
        std::fs::write(
            dir.join("harness.toml"),
            format!("[memory]\nembeddings_url = \"{a}\"\nembeddings_model = \"old-e\"\nembeddings_key_secret = \"embeddings\"\n"),
        )
        .unwrap();
        harness_master::secrets::set(
            &dir.join("secrets.toml"),
            "embeddings",
            Some(&Secret::new("ek-old".into())),
        )
        .unwrap();
    })
    .await;
    let (_, v) = call(&c, Method::GET, "/api/admin/memory", None).await;
    assert_eq!(
        (&v["recall"], &v["source"], &v["key"], &v["api_style"]),
        (
            &json!("meaning"),
            &json!("custom"),
            &json!("stored"),
            &json!("openai")
        )
    );
    assert_eq!(v["active_model"], "old-e");
    recall(&c).await;
    assert_eq!(seen.last()["authorization"], "Bearer ek-old");
}

/// The custom source's stored key lists that endpoint's models when asked
/// (`memory: true`, same URL only), and a failed save keeps the old key.
#[tokio::test]
async fn custom_source_key_lists_models_and_survives_a_failed_save() {
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    let app = axum::Router::new()
        .route(
            "/v1/models",
            axum::routing::get(|h: HeaderMap| async move {
                if h.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer ek-9") {
                    axum::Json(json!({"data": [{"id": "emb-1"}]})).into_response()
                } else {
                    (StatusCode::UNAUTHORIZED, "no").into_response()
                }
            }),
        )
        .route(
            "/v1/embeddings",
            axum::routing::post(|| async {
                axum::Json(json!({"data": [{"index": 0, "embedding": [1.0, 0.0]}]}))
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let c = start_cfg(|_| {}).await;
    let custom = |key: &str| {
        json!({"recall": "meaning", "source": "custom", "embeddings_url": url,
               "embeddings_model": "e", "embeddings_key": key})
    };
    let (st, v) = put_mem(&c, custom("ek-9")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let test = |memory: bool| {
        let body =
            json!({"preset": "custom_openai", "base_url": format!("{url}/"), "memory": memory});
        let c = &c;
        async move {
            call(c, Method::POST, "/api/admin/providers/test", Some(body))
                .await
                .1
        }
    };
    let v = test(true).await;
    assert_eq!(
        (&v["ok"], &v["models"][0]["id"]),
        (&json!(true), &json!("emb-1")),
        "{v}"
    );
    assert_eq!(test(false).await["ok"], false, "only when asked");

    let tmp = c.dir.path().join("harness.toml.tmp");
    std::fs::create_dir(&tmp).unwrap();
    let (st, _) = put_mem(&c, custom("ek-10")).await;
    assert!(!st.is_success());
    std::fs::remove_dir(&tmp).unwrap();
    let secrets = file(&c, "secrets.toml");
    assert!(
        secrets.contains("ek-9") && !secrets.contains("ek-10"),
        "{secrets}"
    );
}

/// A provider change made while the memory test embedding runs is not
/// undone by the memory save, and a provider the test relied on that
/// changed meanwhile makes the save ask for a new test.
#[tokio::test]
async fn memory_save_does_not_undo_concurrent_provider_changes() {
    let app = axum::Router::new().route(
        "/v1/embeddings",
        axum::routing::post(|| async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            axum::Json(json!({"data": [{"index": 0, "embedding": [1.0, 0.0]}]}))
        }),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let slow = format!("http://{}/v1", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let (fast, _) = fake_embeddings(true).await;
    let c = start_cfg(|_| {}).await;
    put_provider(&c, "gone", "custom_openai", &fast, "k-gone").await;
    put_provider(&c, "emb", "custom_openai", &slow, "k-emb").await;
    let later = |method: Method, path: &'static str, body: Option<Value>| {
        let c = &c;
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            call(c, method, path, body).await
        }
    };

    let ((st, v), (dst, _)) = tokio::join!(
        put_mem(
            &c,
            json!({"recall": "meaning", "source": "custom", "embeddings_url": slow,
                   "embeddings_model": "e"}),
        ),
        later(Method::DELETE, "/api/admin/providers/gone", None),
    );
    assert_eq!((st, dst), (StatusCode::OK, StatusCode::OK), "{v}");
    let (_, p) = call(&c, Method::GET, "/api/admin/providers", None).await;
    assert!(!p.to_string().contains("\"gone\""), "{p}");
    assert!(!file(&c, "harness.toml").contains("gone"));

    let moved = json!({"name": "Acme emb", "preset": "custom_openai", "base_url": fast,
                       "api_key": "k-emb-2", "models": [{"id": "chat-1"}]});
    let ((st, v), (pst, _)) = tokio::join!(
        put_mem(
            &c,
            json!({"recall": "meaning", "source": "provider", "provider_id": "emb",
                   "embeddings_model": "e"}),
        ),
        later(Method::PUT, "/api/admin/providers/emb", Some(moved)),
    );
    assert_eq!(pst, StatusCode::OK);
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (_, p) = call(&c, Method::GET, "/api/admin/providers", None).await;
    assert_eq!(
        p["providers"][0]["base_url"],
        json!(fast),
        "only emb is left"
    );
}
