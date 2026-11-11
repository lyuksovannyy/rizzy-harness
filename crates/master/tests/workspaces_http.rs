//! Workspaces over HTTP: Personal workspaces, team workspaces and their
//! members, the role guards, chats following membership (and staying
//! private), the agentic gate, server-admin views and owner recovery.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim, LOOKUP_MAX_FAILURES};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const TOKEN: &str = "api-secret";

struct Server {
    base: String,
    addr: std::net::SocketAddr,
    ws: std::path::PathBuf,
    admin: String,
    admin_id: String,
    _dir: tempfile::TempDir,
}

/// A node with the mock model: an agentic worker, or the chat runtime.
async fn node(root: &std::path::Path, id: &str, profile: Profile) -> NodeRuntime {
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
        artifacts: None,
    })
    .await
    .unwrap()
}

/// A server with the demo provider (so the workspace policy can be saved),
/// one agentic node, the chat runtime, and a claimed admin.
async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let worker = node(dir.path(), "local", Profile::Agentic).await;
    let chats = node(dir.path(), "master", Profile::Chat).await;
    let mut cfg = Config::default();
    // Non-admins with the agentic grant use the embedded node here; the
    // default (admins only) has its own tests in local_node_http.rs.
    cfg.nodes.local_node_users = harness_master::config::LocalNodeUsers::Granted;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
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
    master.set_provider_runtime(Arc::new(rt));
    master.add_link(worker.attach().await, true).await.unwrap();
    master.add_link(chats.attach().await, true).await.unwrap();
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        addr,
        ws,
        admin: String::new(),
        admin_id: String::new(),
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
    s.admin_id = call(&s, Method::GET, "/api/me", None, &s.admin).await.1["user"]["user_id"]
        .as_str()
        .unwrap()
        .to_string();
    s
}

/// One request. `auth` is a cookie, or `TOKEN` for the API token.
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

/// An account created by the admin, logged in with its own password.
/// Returns (cookie, user id).
async fn user(s: &Server, name: &str, can_agentic: bool) -> (String, String) {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "can_agentic": can_agentic})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user_id"].as_str().unwrap().to_string();
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
    (cookie.unwrap(), id)
}

/// A team workspace created by `owner`; returns its id.
async fn team(s: &Server, owner: &str, name: &str) -> String {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        owner,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["workspace_id"].as_str().unwrap().to_string()
}

async fn add(s: &Server, by: &str, ws: &str, username: &str, role: &str) -> (StatusCode, Value) {
    call(
        s,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({ "username": username, "role": role })),
        by,
    )
    .await
}

async fn personal_id(s: &Server, who: &str) -> String {
    let (_, v) = call(s, Method::GET, "/api/workspaces", None, who).await;
    let first = &v["workspaces"][0];
    assert_eq!(first["personal"], true, "{v}");
    first["workspace_id"].as_str().unwrap().to_string()
}

/// An agentic chat; `ws` is the requested workspace (`None`: the default).
async fn chat(s: &Server, who: &str, ws: Option<&str>, title: &str) -> (StatusCode, Value) {
    let mut body = json!({ "cwd": s.ws.display().to_string(), "title": title });
    if let Some(ws) = ws {
        body["workspace_id"] = json!(ws);
    }
    call(s, Method::POST, "/api/sessions", Some(body), who).await
}

