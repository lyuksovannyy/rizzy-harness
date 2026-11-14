//! Execution levels per member (O13, N.10a part 2; docs/execution-modes.md):
//! an account's ceiling, set by server admins; a member's level in each
//! workspace, set by its admins; the effective level is the lower, and a
//! chat's mode must be at most it, at create and on every later action that
//! starts work. Lowering a level stops what it no longer allows, and only
//! that. The node decides last (`crates/node/tests/exec_modes.rs`): it
//! knows nothing of levels and refuses only modes it does not offer.

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
use harness_master::{CommandError, CreateSpec, Master, PromptOpts, router};
use harness_node::exec::ExecOffers;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const TOKEN: &str = "api-secret";
/// Offers `sandbox` and `host`.
const BOTH: &str = "node-both1";

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    admin_id: String,
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
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
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

/// `local` (sandbox only), the chat runtime, a remote node offering both
/// modes, and a claimed server admin ("alex").
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
    let local = node(dir.path(), "local", Profile::Agentic, ExecOffers::default()).await;
    let chats = node(dir.path(), "master", Profile::Chat, ExecOffers::default()).await;
    master.add_link(local.attach().await, true).await.unwrap();
    master.add_link(chats.attach().await, true).await.unwrap();
    let both = ExecOffers::new(Ok(()), Ok(()), vec![]);
    let n = node(dir.path(), BOTH, Profile::Agentic, both).await;
    master.add_link(n.attach().await, false).await.unwrap();
    let app = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(app)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        ws,
        admin: String::new(),
        admin_id: String::new(),
        mirror,
        master: master.clone(),
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
    let (_, me) = call(&s, Method::GET, "/api/me", None, &s.admin).await;
    s.admin_id = me["user"]["user_id"].as_str().unwrap().to_string();
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

