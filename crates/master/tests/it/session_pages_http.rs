//! The paged chat list (`GET /api/sessions`, FIX_PLAN 4.5 slice 2,
//! `docs/store-readers.md` §4 and §6): the owner comes from the caller
//! alone, a cursor only moves the start inside the caller's own chats, a
//! project filter never widens the owner, and every chat is listed exactly
//! once across pages.
//!
//! Chats are written straight into the mirror (as a node's events would),
//! so their times, ties, parents and links are exact.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashSet;

use harness_core::event::{AssistantDelta, EventKind, Profile, SessionCreated};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::router;
use harness_store::NewProject;
use harness_store::Store;
use harness_store::routines::{NewRoutine, NewRun, RoutineCaps, RunPatch};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const TOKEN: &str = "api-secret";
const NODE: &str = "local";

struct T {
    base: String,
    store: Store,
    admin: String,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new(TOKEN.into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let store = Store::open_in_memory().unwrap();
    let master = harness_master::Master::new(store.clone(), auth);
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    let mut t = T {
        base: format!("http://{addr}"),
        store,
        admin: String::new(),
        _dir: dir,
    };
    let (st, cookie, v) = send(
        &t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.admin = cookie.unwrap();
    t
}

async fn send(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
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
    let cookie = common::set_cookie(&r);
    let text = r.text().await.unwrap();
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    if let Some(c) = &cookie {
        common::remember(c, &body);
    }
    (status, cookie, body)
}

async fn get(t: &T, path: &str, auth: &str) -> (StatusCode, Value) {
    let (st, _, v) = send(t, Method::GET, path, None, Some(auth)).await;
    (st, v)
}

/// A page that must succeed.
async fn page(t: &T, path: &str, auth: &str) -> Value {
    let (st, v) = get(t, path, auth).await;
    assert_eq!(st, StatusCode::OK, "{path}: {v}");
    v
}

/// An account made by the admin, signed in with its own password:
/// (cookie, user id).
async fn user(t: &T, name: &str) -> (String, String) {
    let (st, _, v) = send(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "exec_ceiling": "none"})),
        Some(&t.admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    let id = v["user_id"].as_str().unwrap().to_string();
    let (_, temp, _) = send(
        t,
        Method::POST,
        "/api/login",
        Some(json!({"username": name, "password": pw})),
        None,
    )
    .await;
    let (st, cookie, v) = send(
        t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "a password of my own"})),
        temp.as_deref(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (cookie.unwrap(), id)
}

async fn me(t: &T, cookie: &str) -> String {
    page(t, "/api/me", cookie).await["user"]["user_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn created(sid: SessionId, ts: i64, parent: Option<SessionId>, incognito: bool) -> RawEvent {
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        forked: None,
        exec_mode: None,
        no_callbacks: false,
        profile: Profile::Chat,
        agent_id: None,
        incognito,
        parent_session: parent,
        title: "t".into(),
        cwd: String::new(),
        model: "m".into(),
        backend: "native".into(),
        node_id: NODE.into(),
        approval_mode: Default::default(),
        sub_folder: None,
    })
    .into_parts();
    RawEvent {
        session_id: sid,
        seq: 1,
        ts_ms: ts,
        kind,
        payload,
    }
}

/// What a chat is made of.
#[derive(Clone, Copy, Default)]
struct Chat<'a> {
    owner: &'a str,
    ts: i64,
    /// A team workspace to map it into (`None`: the owner's Personal).
    ws: Option<&'a str>,
    parent: Option<SessionId>,
    incognito: bool,
    project: Option<&'a str>,
}

fn chat(t: &T, c: Chat<'_>) -> SessionId {
    let sid = SessionId::new();
    t.store
        .append(NODE, &created(sid, c.ts, c.parent, c.incognito))
        .unwrap();
    t.store.claim_session(sid, Some(c.owner), c.ws).unwrap();
    if let Some(p) = c.project {
        t.store.set_session_project(sid, Some(p)).unwrap();
    }
    sid
}

fn plain(t: &T, owner: &str, ts: i64) -> SessionId {
    chat(
        t,
        Chat {
            owner,
            ts,
            ..Default::default()
        },
    )
}

/// The chat's next event, at `ts`: it moves to that time in the list.
fn touch(t: &T, sid: SessionId, seq: u64, ts: i64) {
    let (kind, payload) = EventKind::AssistantDelta(AssistantDelta {
        author: None,
        variant: None,
        turn_id: "t1".into(),
        message_id: "m1".into(),
        text: "x".into(),
    })
    .into_parts();
    t.store
        .append(
            NODE,
            &RawEvent {
                session_id: sid,
                seq,
                ts_ms: ts,
                kind,
                payload,
            },
        )
        .unwrap();
}

