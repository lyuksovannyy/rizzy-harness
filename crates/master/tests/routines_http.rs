//! Routines over HTTP, on the routine clock (docs/routines.md): Run now
//! into a team workspace and what its result looks like, the trigger rule
//! and validation, who sees and changes what (T10), an interval across a
//! restart and catch-up, approvals parking a run (a delegate's too) and the
//! timeouts, revocation, caps and the preview. Nodes are real runtimes on
//! the scripted mock model.

mod common;
use common::{Login as _, SameOrigin as _};

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::routines::{self, Clock};
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::routines::NewRun;
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const MIN: i64 = 60_000;

/// Sunday 8 November 2026, 09:30 UTC: where every test's clock starts.
fn t0() -> i64 {
    "2026-11-08T09:30:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond()
}

async fn rt(root: &Path, id: &str, profile: Profile) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: id.into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.into()],
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            if profile == Profile::Agentic {
                harness_tools::builtin()
            } else {
                vec![]
            },
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        // Beside the workspace root: sub-agent worktrees need it (R.1).
        checkpoints_dir: root.parent().map(|p| p.join(format!("checkpoints-{id}"))),
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

struct T {
    base: String,
    addr: std::net::SocketAddr,
    master: Master,
    clock: Arc<AtomicI64>,
    root: String,
    /// The chat runtime (a test stops it as the server's stop does).
    chat: NodeRuntime,
}

/// A server on `store` with a chat runtime and an agentic node "local"
/// (both on the mock model), routines on a clock at `clock`; claimable by
/// "alex" when `claim`.
async fn serve(store: Store, dir: &Path, clock: Arc<AtomicI64>, claim: bool) -> T {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut auth = Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false);
    if claim {
        let path = dir.join("claim-code");
        std::fs::write(&path, "X").unwrap();
        auth = auth.with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path,
        });
    }
    let master = Master::new(store, auth);
    master.set_agents(harness_agents::AgentSet::load(&dir.join("none")).0);
    let root = dir.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let chat = rt(&root, "master", Profile::Chat).await;
    master.add_link(chat.attach().await, true).await.unwrap();
    let local = rt(&root, "local", Profile::Agentic).await;
    master.add_link(local.attach().await, true).await.unwrap();
    routines::start_with(&master, Clock::manual(clock.clone()), false);
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master,
        clock,
        root: root.display().to_string(),
        chat,
    }
}

async fn start(dir: &Path) -> T {
    serve(
        Store::open_in_memory().unwrap(),
        dir,
        Arc::new(AtomicI64::new(t0())),
        true,
    )
    .await
}

impl T {
    fn set_clock(&self, ms: i64) {
        self.clock.store(ms, Ordering::SeqCst);
    }

    async fn tick(&self) {
        routines::handle(&self.master)
            .unwrap()
            .tick(&self.master)
            .await;
    }

    async fn sweep(&self) {
        routines::handle(&self.master)
            .unwrap()
            .sweep_timeouts(&self.master)
            .await;
    }
}

async fn send(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: (&str, &str),
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new()
        .request(method, format!("{}{path}", t.base))
        .header(auth.0, auth.1)
        .same_origin();
    if auth.0 == "cookie"
        && let Some(k) = common::key_of(auth.1)
    {
        rb = rb.header(common::LOGIN_HEADER, k);
    }
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    send(t, method, path, body, ("cookie", c)).await
}

async fn token(t: &T, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    send(t, method, path, body, ("authorization", "Bearer tok")).await
}

/// The cookie a login answer set; its login key is recorded (P-1).
async fn cookie(r: reqwest::Response) -> String {
    common::signed_in(r).await
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    rb.send().await.unwrap()
}

async fn user_id(t: &T, c: &str) -> String {
    let (_, me) = req(t, Method::GET, "/api/me", None, c).await;
    me["user"]["user_id"].as_str().unwrap().to_string()
}

/// The admin "alex" (agentic allowed), from the claim.
async fn alex(t: &T) -> (String, String) {
    let body = json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"});
    let c = cookie(post_anon(t, "/api/setup", body, None).await).await;
    let id = user_id(t, &c).await;
    (c, id)
}

