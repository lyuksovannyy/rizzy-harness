//! FIX_PLAN 4.5 slice 4 (`docs/store-readers.md` §5): the requests that
//! once read every chat, or one store call per member or per hit, run a
//! fixed number of statements whatever the data, and still show only what
//! the caller may see.
//!
//! Statements are counted on the mirror's connection while one request
//! runs (`Store::count_statements`, feature `test-support`); the smallest
//! of three runs is kept, so a background task that happens to touch the
//! store cannot make a run look bigger.

use crate::common;
use common::{Login as _, SameOrigin as _};

use harness_core::event::{EventKind, Profile, SessionCreated};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::router;
use harness_store::{NewCard, NewColumn, NewProject, NewUser, Role, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const NODE: &str = "local";

struct T {
    base: String,
    store: Store,
    admin: String,
    admin_id: String,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth = Auth::new(
        Secret::new("api-secret".into()),
        vec![addr.to_string()],
        false,
    )
    .with_claim(Claim {
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
        admin_id: String::new(),
        _dir: dir,
    };
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = common::set_cookie(&r).unwrap();
    let body: Value = r.json().await.unwrap();
    common::remember(&cookie, &body);
    t.admin = cookie;
    t.admin_id = get(&t, "/api/me").await["user"]["user_id"]
        .as_str()
        .unwrap()
        .to_string();
    t
}

/// GET as the admin; must succeed.
async fn get(t: &T, path: &str) -> Value {
    let r = reqwest::Client::new()
        .request(Method::GET, format!("{}{path}", t.base))
        .login(&t.admin)
        .same_origin()
        .send()
        .await
        .unwrap();
    let st = r.status();
    let v: Value = r.json().await.unwrap();
    assert_eq!(st, StatusCode::OK, "{path}: {v}");
    v
}

/// The statements one GET runs on the mirror: the smallest of three runs.
async fn statements(t: &T, path: &str) -> (usize, Value) {
    let mut best = usize::MAX;
    let mut last = Value::Null;
    for _ in 0..3 {
        let c = t.store.count_statements();
        last = get(t, path).await;
        best = best.min(c.count());
    }
    (best, last)
}

fn account(t: &T, id: &str) {
    t.store
        .create_user(&NewUser {
            user_id: id.into(),
            username: id.into(),
            password_hash: "$argon2id$x".into(),
            role: Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::None,
            must_change_password: false,
        })
        .unwrap();
    t.store.personal_workspace(id).unwrap();
}

/// A chat of `owner` titled `title`, written as a node's event would be.
fn chat(t: &T, owner: &str, ts: i64, title: &str) -> SessionId {
    let sid = SessionId::new();
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        forked: None,
        exec_mode: None,
        no_callbacks: false,
        profile: Profile::Chat,
        agent_id: None,
        incognito: false,
        parent_session: None,
        title: title.into(),
        cwd: String::new(),
        model: "m".into(),
        backend: "native".into(),
        node_id: NODE.into(),
        approval_mode: Default::default(),
        sub_folder: None,
    })
    .into_parts();
    t.store
        .append(
            NODE,
            &RawEvent {
                session_id: sid,
                seq: 1,
                ts_ms: ts,
                kind,
                payload,
            },
        )
        .unwrap();
    t.store.claim_session(sid, Some(owner), None).unwrap();
    sid
}

// ---- the members roster ----

#[tokio::test]
async fn a_rosters_statements_do_not_grow_with_its_members() {
    let t = start().await;
    let ws = t
        .store
        .create_workspace("Team", &t.admin_id)
        .unwrap()
        .workspace_id;
    let path = format!("/api/workspaces/{ws}/members");
    account(&t, "m0");
    t.store.add_member(&ws, "m0", "member").unwrap();
    let (small, v) = statements(&t, &path).await;
    assert_eq!(v["members"].as_array().unwrap().len(), 2);
    for i in 1..20 {
        let id = format!("m{i}");
        account(&t, &id);
        t.store.add_member(&ws, &id, "member").unwrap();
        if i % 2 == 0 {
            let prefs = harness_master::prefs::Prefs {
                display_name: Some(format!("Member {i}")),
                ..Default::default()
            };
            t.store
                .kv_set(
                    &format!("settings:{id}"),
                    &serde_json::to_string(&prefs).unwrap(),
                )
                .unwrap();
        }
    }
    let (large, v) = statements(&t, &path).await;
    let members = v["members"].as_array().unwrap();
    assert_eq!(members.len(), 21);
    // What each member's own row read before: their display name.
    let m2 = members.iter().find(|m| m["user_id"] == "m2").unwrap();
    assert_eq!(m2["display_name"], "Member 2");
    let m3 = members.iter().find(|m| m["user_id"] == "m3").unwrap();
    assert_eq!(m3["display_name"], Value::Null);
    assert_eq!(
        small, large,
        "a roster of 2 ran {small} statements, of 21 {large}"
    );
    eprintln!("roster: {large} statements (2 and 21 members)");
}

// ---- a project's board ----

