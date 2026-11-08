//! The search panel's API (chats, projects, the All tab, paging, privacy),
//! similar-chat suggestions, the per-viewer feature flags and the
//! classifier store.

use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, ModelInfo, router};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct T {
    base: String,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            Vec::new(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
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
    master.set_models(vec![
        ModelInfo {
            id: "mock".into(),
            provider: "mock".into(),
            effort: false,
            vision: false,
            default: true,
            ..Default::default()
        },
        ModelInfo {
            id: "thinker".into(),
            provider: "mock".into(),
            effort: true,
            vision: false,
            default: false,
            ..Default::default()
        },
    ]);
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        _dir: dir,
    }
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb = match cookie {
        Some(c) => rb.header("cookie", c),
        None => rb.bearer_auth("tok"),
    };
    let r = rb.send().await.unwrap();
    let status = r.status();
    let c = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, c, r.json().await.unwrap_or(Value::Null))
}

async fn login_admin(t: &T) -> String {
    let (_, c, _) = req(
        t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    c.unwrap()
}

async fn chat(t: &T, cookie: &str, text: &str) -> String {
    chat_in(t, cookie, None, text, &[text]).await
}

/// A chat titled `title` in `ws` (`None`: the active workspace), sent
/// `prompts` one after another; returns once each has its mock reply.
async fn chat_in(t: &T, cookie: &str, ws: Option<&str>, title: &str, prompts: &[&str]) -> String {
    let mut body = json!({"profile": "chat", "title": title});
    if let Some(ws) = ws {
        body["workspace_id"] = json!(ws);
    }
    let (st, _, v) = req(t, Method::POST, "/api/sessions", Some(body), Some(cookie)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    for (n, text) in prompts.iter().enumerate() {
        let (st, _, v) = req(
            t,
            Method::POST,
            &format!("/api/sessions/{sid}/prompt"),
            Some(json!({"text": text})),
            Some(cookie),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        wait_replies(t, cookie, &sid, n + 1).await;
    }
    sid
}

/// Wait until chat `sid` has `n` assistant replies mirrored (and indexed)
/// and is idle.
async fn wait_replies(t: &T, cookie: &str, sid: &str, n: usize) {
    for _ in 0..300 {
        let (_, _, row) = req(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}"),
            None,
            Some(cookie),
        )
        .await;
        let (_, _, ev) = req(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}/events?after=0"),
            None,
            Some(cookie),
        )
        .await;
        let replies = ev["events"]
            .as_array()
            .map(|e| {
                e.iter()
                    .filter(|e| e["kind"] == "assistant_message")
                    .count()
            })
            .unwrap_or(0);
        if row["running"] == false && replies >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("chat {sid} never got {n} replies");
}

/// An account created by the admin, logged in with its own password.
async fn new_user(t: &T, admin: &str, name: &str) -> String {
    let (st, _, v) = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "password": "temporary pw"})),
        Some(admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, temp, _) = req(
        t,
        Method::POST,
        "/api/login",
        Some(json!({"username": name, "password": "temporary pw"})),
        None,
    )
    .await;
    let (st, c, v) = req(
        t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "a password of my own"})),
        temp.as_deref(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    c.unwrap()
}