/// A member account `name` made by `admin`, signed in with its own password.
async fn account(t: &T, admin: &str, name: &str, agentic: bool) -> (String, String) {
    let body = json!({"username": name, "can_agentic": agentic});
    let (st, v) = req(t, Method::POST, "/api/users", Some(body), admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let login = json!({"username": name, "password": pw});
    let first = post_anon(t, "/api/login", login, None).await;
    let change = json!({"current": pw, "new": format!("{name}s own password")});
    let c =
        cookie(post_anon(t, "/api/me/password", change, Some(&cookie(first).await)).await).await;
    let id = user_id(t, &c).await;
    (c, id)
}

async fn login(t: &T, name: &str, password: &str) -> String {
    let body = json!({"username": name, "password": password});
    cookie(post_anon(t, "/api/login", body, None).await).await
}

/// A team workspace "Acme" owned by `owner`, with `members` in it.
async fn acme(t: &T, owner: &str, members: &[(&str, &str)]) -> String {
    let (st, w) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        owner,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let ws = w["workspace_id"].as_str().unwrap().to_string();
    for (name, role) in members {
        let body = json!({"username": name, "role": role});
        let path = format!("/api/workspaces/{ws}/members");
        let (st, v) = req(t, Method::POST, &path, Some(body), owner).await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    ws
}

fn chat_spec(prompt: &str) -> Value {
    json!({"profile": "chat", "agent_id": "general", "prompt": prompt})
}

fn every(minutes: i64) -> Value {
    json!([{"kind": "schedule", "config": {"type": "interval", "every_minutes": minutes}}])
}

async fn create(
    t: &T,
    c: &str,
    ws: Option<&str>,
    name: &str,
    spec: Value,
    triggers: Value,
) -> (StatusCode, Value) {
    let mut body = json!({"name": name, "spec": spec, "triggers": triggers});
    if let Some(ws) = ws {
        body["workspace_id"] = json!(ws);
    }
    req(t, Method::POST, "/api/routines", Some(body), c).await
}

/// Create a routine that must be accepted; its id.
async fn routine(
    t: &T,
    c: &str,
    ws: Option<&str>,
    name: &str,
    spec: Value,
    triggers: Value,
) -> String {
    let (st, v) = create(t, c, ws, name, spec, triggers).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    v["routine"]["routine_id"].as_str().unwrap().to_string()
}

async fn run_now(t: &T, c: &str, id: &str) -> (StatusCode, Value) {
    req(t, Method::POST, &format!("/api/routines/{id}/run"), None, c).await
}

async fn started(t: &T, c: &str, id: &str) -> String {
    let (st, v) = run_now(t, c, id).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    v["run_id"].as_str().expect("a run started").to_string()
}

async fn run_of(t: &T, c: &str, id: &str, run: &str) -> Value {
    let (st, v) = req(
        t,
        Method::GET,
        &format!("/api/routines/{id}/runs/{run}"),
        None,
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

/// Poll run `run` until its status is `status`.
async fn wait_status(t: &T, c: &str, id: &str, run: &str, status: &str) -> Value {
    for _ in 0..500 {
        let v = run_of(t, c, id, run).await;
        if v["status"] == status {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("run never became {status}: {}", run_of(t, c, id, run).await);
}

async fn runs(t: &T, c: &str, id: &str) -> Vec<Value> {
    let (st, v) = req(t, Method::GET, &format!("/api/routines/{id}/runs"), None, c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["runs"].as_array().unwrap().clone()
}

/// Poll the routine's runs until `pred` holds for them.
async fn wait_runs(t: &T, c: &str, id: &str, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    for _ in 0..500 {
        let list = runs(t, c, id).await;
        if pred(&list) {
            return list;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("runs never matched: {:#?}", runs(t, c, id).await);
}

async fn session_ids(t: &T, path: &str, c: &str) -> Vec<String> {
    let (st, v) = req(t, Method::GET, path, None, c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_id"].as_str().unwrap().to_string())
        .collect()
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
    v["events"].as_array().unwrap().clone()
}

/// The id of the newest approval asked in chat `sid`, once there is one.
async fn approval_in(t: &T, c: &str, sid: &str) -> String {
    for _ in 0..500 {
        if let Some(e) = events(t, c, sid)
            .await
            .iter()
            .rev()
            .find(|e| e["kind"] == "approval_requested")
        {
            return e["payload"]["approval_id"].as_str().unwrap().to_string();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no approval in {sid}");
}

async fn approve(t: &T, c: &str, sid: &str, approval: &str) {
    let path = format!("/api/sessions/{sid}/approvals/{approval}");
    let (st, v) = req(
        t,
        Method::POST,
        &path,
        Some(json!({"decision": "allow"})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn attention(t: &T, c: &str) -> Value {
    let (st, v) = req(t, Method::GET, "/api/attention", None, c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn socket(t: &T, c: &str) -> Socket {
    let mut r = format!("ws://{}/api/ws", t.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", c.parse().unwrap());
    let origin = format!("http://{}", r.uri().authority().unwrap());
    r.headers_mut().insert("origin", origin.parse().unwrap());
    let (mut s, _) = tokio_tungstenite::connect_async(r).await.unwrap();
    s.send(Message::Text(common::socket_login(c).into()))
        .await
        .unwrap();
    s.send(Message::Text("hello".into())).await.unwrap();
    // The server answers the bad message: the socket is listening.
    loop {
        if next_type(&mut s).await == "error" {
            return s;
        }
    }
}

async fn next_type(s: &mut Socket) -> String {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), s.next())
            .await
            .expect("socket went quiet");
        if let Some(Ok(Message::Text(text))) = msg {
            let v: Value = serde_json::from_str(&text).unwrap();
            return v["type"].as_str().unwrap().to_string();
        }
    }
}

/// How many `routines_changed` a socket got before `artifacts_changed`.
async fn routine_topics(s: &mut Socket) -> usize {
    let mut n = 0;
    loop {
        match next_type(s).await.as_str() {
            "routines_changed" => n += 1,
            "artifacts_changed" => return n,
            _ => {}
        }
    }
}

#[tokio::test]
async fn run_now_starts_a_chat_in_the_team_workspace_and_reports_back() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let (bob, _) = account(&t, &alex, "bob", false).await;
    let ws = acme(&t, &alex, &[("bob", "member")]).await;
    let mut a_sock = socket(&t, &alex).await;
    let mut b_sock = socket(&t, &bob).await;

    let prompt = "News for {{user.name}} in {{workspace.name}}, run {{run.number}} ({{run.trigger}}) on {{now.date}}. Then more.";
    let (st, v) = create(
        &t,
        &alex,
        Some(&ws),
        "Daily news",
        chat_spec(prompt),
        every(60),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let r = &v["routine"];
    assert_eq!(r["workspace_id"], ws.as_str());
    assert_eq!(r["enabled"], true);
    assert_eq!(r["paused_reason"], Value::Null);
    assert_eq!(r["blocked"], Value::Null);
    assert_eq!(r["next_run_ms"], t0() + 60 * MIN);
    assert_eq!(r["triggers"][0]["config"]["type"], "interval");
    assert_eq!(r["triggers"][0]["config"]["anchor_ms"], t0());
    assert_eq!(r["spec"]["notify"], "failures");
    assert_eq!(v["secrets"], json!({}));
    let id = r["routine_id"].as_str().unwrap().to_string();

    let run = started(&t, &alex, &id).await;
    let done = wait_status(&t, &alex, &id, &run, "completed").await;
    assert_eq!(done["trigger_kind"], "manual");
    assert_eq!(done["scheduled_ms"], Value::Null);
    assert_eq!(done["started_ms"], t0());
    assert_eq!(done["seen"], false);
    assert_eq!(done["warnings"], json!([]));
    assert_eq!(done["untrusted"], false);
    assert!(
        done["usage"]["input_tokens"].as_u64().unwrap() > 0,
        "{done}"
    );
    let rendered = "News for alex in Acme, run 1 (manual) on 2026-11-08. Then more.";
    assert_eq!(done["prompt"], rendered);
    // The answer echoes the prompt; its first sentence is the preview.
    assert_eq!(
        done["answer_preview"],
        "You said: News for alex in Acme, run 1 (manual) on 2026-11-08."
    );
    let sid = done["session_id"].as_str().unwrap().to_string();

    // The chat: the owner's, in the team workspace, a fixed title, the
    // rendered prompt as its message, marked as the run's.
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["workspace_id"], ws.as_str());
    assert_eq!(row["title"], "Daily news · 2026-11-08 09:30");
    assert_eq!(row["agent_id"], "general");
    assert_eq!(
        row["routine"],
        json!({"routine_id": id, "name": "Daily news", "run_id": run, "status": "completed",
               "seen": false, "promoted": false, "untrusted": false})
    );
    let evs = events(&t, &alex, &sid).await;
    let msg = evs.iter().find(|e| e["kind"] == "user_message").unwrap();
    assert_eq!(msg["payload"]["text"], rendered);
    // Bob never sees it.
    let (st, _) = req(&t, Method::GET, &format!("/api/sessions/{sid}"), None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(&t, Method::GET, &format!("/api/routines/{id}"), None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // The result: an attention row, listed in Recent while unseen.
    let att = attention(&t, &alex).await;
    let rows = att["routines"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{att}");
    assert_eq!(rows[0]["kind"], "result");
    assert_eq!(rows[0]["run_id"], run.as_str());
    assert_eq!(rows[0]["session_id"], sid.as_str());
    assert_eq!(rows[0]["workspace_id"], ws.as_str());
    assert_eq!(rows[0]["unseen"], 1);
    assert_eq!(rows[0]["notify"], "failures");
    assert_eq!(attention(&t, &bob).await["routines"], json!([]));
    let list = format!("/api/sessions?workspace={ws}");
    assert!(session_ids(&t, &list, &alex).await.contains(&sid));
    let (_, r) = req(&t, Method::GET, &format!("/api/routines/{id}"), None, &alex).await;
    assert_eq!(r["unseen"], 1);
    assert_eq!(r["last_run"]["run_id"], run.as_str());

    // Seen: the row clears and the chat leaves the list (kept with
    // `routine_runs=1`).
    let path = format!("/api/routines/{id}/runs/{run}/seen");
    let (st, _) = req(&t, Method::POST, &path, None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::POST, &path, None, &alex).await;
    assert_eq!(st, StatusCode::OK, "idempotent");
    let (st, _) = req(&t, Method::POST, &path, None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(attention(&t, &alex).await["routines"], json!([]));
    assert!(!session_ids(&t, &list, &alex).await.contains(&sid));
    let all = format!("/api/sessions?workspace={ws}&routine_runs=1");
    assert!(session_ids(&t, &all, &alex).await.contains(&sid));

    // Live: alex heard of it, bob did not.
    t.master.changed("artifacts", None);
    assert!(routine_topics(&mut a_sock).await > 0);
    assert_eq!(routine_topics(&mut b_sock).await, 0);

    // Continuing the chat brings it back to the list for good.
    let path = format!("/api/sessions/{sid}/prompt");
    let (st, v) = req(
        &t,
        Method::POST,
        &path,
        Some(json!({"text": "and more?"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(session_ids(&t, &list, &alex).await.contains(&sid));
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["routine"]["promoted"], true);
    assert_eq!(
        row["routine"]["status"], "completed",
        "a later turn is not the run's"
    );

    // Deleting the chat leaves the run without its text.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let gone = run_of(&t, &alex, &id, &run).await;
    assert_eq!(gone["chat_deleted"], true);
    assert_eq!(gone["session_id"], Value::Null);
    assert_eq!(gone["prompt"], Value::Null);
    assert_eq!(gone["status"], "completed");
    assert!(gone["usage"]["input_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn saving_checks_triggers_specs_and_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let spec = chat_spec("hello");
    let sched = |expr: &str| json!({"kind": "schedule", "config": {"type": "cron", "expr": expr}});
    let hook = json!({"kind": "webhook", "config": {}});
    let refused = |want: StatusCode, v: (StatusCode, Value)| {
        assert_eq!(v.0, want, "{}", v.1);
        v.1["error"].as_str().unwrap().to_string()
    };

    let e = refused(
        StatusCode::BAD_REQUEST,
        create(
            &t,
            &alex,
            None,
            "x",
            spec.clone(),
            json!([sched("0 8 * * *"), hook]),
        )
        .await,
    );
    assert_eq!(e, "A routine runs on a schedule or on one event, not both");
    let e = refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", spec.clone(), json!([hook, hook])).await,
    );
    assert!(e.contains("one webhook"), "{e}");
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", spec.clone(), json!([])).await,
    );
    let nine: Vec<Value> = (0..9).map(|h| sched(&format!("0 {h} * * *"))).collect();
    refused(
        StatusCode::CONFLICT,
        create(&t, &alex, None, "x", spec.clone(), json!(nine)).await,
    );
    let e = refused(
        StatusCode::BAD_REQUEST,
        create(
            &t,
            &alex,
            None,
            "x",
            spec.clone(),
            json!([sched("* * * * *")]),
        )
        .await,
    );
    assert!(e.contains("5 minutes"), "{e}");
    refused(
        StatusCode::BAD_REQUEST,
        create(
            &t,
            &alex,
            None,
            "x",
            spec.clone(),
            json!([sched("0 0 8 * * *")]),
        )
        .await,
    );

    // The prompt: event.* is for webhook routines; errors say where.
    let (st, v) = create(
        &t,
        &alex,
        None,
        "x",
        chat_spec("Hi {{event.text}}"),
        every(60),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["offset"], 3);
    let (st, v) = create(&t, &alex, None, "x", chat_spec("{{user.name"), every(60)).await;
    assert_eq!(
        (st, v["offset"].clone()),
        (StatusCode::BAD_REQUEST, json!(0))
    );

    // The spec.
    let mut extra = spec.clone();
    extra["trusted_tools"] = json!(["bash"]);
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", extra, every(60)).await,
    );
    let auto = json!({"profile": "agentic", "agent_id": "coder", "prompt": "x",
                      "node_id": "local", "cwd": t.root, "approval_mode": "auto"});
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", auto, every(60)).await,
    );
    let e = refused(
        StatusCode::BAD_REQUEST,
        create(
            &t,
            &alex,
            None,
            "x",
            json!({"profile": "chat", "agent_id": "nobody", "prompt": "x"}),
            every(60),
        )
        .await,
    );
    assert!(e.contains("nobody"), "{e}");
    let nowhere = json!({"profile": "agentic", "agent_id": "coder", "prompt": "x",
                         "node_id": "elsewhere", "cwd": t.root});
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", nowhere, every(60)).await,
    );
    let chat_node = json!({"profile": "agentic", "agent_id": "coder", "prompt": "x",
                           "node_id": "master", "cwd": t.root});
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", chat_node, every(60)).await,
    );
    let mut project = spec.clone();
    project["project_id"] = json!("0193f1c2a4b57d6e8f9012345678abcd");
    refused(
        StatusCode::BAD_REQUEST,
        create(&t, &alex, None, "x", project, every(60)).await,
    );
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/routines",
        Some(json!({"name": "x", "spec": spec, "triggers": [], "color": 1})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Names are the owner's own per workspace.
    let id = routine(
        &t,
        &alex,
        None,
        "Morning",
        spec.clone(),
        json!([sched("0 8 * * *")]),
    )
    .await;
    refused(
        StatusCode::CONFLICT,
        create(&t, &alex, None, "morning", spec.clone(), every(60)).await,
    );

    // An edit keeps the ids of the triggers it names.
    let (_, r) = req(&t, Method::GET, &format!("/api/routines/{id}"), None, &alex).await;
    let tid = r["triggers"][0]["trigger_id"].as_str().unwrap().to_string();
    assert_eq!(tid.len(), 32);
    let triggers = json!([
        {"trigger_id": tid, "kind": "schedule", "config": {"type": "cron", "expr": "0 9 * * *", "tz": "Europe/Riga"}},
        {"kind": "schedule", "config": {"type": "once", "at": "2026-12-24T18:00"}}
    ]);
    let path = format!("/api/routines/{id}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"triggers": triggers, "name": "Mornings"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let r = &v["routine"];
    assert_eq!(r["name"], "Mornings");
    assert_eq!(r["triggers"][0]["trigger_id"], tid.as_str());
    assert_eq!(r["triggers"][0]["config"]["tz"], "Europe/Riga");
    assert_eq!(
        r["triggers"][1]["config"]["tz"], "UTC",
        "the owner's zone, else UTC"
    );
    // 09:00 Riga on Monday 9 November = 07:00 UTC.
    let monday = "2026-11-09T07:00:00Z".parse::<jiff::Timestamp>().unwrap();
    assert_eq!(r["next_run_ms"], monday.as_millisecond());
    let bad = json!([{"trigger_id": "f".repeat(32), "kind": "schedule", "config": {"type": "cron", "expr": "0 9 * * *"}}]);
    let (st, _) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"triggers": bad})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // The list says what the editor needs.
    let (_, l) = req(&t, Method::GET, "/api/routines", None, &alex).await;
    assert_eq!(l["can_create"], true);
    assert_eq!(l["can_agentic"], true);
    assert_eq!(l["why"], Value::Null);
    assert_eq!(l["sources"][0]["kind"], "webhook");
    assert_eq!(l["sources"][0]["free_prefix"], "event.body");
    assert_eq!(l["orphaned"], false);
    assert_eq!(l["limits"]["max_triggers"], 8);
    assert_eq!(l["variables"][0]["namespace"], "now");
    assert_eq!(l["routines"].as_array().unwrap().len(), 1);

    let (st, _) = req(&t, Method::DELETE, &path, None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::GET, &path, None, &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn routines_are_their_owners_alone() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let (bob, bob_id) = account(&t, &alex, "bob", false).await;
    let (gus, _) = account(&t, &alex, "gus", false).await;
    let ws = acme(&t, &alex, &[("bob", "member"), ("gus", "guest")]).await;

    // A guest may not; nor agentic work without the account's grant.
    let (st, v) = create(&t, &gus, Some(&ws), "g", chat_spec("hi"), every(60)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (_, f) = req(
        &t,
        Method::GET,
        &format!("/api/features?workspace={ws}"),
        None,
        &gus,
    )
    .await;
    assert_eq!(f["routines"]["show"], false, "{f}");
    assert_eq!(f["routines"]["why"], "Guests can't run routines in Acme");
    let (_, f) = req(
        &t,
        Method::GET,
        &format!("/api/features?workspace={ws}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(
        f["routines"],
        json!({"enabled": true, "show": true, "webhooks": true, "why": null})
    );
    let agentic = json!({"profile": "agentic", "agent_id": "coder", "prompt": "x",
                         "node_id": "local", "cwd": t.root});
    let (st, v) = create(&t, &bob, None, "a", agentic, every(60)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");

    // Bob's routine: alex, an admin and the workspace's owner, never sees it.
    let id = routine(&t, &bob, Some(&ws), "Bob's", chat_spec("hi"), every(60)).await;
    let path = format!("/api/routines/{id}");
    for (method, p) in [
        (Method::GET, path.clone()),
        (Method::PATCH, path.clone()),
        (Method::DELETE, path.clone()),
        (Method::POST, format!("{path}/run")),
        (Method::GET, format!("{path}/runs")),
    ] {
        let body = (method == Method::PATCH).then(|| json!({"name": "mine"}));
        let (st, _) = req(&t, method.clone(), &p, body, &alex).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{method} {p}");
    }
    let (_, l) = req(
        &t,
        Method::GET,
        &format!("/api/routines?workspace={ws}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(l["routines"], json!([]));
    let body = json!({"routine_id": id, "prompt": "x"});
    let (st, _) = req(&t, Method::POST, "/api/routines/preview", Some(body), &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(
        &t,
        Method::GET,
        "/api/routines?workspace=0193f1c2a4b57d6e8f9012345678abcd",
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "not a member");

    // The API token reads every routine and owns none.
    let (st, l) = token(&t, Method::GET, "/api/routines", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(l["routines"].as_array().unwrap().len(), 1);
    assert_eq!(l["can_create"], false);
    let (st, _) = token(&t, Method::GET, &path, None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = token(&t, Method::POST, &format!("{path}/run"), None).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let body =
        json!({"workspace_id": ws, "name": "t", "spec": chat_spec("x"), "triggers": every(60)});
    let (st, _) = token(&t, Method::POST, "/api/routines", Some(body)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Demoted to guest: listed as orphaned, may delete but not change.
    let role = format!("/api/workspaces/{ws}/members/{bob_id}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &role,
        Some(json!({"role": "guest"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, l) = req(
        &t,
        Method::GET,
        &format!("/api/routines?workspace={ws}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(l["can_create"], false);
    assert_eq!(l["orphaned"], true);
    assert_eq!(l["why"], "Guests can't run routines in Acme");
    let blocked = l["routines"][0]["blocked"].as_str().unwrap();
    assert!(blocked.contains("routines"), "{blocked}");
    let (_, f) = req(
        &t,
        Method::GET,
        &format!("/api/features?workspace={ws}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(f["routines"]["show"], true, "{f}");
    let (st, _) = req(
        &t,
        Method::PATCH,
        &path,
        Some(json!({"enabled": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = run_now(&t, &bob, &id).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = req(&t, Method::DELETE, &path, None, &bob).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn an_interval_fires_across_a_restart_and_catches_up() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("master.db");
    let clock = Arc::new(AtomicI64::new(t0()));
    let open = || Store::open(&db, Durability::Normal).unwrap();
    let t = serve(open(), dir.path(), clock.clone(), true).await;
    let (alex, _) = alex(&t).await;
    let skip = routine(
        &t,
        &alex,
        None,
        "Every five",
        chat_spec("tick {{run.number}}"),
        every(5),
    )
    .await;
    let mut once_spec = chat_spec("catch up {{run.scheduled}}");
    once_spec["catch_up"] = json!("once");
    let once = routine(&t, &alex, None, "Catch up", once_spec, every(5)).await;
    let stray = routine(&t, &alex, None, "Stray", chat_spec("x"), every(60)).await;

    t.tick().await;
    assert!(runs(&t, &alex, &skip).await.is_empty(), "nothing due yet");
    t.set_clock(t0() + 5 * MIN);
    t.tick().await;
    for id in [&skip, &once] {
        let list = wait_runs(&t, &alex, id, |r| {
            r.len() == 1 && r[0]["status"] == "completed"
        })
        .await;
        assert_eq!(list[0]["trigger_kind"], "schedule");
        assert_eq!(list[0]["scheduled_ms"], t0() + 5 * MIN);
    }
    let (_, r) = req(
        &t,
        Method::GET,
        &format!("/api/routines/{skip}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(r["next_run_ms"], t0() + 10 * MIN);
    assert_eq!(r["triggers"][0]["last_fired_ms"], t0() + 5 * MIN);
    // A tick at the same moment fires nothing twice.
    t.tick().await;
    assert_eq!(runs(&t, &alex, &skip).await.len(), 1);
    // A run this process never got to start.
    let pending = NewRun {
        run_id: "0193f1c2a4b57d6e8f90123456780000".into(),
        routine_id: stray.clone(),
        trigger_id: None,
        trigger_kind: "manual".into(),
        scheduled_ms: None,
        started_ms: t0() + 5 * MIN,
        status: "pending".into(),
        reason: None,
        repeats: 1,
        event: None,
    };
    t.master.inner_mirror().run_start(&pending).unwrap();

    // A new master on the same files, the clock moved on.
    clock.store(t0() + 10 * MIN, Ordering::SeqCst);
    let dir2 = tempfile::tempdir().unwrap();
    let t2 = serve(open(), dir2.path(), clock.clone(), false).await;
    let alex = login(&t2, "alex", "admin password").await;
    t2.tick().await;
    let left = run_of(&t2, &alex, &stray, &pending.run_id).await;
    assert_eq!(
        left["status"], "interrupted",
        "never retried after a restart"
    );
    // Never started: not the routine's failure (review R-2).
    assert_eq!(
        left["reason"],
        "the server restarted before the run started"
    );
    let r = t2.master.inner_mirror().routine(&stray).unwrap().unwrap();
    assert_eq!(r.consecutive_failures, 0);
    let list = wait_runs(&t2, &alex, &skip, |r| {
        r.len() == 2 && r[0]["status"] == "completed"
    })
    .await;
    assert_eq!(list[0]["scheduled_ms"], t0() + 10 * MIN);
    let evs = events(&t2, &alex, list[0]["session_id"].as_str().unwrap()).await;
    let msg = evs.iter().find(|e| e["kind"] == "user_message").unwrap();
    assert_eq!(msg["payload"]["text"], "tick 2");
    wait_runs(&t2, &alex, &once, |r| {
        r.len() == 2 && r[0]["status"] == "completed"
    })
    .await;

    // Down for 21 minutes: four occurrences went by. One `missed` row for
    // the skipping routine, one late run for the other, and both move past
    // now.
    t2.set_clock(t0() + 31 * MIN);
    t2.tick().await;
    let list = wait_runs(&t2, &alex, &skip, |r| r.len() == 3).await;
    assert_eq!(list[0]["status"], "missed");
    assert_eq!(list[0]["repeats"], 4);
    assert_eq!(list[0]["scheduled_ms"], t0() + 15 * MIN);
    let list = wait_runs(&t2, &alex, &once, |r| {
        r.len() == 3 && r[0]["status"] == "completed"
    })
    .await;
    assert_eq!(list[0]["scheduled_ms"], t0() + 15 * MIN);
    for id in [&skip, &once] {
        let (_, r) = req(
            &t2,
            Method::GET,
            &format!("/api/routines/{id}"),
            None,
            &alex,
        )
        .await;
        assert_eq!(r["next_run_ms"], t0() + 35 * MIN);
    }
    // Missed runs are no result to look at.
    let att = attention(&t2, &alex).await;
    assert!(
        att["routines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] != "missed"),
        "{att}"
    );

    // Disabled routines do not fire; enabling starts again from now.
    let path = format!("/api/routines/{skip}");
    req(
        &t2,
        Method::PATCH,
        &path,
        Some(json!({"enabled": false})),
        &alex,
    )
    .await;
    t2.set_clock(t0() + 50 * MIN);
    t2.tick().await;
    assert_eq!(runs(&t2, &alex, &skip).await.len(), 3);
    let (_, v) = req(
        &t2,
        Method::PATCH,
        &path,
        Some(json!({"enabled": true})),
        &alex,
    )
    .await;
    assert_eq!(v["routine"]["next_run_ms"], t0() + 55 * MIN);
}

#[tokio::test]
async fn agentic_runs_wait_for_approval_and_time_out() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let spec = |agent: &str, prompt: &str| {
        json!({"profile": "agentic", "agent_id": agent, "prompt": prompt,
               "node_id": "local", "cwd": t.root})
    };
    let id = routine(
        &t,
        &alex,
        None,
        "Check",
        spec("coder", "run: echo hi"),
        every(60),
    )
    .await;
    let run = started(&t, &alex, &id).await;
    let waiting = wait_status(&t, &alex, &id, &run, "waiting").await;
    let sid = waiting["session_id"].as_str().unwrap().to_string();
    // An ordinary chat waiting for its owner.
    let att = attention(&t, &alex).await;
    let item = att["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["session_id"] == sid.as_str())
        .unwrap_or_else(|| panic!("{att}"));
    assert_eq!(item["reason"], "approval");
    assert_eq!(att["routines"], json!([]), "not a result yet");
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["routine"]["status"], "waiting");
    // Fires are skipped while a run waits (and the skips fold).
    for _ in 0..2 {
        let (st, v) = run_now(&t, &alex, &id).await;
        assert_eq!(
            (st, v),
            (StatusCode::ACCEPTED, json!({"skipped": "overlap"}))
        );
    }
    let list = runs(&t, &alex, &id).await;
    assert_eq!(list.len(), 2);
    assert_eq!(
        (list[0]["status"].clone(), list[0]["repeats"].clone()),
        (json!("skipped"), json!(2))
    );
    // Approving lets it finish.
    let aid = approval_in(&t, &alex, &sid).await;
    approve(&t, &alex, &sid, &aid).await;
    wait_status(&t, &alex, &id, &run, "completed").await;

    // Nobody answers for 24 hours: cancelled, recorded as timed out.
    let run = started(&t, &alex, &id).await;
    let waiting = wait_status(&t, &alex, &id, &run, "waiting").await;
    let sid = waiting["session_id"].as_str().unwrap().to_string();
    t.set_clock(t0() + 23 * 60 * MIN);
    t.sweep().await;
    assert_eq!(run_of(&t, &alex, &id, &run).await["status"], "waiting");
    t.set_clock(t0() + 24 * 60 * MIN);
    t.sweep().await;
    let out = run_of(&t, &alex, &id, &run).await;
    assert_eq!(out["status"], "timed_out");
    assert_eq!(out["reason"], "waited 24 hours for an approval");
    for _ in 0..500 {
        let (_, row) = req(
            &t,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            &alex,
        )
        .await;
        if row["running"] == false {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["running"], false, "the turn was cancelled");
    assert_eq!(run_of(&t, &alex, &id, &run).await["status"], "timed_out");
    let att = attention(&t, &alex).await;
    assert_eq!(att["routines"][0]["kind"], "failed", "{att}");
    assert_eq!(att["routines"][0]["status"], "timed_out");

    // A delegate's approval parks the run too. Coder can change files, so
    // it needs a repository for its worktree, or it is not started.
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&t.root)
            .status()
            .unwrap()
            .success()
    );
    let deleg = routine(
        &t,
        &alex,
        None,
        "Orchestrate",
        spec("teamlead", "delegate: coder=run: echo hi"),
        every(60),
    )
    .await;
    let run = started(&t, &alex, &deleg).await;
    let waiting = wait_status(&t, &alex, &deleg, &run, "waiting").await;
    let parent = waiting["session_id"].as_str().unwrap().to_string();
    let (_, all) = req(&t, Method::GET, "/api/sessions?routine_runs=1", None, &alex).await;
    let kid = all["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["parent_id"] == parent.as_str())
        .unwrap_or_else(|| panic!("{all}"))["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let aid = approval_in(&t, &alex, &kid).await;
    approve(&t, &alex, &kid, &aid).await;
    wait_status(&t, &alex, &deleg, &run, "completed").await;
}

#[tokio::test]
async fn a_long_run_times_out_on_its_own_limit() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let mut spec = chat_spec("slow: 2000");
    spec["max_runtime_min"] = json!(1);
    let id = routine(&t, &alex, None, "Long", spec, every(60)).await;
    let run = started(&t, &alex, &id).await;
    let v = wait_status(&t, &alex, &id, &run, "running").await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    // Wait for its turn to start, then a minute passes.
    for _ in 0..500 {
        if run_of(&t, &alex, &id, &run).await["status"] == "running"
            && events(&t, &alex, &sid)
                .await
                .iter()
                .any(|e| e["kind"] == "turn_started")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    t.sweep().await;
    assert_eq!(run_of(&t, &alex, &id, &run).await["status"], "running");
    t.set_clock(t0() + MIN);
    t.sweep().await;
    let out = run_of(&t, &alex, &id, &run).await;
    assert_eq!(out["status"], "timed_out");
    assert_eq!(out["reason"], "ran past its limit of 1 min");
}

#[tokio::test]
async fn losing_access_blocks_and_pauses_the_routine() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let (bob, bob_id) = account(&t, &alex, "bob", false).await;
    let ws = acme(&t, &alex, &[("bob", "member")]).await;
    let id = routine(&t, &bob, Some(&ws), "Bob's", chat_spec("hello"), every(5)).await;
    let slow = routine(
        &t,
        &bob,
        Some(&ws),
        "Slow",
        chat_spec("slow: 300"),
        every(60),
    )
    .await;

    // A chat run already going when the access goes finishes its turn.
    let run = started(&t, &bob, &slow).await;
    let v = wait_status(&t, &bob, &slow, &run, "running").await;
    assert!(v["session_id"].is_string());
    let before = t.master.inner_mirror().audit_count().unwrap();
    let role = format!("/api/workspaces/{ws}/members/{bob_id}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &role,
        Some(json!({"role": "guest"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_status(&t, &bob, &slow, &run, "completed").await;

    // The next fire is refused: blocked, the routine paused, the owner told.
    t.set_clock(t0() + 5 * MIN);
    t.tick().await;
    let list = wait_runs(&t, &bob, &id, |r| {
        r.len() == 1 && r[0]["status"] == "blocked"
    })
    .await;
    let reason = list[0]["reason"].as_str().unwrap();
    assert!(reason.contains("cannot run routines"), "{reason}");
    assert_eq!(list[0]["session_id"], Value::Null);
    let (_, r) = req(&t, Method::GET, &format!("/api/routines/{id}"), None, &bob).await;
    assert_eq!(r["paused_reason"], "blocked");
    assert_eq!(r["next_run_ms"], Value::Null);
    let att = attention(&t, &bob).await;
    let rows = att["routines"].as_array().unwrap();
    let kinds: Vec<&str> = rows
        .iter()
        .filter(|r| r["routine_id"] == id.as_str())
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"paused") && kinds.contains(&"failed"),
        "{att}"
    );
    let paused = rows.iter().find(|r| r["kind"] == "paused").unwrap();
    assert!(
        paused["reason"]
            .as_str()
            .unwrap()
            .contains("cannot run routines")
    );
    assert!(t.master.inner_mirror().audit_count().unwrap() >= before + 2);
    // Paused: it no longer fires.
    t.set_clock(t0() + 10 * MIN);
    t.tick().await;
    assert_eq!(runs(&t, &bob, &id).await.len(), 1);

    // Back as a member, re-enabling resumes it.
    req(
        &t,
        Method::PATCH,
        &role,
        Some(json!({"role": "member"})),
        &alex,
    )
    .await;
    let path = format!("/api/routines/{id}/run");
    let (st, v) = req(&t, Method::POST, &path, None, &bob).await;
    assert_eq!(st, StatusCode::CONFLICT, "paused: {v}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/routines/{id}"),
        Some(json!({"enabled": true})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["routine"]["paused_reason"], Value::Null);
    assert_eq!(v["routine"]["next_run_ms"], t0() + 15 * MIN);
    let att = attention(&t, &bob).await;
    assert!(
        att["routines"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["kind"] != "paused")
    );
}

#[tokio::test]
async fn caps_hold_at_save_and_at_run_time() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    // Two of the owner's runs at a time: a third is skipped.
    let mut ids = Vec::new();
    for i in 0..3 {
        ids.push(
            routine(
                &t,
                &alex,
                None,
                &format!("slow {i}"),
                chat_spec("slow: 300"),
                every(60),
            )
            .await,
        );
    }
    let first = started(&t, &alex, &ids[0]).await;
    started(&t, &alex, &ids[1]).await;
    let (st, v) = run_now(&t, &alex, &ids[2]).await;
    assert_eq!((st, v), (StatusCode::ACCEPTED, json!({"skipped": "cap"})));
    let list = runs(&t, &alex, &ids[2]).await;
    assert_eq!(list[0]["status"], "skipped");
    assert!(
        list[0]["reason"]
            .as_str()
            .unwrap()
            .contains("already running")
    );
    wait_status(&t, &alex, &ids[0], &first, "completed").await;

    // 25 routines per account.
    for i in 3..25 {
        routine(&t, &alex, None, &format!("r{i}"), chat_spec("x"), every(60)).await;
    }
    let (st, v) = create(&t, &alex, None, "one more", chat_spec("x"), every(60)).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
}

#[tokio::test]
async fn the_preview_shows_runs_and_the_rendered_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let preview = |body: Value| {
        let t = &t;
        let alex = alex.clone();
        async move {
            let (st, v) = req(t, Method::POST, "/api/routines/preview", Some(body), &alex).await;
            assert_eq!(st, StatusCode::OK, "{v}");
            v
        }
    };
    let v = preview(json!({
        "prompt": "Hi {{user.name}} at {{now.time}}, run {{run.number}} for {{run.scheduled}}",
        "triggers": every(60),
    }))
    .await;
    let next: Vec<i64> = (1..=5).map(|h| t0() + h * 60 * MIN).collect();
    assert_eq!(v["next_runs"], json!(next));
    assert_eq!(v["chats_per_day"], 24.0);
    assert_eq!(
        v["rendered"],
        "Hi alex at 09:30, run 1 for 2026-11-08 10:30"
    );
    assert_eq!(v["errors"], json!([]));
    assert_eq!(v["untrusted"], false);
    assert_eq!(v["event_paths"], json!([]));

    let v = preview(json!({
        "prompt": "Morning {{now.time}}",
        "triggers": [{"kind": "schedule", "config": {"type": "cron", "expr": "0 8 * * 1-5", "tz": "Europe/Riga"}}],
    }))
    .await;
    let monday = "2026-11-09T06:00:00Z".parse::<jiff::Timestamp>().unwrap();
    assert_eq!(v["next_runs"][0], monday.as_millisecond());
    assert_eq!(v["chats_per_day"], 0.7);
    assert_eq!(v["rendered"], "Morning 11:30", "in the schedule's zone");

    let v = preview(json!({"prompt": "Bad {{nope.x}} here", "triggers": []})).await;
    assert_eq!(v["errors"][0]["offset"], 4);
    assert_eq!(v["rendered"], Value::Null);
    assert_eq!(v["chats_per_day"], Value::Null);
    let v = preview(json!({"prompt": "x", "triggers": [{"kind": "schedule", "config": {"type": "cron", "expr": "* * * * *"}}]})).await;
    assert!(
        v["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("schedule 1"),
        "{v}"
    );
    assert_eq!(v["errors"][0]["offset"], Value::Null);

    // A saved routine's own values.
    let id = routine(
        &t,
        &alex,
        None,
        "Saved",
        chat_spec("{{routine.name}} #{{run.number}}"),
        every(30),
    )
    .await;
    let v = preview(json!({"routine_id": id})).await;
    assert_eq!(v["rendered"], "Saved #1");
    assert_eq!(v["next_runs"][0], t0() + 30 * MIN);
}

/// An agentic node "gone" that said hello and left at once.
async fn offline_node(t: &T) {
    use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
    let (to_node, _commands) = tokio::sync::mpsc::channel::<MasterToNode>(1024);
    let (to_master, from_node) = tokio::sync::mpsc::channel::<NodeToMaster>(1024);
    to_master
        .try_send(NodeToMaster::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: "gone".into(),
            hostname: "h".into(),
            version: "0".into(),
            capabilities: vec!["exec:sandbox".into()],
            sessions: vec![],
        })
        .unwrap();
    drop(to_master);
    let link = harness_node::NodeLink { to_node, from_node };
    t.master.add_link(link, false).await.unwrap();
    for _ in 0..500 {
        if t.master
            .nodes()
            .iter()
            .any(|n| n.node_id == "gone" && !n.connected)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the node never left");
}

/// Poll run `run` until it is on attempt `n`.
async fn wait_attempt(t: &T, c: &str, id: &str, run: &str, n: i64) -> Value {
    for _ in 0..500 {
        let v = run_of(t, c, id, run).await;
        if v["attempt"] == n {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "run never reached attempt {n}: {}",
        run_of(t, c, id, run).await
    );
}

/// Review L-k: a routine's chat always asks for the sandbox mode, whoever
/// owns the routine (an admin here), however the node is set up: at a node
/// that offers only the host shell the run is blocked and makes no chat; at one
/// that offers both, its chat records `sandbox`.
#[tokio::test]
async fn a_routines_chat_is_always_in_the_sandbox() {
    use harness_node::exec::ExecOffers;
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    for (id, offers) in [
        (
            "node-hostonly1",
            ExecOffers::new(Err("bubblewrap not found".into()), Ok(()), vec![]),
        ),
        ("node-both1", ExecOffers::new(Ok(()), Ok(()), vec![])),
    ] {
        let node = NodeRuntime::start(NodeConfig {
            node_id: id.into(),
            hostname: id.into(),
            policy: NodePolicy {
                workspace_roots: vec![t.root.clone().into()],
                exec: offers,
                ..Default::default()
            },
            store: Store::open_in_memory().unwrap(),
            backend: Arc::new(NativeBackend::new(
                Arc::new(Mock {
                    delay: Duration::from_millis(5),
                }),
                harness_tools::builtin(),
                NativeConfig::default(),
            )),
            default_model: "mock".into(),
            secrets: Default::default(),
            profile: Profile::Agentic,
            incognito_ttl: None,
            checkpoints_dir: None,
            git_sandbox: None,
            artifacts: None,
        })
        .await
        .unwrap();
        t.master.add_link(node.attach().await, false).await.unwrap();
    }
    let spec = |node: &str| {
        json!({"profile": "agentic", "agent_id": "coder", "prompt": "hello",
               "node_id": node, "cwd": t.root})
    };
    let id = routine(
        &t,
        &alex,
        None,
        "Host only",
        spec("node-hostonly1"),
        every(60),
    )
    .await;
    let run = started(&t, &alex, &id).await;
    let v = wait_status(&t, &alex, &id, &run, "blocked").await;
    assert!(
        v["reason"].as_str().unwrap().contains("sandbox mode"),
        "{v}"
    );
    assert_eq!(v["session_id"], Value::Null, "{v}");
    let id = routine(&t, &alex, None, "Both", spec("node-both1"), every(60)).await;
    let run = started(&t, &alex, &id).await;
    let v = wait_status(&t, &alex, &id, &run, "completed").await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    let created = events(&t, &alex, &sid)
        .await
        .into_iter()
        .find(|e| e["kind"] == "session_created")
        .unwrap();
    assert_eq!(created["payload"]["exec_mode"], "sandbox", "{created}");
}

#[tokio::test]
async fn an_offline_node_is_retried_then_fails_and_failures_pause() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    offline_node(&t).await;
    let spec = json!({"profile": "agentic", "agent_id": "coder", "prompt": "run: true",
                      "node_id": "gone", "cwd": t.root});

    // Run now: retried after 1, 5 and 15 minutes, then failed.
    let id = routine(&t, &alex, None, "Away", spec.clone(), every(60)).await;
    let run = started(&t, &alex, &id).await;
    let mut at = t0();
    for (attempt, delay) in [(2, 1), (3, 5), (4, 15)] {
        let v = wait_attempt(&t, &alex, &id, &run, attempt).await;
        assert_eq!(v["status"], "pending", "{v}");
        let reason = v["reason"].as_str().unwrap();
        assert!(
            reason.contains(&format!("trying again in {delay} min")),
            "{reason}"
        );
        // Not before its time.
        t.tick().await;
        assert_eq!(run_of(&t, &alex, &id, &run).await["attempt"], attempt);
        at += delay * MIN;
        t.set_clock(at);
        t.tick().await;
    }
    let v = wait_status(&t, &alex, &id, &run, "failed").await;
    assert!(
        v["reason"].as_str().unwrap().contains("not connected"),
        "{v}"
    );
    assert_eq!(v["session_id"], Value::Null);

    // On a schedule, a retry never runs past the next occurrence; five
    // failures in a row pause the routine.
    let sched = routine(&t, &alex, None, "Every five", spec, every(5)).await;
    let base = at - at.rem_euclid(MIN);
    for k in 1..=5 {
        let fire = base + k * 5 * MIN;
        t.set_clock(fire);
        t.tick().await;
        let list = wait_runs(&t, &alex, &sched, |r| {
            r.len() == k as usize && r[0]["attempt"] == 2 && r[0]["status"] == "pending"
        })
        .await;
        let run = list[0]["run_id"].as_str().unwrap().to_string();
        t.set_clock(fire + MIN);
        t.tick().await;
        let v = wait_status(&t, &alex, &sched, &run, "failed").await;
        assert!(
            v["reason"]
                .as_str()
                .unwrap()
                .contains("the next run is due first"),
            "{v}"
        );
    }
    let (_, r) = req(
        &t,
        Method::GET,
        &format!("/api/routines/{sched}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(r["paused_reason"], "failures");
    let att = attention(&t, &alex).await;
    let paused = att["routines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "paused")
        .unwrap_or_else(|| panic!("{att}"));
    assert_eq!(paused["routine_id"], sched.as_str());
    t.set_clock(base + 30 * MIN);
    t.tick().await;
    assert_eq!(
        runs(&t, &alex, &sched).await.len(),
        5,
        "paused routines do not fire"
    );
}

/// The embedded node is for server admins by default (`[nodes]
/// local_node_users`): an agentic routine there is refused to a member
/// with the agentic grant, as their chats there are.
#[tokio::test]
async fn agentic_routines_on_the_local_node_are_for_admins() {
    let dir = tempfile::tempdir().unwrap();
    let t = start(dir.path()).await;
    let (alex, _) = alex(&t).await;
    let (bob, _) = account(&t, &alex, "bob", true).await;
    let agentic = json!({"profile": "agentic", "agent_id": "coder", "prompt": "x",
                         "node_id": "local", "cwd": t.root});
    let (st, v) = create(&t, &bob, None, "a", agentic.clone(), every(60)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("remote node"), "{v}");
    let (st, v) = create(&t, &alex, None, "a", agentic, every(60)).await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
}

/// Prompts this master's chats were sent, over every chat of the
/// routine's runs: one `turn_started` per prompt that ran.
async fn turns_of_runs(t: &T, c: &str, id: &str) -> usize {
    let mut n = 0;
    for r in runs(t, c, id).await {
        if let Some(sid) = r["session_id"].as_str() {
            n += events(t, c, sid)
                .await
                .iter()
                .filter(|e| e["kind"] == "turn_started")
                .count();
        }
    }
    n
}

/// Review A19-2 and the re-check's R-1 to R-4: a run a stop cut is
/// recorded `interrupted` with the reason, is not counted, and is not run
/// again by itself; the next occurrence runs as scheduled. Five stops in a
/// row (scheduled and manual runs) leave the routine unpaused.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_cut_by_a_stop_is_interrupted_uncounted_and_not_run_again() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("master.db");
    let clock = Arc::new(AtomicI64::new(t0()));
    let open = || Store::open(&db, Durability::Normal).unwrap();
    let mut t = serve(open(), dir.path(), clock.clone(), true).await;
    let (mut alex, _) = alex(&t).await;
    let id = routine(&t, &alex, None, "Long", chat_spec("slow: 400"), every(5)).await;
    let mut kept = Vec::new();
    for stop in 1..=5 {
        // Stops 1 and 2: the schedule's run; then "run now".
        let run = if stop <= 2 {
            t.set_clock(t0() + 5 * MIN * stop);
            t.tick().await;
            let list = wait_runs(&t, &alex, &id, |r| r.len() == stop as usize).await;
            list[0]["run_id"].as_str().unwrap().to_string()
        } else {
            started(&t, &alex, &id).await
        };
        wait_status(&t, &alex, &id, &run, "running").await;
        t.master.begin_stop();
        assert!(t.chat.stop(Duration::from_secs(20)).await.is_empty());
        let v = wait_status(&t, &alex, &id, &run, "interrupted").await;
        assert_eq!(
            v["reason"],
            "the node or the server was stopped while the run's turn ran; it is not run again by itself",
            "stop {stop}"
        );
        let r = t.master.inner_mirror().routine(&id).unwrap().unwrap();
        assert_eq!(
            (r.consecutive_failures, r.paused_reason.as_deref()),
            (0, None),
            "stop {stop}: {v}"
        );
        let turns = turns_of_runs(&t, &alex, &id).await;
        assert_eq!(turns, stop as usize, "stop {stop}");
        // The next start, on the same files: nothing is run again.
        let d = tempfile::tempdir().unwrap();
        let next = serve(open(), d.path(), clock.clone(), false).await;
        kept.push((t, d));
        t = next;
        alex = login(&t, "alex", "admin password").await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(runs(&t, &alex, &id).await.len(), stop as usize);
        assert_eq!(
            turns_of_runs(&t, &alex, &id).await,
            stop as usize,
            "relaunched"
        );
        assert_eq!(run_of(&t, &alex, &id, &run).await["status"], "interrupted");
    }
    // The next occurrence fires as scheduled and completes.
    t.set_clock(t0() + 5 * MIN * 3);
    t.tick().await;
    let list = wait_runs(&t, &alex, &id, |r| {
        r.len() == 6 && r[0]["status"] == "completed"
    })
    .await;
    assert_eq!(list[0]["trigger_kind"], "schedule");
    let r = t.master.inner_mirror().routine(&id).unwrap().unwrap();
    assert_eq!((r.consecutive_failures, r.paused_reason), (0, None));
}

/// Review R-3: a stop in the moment of a run's launch leaves a `running`
/// run whose chat never started its turn; the next start ends it
/// `interrupted`, uncounted, and runs nothing.
#[tokio::test]
async fn a_run_whose_turn_never_started_is_interrupted_uncounted() {
    use harness_store::routines::RunPatch;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("master.db");
    let clock = Arc::new(AtomicI64::new(t0()));
    let open = || Store::open(&db, Durability::Normal).unwrap();
    let t = serve(open(), dir.path(), clock.clone(), true).await;
    let (alex, _) = alex(&t).await;
    let id = routine(&t, &alex, None, "Cut", chat_spec("x"), every(60)).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let run = NewRun {
        run_id: "0193f1c2a4b57d6e8f90123456780001".into(),
        routine_id: id.clone(),
        trigger_id: None,
        trigger_kind: "manual".into(),
        scheduled_ms: None,
        started_ms: t0(),
        status: "pending".into(),
        reason: None,
        repeats: 1,
        event: None,
    };
    let store = t.master.inner_mirror();
    store.run_start(&run).unwrap();
    let patch = RunPatch {
        status: Some("running".into()),
        session_id: Some(sid.clone()),
        ..Default::default()
    };
    assert!(store.run_update(&run.run_id, &patch).unwrap());
    let d = tempfile::tempdir().unwrap();
    let t2 = serve(open(), d.path(), clock.clone(), false).await;
    let alex = login(&t2, "alex", "admin password").await;
    let v = wait_status(&t2, &alex, &id, &run.run_id, "interrupted").await;
    assert_eq!(
        v["reason"],
        "the server stopped before the run's turn started; it is not run again by itself"
    );
    let r = t2.master.inner_mirror().routine(&id).unwrap().unwrap();
    assert_eq!(r.consecutive_failures, 0);
    let evs = events(&t2, &alex, &sid).await;
    assert!(!evs.iter().any(|e| e["kind"] == "turn_started"), "{evs:?}");
    drop(t);
}