fn project(t: &T, id: &str, ws: &str) {
    t.store
        .create_project(
            &NewProject {
                project_id: id.into(),
                workspace_id: ws.into(),
                name: id.into(),
                description: String::new(),
                icon: "folder".into(),
                accent: "blue".into(),
                instructions: String::new(),
                settings: "{}".into(),
                created_by: None,
                columns: Vec::new(),
                source: None,
            },
            100,
        )
        .unwrap();
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key}: {v}"))
        .iter()
        .map(|r| r["session_id"].as_str().unwrap().to_string())
        .collect()
}

fn next(v: &Value) -> Option<String> {
    v["next"].as_str().map(str::to_string)
}

/// Every page of `base` (which has a `?`), checking that each page with a
/// `next` is full.
async fn walk(t: &T, base: &str, limit: usize, auth: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..1_000 {
        let path = match &after {
            Some(c) => format!("{base}&limit={limit}&after={c}"),
            None => format!("{base}&limit={limit}"),
        };
        let v = page(t, &path, auth).await;
        let rows = ids(&v, "sessions");
        after = next(&v);
        if after.is_some() {
            assert_eq!(rows.len(), limit, "a page with a next is full: {path}");
        }
        out.extend(rows);
        if after.is_none() {
            return out;
        }
    }
    panic!("no end")
}

fn s(id: SessionId) -> String {
    id.to_string()
}

fn personal(t: &T, user: &str) -> String {
    t.store.personal_workspace(user).unwrap()
}

#[tokio::test]
async fn the_response_has_sessions_children_and_next() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let ws = personal(&t, &alex);
    for k in 0..3 {
        plain(&t, &alex, 100 + k);
    }
    let v = page(&t, &format!("/api/sessions?workspace={ws}"), &t.admin).await;
    assert_eq!(ids(&v, "sessions").len(), 3);
    assert_eq!(v["children"], json!([]));
    assert_eq!(v["next"], Value::Null);
    assert_eq!(v["children_truncated"], false);
    // No workspace: the active one, as before.
    let v = page(&t, "/api/sessions", &t.admin).await;
    assert_eq!(ids(&v, "sessions").len(), 3);
}

/// §6: a cursor taken from another user's rows only moves the start inside
/// the caller's own chats.
#[tokio::test]
async fn another_users_cursor_lists_only_the_callers_chats() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let (bob_c, bob) = user(&t, "bob").await;
    // Distinct times, so the position is exact.
    for k in 0..30 {
        plain(&t, &alex, 1_000 + 2 * k);
        plain(&t, &bob, 1_001 + 2 * k);
    }
    let bob_page = page(&t, "/api/sessions?limit=5", &bob_c).await;
    let cursor = next(&bob_page).unwrap();
    let theirs: HashSet<String> = walk(&t, "/api/sessions?", 7, &bob_c)
        .await
        .into_iter()
        .collect();
    assert_eq!(theirs.len(), 30);
    let v = page(
        &t,
        &format!("/api/sessions?limit=200&after={cursor}"),
        &t.admin,
    )
    .await;
    let got = ids(&v, "sessions");
    // Bob's fifth newest is at 1_051: alex's 26 chats below it.
    assert_eq!(got.len(), 26, "alex's chats after that position: {v}");
    assert!(got.iter().all(|id| !theirs.contains(id)));
    for r in v["sessions"].as_array().unwrap() {
        assert_eq!(r["owner_id"], alex.as_str());
    }
}

/// §6: a cursor from workspace A used in B lists B's chats from that
/// position and nothing of A's.
#[tokio::test]
async fn a_cursor_from_one_workspace_lists_the_other_from_that_position() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let team = t
        .store
        .create_workspace("Team", &alex)
        .unwrap()
        .workspace_id;
    let mine = personal(&t, &alex);
    let mut in_team = Vec::new();
    for k in 0..20 {
        plain(&t, &alex, 100 + 2 * k);
        in_team.push(chat(
            &t,
            Chat {
                owner: &alex,
                ts: 101 + 2 * k,
                ws: Some(&team),
                ..Default::default()
            },
        ));
    }
    let a = page(
        &t,
        &format!("/api/sessions?workspace={mine}&limit=10"),
        &t.admin,
    )
    .await;
    let cursor = next(&a).unwrap();
    // Personal's tenth newest is at 100 + 2*10 = 120: team chats below it.
    let v = page(
        &t,
        &format!("/api/sessions?workspace={team}&limit=200&after={cursor}"),
        &t.admin,
    )
    .await;
    let got = ids(&v, "sessions");
    let want: Vec<String> = in_team.iter().rev().skip(10).map(|x| s(*x)).collect();
    assert_eq!(got, want);
    for r in v["sessions"].as_array().unwrap() {
        assert_eq!(r["workspace_id"], team.as_str());
    }
}

