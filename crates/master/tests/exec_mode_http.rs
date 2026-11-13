//! Who may have which execution mode (N.10a part 1, the interim rule until
//! the levels per member of part 2): `host` only for a server admin, on
//! every node, at create and again on every later action; asked for only
//! by name, `sandbox` sent otherwise. A node must have announced the mode
//! it is sent. The node decides last (`crates/node/tests/exec_modes.rs`).

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
/// Announces no mode at all (as a node from before modes would).
const SILENT: &str = "node-silent1";

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    mirror: Store,
    master: Master,
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
    let mirror = Store::open_in_memory().unwrap();
    let master = Master::new(mirror.clone(), auth);
    master.set_provider_runtime(rt.clone());
    let both = || ExecOffers::new(Ok(()), Ok(()), vec![harness_node::exec::GAP_HOST_NETNS]);
    let host_only = || ExecOffers::new(Err("bubblewrap not found".into()), Ok(()), vec![]);
    let local = node(dir.path(), "local", Profile::Agentic, ExecOffers::default()).await;
    let chats = node(dir.path(), "master", Profile::Chat, ExecOffers::default()).await;
    master.add_link(local.attach().await, true).await.unwrap();
    master.add_link(chats.attach().await, true).await.unwrap();
    let silent = || {
        let no = || Err("off in this test".to_string());
        ExecOffers::new(no(), no(), vec![])
    };
    for (id, o) in [(BOTH, both()), (HOST_ONLY, host_only()), (SILENT, silent())] {
        let n = node(dir.path(), id, Profile::Agentic, o).await;
        master.add_link(n.attach().await, false).await.unwrap();
    }
    let app = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(app)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        ws,
        admin: String::new(),
        mirror,
        master,
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
    account_with(s, name, true).await.0
}

/// A non-admin account, logged in: (cookie, user id).
async fn account_with(s: &Server, name: &str, agentic: bool) -> (String, String) {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "can_agentic": agentic})),
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

async fn create(
    s: &Server,
    who: &str,
    ws: &str,
    node: &str,
    mode: Option<&str>,
) -> (StatusCode, Value) {
    create_in(s, who, Some(ws), node, mode).await
}