#[tokio::test]
async fn a_boards_statements_do_not_grow_with_its_cards_and_others_chats_stay_private() {
    let t = start().await;
    account(&t, "bob");
    let ws = t.store.personal_workspace(&t.admin_id).unwrap();
    t.store
        .create_project(
            &NewProject {
                project_id: "p1".into(),
                workspace_id: ws,
                name: "p1".into(),
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
    let col = t
        .store
        .add_column(
            "p1",
            &NewColumn {
                name: "Todo".into(),
                done: false,
            },
            20,
        )
        .unwrap();
    // Every third card the admin's chat, every third bob's, every third none
    // (or a chat that does not exist).
    let card = |k: usize| {
        let linked = match k % 3 {
            0 => Some(chat(&t, &t.admin_id, k as i64, &format!("mine {k}")).to_string()),
            1 => Some(chat(&t, "bob", k as i64, &format!("bob's secret {k}")).to_string()),
            _ => k.is_multiple_of(2).then(|| SessionId::new().to_string()),
        };
        t.store
            .add_card(
                "p1",
                &NewCard {
                    card_id: format!("c{k}"),
                    column_id: col.clone(),
                    title: format!("card {k}"),
                    notes: String::new(),
                    session_id: linked,
                    created_by: None,
                    agent: None,
                },
                1_000,
            )
            .unwrap();
    };
    for k in 0..5 {
        card(k);
    }
    let (small, _) = statements(&t, "/api/projects/p1/board").await;
    for k in 5..50 {
        card(k);
    }
    let (large, v) = statements(&t, "/api/projects/p1/board").await;
    let cards = v["columns"][0]["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 50);
    for c in cards {
        let k: usize = c["card_id"].as_str().unwrap()[1..].parse().unwrap();
        match k % 3 {
            0 => assert_eq!(c["chat"]["title"], format!("mine {k}"), "{c}"),
            1 => assert_eq!(c["chat"], "private", "{c}"),
            _ => assert_eq!(c["chat"], Value::Null, "{c}"),
        }
    }
    assert!(!v.to_string().contains("secret"), "{v}");
    assert_eq!(
        small, large,
        "a board of 5 cards ran {small} statements, of 50 {large}"
    );
    eprintln!("board: {large} statements (5 and 50 cards)");
}

// ---- search ----

#[tokio::test]
async fn a_searchs_statements_do_not_grow_with_its_hits_and_stay_the_callers() {
    let t = start().await;
    account(&t, "bob");
    for k in 0..30 {
        chat(&t, "bob", k, &format!("zebra of bob {k}"));
    }
    for k in 0..5 {
        chat(&t, &t.admin_id, 100 + k, &format!("zebra {k}"));
    }
    let words = "/api/search?in=chats&limit=50&q=zebra";
    let empty = "/api/search?in=chats&limit=50";
    let (words_small, v) = statements(&t, words).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 5, "{v}");
    let (empty_small, v) = statements(&t, empty).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 5, "{v}");
    for k in 5..60 {
        chat(&t, &t.admin_id, 100 + k, &format!("zebra {k}"));
    }
    let (words_large, v) = statements(&t, words).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 50, "{v}");
    assert!(!v.to_string().contains("of bob"), "{v}");
    let (empty_large, v) = statements(&t, empty).await;
    assert_eq!(v["results"].as_array().unwrap().len(), 50, "{v}");
    assert!(!v.to_string().contains("of bob"), "{v}");
    assert_eq!(
        words_small, words_large,
        "a search with 5 hits ran {words_small} statements, with 50 {words_large}"
    );
    assert_eq!(
        empty_small, empty_large,
        "the empty search with 5 chats ran {empty_small} statements, with 50 {empty_large}"
    );
    eprintln!("search: {words_large} statements with words, {empty_large} without (5 and 50 hits)");
}

// ---- the project page's "Add a chat" (review F5 of slice 2+3) ----

/// `loose=1` pages the caller's chats in no project, past the first page,
/// never anyone else's and never one already in a project.
#[tokio::test]
async fn add_a_chat_pages_the_callers_chats_in_no_project() {
    let t = start().await;
    account(&t, "bob");
    let ws = t.store.personal_workspace(&t.admin_id).unwrap();
    t.store
        .create_project(
            &NewProject {
                project_id: "p1".into(),
                workspace_id: ws.clone(),
                name: "p1".into(),
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
    let mut loose = Vec::new();
    for k in 0..130 {
        let sid = chat(&t, &t.admin_id, k, &format!("mine {k}"));
        if k % 10 == 0 {
            t.store.set_session_project(sid, Some("p1")).unwrap();
        } else {
            loose.push(sid.to_string());
        }
        chat(&t, "bob", k, &format!("bob's {k}"));
    }
    loose.reverse();
    let mut got = Vec::new();
    let mut path = format!("/api/sessions?workspace={ws}&loose=1&limit=50");
    loop {
        let v = get(&t, &path).await;
        for r in v["sessions"].as_array().unwrap() {
            assert_eq!(r["project_id"], Value::Null, "{r}");
            got.push(r["session_id"].as_str().unwrap().to_string());
        }
        match v["next"].as_str() {
            Some(n) => path = format!("/api/sessions?workspace={ws}&loose=1&limit=50&after={n}"),
            None => break,
        }
    }
    assert_eq!(got, loose);
    // Not with a project.
    let r = reqwest::Client::new()
        .get(format!("{}/api/sessions?loose=1&project=p1", t.base))
        .login(&t.admin)
        .same_origin()
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}