/// A team workspace of `owner` with `members` as (username, role).
async fn team(t: &T, owner: &str, name: &str, members: &[(&str, &str)]) -> String {
    let (st, _, v) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        Some(owner),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let ws = v["workspace_id"].as_str().unwrap().to_string();
    for (username, role) in members {
        let (st, _, v) = req(
            t,
            Method::POST,
            &format!("/api/workspaces/{ws}/members"),
            Some(json!({ "username": username, "role": role })),
            Some(owner),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    ws
}

/// A project in `ws` (`None`: the active workspace); returns its id.
async fn project(t: &T, cookie: &str, ws: Option<&str>, name: &str, description: &str) -> String {
    let mut body = json!({ "name": name, "description": description });
    if let Some(ws) = ws {
        body["workspace_id"] = json!(ws);
    }
    let (st, _, v) = req(t, Method::POST, "/api/projects", Some(body), Some(cookie)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["project_id"].as_str().unwrap().to_string()
}

/// `GET /api/search?<query>`; `who` is a cookie, or `None` for the API token.
async fn panel(t: &T, who: Option<&str>, query: &str) -> (StatusCode, Value) {
    let (st, _, v) = req(t, Method::GET, &format!("/api/search?{query}"), None, who).await;
    (st, v)
}

/// The ids of a panel answer, in order.
fn ids(v: &Value) -> Vec<String> {
    v["results"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

/// The answer's results of `kind`.
fn of_kind<'a>(v: &'a Value, kind: &str) -> Vec<&'a Value> {
    v["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == kind)
        .collect()
}

async fn results(t: &T, cookie: &str, q: &str, suggest: bool) -> Vec<String> {
    let path = format!(
        "/api/search?q={}{}",
        q.replace(' ', "%20")
            .replace('"', "%22")
            .replace('*', "%2A")
            .replace('(', "%28")
            .replace(')', "%29")
            .replace(':', "%3A"),
        if suggest { "&suggest=true" } else { "" }
    );
    let (st, _, v) = req(t, Method::GET, &path, None, Some(cookie)).await;
    assert_eq!(st, StatusCode::OK, "{q}: {v}");
    v["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn similar_chats_are_strict_safe_and_private() {
    let t = start().await;
    let admin = login_admin(&t).await;
    let tokio_sid = chat(
        &t,
        &admin,
        "how do I configure tokio runtime worker threads",
    )
    .await;
    let _other = chat(&t, &admin, "best pasta recipe with garlic").await;

    // Similar wording, typed as you go: suggested.
    assert_eq!(
        results(&t, &admin, "configure tokio runtime thr", true).await,
        std::slice::from_ref(&tokio_sid)
    );
    // Too short, unrelated, or time-sensitive: nothing.
    assert!(results(&t, &admin, "tokio", true).await.is_empty());
    assert!(
        results(&t, &admin, "garlic bread with butter tonight", true)
            .await
            .is_empty()
    );
    assert!(
        results(&t, &admin, "tokio runtime news today please", true)
            .await
            .is_empty()
    );
    // Sidebar search: all words must match.
    assert_eq!(results(&t, &admin, "pasta garlic", false).await.len(), 1);
    assert!(results(&t, &admin, "pasta tokio", false).await.is_empty());
    // FTS5 syntax in the input is harmless.
    for q in [
        "\"tokio",
        "tokio*",
        "NEAR(tokio runtime)",
        "-tokio",
        "title:tokio",
        "\"\" OR \"\"",
        ")(",
    ] {
        results(&t, &admin, q, false).await;
        results(&t, &admin, q, true).await;
    }

    // Another user cannot find alex's chats.
    req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        Some(&admin),
    )
    .await;
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/login",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        None,
    )
    .await;
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": "temporary pw", "new": "bobs own password"})),
        Some(&bc.unwrap()),
    )
    .await;
    let bc = bc.unwrap();
    assert!(
        results(&t, &bc, "configure tokio runtime thr", true)
            .await
            .is_empty()
    );
    assert!(results(&t, &bc, "tokio", false).await.is_empty());
}

#[tokio::test]
async fn classifier_state_is_validated_per_user() {
    let t = start().await;
    let admin = login_admin(&t).await;
    let (st, _, v) = req(&t, Method::GET, "/api/me/classifier", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v.is_null());
    let good = json!({"version": 1, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 3, "rules_window": [1, 0, 1], "model_window": [1, 1, 0]});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/classifier",
        Some(good.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/classifier", None, Some(&admin)).await;
    assert_eq!(v["labels"], 3);
    for bad in [
        json!({"version": 1, "weights": [0.0], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
        json!({"version": 2, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
        json!({"version": 1, "weights": vec![0.0; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": vec![1; 101], "model_window": []}),
        json!({"version": 1, "weights": vec![1e9; 4096 * 3], "bias": [0.0, 0.0, 0.0], "labels": 0, "rules_window": [], "model_window": []}),
    ] {
        let (st, _, _) = req(
            &t,
            Method::PUT,
            "/api/me/classifier",
            Some(bad),
            Some(&admin),
        )
        .await;
        assert!(st.is_client_error(), "{st}");
    }
}

const M: char = '\u{2}';
const E: char = '\u{3}';

#[tokio::test]
async fn the_panel_searches_each_kind_and_pages() {
    let t = start().await;
    let alex = login_admin(&t).await;
    let tokio_sid = chat(&t, &alex, "configure tokio runtime").await;
    let pasta = chat(&t, &alex, "best pasta recipe").await;
    // The word is only in an older message: neither title nor preview shows it.
    let fish = chat_in(
        &t,
        &alex,
        None,
        "aquarium notes",
        &["zebrafish need warm water", "thanks"],
    )
    .await;
    let garden = project(
        &t,
        &alex,
        None,
        "Tokio rewrite",
        "Move the runtime to tokio",
    )
    .await;

    // A tab: one kind, marked, with the latest message's first sentence.
    let (st, v) = panel(&t, Some(&alex), "q=tokio&in=chats").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(ids(&v), vec![tokio_sid.clone()]);
    assert_eq!(v["kinds"], json!(["chats", "projects"]));
    assert_eq!(v["next"], Value::Null);
    let r = &v["results"][0];
    assert_eq!(r["kind"], "chat");
    assert_eq!(r["type"], "chat");
    assert_eq!(r["title"], format!("configure {M}tokio{E} runtime"));
    assert_eq!(
        r["preview"],
        format!("You said: configure {M}tokio{E} runtime")
    );
    assert_eq!(
        r["snippet"],
        Value::Null,
        "title and preview show every word"
    );
    assert_eq!(r["running"], false);
    assert_eq!(r["project_id"], Value::Null);
    assert!(r["time_ms"].as_i64().unwrap() > 0);
    assert!(r.get("score").is_none() && r.get("tier").is_none(), "{r}");
    // `in` defaults to chats.
    let (_, v) = panel(&t, Some(&alex), "q=tokio").await;
    assert_eq!(ids(&v), vec![tokio_sid.clone()]);

    // The excerpt comes only when the row would not show the words.
    let (_, v) = panel(&t, Some(&alex), "q=zebrafish&in=chats").await;
    assert_eq!(ids(&v), vec![fish.clone()]);
    let r = &v["results"][0];
    assert_eq!(r["preview"], "You said: thanks");
    assert!(
        r["snippet"]
            .as_str()
            .unwrap()
            .contains(&format!("{M}zebrafish{E}")),
        "{r}"
    );

    let (_, v) = panel(&t, Some(&alex), "q=tokio&in=projects").await;
    assert_eq!(ids(&v), vec![garden.clone()]);
    let r = &v["results"][0];
    assert_eq!(
        (&r["kind"], &r["type"]),
        (&json!("project"), &json!("project"))
    );
    assert_eq!(r["title"], format!("{M}Tokio{E} rewrite"));
    assert_eq!(r["preview"], format!("Move the runtime to {M}tokio{E}"));

    // All: both kinds, names holding every word first, then the newest.
    let (_, v) = panel(&t, Some(&alex), "q=tokio&in=all").await;
    assert_eq!(ids(&v), [garden.clone(), tokio_sid.clone()]);
    assert_eq!(v["next"], Value::Null);
    let (_, v) = panel(&t, Some(&alex), "q=runtime&in=all").await;
    assert_eq!(
        ids(&v),
        [tokio_sid.clone(), garden.clone()],
        "a title match ranks above a description match"
    );

    // Areas the viewer does not have answer nothing; unknown ones are refused.
    for k in ["media", "artifacts"] {
        let (st, v) = panel(&t, Some(&alex), &format!("q=tokio&in={k}")).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(ids(&v).is_empty());
    }
    let (st, _) = panel(&t, Some(&alex), "q=tokio&in=everything").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Nothing typed: the newest first, per tab and across kinds.
    let (_, v) = panel(&t, Some(&alex), "in=chats").await;
    assert_eq!(ids(&v), [fish.clone(), pasta.clone(), tokio_sid.clone()]);
    assert!(of_kind(&v, "chat").iter().all(|r| r["snippet"].is_null()));
    let (_, v) = panel(&t, Some(&alex), "q=&in=all").await;
    assert_eq!(
        ids(&v),
        [
            garden.clone(),
            fish.clone(),
            pasta.clone(),
            tokio_sid.clone()
        ]
    );

    // Pages: `next` is the following offset, null at the end.
    let (_, v) = panel(&t, Some(&alex), "in=chats&limit=2").await;
    assert_eq!(ids(&v), [fish.clone(), pasta.clone()]);
    assert_eq!(v["next"], "2");
    let (_, v) = panel(&t, Some(&alex), "in=chats&limit=2&cursor=2").await;
    assert_eq!(ids(&v), vec![tokio_sid.clone()]);
    assert_eq!(v["next"], Value::Null);
    // Every mock reply says "You said: …": a query pages the same way.
    let (_, v) = panel(&t, Some(&alex), "q=said&in=chats&limit=2").await;
    assert_eq!(ids(&v).len(), 2);
    assert_eq!(v["next"], "2");
    let (_, v) = panel(&t, Some(&alex), "q=said&in=chats&limit=2&cursor=2").await;
    assert_eq!(ids(&v).len(), 1);
    assert_eq!(v["next"], Value::Null);
    let (st, _) = panel(&t, Some(&alex), "in=chats&cursor=x").await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // FTS5 syntax in the input is harmless in every tab.
    for q in [
        "%22tokio",
        "tokio%2A",
        "NEAR%28tokio%20runtime%29",
        "-tokio",
        "title%3Atokio",
        ")(",
    ] {
        for k in ["all", "chats", "projects"] {
            let (st, v) = panel(&t, Some(&alex), &format!("q={q}&in={k}")).await;
            assert_eq!(st, StatusCode::OK, "{q} in {k}: {v}");
        }
    }
}

/// T10: chats are the caller's own, admins included; the workspace is one
/// the caller is in; projects are every member's, guests too.
#[tokio::test]
async fn search_is_private_and_stays_in_the_workspace() {
    let t = start().await;
    let alex = login_admin(&t).await;
    let bob = new_user(&t, &alex, "bob").await;
    let carol = new_user(&t, &alex, "carol").await;
    let acme = team(&t, &bob, "Acme", &[("alex", "member"), ("carol", "guest")]).await;
    let at_work = chat_in(&t, &bob, Some(&acme), "zebra migration plans", &["zebra"]).await;
    let at_home = chat_in(&t, &bob, None, "zebra at home", &["zebra"]).await;
    let plot = project(
        &t,
        &bob,
        Some(&acme),
        "Garden allotment",
        "Zebra-striped beans",
    )
    .await;

    let (_, v) = panel(
        &t,
        Some(&bob),
        &format!("q=zebra&in=chats&workspace={acme}"),
    )
    .await;
    assert_eq!(ids(&v), vec![at_work.clone()]);
    let (_, v) = panel(&t, Some(&bob), "q=zebra&in=all").await;
    assert_eq!(
        ids(&v),
        vec![at_home.clone()],
        "the active workspace by default"
    );

    // The server admin is a member of Acme and still finds none of bob's chats.
    for q in ["q=zebra&in=all", "q=zebra&in=chats", "in=chats", "in=all"] {
        let (st, v) = panel(&t, Some(&alex), &format!("{q}&workspace={acme}")).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(of_kind(&v, "chat").is_empty(), "{q}: {v}");
    }
    // Nobody searches a workspace they are not in, admins included.
    let (_, _, v) = req(&t, Method::GET, "/api/workspaces", None, Some(&bob)).await;
    let bob_personal = v["workspaces"][0]["workspace_id"]
        .as_str()
        .unwrap()
        .to_string();
    for who in [&alex, &carol] {
        let (st, _) = panel(&t, Some(who), &format!("q=zebra&workspace={bob_personal}")).await;
        assert_eq!(st, StatusCode::NOT_FOUND);
    }

    // A guest finds the workspace's projects, never bob's chats.
    let (_, v) = panel(
        &t,
        Some(&carol),
        &format!("q=garden&in=projects&workspace={acme}"),
    )
    .await;
    assert_eq!(ids(&v), vec![plot.clone()]);
    let (_, v) = panel(
        &t,
        Some(&carol),
        &format!("q=zebra&in=all&workspace={acme}"),
    )
    .await;
    assert_eq!(ids(&v), vec![plot.clone()]);
    let (_, v) = panel(&t, Some(&carol), &format!("in=all&workspace={acme}")).await;
    assert_eq!(ids(&v), vec![plot.clone()]);

    // The API token sees every chat, optionally in one workspace.
    let (_, v) = panel(&t, None, "q=zebra&in=chats").await;
    let mut all = ids(&v);
    all.sort();
    let mut want = vec![at_work.clone(), at_home.clone()];
    want.sort();
    assert_eq!(all, want);
    let (_, v) = panel(&t, None, &format!("q=zebra&in=chats&workspace={acme}")).await;
    assert_eq!(ids(&v), [at_work]);
}

/// What the sidebar and the panel show depends on the viewer's role here.
#[tokio::test]
async fn features_follow_the_viewer() {
    let t = start().await;
    let alex = login_admin(&t).await;
    let bob = new_user(&t, &alex, "bob").await;
    let carol = new_user(&t, &alex, "carol").await;
    let acme = team(&t, &bob, "Acme", &[("carol", "guest")]).await;
    // A guest cannot run routines; without the agentic grant there is no
    // Artifacts area to show.
    let (st, _, v) = req(
        &t,
        Method::GET,
        &format!("/api/features?workspace={acme}"),
        None,
        Some(&carol),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["routines"]["show"], false);
    assert_eq!(v["artifacts"]["show"], false);
    assert_eq!(v["search"]["kinds"], json!(["chats", "projects"]));
    // The panel's tabs are the same list.
    let (_, v) = panel(&t, Some(&carol), &format!("in=all&workspace={acme}")).await;
    assert_eq!(v["kinds"], json!(["chats", "projects"]));
    // Non-members get nothing.
    let (st, _, _) = req(
        &t,
        Method::GET,
        &format!("/api/features?workspace={acme}"),
        None,
        Some(&alex),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