/// A non-admin account at `ceiling`, logged in: (cookie, user id).
async fn account(s: &Server, name: &str, ceiling: &str) -> (String, String) {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user_id"].as_str().unwrap().to_string();
    assert_eq!(
        v["exec_ceiling"], "none",
        "a new account starts at none: {v}"
    );
    if ceiling != "none" {
        let (st, v) = set_ceiling(s, &s.admin, &id, ceiling).await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
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

async fn set_ceiling(s: &Server, who: &str, id: &str, level: &str) -> (StatusCode, Value) {
    call(
        s,
        Method::PUT,
        &format!("/api/users/{id}/exec-ceiling"),
        Some(json!({ "level": level })),
        who,
    )
    .await
}

async fn set_level(s: &Server, who: &str, ws: &str, id: &str, level: &str) -> (StatusCode, Value) {
    call(
        s,
        Method::PUT,
        &format!("/api/workspaces/{ws}/members/{id}/exec-level"),
        Some(json!({ "level": level })),
        who,
    )
    .await
}

async fn team(s: &Server, who: &str, name: &str) -> String {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        who,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["workspace_id"].as_str().unwrap().to_string()
}

async fn add(s: &Server, who: &str, ws: &str, name: &str, role: &str) -> Value {
    let (st, v) = call(
        s,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({ "username": name, "role": role })),
        who,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

async fn personal(s: &Server, who: &str) -> String {
    let (_, v) = call(s, Method::GET, "/api/workspaces", None, who).await;
    let first = &v["workspaces"][0];
    assert_eq!(first["personal"], true, "{v}");
    first["workspace_id"].as_str().unwrap().to_string()
}

/// An agentic chat on `BOTH` that asks before every command.
async fn create(s: &Server, who: &str, ws: &str, mode: Option<&str>) -> (StatusCode, Value) {
    let mut body = json!({"cwd": s.ws.display().to_string(), "title": "t", "node_id": BOTH,
        "workspace_id": ws, "approval_mode": "ask"});
    if let Some(m) = mode {
        body["exec_mode"] = json!(m);
    }
    call(s, Method::POST, "/api/sessions", Some(body), who).await
}

async fn created(s: &Server, who: &str, ws: &str, mode: Option<&str>) -> String {
    let (st, v) = create(s, who, ws, mode).await;
    assert_eq!(st, StatusCode::OK, "{mode:?}: {v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn post(s: &Server, who: &str, path: String, body: Value) -> (StatusCode, Value) {
    call(s, Method::POST, &path, Some(body), who).await
}

async fn events(s: &Server, who: &str, sid: &str) -> Vec<Value> {
    let (_, v) = call(
        s,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        who,
    )
    .await;
    v["events"].as_array().cloned().unwrap_or_default()
}

async fn nth_event(s: &Server, who: &str, sid: &str, kind: &str, n: usize) -> Value {
    for _ in 0..500 {
        let found: Vec<Value> = events(s, who, sid)
            .await
            .into_iter()
            .filter(|e| e["kind"] == kind)
            .collect();
        if found.len() >= n {
            return found[n - 1].clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no {kind} #{n} in {sid}");
}

async fn session(s: &Server, who: &str, sid: &str) -> Value {
    call(s, Method::GET, &format!("/api/sessions/{sid}"), None, who)
        .await
        .1
}

/// A turn of `sid` waiting for its first command's approval; its id.
async fn waiting_turn(s: &Server, who: &str, sid: &str, n: usize) -> String {
    let (st, v) = post(
        s,
        who,
        format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: echo hi"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let asked = nth_event(s, who, sid, "approval_requested", n).await;
    assert_eq!(session(s, who, sid).await["running"], true);
    asked["payload"]["approval_id"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn wait_stopped(s: &Server, who: &str, sid: &str) {
    for _ in 0..500 {
        if session(s, who, sid).await["running"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{sid} kept running");
}

fn forbidden_with(st: StatusCode, v: &Value, words: &str) {
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let msg = v["error"].as_str().unwrap_or_default();
    assert!(msg.contains(words), "expected {words:?} in {msg:?}");
}

/// The plan row's first test: a member at `none` is refused, with the
/// reason, on every action that starts work: create, prompt, allowing an
/// approval, restore, save as artifact, and the two that had no gate,
/// `set_agent` and `select_variant`. Denying and cancelling stay theirs,
/// and so does reading.
#[tokio::test]
async fn a_member_at_none_is_refused_every_action_that_starts_work() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    let added = add(&s, &s.admin, &acme, "bob", "member").await;
    assert_eq!(
        added["exec_level"], "none",
        "a new member starts at none: {added}"
    );
    let none = "your level in Acme is none, so no agent chats";
    let (st, v) = create(&s, &bob, &acme, None).await;
    forbidden_with(st, &v, none);

    // A chat made while he could, then his level goes back to none.
    let (st, v) = set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    assert_eq!(
        (st, &v["exec_level"]),
        (StatusCode::OK, &json!("sandbox")),
        "{v}"
    );
    let sid = created(&s, &bob, &acme, None).await;
    let approval = waiting_turn(&s, &bob, &sid, 1).await;
    let (st, v) = set_level(&s, &s.admin, &acme, &bob_id, "none").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The running turn stops at once (the revocation sweep).
    wait_stopped(&s, &bob, &sid).await;
    let blocked = session(&s, &bob, &sid).await["blocked"].clone();
    assert!(
        blocked.as_str().unwrap_or_default().starts_with(none),
        "{blocked}"
    );
    let actions = [
        (format!("/api/sessions/{sid}/prompt"), json!({"text": "hi"})),
        (
            format!("/api/sessions/{sid}/approvals/{approval}"),
            json!({"decision": "allow"}),
        ),
        (format!("/api/sessions/{sid}/turns/t1/restore"), json!({})),
        (
            format!("/api/sessions/{sid}/artifacts"),
            json!({"path": "a.txt"}),
        ),
        (
            format!("/api/sessions/{sid}/agent"),
            json!({"agent_id": "coder"}),
        ),
        (
            format!("/api/sessions/{sid}/variants"),
            json!({"group": "g", "message_id": "m"}),
        ),
    ];
    for (path, body) in actions {
        let (st, v) = post(&s, &bob, path.clone(), body).await;
        forbidden_with(st, &v, none);
    }
    // Reading stays allowed, and refusing is still his.
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = post(
        &s,
        &bob,
        format!("/api/sessions/{sid}/approvals/{approval}"),
        json!({"decision": "deny"}),
    )
    .await;
    assert_ne!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = post(&s, &bob, format!("/api/sessions/{sid}/cancel"), json!({})).await;
    assert_ne!(st, StatusCode::FORBIDDEN, "{v}");
}

/// A member at `sandbox` is refused a `host` chat by the master, which
/// never sends the create (no session is made), and gets `sandbox` without
/// asking. `container` exists as a level but runs nothing until N.4, and
/// says so. The node's own refusal of a mode it does not offer, whatever a
/// test master sends, is `crates/node/tests/exec_modes.rs`.
#[tokio::test]
async fn a_member_at_sandbox_is_refused_host_and_container_runs_nothing_yet() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    let before = s.mirror.sessions_of(Some(&bob_id)).unwrap().len();
    let (st, v) = create(&s, &bob, &acme, Some("host")).await;
    forbidden_with(
        st,
        &v,
        "your level in Acme is sandbox, which does not include the host mode",
    );
    assert!(v["error"].as_str().unwrap().contains("owner"), "{v}");
    assert_eq!(s.mirror.sessions_of(Some(&bob_id)).unwrap().len(), before);
    let sid = created(&s, &bob, &acme, None).await;
    let row = s.mirror.session(sid.parse().unwrap()).unwrap().unwrap();
    assert_eq!(row.exec_mode.as_deref(), Some("sandbox"));
    // The summary carries the effective level for the new-chat choice.
    let (_, list) = call(&s, Method::GET, "/api/workspaces", None, &bob).await;
    let acme_row = list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["workspace_id"] == acme.as_str())
        .unwrap()
        .clone();
    assert_eq!(acme_row["exec_level"], "sandbox", "{acme_row}");

    set_level(&s, &s.admin, &acme, &bob_id, "container").await;
    let (st, v) = create(&s, &bob, &acme, None).await;
    forbidden_with(st, &v, "no node offers containers yet");
}

/// Lowering a level stops the chats above the new level, and only those
/// (review RN-1's lesson): the workspace level from `host` to `sandbox`
/// stops the running host turn and leaves the sandbox turn running; then
/// the account's ceiling lowered to `none` while that sandbox turn runs in
/// a team where his level is still `host` stops it too. A chat above the
/// level stays readable.
#[tokio::test]
async fn lowering_a_level_stops_only_the_chats_above_it() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    let (st, v) = set_level(&s, &s.admin, &acme, &bob_id, "host").await;
    assert_eq!(st, StatusCode::OK, "the owner grants host: {v}");
    let host = created(&s, &bob, &acme, Some("host")).await;
    let sandbox = created(&s, &bob, &acme, None).await;
    waiting_turn(&s, &bob, &host, 1).await;
    let approval = waiting_turn(&s, &bob, &sandbox, 1).await;

    let (st, v) = set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_stopped(&s, &bob, &host).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        session(&s, &bob, &sandbox).await["running"],
        true,
        "the sandbox turn was stopped"
    );
    let (st, v) = post(
        &s,
        &bob,
        format!("/api/sessions/{host}/prompt"),
        json!({"text": "hi"}),
    )
    .await;
    forbidden_with(st, &v, "does not include the host mode");
    // Reading the host chat stays allowed.
    let (st, _) = call(
        &s,
        Method::GET,
        &format!("/api/sessions/{host}/events"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(!events(&s, &bob, &host).await.is_empty());

    // His level in Acme back to host; his account's ceiling to none.
    set_level(&s, &s.admin, &acme, &bob_id, "host").await;
    let (st, v) = set_ceiling(&s, &s.admin, &bob_id, "none").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_stopped(&s, &bob, &sandbox).await;
    let (st, v) = post(
        &s,
        &bob,
        format!("/api/sessions/{sandbox}/approvals/{approval}"),
        json!({"decision": "allow"}),
    )
    .await;
    forbidden_with(st, &v, "your account's level is none");
}

/// A server admin demoted while their host turn runs: as a server admin
/// their ceiling was `host`; demoted, their stored ceiling applies and the
/// host turn stops.
#[tokio::test]
async fn a_demoted_server_admins_host_turn_stops() {
    let s = start().await;
    let (dana, dana_id) = account(&s, "dana", "sandbox").await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{dana_id}"),
        Some(json!({"role": "admin"})),
        &s.admin,
    )
    .await;
    assert_eq!(
        (st, &v["exec_ceiling"]),
        (StatusCode::OK, &json!("host")),
        "{v}"
    );
    let ws = personal(&s, &dana).await;
    let host = created(&s, &dana, &ws, Some("host")).await;
    waiting_turn(&s, &dana, &host, 1).await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{dana_id}"),
        Some(json!({"role": "member"})),
        &s.admin,
    )
    .await;
    assert_eq!(
        (st, &v["exec_ceiling"]),
        (StatusCode::OK, &json!("sandbox")),
        "{v}"
    );
    wait_stopped(&s, &dana, &host).await;
    let (st, v) = post(
        &s,
        &dana,
        format!("/api/sessions/{host}/prompt"),
        json!({"text": "hi"}),
    )
    .await;
    forbidden_with(st, &v, "your account's level is sandbox");
}

/// Who grants what: nobody changes their own level; nobody grants above
/// their own effective level; `host` is the owner's to grant; an owner's
/// level is always `host`; only members with the members permission set
/// levels.
#[tokio::test]
async fn an_admin_grants_only_up_to_their_own_level_and_host_only_by_an_owner() {
    let s = start().await;
    let (carol, carol_id) = account(&s, "carol", "host").await;
    let (dave, dave_id) = account(&s, "dave", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "carol", "admin").await;
    add(&s, &s.admin, &acme, "dave", "member").await;
    set_level(&s, &s.admin, &acme, &carol_id, "sandbox").await;

    // Not her own, in either direction.
    let (st, v) = set_level(&s, &carol, &acme, &carol_id, "host").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, v) = set_level(&s, &carol, &acme, &carol_id, "none").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    // Host is the owner's, even for an admin whose level were high enough.
    let (st, v) = set_level(&s, &carol, &acme, &dave_id, "host").await;
    forbidden_with(st, &v, "only an owner of Acme can grant host");
    // Up to her own.
    let (st, v) = set_level(&s, &carol, &acme, &dave_id, "sandbox").await;
    assert_eq!(
        (st, &v["exec_level"]),
        (StatusCode::OK, &json!("sandbox")),
        "{v}"
    );
    // Her effective level is the lower one: her ceiling at container
    // makes sandbox above her own.
    set_ceiling(&s, &s.admin, &carol_id, "container").await;
    set_level(&s, &s.admin, &acme, &dave_id, "none").await;
    let (st, v) = set_level(&s, &carol, &acme, &dave_id, "sandbox").await;
    forbidden_with(st, &v, "above your own (yours in Acme is container)");
    // An owner's level cannot be set, even by another owner (the owner
    // rule itself, not the rule on acting above one's role).
    let (_erin, erin_id) = account(&s, "erin", "host").await;
    add(&s, &s.admin, &acme, "erin", "member").await;
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{acme}/members/{erin_id}"),
        Some(json!({"role": "owner"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = set_level(&s, &s.admin, &acme, &erin_id, "none").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with("an owner's level here is always host"),
        "{v}"
    );
    // A member cannot set levels at all.
    let (st, v) = set_level(&s, &dave, &acme, &carol_id, "none").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // The owner grants host; an unknown level is refused.
    let (st, v) = set_level(&s, &s.admin, &acme, &dave_id, "host").await;
    assert_eq!(
        (st, &v["exec_level"]),
        (StatusCode::OK, &json!("host")),
        "{v}"
    );
    let (st, _) = set_level(&s, &s.admin, &acme, &dave_id, "root").await;
    assert!(st.is_client_error());
    // Only server admins set ceilings, never their own.
    let (st, _) = set_ceiling(&s, &carol, &dave_id, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = set_ceiling(&s, &s.admin, &s.admin_id, "none").await;
    assert_eq!(st, StatusCode::CONFLICT);
    // The old switch is gone, loudly.
    let (st, v) = call(
        &s,
        Method::PATCH,
        &format!("/api/users/{dave_id}"),
        Some(json!({"can_agentic": true})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

/// A self-made team owner whose account's ceiling is `sandbox` cannot get
/// `host` anywhere: not in the team they own (an owner's level there is
/// `host`, the ceiling still applies), not in their Personal workspace, not
/// for a friend (above their own), not by changing their own.
#[tokio::test]
async fn a_self_made_team_owner_with_a_sandbox_ceiling_cannot_get_host_anywhere() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let (_friend, friend_id) = account(&s, "friend", "host").await;
    let own = team(&s, &bob, "Bobs").await;
    add(&s, &bob, &own, "friend", "member").await;
    let (st, v) = create(&s, &bob, &own, Some("host")).await;
    forbidden_with(st, &v, "your account's level is sandbox");
    let mine = personal(&s, &bob).await;
    let (st, v) = create(&s, &bob, &mine, Some("host")).await;
    forbidden_with(st, &v, "your account's level is sandbox");
    let (st, v) = set_level(&s, &bob, &own, &friend_id, "host").await;
    forbidden_with(st, &v, "above your own");
    let (st, _) = set_level(&s, &bob, &own, &bob_id, "host").await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Sandbox works for him, and he may grant it.
    created(&s, &bob, &own, None).await;
    let (st, v) = set_level(&s, &bob, &own, &friend_id, "sandbox").await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// In a Personal workspace the level is the account's ceiling, and it has
/// no level of its own to set. A server admin's ceiling is `host`, but in a
/// team they have a level only as its member.
#[tokio::test]
async fn the_personal_workspace_level_is_the_ceiling() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "none").await;
    let mine = personal(&s, &bob).await;
    let (st, v) = create(&s, &bob, &mine, None).await;
    forbidden_with(st, &v, "your account's level is none");
    set_ceiling(&s, &s.admin, &bob_id, "sandbox").await;
    created(&s, &bob, &mine, None).await;
    let (st, v) = create(&s, &bob, &mine, Some("host")).await;
    forbidden_with(st, &v, "your account's level is sandbox");
    set_ceiling(&s, &s.admin, &bob_id, "host").await;
    created(&s, &bob, &mine, Some("host")).await;
    // Not even the API token, owner of every workspace, sets a level there.
    let (st, v) = set_level(&s, TOKEN, &mine, &bob_id, "none").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .starts_with("a Personal workspace has no levels"),
        "{v}"
    );
    // The server admin, host in their own Personal workspace...
    let theirs = personal(&s, &s.admin).await;
    created(&s, &s.admin, &theirs, Some("host")).await;
    // ...and nothing in a team they are not a member of.
    let bobs = team(&s, &bob, "Bobs").await;
    let (st, v) = create(&s, &s.admin, &bobs, None).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
}

/// A member removed while their turn runs: it stops, and their chats from
/// the workspace go back to their Personal one, where their ceiling
/// applies.
#[tokio::test]
async fn a_removed_members_running_turn_stops() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    let sid = created(&s, &bob, &acme, None).await;
    waiting_turn(&s, &bob, &sid, 1).await;
    let (st, v) = call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{acme}/members/{bob_id}"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_stopped(&s, &bob, &sid).await;
}

/// Every level change is one audit line: who, whose, which workspace or
/// account, from, to; none for a change that changes nothing; no content.
#[tokio::test]
async fn every_level_change_is_one_audit_line() {
    let s = start().await;
    let (_bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    let account_lines = s.mirror.audit_lines("level.account").unwrap();
    assert_eq!(account_lines.len(), 1, "{account_lines:?}");
    let l: Value = serde_json::from_str(&account_lines[0].1).unwrap();
    let keys = |l: &Value| {
        let mut k: Vec<String> = l.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&l), ["by", "from", "to", "user"], "no content: {l}");
    assert_eq!(
        (&l["by"], &l["user"], &l["from"], &l["to"]),
        (
            &json!(s.admin_id),
            &json!(bob_id),
            &json!("none"),
            &json!("sandbox")
        ),
        "{l}"
    );
    let ws_lines = s.mirror.audit_lines("level.workspace").unwrap();
    assert_eq!(ws_lines.len(), 1, "{ws_lines:?}");
    let l: Value = serde_json::from_str(&ws_lines[0].1).unwrap();
    assert_eq!(
        keys(&l),
        ["by", "from", "to", "user", "workspace"],
        "no content: {l}"
    );
    assert_eq!(
        (&l["workspace"], &l["user"], &l["from"], &l["to"]),
        (
            &json!(acme),
            &json!(bob_id),
            &json!("none"),
            &json!("sandbox")
        ),
        "{l}"
    );
    // Removal is a change too.
    call(
        &s,
        Method::DELETE,
        &format!("/api/workspaces/{acme}/members/{bob_id}"),
        None,
        &s.admin,
    )
    .await;
    let ws_lines = s.mirror.audit_lines("level.workspace").unwrap();
    assert_eq!(ws_lines.len(), 2, "{ws_lines:?}");
}

/// The API token is the machine owner's root key: it acts as an owner of
/// every workspace and is `host` (its own chats have no owner). On a
/// member's chat it gets the member's answer, not its own.
#[tokio::test]
async fn the_api_token_is_host_but_a_members_chat_keeps_the_members_level() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    let (st, v) = create(&s, TOKEN, &acme, Some("host")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, TOKEN, &acme, &bob_id, "sandbox").await;
    let sid = created(&s, &bob, &acme, None).await;
    set_level(&s, TOKEN, &acme, &bob_id, "none").await;
    let (st, v) = post(
        &s,
        TOKEN,
        format!("/api/sessions/{sid}/prompt"),
        json!({"text": "hi"}),
    )
    .await;
    forbidden_with(st, &v, "your level in Acme is none");
}

/// Review L2-3: deleting a team stops what ran in its chats, as removing a
/// member does (the chats return to Personal either way), whether its owner
/// deletes it or a server admin does.
#[tokio::test]
async fn deleting_a_workspace_stops_what_ran_there() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    for by_admin in [false, true] {
        let acme = team(&s, &s.admin, "Acme").await;
        add(&s, &s.admin, &acme, "bob", "member").await;
        set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
        let sid = created(&s, &bob, &acme, None).await;
        waiting_turn(&s, &bob, &sid, 1).await;
        let path = match by_admin {
            false => format!("/api/workspaces/{acme}"),
            true => format!("/api/admin/workspaces/{acme}"),
        };
        let (st, v) = call(&s, Method::DELETE, &path, None, &s.admin).await;
        assert_eq!(st, StatusCode::OK, "{path}: {v}");
        wait_stopped(&s, &bob, &sid).await;
    }
}

/// Review L2-10 (decided): nobody changes a level above their own. A
/// workspace admin at sandbox can neither lower nor otherwise change a
/// member the owner gave host; one at their own level or below, yes.
#[tokio::test]
async fn an_admin_cannot_change_a_member_whose_level_is_above_their_own() {
    let s = start().await;
    let (carol, carol_id) = account(&s, "carol", "host").await;
    let (_dave, dave_id) = account(&s, "dave", "host").await;
    let (_eve, eve_id) = account(&s, "eve", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "carol", "admin").await;
    add(&s, &s.admin, &acme, "dave", "member").await;
    add(&s, &s.admin, &acme, "eve", "member").await;
    set_level(&s, &s.admin, &acme, &carol_id, "sandbox").await;
    set_level(&s, &s.admin, &acme, &dave_id, "host").await;
    for to in ["none", "sandbox"] {
        let (st, v) = set_level(&s, &carol, &acme, &dave_id, to).await;
        forbidden_with(st, &v, "whose level is above your own");
    }
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}/members"),
        None,
        &s.admin,
    )
    .await;
    assert!(v.to_string().contains("host"), "{v}");
    set_level(&s, &s.admin, &acme, &eve_id, "sandbox").await;
    let (st, v) = set_level(&s, &carol, &acme, &eve_id, "none").await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Review L2-9 and the 409 at the HTTP layer: a level change is written
/// only while what its guards read still holds, the setter's own standing
/// included. A grant by an admin who is lowered meanwhile, and the second
/// of two changes decided on the same reading, each get 409.
#[tokio::test]
async fn a_change_decided_on_a_stale_reading_gets_409() {
    let s = start().await;
    let (carol, carol_id) = account(&s, "carol", "host").await;
    let (_dave, dave_id) = account(&s, "dave", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "carol", "admin").await;
    add(&s, &s.admin, &acme, "dave", "member").await;
    set_level(&s, &s.admin, &acme, &carol_id, "sandbox").await;
    // Every level write waits this long between its guards and its write.
    s.master.set_level_write_delay(Duration::from_millis(400));
    // The owner lowers carol; carol, a moment later, grants dave sandbox on
    // her old reading. The owner's write lands first.
    let lower = set_level(&s, &s.admin, &acme, &carol_id, "none");
    let grant = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        set_level(&s, &carol, &acme, &dave_id, "sandbox").await
    };
    let ((st1, v1), (st2, v2)) = tokio::join!(lower, grant);
    assert_eq!(st1, StatusCode::OK, "{v1}");
    assert_eq!(st2, StatusCode::CONFLICT, "{v2}");
    // Two changes of one member on the same reading: the second is refused.
    let first = set_level(&s, &s.admin, &acme, &dave_id, "container");
    let second = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        set_level(&s, TOKEN, &acme, &dave_id, "sandbox").await
    };
    let ((st1, v1), (st2, v2)) = tokio::join!(first, second);
    assert_eq!(st1, StatusCode::OK, "{v1}");
    assert_eq!(st2, StatusCode::CONFLICT, "{v2}");
    s.master.set_level_write_delay(Duration::ZERO);
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}/members"),
        None,
        &s.admin,
    )
    .await;
    let dave = v["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["user_id"] == dave_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(dave["exec_level"], "container", "{v}");
}

/// Review L2-7 (decided): a member who is not an admin of the workspace sees
/// only their own level; admins see every member's, as set and as it
/// counts (an owner at a none ceiling counts as none, not host).
#[tokio::test]
async fn the_member_list_shows_levels_to_admins_and_each_member_their_own() {
    let s = start().await;
    let (gus, gus_id) = account(&s, "gus", "sandbox").await;
    let (_dan, dan_id) = account(&s, "dan", "host").await;
    let (_olga, olga_id) = account(&s, "olga", "none").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "gus", "guest").await;
    add(&s, &s.admin, &acme, "dan", "member").await;
    add(&s, &s.admin, &acme, "olga", "member").await;
    set_level(&s, &s.admin, &acme, &gus_id, "host").await;
    set_level(&s, &s.admin, &acme, &dan_id, "host").await;
    call(
        &s,
        Method::PATCH,
        &format!("/api/workspaces/{acme}/members/{olga_id}"),
        Some(json!({"role": "owner"})),
        &s.admin,
    )
    .await;
    let row = |v: &Value, id: &str| {
        v["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["user_id"] == id)
            .unwrap()
            .clone()
    };
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}/members"),
        None,
        &gus,
    )
    .await;
    assert_eq!(row(&v, &dan_id).get("exec_level"), None, "{v}");
    assert_eq!(row(&v, &dan_id).get("effective_level"), None, "{v}");
    // His own: set host here, sandbox as it counts (his ceiling).
    assert_eq!(row(&v, &gus_id)["effective_level"], "sandbox", "{v}");
    let (_, v) = call(
        &s,
        Method::GET,
        &format!("/api/workspaces/{acme}/members"),
        None,
        &s.admin,
    )
    .await;
    assert_eq!(row(&v, &dan_id)["exec_level"], "host", "{v}");
    assert_eq!(row(&v, &dan_id)["effective_level"], "host", "{v}");
    assert_eq!(row(&v, &olga_id)["exec_level"], "host", "{v}");
    assert_eq!(row(&v, &olga_id)["effective_level"], "none", "{v}");
}

