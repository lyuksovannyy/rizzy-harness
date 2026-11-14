//! Independent review of N.10a part 2 (execution levels per member).
//! Copied into crates/master/tests/ by run.sh, run, then removed.
//! Each test states the property it measures; a test named `gap_*` passes
//! when the gap is present (it documents behaviour, not a wish).

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
const BOTH: &str = "node-both1";

struct Server {
    base: String,
    ws: std::path::PathBuf,
    admin: String,
    admin_id: String,
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

async fn start() -> Server {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
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

async fn call(s: &Server, m: Method, path: &str, body: Option<Value>, auth: &str) -> (StatusCode, Value) {
    let (st, _, v) = send(s, m, path, body, Some(auth)).await;
    (st, v)
}

async fn account(s: &Server, name: &str, ceiling: &str) -> (String, String) {
    let (st, v) = call(s, Method::POST, "/api/users", Some(json!({"username": name})), &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user_id"].as_str().unwrap().to_string();
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
    let p = format!("/api/users/{id}/exec-ceiling");
    call(s, Method::PUT, &p, Some(json!({ "level": level })), who).await
}

async fn set_level(s: &Server, who: &str, ws: &str, id: &str, level: &str) -> (StatusCode, Value) {
    let p = format!("/api/workspaces/{ws}/members/{id}/exec-level");
    call(s, Method::PUT, &p, Some(json!({ "level": level })), who).await
}

async fn set_role(s: &Server, who: &str, ws: &str, id: &str, role: &str) -> (StatusCode, Value) {
    let p = format!("/api/workspaces/{ws}/members/{id}");
    call(s, Method::PATCH, &p, Some(json!({ "role": role })), who).await
}

async fn team(s: &Server, who: &str, name: &str) -> String {
    let (st, v) = call(s, Method::POST, "/api/workspaces", Some(json!({ "name": name })), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["workspace_id"].as_str().unwrap().to_string()
}

async fn add(s: &Server, who: &str, ws: &str, name: &str, role: &str) {
    let p = format!("/api/workspaces/{ws}/members");
    let (st, v) = call(s, Method::POST, &p, Some(json!({ "username": name, "role": role })), who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn personal(s: &Server, who: &str) -> String {
    let (_, v) = call(s, Method::GET, "/api/workspaces", None, who).await;
    let first = &v["workspaces"][0];
    assert_eq!(first["personal"], true, "{v}");
    first["workspace_id"].as_str().unwrap().to_string()
}

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
    let (_, v) = call(s, Method::GET, &format!("/api/sessions/{sid}/events"), None, who).await;
    v["events"].as_array().cloned().unwrap_or_default()
}

async fn nth_event(s: &Server, who: &str, sid: &str, kind: &str, n: usize) -> Value {
    for _ in 0..500 {
        let found: Vec<Value> = events(s, who, sid).await.into_iter().filter(|e| e["kind"] == kind).collect();
        if found.len() >= n {
            return found[n - 1].clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no {kind} #{n} in {sid}");
}

async fn session(s: &Server, who: &str, sid: &str) -> Value {
    call(s, Method::GET, &format!("/api/sessions/{sid}"), None, who).await.1
}

async fn waiting_turn(s: &Server, who: &str, sid: &str, n: usize) -> String {
    let (st, v) = post(s, who, format!("/api/sessions/{sid}/prompt"), json!({"text": "run: echo hi"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let asked = nth_event(s, who, sid, "approval_requested", n).await;
    assert_eq!(session(s, who, sid).await["running"], true);
    asked["payload"]["approval_id"].as_str().unwrap().to_string()
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

async fn still_running(s: &Server, who: &str, sid: &str) -> bool {
    tokio::time::sleep(Duration::from_millis(500)).await;
    session(s, who, sid).await["running"] == true
}

/// RN-1 across workspaces: lowering bob's level in Acme to none stops his
/// Acme turn and leaves his Personal sandbox turn and his Beta sandbox
/// turn running.
#[tokio::test]
async fn lowering_in_one_team_leaves_personal_and_other_teams_running() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    let beta = team(&s, &s.admin, "Beta").await;
    for w in [&acme, &beta] {
        add(&s, &s.admin, w, "bob", "member").await;
        let (st, v) = set_level(&s, &s.admin, w, &bob_id, "sandbox").await;
        assert_eq!(st, StatusCode::OK, "{v}");
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
    assert!(still_running(&s, &bob, &b).await, "Beta stopped");
    assert!(still_running(&s, &bob, &p).await, "Personal stopped");
}

/// The send_command check holds without the HTTP gate: Master::prompt,
/// Master::respond_approval(allow) and Master::create_session called
/// directly for an owner whose level no longer allows the mode are refused
/// with CommandError::Forbidden.
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
    let r = s.master.prompt(id, "hi".into(), PromptOpts::default()).await;
    assert!(matches!(r, Err(CommandError::Forbidden(_))), "{r:?}");
    let r = s
        .master
        .respond_approval(id, approval, harness_core::policy::Decision::Allow)
        .await;
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

/// Owner leaves / is demoted: a co-owner demoted to member falls to their
/// stored level (none here) and their running host turn in the team stops;
/// their Personal host turn (ceiling host) keeps running.
#[tokio::test]
async fn a_demoted_owners_team_host_turn_stops_and_personal_goes_on() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    let (st, v) = set_role(&s, &s.admin, &acme, &bob_id, "owner").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["exec_level"], "host", "{v}");
    let t = created(&s, &bob, &acme, Some("host")).await;
    let mine = personal(&s, &bob).await;
    let p = created(&s, &bob, &mine, Some("host")).await;
    waiting_turn(&s, &bob, &t, 1).await;
    waiting_turn(&s, &bob, &p, 1).await;
    let (st, v) = set_role(&s, &s.admin, &acme, &bob_id, "member").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["exec_level"], "none", "{v}");
    wait_stopped(&s, &bob, &t).await;
    assert!(still_running(&s, &bob, &p).await, "Personal host turn stopped");
}

/// The co-owner question, measured. Bob (ceiling sandbox) makes a team,
/// adds friend (ceiling host) and makes them co-owner: friend gets host in
/// Bob's team, which Bob could not grant directly. Friend could already
/// run host in their own Personal workspace on the same node, so today the
/// gain is only that friend's host chats (and those of members friend now
/// grants host) live in Bob's team. Friend, now owner, grants host to
/// carol (ceiling host); Bob still cannot.
#[tokio::test]
async fn gap_coowner_gives_host_in_team_but_nothing_beyond_the_ceiling_today() {
    let s = start().await;
    let (bob, _bob_id) = account(&s, "bob", "sandbox").await;
    let (friend, friend_id) = account(&s, "friend", "host").await;
    let (_carol, carol_id) = account(&s, "carol", "host").await;
    let own = team(&s, &bob, "Bobs").await;
    add(&s, &bob, &own, "friend", "member").await;
    add(&s, &bob, &own, "carol", "member").await;
    let (st, _) = set_level(&s, &bob, &own, &friend_id, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = set_role(&s, &bob, &own, &friend_id, "owner").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["exec_level"], "host", "{v}");
    created(&s, &friend, &own, Some("host")).await;
    let fp = personal(&s, &friend).await;
    created(&s, &friend, &fp, Some("host")).await;
    let (st, v) = set_level(&s, &friend, &own, &carol_id, "host").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Bob, still owner at a sandbox ceiling, cannot do what friend did.
    let (st, _) = set_level(&s, &bob, &own, &carol_id, "sandbox").await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = set_level(&s, &bob, &own, &carol_id, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

/// Guests: Perm::Agentic is gone; a guest given sandbox runs agent chats,
/// and demoting a member at sandbox to guest does not stop their turn.
#[tokio::test]
async fn gap_a_guest_with_a_level_runs_agent_chats_and_demotion_to_guest_stops_nothing() {
    let s = start().await;
    let (gus, gus_id) = account(&s, "gus", "sandbox").await;
    let (mel, mel_id) = account(&s, "mel", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "gus", "guest").await;
    add(&s, &s.admin, &acme, "mel", "member").await;
    let (st, v) = set_level(&s, &s.admin, &acme, &gus_id, "sandbox").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let g = created(&s, &gus, &acme, None).await;
    waiting_turn(&s, &gus, &g, 1).await;
    set_level(&s, &s.admin, &acme, &mel_id, "sandbox").await;
    let m = created(&s, &mel, &acme, None).await;
    waiting_turn(&s, &mel, &m, 1).await;
    let (st, v) = set_role(&s, &s.admin, &acme, &mel_id, "guest").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(still_running(&s, &mel, &m).await, "guest demotion stopped the turn");
}

/// Deleting a team does not stop its members' running chats (they move to
/// Personal at the ceiling), while removing one member does stop theirs.
#[tokio::test]
async fn gap_workspace_deletion_does_not_stop_what_removal_stops() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    let sid = created(&s, &bob, &acme, None).await;
    waiting_turn(&s, &bob, &sid, 1).await;
    let (st, v) = call(&s, Method::DELETE, &format!("/api/workspaces/{acme}"), None, &s.admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(still_running(&s, &bob, &sid).await, "deletion stopped the turn");
}

/// A member whose team level was lowered to none leaves the team: the chat
/// the team blocked is back in Personal and takes a prompt again (T11's
/// P-4, now with levels).
#[tokio::test]
async fn gap_leaving_unblocks_a_chat_the_team_blocked() {
    let s = start().await;
    let (bob, bob_id) = account(&s, "bob", "sandbox").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "bob", "member").await;
    set_level(&s, &s.admin, &acme, &bob_id, "sandbox").await;
    let sid = created(&s, &bob, &acme, None).await;
    set_level(&s, &s.admin, &acme, &bob_id, "none").await;
    let (st, _) = post(&s, &bob, format!("/api/sessions/{sid}/prompt"), json!({"text": "hi"})).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, v) = call(&s, Method::DELETE, &format!("/api/workspaces/{acme}/members/{bob_id}"), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = post(&s, &bob, format!("/api/sessions/{sid}/prompt"), json!({"text": "hi"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// What a guest sees: every member's level in the member list.
#[tokio::test]
async fn gap_the_member_list_shows_every_level_to_a_guest() {
    let s = start().await;
    let (gus, _) = account(&s, "gus", "none").await;
    let (_dan, dan_id) = account(&s, "dan", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "gus", "guest").await;
    add(&s, &s.admin, &acme, "dan", "member").await;
    set_level(&s, &s.admin, &acme, &dan_id, "host").await;
    let (st, v) = call(&s, Method::GET, &format!("/api/workspaces/{acme}/members"), None, &gus).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let dan = v["members"].as_array().unwrap().iter().find(|m| m["user_id"] == dan_id.as_str()).unwrap().clone();
    assert_eq!(dan["exec_level"], "host", "{v}");
    // The owner shows host whatever their ceiling (alex is a server admin
    // here, so host is also true for him; the shape is what is measured).
    assert!(v.to_string().contains("exec_level"));
}

/// A workspace admin at sandbox lowers a member the owner granted host to
/// none: lowering someone above you is allowed.
#[tokio::test]
async fn gap_an_admin_below_host_can_undo_an_owners_host_grant() {
    let s = start().await;
    let (carol, carol_id) = account(&s, "carol", "host").await;
    let (_dave, dave_id) = account(&s, "dave", "host").await;
    let acme = team(&s, &s.admin, "Acme").await;
    add(&s, &s.admin, &acme, "carol", "admin").await;
    add(&s, &s.admin, &acme, "dave", "member").await;
    set_level(&s, &s.admin, &acme, &carol_id, "sandbox").await;
    set_level(&s, &s.admin, &acme, &dave_id, "host").await;
    let (st, v) = set_level(&s, &carol, &acme, &dave_id, "none").await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// The token's view: TOKEN acting on member chat is the member's answer;
/// a chat with no owner (the token's own) is host and takes a host
/// prompt.
#[tokio::test]
async fn the_tokens_own_host_chat_works() {
    let s = start().await;
    let acme = team(&s, &s.admin, "Acme").await;
    let sid = created(&s, TOKEN, &acme, Some("host")).await;
    let (st, v) = post(&s, TOKEN, format!("/api/sessions/{sid}/prompt"), json!({"text": "hi"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}
