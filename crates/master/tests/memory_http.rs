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