/// Review L2's model: the send_command check and create_session's second
/// check hold for a caller that skips HTTP. Bob's host chat, once his level
/// there is sandbox: Master::prompt, an allowing respond_approval and a
/// direct create_session of a host chat are refused with Forbidden.
#[tokio::test]
async fn the_master_refuses_direct_callers_too() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "host").await;
    let sid = created(&s, &bob, &acme, Some("host")).await;
    let approval = waiting_turn(&s, &bob, &sid, 1).await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    wait_stopped(&s, &bob, &sid).await;
    let id = sid.parse().unwrap();
    let r = s
        .master
        .prompt(id, "hi".into(), PromptOpts::default())
        .await;
    assert!(matches!(r, Err(CommandError::Forbidden(_))), "{r:?}");
    let r = s
        .master
        .respond_approval(id, approval, harness_core::policy::Decision::Allow)
        .await;
    assert!(matches!(r, Err(CommandError::Forbidden(_))), "{r:?}");
    let r = s.master.set_agent(id, "coder").await;
    assert!(matches!(r, Err(CommandError::Forbidden(_))), "{r:?}");
    let r = s
        .master
        .create_session(CreateSpec {
            node_id: Some(BOTH.into()),
            title: "x".into(),
            cwd: s.ws.display().to_string(),
            owner_id: Some(bob_id.clone()),
            workspace_id: Some(acme.clone()),
            exec_mode: Some(harness_core::policy::ExecMode::Host),
            ..Default::default()
        })
        .await;
    assert!(matches!(r, Err(CommandError::Forbidden(_))), "{r:?}");
}

