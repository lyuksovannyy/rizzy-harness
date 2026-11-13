//! Who may ask for which execution mode at create (N.10a part 1, the
//! interim rule until the levels per member of part 2): `host` only for the
//! workspace's owner or a server admin, and on the embedded node only for a
//! server admin; without a request the node's default, except that a caller
//! who may not have `host` is sent `sandbox` explicitly. The node decides
//! last (`crates/node/tests/exec_modes.rs`).

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::config::{Config, LocalNodeUsers};
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::exec::ExecOffers;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const TOKEN: &str = "api-secret";
/// Offers `sandbox` and `host`.
const BOTH: &str = "node-both1";
/// Offers only `host` (no bubblewrap there).
const HOST_ONLY: &str = "node-hostonly1";

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    _dir: tempfile::TempDir,
}

async fn node(root: &std::path::Path, id: &str, profile: Profile, exec: ExecOffers) -> NodeRuntime {
    NodeRuntime::start(NodeConfig {
        node_id: id.into(),
        hostname: "h".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.into()],
            exec,
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            match profile {
                Profile::Agentic => harness_tools::builtin(),
                Profile::Chat => Vec::new(),
            },
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap()
}

/// `local` (sandbox only: `host` is off on it), the chat runtime, the two
/// remote workers, and a claimed admin. Every account with the grant may
/// use `local` here, so the `host` rule for it is what is tested.
async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let mut cfg = Config::default();
    cfg.nodes.local_node_users = LocalNodeUsers::Granted;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    let rt = Arc::new(rt);
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
    master.set_provider_runtime(rt.clone());
    let both = || ExecOffers::new(Ok(()), Ok(()), vec![harness_node::exec::GAP_HOST_NETNS]);
    let host_only = || ExecOffers::new(Err("bubblewrap not found".into()), Ok(()), vec![]);
    let local = node(dir.path(), "local", Profile::Agentic, ExecOffers::default()).await;
    let chats = node(dir.path(), "master", Profile::Chat, ExecOffers::default()).await;
    master.add_link(local.attach().await, true).await.unwrap();
    master.add_link(chats.attach().await, true).await.unwrap();
    for (id, o) in [(BOTH, both()), (HOST_ONLY, host_only())] {
        let n = node(dir.path(), id, Profile::Agentic, o).await;
        master.add_link(n.attach().await, false).await.unwrap();
    }
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        ws,
        admin: String::new(),
        _dir: dir,
    };
    let (st, cookie, v) = send(
        &s,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    s.admin = cookie.unwrap();
    s
}

async fn send(
    s: &Server,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    match auth {
        Some(TOKEN) => rb = rb.bearer_auth(TOKEN),
        Some(c) => rb = rb.login(c).same_origin(),
        None => {}
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let body = r.json().await.unwrap_or(Value::Null);
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
}

async fn call(
    s: &Server,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: &str,
) -> (StatusCode, Value) {
    let (st, _, v) = send(s, method, path, body, Some(auth)).await;
    (st, v)
}

/// A non-admin account with the agentic grant, logged in.
async fn account(s: &Server, name: &str) -> String {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "can_agentic": true})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let (_, temp, _) = send(
        s,
        Method::POST,
        "/api/login",
        Some(json!({"username": name, "password": pw})),
        None,
    )
    .await;
    let (st, cookie, v) = send(
        s,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "a password of my own"})),
        temp.as_deref(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    cookie.unwrap()
}

async fn create(
    s: &Server,
    who: &str,
    ws: &str,
    node: &str,
    mode: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({"cwd": s.ws.display().to_string(), "title": "t",
        "workspace_id": ws, "node_id": node});
    if let Some(m) = mode {
        body["exec_mode"] = json!(m);
    }
    call(s, Method::POST, "/api/sessions", Some(body), who).await
}

/// The mode the node recorded in the chat's `session_created`.
async fn recorded(s: &Server, who: &str, sid: &str) -> Value {
    for _ in 0..200 {
        let (_, v) = call(
            s,
            Method::GET,
            &format!("/api/sessions/{sid}/events"),
            None,
            who,
        )
        .await;
        if let Some(e) = v["events"]
            .as_array()
            .and_then(|a| a.iter().find(|e| e["kind"] == "session_created"))
        {
            return e["payload"]["exec_mode"].clone();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no session_created for {sid}");
}

#[tokio::test]
async fn host_is_refused_to_a_plain_member_and_allowed_to_the_owner() {
    let s = start().await;
    let bob = account(&s, "bob").await;
    let carol = account(&s, "carol").await;
    // Bob's team workspace, with Carol a plain member.
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Team"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/workspaces/{team}/members"),
        Some(json!({"username": "carol", "role": "member"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // A plain member: refused, with the reason; the node never hears of it.
    let (st, v) = create(&s, &carol, &team, BOTH, Some("host")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(
        v.to_string()
            .contains("workspace's owner or a server admin"),
        "{v}"
    );
    // Without a mode she gets `sandbox`, asked for explicitly: on a node
    // that offers only `host` she is refused by the node, never handed it.
    let (st, v) = create(&s, &carol, &team, BOTH, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &carol, sid).await, json!("sandbox"));
    let (st, v) = create(&s, &carol, &team, HOST_ONLY, None).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(
        v.to_string().contains("does not offer the sandbox mode"),
        "{v}"
    );

    // The owner (no server admin) may ask for it.
    let (st, v) = create(&s, &bob, &team, BOTH, Some("host")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &bob, sid).await, json!("host"));
    // Without a request, the node's most confined mode.
    let (st, v) = create(&s, &bob, &team, BOTH, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &bob, sid).await, json!("sandbox"));

    // On the embedded node `host` is for server admins only, owner or not;
    // and a node that does not offer it refuses the admin too.
    let (st, v) = create(&s, &bob, &team, "local", Some("host")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("server admins"), "{v}");
    let (_, v) = call(&s, Method::GET, "/api/workspaces", None, &s.admin).await;
    let mine = v["workspaces"][0]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, v) = create(&s, &s.admin, &mine, "local", Some("host")).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(
        v.to_string().contains("does not offer the host mode"),
        "{v}"
    );

    // A chat has none.
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "title": "c", "exec_mode": "sandbox"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

/// What the node announced is in the node list, for the UI to offer and
/// name; unknown capabilities are not passed on.
#[tokio::test]
async fn the_node_list_carries_the_modes_and_gaps_each_node_announced() {
    let s = start().await;
    let (st, v) = call(&s, Method::GET, "/api/nodes", None, &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let find = |id: &str| {
        v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["node_id"] == id)
            .cloned()
            .unwrap_or_else(|| panic!("no {id}: {v}"))
    };
    assert_eq!(find(BOTH)["exec_modes"], json!(["sandbox", "host"]));
    assert_eq!(find(BOTH)["exec_gaps"], json!(["gap:host:netns"]));
    assert_eq!(find(HOST_ONLY)["exec_modes"], json!(["host"]));
    assert_eq!(find("local")["exec_modes"], json!(["sandbox"]));
}