/// §6: `limit` is clamped to 1..=200; a non-number is 400.
#[tokio::test]
async fn the_limit_is_clamped_and_a_non_number_is_refused() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    for k in 0..230 {
        plain(&t, &alex, 1_000 + k);
    }
    let n = |v: &Value| ids(v, "sessions").len();
    assert_eq!(n(&page(&t, "/api/sessions", &t.admin).await), 50);
    for (limit, want) in [
        ("1000000000", 200),
        ("200", 200),
        ("201", 200),
        ("0", 1),
        ("-5", 1),
        ("3", 3),
    ] {
        let v = page(&t, &format!("/api/sessions?limit={limit}"), &t.admin).await;
        assert_eq!(n(&v), want, "limit={limit}");
        assert!(next(&v).is_some());
    }
    for bad in ["abc", "1e3", "", "99999999999999999999999", "1.5"] {
        let (st, v) = get(&t, &format!("/api/sessions?limit={bad}"), &t.admin).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "limit={bad}: {v}");
    }
}

/// §6: a malformed or huge cursor is 400, never a list from the top.
#[tokio::test]
async fn a_malformed_cursor_is_refused() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    for k in 0..5 {
        plain(&t, &alex, 1_000 + k);
    }
    let good = next(&page(&t, "/api/sessions?limit=2", &t.admin).await).unwrap();
    let long = "A".repeat(10_000);
    for bad in [
        "",
        "x",
        "1:1000:0194f5c0-0000-7000-8000-000000000000",
        &good[..good.len() - 1],
        &format!("{good}A"),
        &long,
    ] {
        let (st, v) = get(&t, &format!("/api/sessions?after={bad}"), &t.admin).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "after={bad:.40}: {v}");
    }
    let (st, _) = get(&t, &format!("/api/sessions?after={good}"), &t.admin).await;
    assert_eq!(st, StatusCode::OK);
}