/// RN-1 across workspaces: lowering bob's level in Acme to none stops his
/// Acme turn and leaves his Personal and his Beta sandbox turns running.
#[tokio::test]
async fn lowering_in_one_team_leaves_personal_and_other_teams_running() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    let beta = team(&s, &s.admin, "Beta").await;
    for w in [&acme, &beta] {
        add(&s, &s.admin, w, "bob", "member").await;
        set_level(&s, &s.admin, w, &bob_id, "sandbox").await;
    }
    let mine = personal(&s, &bob).await;
    let a = created(&s, &bob, &acme, None).await;
    let b = created(&s, &bob, &beta, None).await;
    let p = created(&s, &bob, &mine, None).await;
    for sid in [&a, &b, &p] {
        waiting_turn(&s, &bob, sid, 1).await;
    }
    let (st, v) = set_level(&s, &s.admin, &acme, &bob_id, "none").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    wait_stopped(&s, &bob, &a).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    for (sid, what) in [(&b, "Beta"), (&p, "Personal")] {
        assert_eq!(
            session(&s, &bob, sid).await["running"],
            true,
            "{what} stopped"
        );
    }
}

/// An owner demoted to member falls to their stored level here (none) and
/// their running host turn in the team stops; their Personal host turn,
/// at their ceiling, goes on.
#[tokio::test]
async fn a_demoted_owners_team_host_turn_stops_and_personal_goes_on() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    let role = |r: &'static str| {
        let (s, acme, id) = (&s, acme.clone(), bob_id.clone());
        async move {
            let (st, v) = call(
                s,
                Method::PATCH,
                &format!("/api/workspaces/{acme}/members/{id}"),
                Some(json!({ "role": r })),
                &s.admin,
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
        }
    };
    role("owner").await;
    let t = created(&s, &bob, &acme, Some("host")).await;
    let mine = personal(&s, &bob).await;
    let p = created(&s, &bob, &mine, Some("host")).await;
    waiting_turn(&s, &bob, &t, 1).await;
    waiting_turn(&s, &bob, &p, 1).await;
    role("member").await;
    wait_stopped(&s, &bob, &t).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        session(&s, &bob, &p).await["running"],
        true,
        "Personal stopped"
    );
}