async fn new_chat(s: &Server, who: &str, ws: Option<&str>, title: &str) -> String {
    let (st, v) = chat(s, who, ws, title).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

fn session_ids(v: &Value) -> Vec<String> {
    let mut ids: Vec<String> = v["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|r| r["session_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[tokio::test]
async fn personal_workspace_for_every_account() {
    let s = start().await;
    let (st, v) = call(&s, Method::GET, "/api/workspaces", None, &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let list = v["workspaces"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{v}");
    let p = &list[0];
    assert_eq!(p["personal"], true);
    assert_eq!(p["role"], "owner");
    assert_eq!(
        p["permissions"],
        json!(["agentic", "settings", "members", "routines"])
    );
    assert_eq!(p["members"], 1);
    assert_eq!(v["active"], p["workspace_id"]);
    assert_eq!(v["can_create"], true);
    // /api/me carries the active one, so the UI needs no second call.
    let (_, me) = call(&s, Method::GET, "/api/me", None, &s.admin).await;
    assert_eq!(&me["workspace"], p);

    // A fresh account has its own.
    let (bob, _) = user(&s, "bob", false).await;
    let (_, me) = call(&s, Method::GET, "/api/me", None, &bob).await;
    assert_eq!(me["workspace"]["personal"], true);
    assert_ne!(me["workspace"]["workspace_id"], p["workspace_id"]);
    let bp = me["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, d) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{bp}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["use_memory"], true);
    assert_eq!(
        d["settings"],
        json!({"default_agent": null, "agent_models": {}})
    );
    // Other people's Personal workspaces do not exist for you.
    let alex_p = p["workspace_id"].as_str().unwrap();
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{alex_p}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Personal: fixed name, no settings, no members, never deleted.
    let path = format!("/api/workspaces/{bp}");
    for body in [json!({"name": "Mine"}), json!({"settings": {}})] {
        let (st, v) = call(&s, Method::PATCH, &path, Some(body), &bob).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    }
    let (st, _) = call(&s, Method::DELETE, &path, None, &bob).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, v) = add(&s, &bob, &bp, "alex", "member").await;
    assert_eq!(
        (st, v["error"].as_str()),
        (
            StatusCode::CONFLICT,
            Some("a Personal workspace has only you")
        )
    );
    let (st, _) = call(
        &s,
        Method::PUT,
        &format!("{path}/memory"),
        Some(json!({"use_memory": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // The API token has no workspaces of its own.
    let (_, v) = call(&s, Method::GET, "/api/workspaces", None, TOKEN).await;
    assert_eq!(
        v,
        json!({"workspaces": [], "active": null, "can_create": false})
    );
    let (_, me) = call(&s, Method::GET, "/api/me", None, TOKEN).await;
    assert_eq!(me["workspace"], Value::Null);
    let (st, _) = call(
        &s,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "X"})),
        TOKEN,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn creating_team_workspaces_and_the_policy() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let acme = team(&s, &bob, "  Acme  ").await;
    let (_, v) = call(&s, Method::GET, "/api/workspaces", None, &bob).await;
    let names: Vec<&str> = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Personal", "Acme"], "Personal first, name trimmed");
    assert_eq!(v["workspaces"][1]["role"], "owner");
    for name in ["", "   ", &"x".repeat(65), "bad\nname"] {
        let (st, _) = call(
            &s,
            Method::POST,
            "/api/workspaces",
            Some(json!({"name": name})),
            &bob,
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{name:?}");
    }

    // Choosing it makes it the active one (members only).
    let (st, v) = call(
        &s,
        Method::PUT,
        "/api/me/workspace",
        Some(json!({"workspace_id": acme})),
        &bob,
    )
    .await;
    assert_eq!((st, &v["active"]), (StatusCode::OK, &json!(acme)));
    let (_, me) = call(&s, Method::GET, "/api/me", None, &bob).await;
    assert_eq!(me["workspace"]["workspace_id"], json!(acme));
    let (st, _) = call(
        &s,
        Method::PUT,
        "/api/me/workspace",
        Some(json!({"workspace_id": acme})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "admins are no members");

    // The server's policy: only admins may create.
    let (st, _) = call(
        &s,
        Method::PUT,
        "/api/admin/workspaces/policy",
        Some(json!({"members_can_create": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(
        &s,
        Method::PUT,
        "/api/admin/workspaces/policy",
        Some(json!({"members_can_create": false})),
        &s.admin,
    )
    .await;
    assert_eq!(
        (st, &v["members_can_create"]),
        (StatusCode::OK, &json!(false))
    );
    let (st, _) = call(
        &s,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "B"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(
        call(&s, Method::GET, "/api/workspaces", None, &bob).await.1["can_create"],
        false
    );
    assert_eq!(
        call(&s, Method::GET, "/api/workspaces", None, &s.admin)
            .await
            .1["can_create"],
        true
    );
    team(&s, &s.admin, "Admins").await;
    let (_, v) = call(&s, Method::GET, "/api/admin/workspaces", None, &s.admin).await;
    assert_eq!(v["members_can_create"], false);
    let saved = std::fs::read_to_string(s._dir.path().join(harness_core::CONFIG_FILE)).unwrap();
    assert!(saved.contains("members_can_create = false"), "{saved}");
}

#[tokio::test]
async fn adding_members_by_username() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let (carol, carol_id) = user(&s, "carol", false).await;
    let (dave, _) = user(&s, "dave", false).await;
    let (_, erin_id) = user(&s, "erin", false).await;
    let acme = team(&s, &bob, "Acme").await;

    let (st, v) = add(&s, &bob, &acme, "Carol", "member").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["user_id"], json!(carol_id));
    assert_eq!(
        (v["username"].as_str(), v["role"].as_str()),
        (Some("carol"), Some("member"))
    );
    let (st, v) = add(&s, &bob, &acme, "carol", "guest").await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::CONFLICT, Some("carol is already a member"))
    );
    let (st, _) = add(&s, &bob, &acme, "carol", "boss").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}/members"),
        None,
        &carol,
    )
    .await;
    let names: Vec<&str> = v["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["bob", "carol"]);
    assert!(v["members"][0].get("display_name").is_some());
    let (_, d) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(
        (d["members"].as_u64(), d["role"].as_str()),
        (Some(2), Some("member"))
    );
    assert_eq!(d["permissions"], json!(["agentic", "routines"]));

    // A non-member gets 404 on every workspace route.
    let p = format!("/api/workspaces/{acme}");
    for (method, path, body) in [
        (Method::GET, p.clone(), None),
        (Method::PATCH, p.clone(), Some(json!({"name": "Mine"}))),
        (Method::DELETE, p.clone(), None),
        (
            Method::PUT,
            format!("{p}/memory"),
            Some(json!({"use_memory": false})),
        ),
        (Method::GET, format!("{p}/members"), None),
        (
            Method::POST,
            format!("{p}/members"),
            Some(json!({"username": "dave"})),
        ),
        (
            Method::PATCH,
            format!("{p}/members/{carol_id}"),
            Some(json!({"role": "guest"})),
        ),
        (Method::DELETE, format!("{p}/members/{carol_id}"), None),
        (
            Method::PUT,
            "/api/me/workspace".into(),
            Some(json!({"workspace_id": acme})),
        ),
        (Method::GET, format!("/api/sessions?workspace={acme}"), None),
        (
            Method::GET,
            format!("/api/search?q=x&workspace={acme}"),
            None,
        ),
    ] {
        let (st, v) = call(&s, method.clone(), &path, body, &dave).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{method} {path}: {v}");
    }
    let (st, _) = chat(&s, &dave, Some(&acme), "x").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "membership is checked first");

    // Unknown and disabled usernames read the same, and failures are
    // throttled per caller.
    call(
        &s,
        Method::PATCH,
        &format!("/api/users/{erin_id}"),
        Some(json!({"disabled": true})),
        &s.admin,
    )
    .await;
    let (st, v) = add(&s, &bob, &acme, "erin", "member").await;
    assert_eq!(
        (st, v["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("no such user"))
    );
    for i in 0..9 {
        let (st, _) = add(&s, &bob, &acme, &format!("ghost{i}"), "member").await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }
    let (st, _) = add(&s, &bob, &acme, "dave", "member").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "even for a real user");
    // Another caller is not affected.
    let other = team(&s, &carol, "Carol's").await;
    let (st, _) = add(&s, &carol, &other, "dave", "member").await;
    assert_eq!(st, StatusCode::OK);
}

/// A burst of parallel adds cannot outrun the throttle: each takes its
/// slot before looking the username up, so the burst learns no more
/// usernames than the failures allowed (T12).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_unknown_usernames_share_one_budget() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let acme = team(&s, &bob, "Acme").await;
    let burst = (0..3 * LOOKUP_MAX_FAILURES).map(|i| {
        let (s, bob, acme) = (&s, &bob, &acme);
        async move { add(s, bob, acme, &format!("ghost{i}"), "member").await.0 }
    });
    let answers = futures::future::join_all(burst).await;
    let told = answers
        .iter()
        .filter(|st| **st != StatusCode::TOO_MANY_REQUESTS)
        .count();
    assert_eq!(told, LOOKUP_MAX_FAILURES, "{answers:?}");
    assert!(
        answers
            .iter()
            .all(|st| [StatusCode::NOT_FOUND, StatusCode::TOO_MANY_REQUESTS].contains(st)),
        "{answers:?}"
    );
    // A known username found within the budget gives its slot back.
    let fresh = team(&s, &s.admin, "Fresh").await;
    for _ in 0..LOOKUP_MAX_FAILURES - 1 {
        let (st, _) = add(&s, &s.admin, &fresh, "ghost", "member").await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }
    let (st, v) = add(&s, &s.admin, &fresh, "bob", "member").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = add(&s, &s.admin, &fresh, "ghost", "member").await;
    assert_eq!(st, StatusCode::NOT_FOUND, "the last failure is still free");
    let (st, _) = add(&s, &s.admin, &fresh, "bob", "member").await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn role_guards_and_the_last_owner() {
    let s = start().await;
    let (bob, bob_id) = user(&s, "bob", false).await;
    let (carol, carol_id) = user(&s, "carol", false).await;
    let (dave, dave_id) = user(&s, "dave", false).await;
    user(&s, "erin", false).await;
    let acme = team(&s, &bob, "Acme").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "admin").await.0,
        StatusCode::OK
    );
    assert_eq!(
        add(&s, &bob, &acme, "dave", "member").await.0,
        StatusCode::OK
    );
    let member = |id: &str| format!("/api/workspaces/{acme}/members/{id}");
    let role = |r: &str| Some(json!({ "role": r }));

    // An admin cannot add an owner, change their own role, or touch an owner.
    let (st, _) = add(&s, &carol, &acme, "erin", "owner").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::PATCH,
        &member(&carol_id),
        role("member"),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&s, Method::PATCH, &member(&carol_id), role("member"), &bob).await;
    assert_eq!(st, StatusCode::OK, "the owner may change it");
    let (st, _) = call(&s, Method::PATCH, &member(&carol_id), role("admin"), &bob).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&s, Method::DELETE, &member(&bob_id), None, &carol).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&s, Method::PATCH, &member(&bob_id), role("member"), &carol).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(&s, Method::PATCH, &member(&dave_id), role("owner"), &carol).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(&s, Method::PATCH, &member(&dave_id), role("guest"), &carol).await;
    assert_eq!((st, v["role"].as_str()), (StatusCode::OK, Some("guest")));
    // A guest or member manages nobody.
    let (st, _) = add(&s, &dave, &acme, "erin", "guest").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{acme}"),
        Some(json!({"name": "X"})),
        &dave,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{acme}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "deleting is for owners");
    // An admin removes a member.
    let (st, _) = call(&s, Method::DELETE, &member(&dave_id), None, &carol).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&s, Method::DELETE, &member(&dave_id), None, &carol).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // The last owner cannot leave while members remain, nor step down.
    let (st, v) = call(&s, Method::DELETE, &member(&bob_id), None, &bob).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("without an owner"),
        "{v}"
    );
    let (st, _) = call(&s, Method::PATCH, &member(&carol_id), role("owner"), &bob).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&s, Method::DELETE, &member(&bob_id), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "another owner remains");
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The last member leaving takes the workspace with them.
    let (st, _) = call(&s, Method::DELETE, &member(&carol_id), None, &carol).await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&s, Method::GET, "/api/admin/workspaces", None, &s.admin).await;
    assert!(!v.to_string().contains(&acme), "{v}");
}