/// `ws`: `None` for the caller's active workspace (their Personal one).
async fn create_in(
    s: &Server,
    who: &str,
    ws: Option<&str>,
    node: &str,
    mode: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({"cwd": s.ws.display().to_string(), "title": "t", "node_id": node});
    if let Some(w) = ws {
        body["workspace_id"] = json!(w);
    }
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

async fn personal(s: &Server, who: &str) -> String {
    let (_, v) = call(s, Method::GET, "/api/workspaces", None, who).await;
    let first = &v["workspaces"][0];
    assert_eq!(first["personal"], true, "{v}");
    first["workspace_id"].as_str().unwrap().to_string()
}

/// Review H1: `host` is for server admins only. Every account owns its
/// Personal workspace and may own a team it made itself, so neither is a
/// reason; refused with the reason, and the node never hears of it.
#[tokio::test]
async fn host_is_for_server_admins_only() {
    let s = start().await;
    let bob = account(&s, "bob").await;
    let carol = account(&s, "carol").await;
    // A plain account in its own Personal workspace, named or not.
    let carol_ws = personal(&s, &carol).await;
    for ws in [None, Some(carol_ws.as_str())] {
        let (st, v) = create_in(&s, &carol, ws, BOTH, Some("host")).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{ws:?}: {v}");
        assert!(v.to_string().contains("for server admins"), "{v}");
    }
    // A plain account in a team it made itself: owner, and still refused.
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
    let (st, v) = create(&s, &bob, &team, BOTH, Some("host")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // Every refusal and the grant are audited; no session was made.
    let (st, v) = create(&s, &bob, &team, BOTH, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &bob, sid).await, json!("sandbox"));

    // A server admin may, by name; without a request, the sandbox (L-a).
    let admin_ws = personal(&s, &s.admin).await;
    let (st, v) = create(&s, &s.admin, &admin_ws, BOTH, Some("host")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &s.admin, sid).await, json!("host"));
    let (st, v) = create(&s, &s.admin, &admin_ws, BOTH, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap();
    assert_eq!(recorded(&s, &s.admin, sid).await, json!("sandbox"));
    // On a node that offers only `host`, a create without a mode is
    // refused, never handed it.
    let (st, v) = create(&s, &s.admin, &admin_ws, HOST_ONLY, None).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(v.to_string().contains("sandbox mode"), "{v}");
    // A node that does not offer it refuses the admin too.
    let (st, v) = create(&s, &s.admin, &admin_ws, "local", Some("host")).await;
    assert_ne!(st, StatusCode::OK, "{v}");
    assert!(
        v.to_string().contains("does not offer the host mode"),
        "{v}"
    );

    // N-d: one audit line per asked-for `host`, granted or refused, with
    // ids only.
    let lines = s.mirror.audit_lines("session.create.host").unwrap();
    let granted: Vec<bool> = lines
        .iter()
        .map(|(_, d)| serde_json::from_str::<Value>(d).unwrap()["granted"] == true)
        .collect();
    assert_eq!(granted, [false, false, false, true, true], "{lines:?}");
    let first: Value = serde_json::from_str(&lines[0].1).unwrap();
    assert_eq!(first["node"], BOTH, "{first}");
    assert!(
        first["user"].is_string() && first.get("cwd").is_none(),
        "{first}"
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

/// L-g: a node is sent only a mode it announced; one that names none (a
/// node from before modes) gets no agentic chat, refused by the master,
/// naming the node.
#[tokio::test]
async fn a_node_that_did_not_announce_the_mode_gets_no_chat() {
    let s = start().await;
    let ws = personal(&s, &s.admin).await;
    for mode in [None, Some("host")] {
        let (st, v) = create(&s, &s.admin, &ws, SILENT, mode).await;
        assert_ne!(st, StatusCode::OK, "{v}");
        let msg = v.to_string();
        assert!(
            msg.contains(SILENT) && msg.contains("its hello did not name it"),
            "{msg}"
        );
    }
}

/// L-h: the rule holds on every later action, not only at create. A
/// server admin's `host` chat, once they are no longer an admin, takes no
/// prompt, no allowing approval and no restore, with the reason; their
/// sandbox chat goes on.
#[tokio::test]
async fn a_host_chat_whose_owner_is_no_longer_an_admin_does_no_more_work() {
    let s = start().await;
    let (dana, dana_id) = account_with(&s, "dana", true).await;
    let promote = |role: &'static str| {
        let (s, id) = (&s, dana_id.clone());
        async move {
            let (st, v) = call(
                s,
                Method::PATCH,
                &format!("/api/users/{id}"),
                Some(json!({ "role": role })),
                &s.admin,
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
        }
    };
    promote("admin").await;
    let ws = personal(&s, &dana).await;
    let (st, v) = create(&s, &dana, &ws, BOTH, Some("host")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let host = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = create(&s, &dana, &ws, BOTH, None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sandbox = v["session_id"].as_str().unwrap().to_string();
    assert_eq!(recorded(&s, &dana, &host).await, json!("host"));
    assert_eq!(recorded(&s, &dana, &sandbox).await, json!("sandbox"));
    promote("member").await;
    for (path, body) in [
        (
            format!("/api/sessions/{host}/prompt"),
            json!({"text": "hi"}),
        ),
        (
            format!("/api/sessions/{host}/approvals/a1"),
            json!({"decision": "allow"}),
        ),
        (format!("/api/sessions/{host}/turns/x/restore"), json!({})),
    ] {
        let (st, v) = call(&s, Method::POST, &path, Some(body), &dana).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{path}: {v}");
        assert!(v.to_string().contains("for server admins"), "{path}: {v}");
    }
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{host}"),
        None,
        &dana,
    )
    .await;
    assert!(
        v["blocked"]
            .as_str()
            .is_some_and(|b| b.contains("for server admins")),
        "{v}"
    );
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sandbox}/prompt"),
        Some(json!({"text": "hi"})),
        &dana,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// L-h, the callback's admission: `agentic_blocked` is what it asks, and
/// for a `host` row it applies the create's rule; a row without a readable
/// mode is refused (fails closed).
#[tokio::test]
async fn the_mode_check_on_a_row_fails_closed_and_refuses_host_to_a_member() {
    let s = start().await;
    let (_, eve_id) = account_with(&s, "eve", true).await;
    let user = s.mirror.user_by_id(&eve_id).unwrap().unwrap();
    let eve = harness_master::auth::Principal::from_user(user);
    let mut row = harness_store::SessionRow {
        session_id: harness_core::SessionId::new(),
        node_id: BOTH.into(),
        title: "t".into(),
        cwd: "/w".into(),
        model: "m".into(),
        created_ms: 0,
        updated_ms: 0,
        head_seq: 1,
        running: false,
        owner_id: Some(eve_id),
        agent_id: None,
        profile: Some("agentic".into()),
        incognito: false,
        parent_id: None,
        workspace_id: None,
        exec_mode: Some("sandbox".into()),
    };
    let m = s.master.clone();
    assert_eq!(
        harness_master::workspaces::agentic_blocked(&m, &eve, &row).await,
        None
    );
    row.exec_mode = Some("host".into());
    let why = harness_master::workspaces::agentic_blocked(&m, &eve, &row)
        .await
        .unwrap();
    assert!(why.contains("for server admins"), "{why}");
    for mode in [None, Some("container".to_string())] {
        row.exec_mode = mode;
        let why = harness_master::workspaces::agentic_blocked(&m, &eve, &row)
            .await
            .unwrap();
        assert!(why.contains("execution mode is not known"), "{why}");
    }
}

/// L-i: what each node can contain is shown only to those who may run
/// agentic chats.
#[tokio::test]
async fn the_modes_are_not_shown_to_an_account_without_the_grant() {
    let s = start().await;
    let (frank, _) = account_with(&s, "frank", false).await;
    let (st, v) = call(&s, Method::GET, "/api/nodes", None, &frank).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for n in v["nodes"].as_array().unwrap() {
        assert_eq!(n["exec_modes"], json!([]), "{n}");
        assert_eq!(n["exec_gaps"], json!([]), "{n}");
    }
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