/// Seen routine run chats are left out in SQL, so they never make a page
/// shorter than its limit; `routine_runs=1` lists them.
#[tokio::test]
async fn seen_routine_runs_never_shorten_a_page() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let ws = personal(&t, &alex);
    t.store
        .routine_create(
            &NewRoutine {
                routine_id: "r1".into(),
                workspace_id: ws.clone(),
                owner_id: alex.clone(),
                name: "daily".into(),
                enabled: true,
                spec: "{}".into(),
                triggers: Vec::new(),
            },
            RoutineCaps {
                per_user: 10,
                per_workspace: 10,
            },
            0,
        )
        .unwrap();
    let mut normal = Vec::new();
    let mut runs = Vec::new();
    for k in 0..40 {
        normal.push(plain(&t, &alex, 1_000 + 2 * k));
        // The newest half of the list is seen, finished run chats.
        let run = plain(&t, &alex, 1_041 + 2 * k);
        let id = format!("run-{k}");
        t.store
            .run_start(&NewRun {
                run_id: id.clone(),
                routine_id: "r1".into(),
                trigger_id: None,
                trigger_kind: "manual".into(),
                scheduled_ms: None,
                started_ms: 0,
                status: "pending".into(),
                reason: None,
                repeats: 0,
                event: None,
            })
            .unwrap();
        t.store
            .run_update(
                &id,
                &RunPatch {
                    session_id: Some(s(run)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(t.store.run_finish(&id, "completed", None, 1).unwrap());
        assert!(t.store.mark_run_seen("r1", &id).unwrap());
        runs.push(run);
    }
    let v = page(&t, "/api/sessions?limit=10", &t.admin).await;
    let got = ids(&v, "sessions");
    assert_eq!(got.len(), 10, "{v}");
    let want: Vec<String> = normal.iter().rev().take(10).map(|x| s(*x)).collect();
    assert_eq!(got, want);
    let all = walk(&t, "/api/sessions?", 10, &t.admin).await;
    assert_eq!(all.len(), 40);
    let with_runs = walk(&t, "/api/sessions?routine_runs=1", 10, &t.admin).await;
    assert_eq!(with_runs.len(), 80);
    let v = page(&t, "/api/sessions?routine_runs=1&limit=1", &t.admin).await;
    assert_eq!(v["sessions"][0]["routine"]["run_id"], "run-39");
    assert_eq!(v["sessions"][0]["unread"], false);
}

/// Sub-agent and incognito chats are not top-level rows; every descendant
/// of a page's chats arrives in `children`, whatever its depth.
#[tokio::test]
async fn sub_agent_and_incognito_chats_are_not_rows_and_children_arrive() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let (bob_c, bob) = user(&t, "bob").await;
    let top = plain(&t, &alex, 100);
    let kid = chat(
        &t,
        Chat {
            owner: &alex,
            ts: 200,
            parent: Some(top),
            ..Default::default()
        },
    );
    let grandkid = chat(
        &t,
        Chat {
            owner: &alex,
            ts: 150,
            parent: Some(kid),
            ..Default::default()
        },
    );
    let secret = chat(
        &t,
        Chat {
            owner: &alex,
            ts: 300,
            incognito: true,
            ..Default::default()
        },
    );
    // A chat of bob's that names alex's as parent is not alex's child
    // (the mirror drops such a parent: it is a top-level chat of bob's).
    let bobs = chat(
        &t,
        Chat {
            owner: &bob,
            ts: 250,
            parent: Some(top),
            ..Default::default()
        },
    );
    let v = page(&t, "/api/sessions", &t.admin).await;
    assert_eq!(ids(&v, "sessions"), vec![s(top)]);
    assert_eq!(ids(&v, "children"), vec![s(kid), s(grandkid)]);
    assert!(!ids(&v, "sessions").contains(&s(secret)));
    assert_eq!(v["children"][0]["parent_id"], s(top).as_str());
    assert_eq!(v["children"][1]["parent_id"], s(kid).as_str());
    let v = page(&t, "/api/sessions", &bob_c).await;
    assert_eq!(ids(&v, "sessions"), vec![s(bobs)], "{v}");
    assert_eq!(v["children"], json!([]));
}

/// Exactly once through the HTTP layer: 120 chats in runs of equal times
/// come back as 50 + 50 + 20, none twice, none lost, newest first.
#[tokio::test]
async fn paging_lists_every_chat_exactly_once_with_ties() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let (_, bob) = user(&t, "bob").await;
    let mut made = Vec::new();
    for k in 0..120 {
        made.push((1_000 + k / 7, s(plain(&t, &alex, 1_000 + k / 7))));
        // Another owner's chats at the same times.
        if k % 4 == 0 {
            plain(&t, &bob, 1_000 + k / 7);
        }
    }
    let p1 = page(&t, "/api/sessions?limit=50", &t.admin).await;
    let p2 = page(
        &t,
        &format!("/api/sessions?limit=50&after={}", next(&p1).unwrap()),
        &t.admin,
    )
    .await;
    let p3 = page(
        &t,
        &format!("/api/sessions?limit=50&after={}", next(&p2).unwrap()),
        &t.admin,
    )
    .await;
    assert_eq!(next(&p3), None);
    let got: Vec<String> = [&p1, &p2, &p3]
        .iter()
        .flat_map(|p| ids(p, "sessions"))
        .collect();
    assert_eq!(
        [&p1, &p2, &p3].map(|p| ids(p, "sessions").len()),
        [50, 50, 20]
    );
    made.sort_by(|a, b| b.cmp(a));
    let want: Vec<String> = made.into_iter().map(|(_, id)| id).collect();
    assert_eq!(got, want);
}

/// A chat that moves to the top between two page reads is passed over by
/// the walk under way (it is listed by the next read from the top); no
/// other chat is repeated or lost.
#[tokio::test]
async fn a_page_after_a_chat_moved_to_the_top() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let mut made = Vec::new();
    for k in 0..30 {
        made.push(plain(&t, &alex, 1_000 + k));
    }
    let p1 = page(&t, "/api/sessions?limit=10", &t.admin).await;
    // The oldest chat, on the third page, becomes the newest.
    let moved = made[0];
    touch(&t, moved, 2, 5_000);
    let mut all = ids(&p1, "sessions");
    let mut after = next(&p1);
    while let Some(c) = after {
        let v = page(&t, &format!("/api/sessions?limit=10&after={c}"), &t.admin).await;
        all.extend(ids(&v, "sessions"));
        after = next(&v);
    }
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(unique.len(), all.len(), "none twice");
    assert_eq!(all.len(), 29, "only the moved chat is passed over");
    assert!(!all.contains(&s(moved)));
    let top = page(&t, "/api/sessions?limit=1", &t.admin).await;
    assert_eq!(ids(&top, "sessions"), vec![s(moved)]);
}