#[tokio::test]
async fn chats_follow_membership_and_stay_private() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let (carol, carol_id) = user(&s, "carol", true).await;
    let acme = team(&s, &bob, "Acme").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "member").await.0,
        StatusCode::OK
    );
    let carol_personal = personal_id(&s, &carol).await;
    let bob_personal = personal_id(&s, &bob).await;

    // Bob asks for Acme; Carol's chat goes to her active workspace.
    let bob_sid = new_chat(&s, &bob, Some(&acme), "quarterly zebra report").await;
    let bob_home = new_chat(&s, &bob, None, "zebra at home").await;
    call(
        &s,
        Method::PUT,
        "/api/me/workspace",
        Some(json!({"workspace_id": acme})),
        &carol,
    )
    .await;
    let carol_sid = new_chat(&s, &carol, None, "carol in acme").await;
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{carol_sid}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(row["workspace_id"], json!(acme));
    assert_eq!(row["blocked"], Value::Null);
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{bob_home}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(row["workspace_id"], json!(bob_personal));

    // Only the caller's own chats, in the asked (or active) workspace.
    let list = |who: &str, q: &str| {
        let (s, who, q) = (&s, who.to_string(), q.to_string());
        async move { call(s, Method::GET, &format!("/api/sessions{q}"), None, &who).await }
    };
    assert_eq!(
        session_ids(&list(&carol, &format!("?workspace={acme}")).await.1),
        std::slice::from_ref(&carol_sid)
    );
    assert_eq!(
        session_ids(&list(&carol, "").await.1),
        std::slice::from_ref(&carol_sid)
    );
    assert_eq!(
        session_ids(&list(&bob, &format!("?workspace={acme}")).await.1),
        std::slice::from_ref(&bob_sid)
    );
    assert_eq!(
        session_ids(&list(&bob, "").await.1),
        std::slice::from_ref(&bob_home)
    );
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{bob_sid}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "another member's chat is private"
    );
    // Search follows the workspace too.
    let hits = |v: &Value| {
        v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/search?q=zebra&workspace={acme}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(hits(&v), std::slice::from_ref(&bob_sid));
    let (_, v) = call(&s, Method::GET, "/api/search?q=zebra", None, &bob).await;
    assert_eq!(
        hits(&v),
        std::slice::from_ref(&bob_home),
        "the active workspace by default"
    );
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/search?q=zebra&workspace={acme}"),
        None,
        &carol,
    )
    .await;
    assert!(hits(&v).is_empty(), "never another member's chats");

    // The API token sees every chat, optionally in one workspace.
    let (_, v) = list(TOKEN, "").await;
    assert_eq!(
        session_ids(&v),
        sorted(vec![bob_sid.clone(), bob_home.clone(), carol_sid.clone()])
    );
    let (_, v) = list(TOKEN, &format!("?workspace={acme}")).await;
    assert_eq!(
        session_ids(&v),
        sorted(vec![bob_sid.clone(), carol_sid.clone()])
    );
    let (st, _) = list(TOKEN, "?workspace=nope").await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Carol leaves: her chat returns to Personal, and so does she.
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{acme}/members/{carol_id}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{carol_sid}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(row["workspace_id"], json!(carol_personal));
    assert_eq!(
        session_ids(&list(&carol, "").await.1),
        std::slice::from_ref(&carol_sid)
    );
    assert_eq!(
        list(&carol, &format!("?workspace={acme}")).await.0,
        StatusCode::NOT_FOUND
    );
    let (_, me) = call(&s, Method::GET, "/api/me", None, &carol).await;
    assert_eq!(me["workspace"]["workspace_id"], json!(carol_personal));

    // Deleting the workspace returns Bob's chat to his Personal workspace.
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{acme}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{bob_sid}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(row["workspace_id"], json!(bob_personal));
    assert_eq!(
        session_ids(&list(&bob, "").await.1),
        sorted(vec![bob_sid, bob_home])
    );
}