/// `project=`: the project's own newest chats, the caller's only. A
/// project of another workspace, one the caller cannot see, or one that
/// does not exist answers exactly alike.
#[tokio::test]
async fn a_project_page_is_the_callers_chats_in_a_project_it_may_see() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let (bob_c, bob) = user(&t, "bob").await;
    let (carol_c, carol) = user(&t, "carol").await;
    let team = t
        .store
        .create_workspace("Team", &alex)
        .unwrap()
        .workspace_id;
    t.store.add_member(&team, &bob, "member").unwrap();
    let elsewhere = t
        .store
        .create_workspace("Else", &carol)
        .unwrap()
        .workspace_id;
    project(&t, "pt", &team);
    project(&t, "pbob", &personal(&t, &bob));
    project(&t, "pcarol", &elsewhere);
    let mut alex_in_pt = Vec::new();
    for k in 0..12 {
        alex_in_pt.push(chat(
            &t,
            Chat {
                owner: &alex,
                ts: 1_000 + k,
                ws: Some(&team),
                project: Some("pt"),
                ..Default::default()
            },
        ));
        chat(
            &t,
            Chat {
                owner: &bob,
                ts: 1_000 + k,
                ws: Some(&team),
                project: Some("pt"),
                ..Default::default()
            },
        );
        // Not in the project: never on its page.
        chat(
            &t,
            Chat {
                owner: &alex,
                ts: 2_000 + k,
                ws: Some(&team),
                ..Default::default()
            },
        );
    }
    chat(
        &t,
        Chat {
            owner: &bob,
            ts: 1_000,
            project: Some("pbob"),
            ..Default::default()
        },
    );
    chat(
        &t,
        Chat {
            owner: &carol,
            ts: 1_000,
            ws: Some(&elsewhere),
            project: Some("pcarol"),
            ..Default::default()
        },
    );
    let base = format!("/api/sessions?workspace={team}&project=pt");
    let got = walk(&t, &base, 5, &t.admin).await;
    let want: Vec<String> = alex_in_pt.iter().rev().map(|x| s(*x)).collect();
    assert_eq!(got, want, "alex's, never bob's");
    let v = page(&t, &format!("{base}&limit=200"), &t.admin).await;
    for r in v["sessions"].as_array().unwrap() {
        assert_eq!(r["owner_id"], alex.as_str());
        assert_eq!(r["project_id"], "pt");
    }
    let bobs = walk(&t, &base, 5, &bob_c).await;
    assert_eq!(bobs.len(), 12);
    assert!(bobs.iter().all(|id| !got.contains(id)));

    // Refused alike: bob's Personal project, carol's workspace's project,
    // the team's project asked for in another workspace, and none at all.
    let mine = personal(&t, &alex);
    let (st, missing) = get(
        &t,
        &format!("/api/sessions?workspace={team}&project=no-such"),
        &t.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    for path in [
        format!("/api/sessions?workspace={team}&project=pbob"),
        format!("/api/sessions?workspace={team}&project=pcarol"),
        format!("/api/sessions?workspace={mine}&project=pt"),
        "/api/sessions?project=pcarol".to_string(),
    ] {
        let (st, v) = get(&t, &path, &t.admin).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}: {v}");
        assert_eq!(v, missing, "{path}");
    }
    // Carol is not in the team: its project is unknown to her too.
    let (st, v) = get(
        &t,
        &format!("/api/sessions?workspace={elsewhere}&project=pt"),
        &carol_c,
    )
    .await;
    assert_eq!((st, v), (StatusCode::NOT_FOUND, missing.clone()));
    // A project id never widens the owner, even for a server admin.
    let v = page(
        &t,
        &format!(
            "/api/sessions?workspace={}&project=pbob",
            personal(&t, &bob)
        ),
        &bob_c,
    )
    .await;
    assert_eq!(ids(&v, "sessions").len(), 1);
    let (st, _) = get(
        &t,
        &format!(
            "/api/sessions?workspace={}&project=pbob",
            personal(&t, &bob)
        ),
        &t.admin,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "alex is not in bob's Personal");
}

/// The API token lists every owner's chats, paged the same way.
#[tokio::test]
async fn the_api_token_lists_every_owner() {
    let t = start().await;
    let alex = me(&t, &t.admin).await;
    let (_, bob) = user(&t, "bob").await;
    for k in 0..15 {
        plain(&t, &alex, 1_000 + k);
        plain(&t, &bob, 1_000 + k);
    }
    let all = walk(&t, "/api/sessions?", 7, TOKEN).await;
    assert_eq!(all.len(), 30);
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(unique.len(), 30);
}