#[tokio::test]
async fn agentic_needs_the_grant_and_the_role() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let (carol, carol_id) = user(&s, "carol", true).await;
    let (dave, _) = user(&s, "dave", false).await;
    let acme = team(&s, &bob, "Acme").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "guest").await.0,
        StatusCode::OK
    );
    assert_eq!(
        add(&s, &bob, &acme, "dave", "member").await.0,
        StatusCode::OK
    );

    // A guest with the server grant: not in Acme, fine in Personal.
    let (st, v) = chat(&s, &carol, Some(&acme), "x").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"], "your role in Acme cannot run agentic sessions");
    new_chat(&s, &carol, None, "home").await;
    // A member without the grant: nowhere.
    let (st, v) = chat(&s, &dave, Some(&acme), "x").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(
        v["error"],
        "agentic sessions are not enabled for this account; ask an admin"
    );

    // Promoted, Carol starts one; demoted again, it shows as blocked.
    let member = format!("/api/workspaces/{acme}/members/{carol_id}");
    call(
        &s,
        Method::PATCH,
        &member,
        Some(json!({"role": "member"})),
        &bob,
    )
    .await;
    let sid = new_chat(&s, &carol, Some(&acme), "work").await;
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/sessions?workspace={acme}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(v["sessions"][0]["blocked"], Value::Null, "{v}");
    call(
        &s,
        Method::PATCH,
        &member,
        Some(json!({"role": "guest"})),
        &bob,
    )
    .await;
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/sessions?workspace={acme}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(
        v["sessions"][0]["blocked"],
        "your role in Acme cannot run agentic sessions"
    );
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{sid}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(
        row["blocked"],
        "your role in Acme cannot run agentic sessions"
    );
    // A guest's plain chat there is hers to drive: never blocked, even
    // listed next to her blocked agentic one.
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "workspace_id": acme})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let plain = v["session_id"].as_str().unwrap().to_string();
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/sessions?workspace={acme}"),
        None,
        &carol,
    )
    .await;
    let listed = |id: &str| {
        v["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["session_id"] == id)
            .unwrap_or_else(|| panic!("{id} not listed: {v}"))
            .clone()
    };
    assert_eq!(
        (&listed(&plain)["profile"], &listed(&plain)["blocked"]),
        (&json!("chat"), &Value::Null)
    );
    assert_eq!(
        listed(&sid)["blocked"],
        "your role in Acme cannot run agentic sessions"
    );
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{plain}"),
        None,
        &carol,
    )
    .await;
    assert_eq!(
        (&row["workspace_id"], &row["blocked"]),
        (&json!(acme), &Value::Null)
    );
    // The API token may put a chat in any existing workspace.
    let (st, v) = chat(&s, TOKEN, Some(&acme), "token").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let tsid = v["session_id"].as_str().unwrap();
    let (_, row) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{tsid}"),
        None,
        TOKEN,
    )
    .await;
    assert_eq!(row["workspace_id"], json!(acme));
    let (st, _) = chat(&s, TOKEN, Some("nope"), "token").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_settings_and_memory_switch() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let (carol, _) = user(&s, "carol", false).await;
    let acme = team(&s, &bob, "Acme").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "member").await.0,
        StatusCode::OK
    );
    let path = format!("/api/workspaces/{acme}");
    let (_, models) = call(&s, Method::GET, "/api/models", None, &bob).await;
    let model = models["models"][0]["id"].as_str().unwrap().to_string();

    let settings = json!({"default_agent": "general", "agent_models": {"general": {"model": model, "fallbacks": []}}});
    let (st, d) = call(
        &s,
        Method::PATCH,
        &path,
        Some(json!({"name": " Acme Corp ", "settings": settings})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{d}");
    assert_eq!(d["name"], "Acme Corp");
    assert_eq!(d["settings"], settings);
    for bad in [
        json!({"settings": {"memory": false}}),
        json!({"settings": {"agent_models": {"general": {"model": "nope::x"}}}}),
        json!({"settings": {"agent_models": {"nobody": {"model": model}}}}),
        json!({"settings": {"agent_models": {"general": {"model": model, "fallbacks": [model]}}}}),
        json!({"settings": {"default_agent": "nobody"}}),
        json!({"name": ""}),
    ] {
        let (st, v) = call(&s, Method::PATCH, &path, Some(bad.clone()), &bob).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    let (st, _) = call(
        &s,
        Method::PATCH,
        &path,
        Some(json!({"name": "Mine"})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (_, d) = call(&s, Method::GET, &path, None, &carol).await;
    assert_eq!(d["settings"], settings, "every member reads the defaults");

    // The memory switch is each member's own, and off until they turn it
    // on (the creator's too).
    assert_eq!(
        call(&s, Method::GET, &path, None, &carol).await.1["use_memory"],
        false
    );
    let (st, d) = call(
        &s,
        Method::PUT,
        &format!("{path}/memory"),
        Some(json!({"use_memory": true})),
        &carol,
    )
    .await;
    assert_eq!((st, &d["use_memory"]), (StatusCode::OK, &json!(true)));
    assert_eq!(
        call(&s, Method::GET, &path, None, &bob).await.1["use_memory"],
        false
    );

    // The API token acts as an owner of any workspace.
    let (st, d) = call(&s, Method::GET, &path, None, TOKEN).await;
    assert_eq!((st, d["role"].as_str()), (StatusCode::OK, Some("owner")));
}

#[tokio::test]
async fn server_admins_list_recover_and_delete() {
    let s = start().await;
    let (bob, bob_id) = user(&s, "bob", false).await;
    let (carol, carol_id) = user(&s, "carol", false).await;
    let (_, dave_id) = user(&s, "dave", false).await;
    let acme = team(&s, &bob, "Acme").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "member").await.0,
        StatusCode::OK
    );
    // Disabling Carol below logs her out.
    let carol_personal = personal_id(&s, &carol).await;

    let (st, _) = call(&s, Method::GET, "/api/admin/workspaces", None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(&s, Method::GET, "/api/admin/workspaces", None, &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let list = v["workspaces"].as_array().unwrap();
    assert_eq!(list.len(), 5, "Acme and four Personal workspaces: {v}");
    assert_eq!(
        list[0]["workspace_id"],
        json!(acme),
        "team workspaces first"
    );
    assert_eq!(list[0]["owners"], json!(["bob"]));
    assert_eq!(list[0]["members"], 2);
    assert!(list[1..].iter().all(|w| w["personal"] == true));
    assert_eq!(v["members_can_create"], true);
    assert!(!v.to_string().contains("session"), "no chats");
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/admin/workspaces/{acme}/members"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(
        v["members"],
        json!([
            {"user_id": bob_id, "username": "bob", "role": "owner", "disabled": false},
            {"user_id": carol_id, "username": "carol", "role": "member", "disabled": false},
        ])
    );
    let (st, _) = call(
        &s,
        Method::GET,
        "/api/admin/workspaces/nope/members",
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Deleting Bob would leave Acme without an owner.
    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        v["error"],
        "bob is the only owner of Acme; make another member an owner first"
    );

    // Owner recovery: an enabled member, never the admin themselves.
    let owner = |id: &str| Some(json!({ "owner": id }));
    let path = format!("/api/admin/workspaces/{acme}");
    let (st, _) = call(&s, Method::PATCH, &path, owner(&s.admin_id), &s.admin).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&s, Method::PATCH, &path, owner(&dave_id), &s.admin).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "not a member");
    call(
        &s,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"disabled": true})),
        &s.admin,
    )
    .await;
    let (st, _) = call(&s, Method::PATCH, &path, owner(&carol_id), &s.admin).await;
    assert_eq!(st, StatusCode::CONFLICT, "disabled");
    call(
        &s,
        Method::PATCH,
        &format!("/api/users/{carol_id}"),
        Some(json!({"disabled": false})),
        &s.admin,
    )
    .await;
    let (st, v) = call(&s, Method::PATCH, &path, owner(&carol_id), &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["owners"], json!(["bob", "carol"]));
    let bob_personal = personal_id(&s, &bob).await;
    let (st, _) = call(
        &s,
        Method::PATCH,
        &format!("/api/admin/workspaces/{bob_personal}"),
        owner(&carol_id),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "Personal");

    // Now Bob can go; a workspace only he was in goes with him.
    let solo = team(&s, &bob, "Solo").await;
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = call(&s, Method::GET, "/api/admin/workspaces", None, &s.admin).await;
    let text = v.to_string();
    assert!(
        !text.contains(&solo) && !text.contains(&bob_personal),
        "{v}"
    );
    assert_eq!(v["workspaces"][0]["owners"], json!(["carol"]));

    // Deleting: team workspaces only.
    let (st, _) = call(
        &s,
        Method::DELETE,
        &format!("/api/admin/workspaces/{carol_personal}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = call(&s, Method::DELETE, &path, None, &s.admin).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call(&s, Method::DELETE, &path, None, &s.admin).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/admin/workspaces/{acme}/members"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(v["error"], "unknown workspace");
}

/// "You may have asked this before" looks only in the asked (or active)
/// workspace, like search.
#[tokio::test]
async fn suggestions_follow_the_workspace() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", true).await;
    let (carol, _) = user(&s, "carol", true).await;
    let acme = team(&s, &bob, "Acme").await;
    let title = "configure tokio runtime threads";
    let at_work = new_chat(&s, &bob, Some(&acme), title).await;
    let at_home = new_chat(&s, &bob, None, title).await;
    let suggest = |who: &str, q: String| {
        let (s, who) = (&s, who.to_string());
        async move {
            let path = format!("/api/search?suggest=true&q=configure+tokio+runtime+threads{q}");
            call(s, Method::GET, &path, None, &who).await
        }
    };
    let ids = |v: &Value| {
        v["results"]
            .as_array()
            .unwrap_or_else(|| panic!("{v}"))
            .iter()
            .map(|h| h["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (st, v) = suggest(&bob, format!("&workspace={acme}")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(ids(&v), [at_work]);
    let (_, v) = suggest(&bob, String::new()).await;
    assert_eq!(ids(&v), [at_home], "the active workspace by default");
    let (st, _) = suggest(&carol, format!("&workspace={acme}")).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "members only");
}

#[tokio::test]
async fn ui_sockets_hear_workspace_changes() {
    let s = start().await;
    let (bob, _) = user(&s, "bob", false).await;
    let (_, carol_id) = user(&s, "carol", false).await;
    let mut req = format!("ws://{}/api/ws", s.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut().insert("cookie", s.admin.parse().unwrap());
    let origin = format!("http://{}", req.uri().authority().unwrap());
    req.headers_mut().insert("origin", origin.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    socket
        .send(Message::Text(common::socket_login(&s.admin).into()))
        .await
        .unwrap();
    // An answer proves the socket listens to live updates before the change.
    socket.send(Message::Text("hello".into())).await.unwrap();
    assert_eq!(next_message(&mut socket).await["type"], "error");
    let acme = team(&s, &bob, "Acme").await;
    wait_for(&mut socket, "workspaces_changed").await;
    assert_eq!(
        add(&s, &bob, &acme, "carol", "guest").await.0,
        StatusCode::OK
    );
    wait_for(&mut socket, "workspaces_changed").await;

    // A role change decides whether the member may drive their chats here
    // (`blocked` on their rows), so the UIs refetch the chats too; so does
    // owner recovery.
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{acme}/members/{carol_id}"),
        Some(json!({"role": "member"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_for(&mut socket, "workspaces_changed").await;
    wait_for(&mut socket, "sessions_changed").await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/admin/workspaces/{acme}"),
        Some(json!({ "owner": carol_id })),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_for(&mut socket, "workspaces_changed").await;
    wait_for(&mut socket, "sessions_changed").await;
}

/// Read the socket until a message of `kind` arrives.
async fn wait_for(socket: &mut Socket, kind: &str) {
    while next_message(socket).await["type"] != kind {}
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn next_message(socket: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .expect("socket went quiet");
        if let Some(Ok(Message::Text(t))) = msg {
            return serde_json::from_str(&t).unwrap();
        }
    }
}
