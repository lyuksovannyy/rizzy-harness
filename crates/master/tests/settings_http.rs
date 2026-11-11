//! Per-user preferences (`/api/me/settings`) and per-message effort.

mod common;
use common::SameOrigin as _;

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
    master: Master,
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
        master,
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
        Some(c) => rb.header("cookie", c).same_origin(),
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

#[tokio::test]
async fn settings_defaults_validation_and_isolation() {
    let t = start().await;
    let (_, admin, _) = req(
        &t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    let admin = admin.unwrap();

    let (st, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["search_url"], "https://duckduckgo.com/?q={q}");
    assert!(v["default_agent"].is_null());

    for bad in [
        json!({"search_url": "http://example.com/?q={q}"}),
        json!({"search_url": "https://example.com/search"}),
        json!({"search_url": "https://e.com/?q={q}", "default_agent": "nope"}),
        json!({"search_url": "https://e.com/?q={q}", "default_model": "nope"}),
    ] {
        let (st, _, v) = req(
            &t,
            Method::PUT,
            "/api/me/settings",
            Some(bad.clone()),
            Some(&admin),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} -> {v}");
    }
    let good = json!({"search_url": "https://www.google.com/search?q={q}", "default_agent": "writer", "default_model": "thinker"});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(good.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    for k in ["search_url", "default_agent", "default_model"] {
        assert_eq!(v[k], good[k], "{k}");
    }
    assert_eq!(v["onboarded"], false);

    // Profile fields: valid ones stick, bad ones are refused.
    let profile = json!({"search_url": "https://e.com/?q={q}", "display_name": "Alex", "timezone": "Europe/Riga", "country": "Latvia", "city": "Riga", "accent": "green", "theme": "dark", "backdrop": "still", "backdrop_chats": true, "onboarded": true});
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(profile.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    assert_eq!(v["timezone"], "Europe/Riga");
    assert_eq!(v["theme"], "dark");
    assert_eq!(v["backdrop"], "still");
    assert_eq!(v["backdrop_chats"], true);
    assert_eq!(v["onboarded"], true);
    // "system" is accepted too, though the web sends null for it.
    let (st, _, v) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(json!({"search_url": "https://e.com/?q={q}", "theme": "system"})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(profile.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Left out: DuckDuckGo images, no custom engines.
    assert_eq!(
        v["image_search_url"],
        "https://duckduckgo.com/?q={q}&iax=images&ia=images"
    );
    assert_eq!(v["custom_engines"], json!([]));
    let engines = json!({
        "search_url": "https://e.com/?q={q}",
        "image_search_url": "https://pics.example/s?q={q}",
        "custom_engines": [
            {"name": "Pics", "image_url": "https://pics.example/s?q={q}"},
            {"name": "Docs", "text_url": "https://docs.example/?q={q}"}
        ]
    });
    let (st, _, v) = req(
        &t,
        Method::PUT,
        "/api/me/settings",
        Some(engines.clone()),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, _, v) = req(&t, Method::GET, "/api/me/settings", None, Some(&admin)).await;
    assert_eq!(v["custom_engines"], engines["custom_engines"]);
    assert_eq!(v["image_search_url"], "https://pics.example/s?q={q}");
    for bad in [
        json!({"search_url": "https://e.com/?q={q}", "timezone": "Europe/Riga; DROP"}),
        json!({"search_url": "https://e.com/?q={q}", "accent": "neon"}),
        json!({"search_url": "https://e.com/?q={q}", "theme": "sepia"}),
        json!({"search_url": "https://e.com/?q={q}", "backdrop": "loud"}),
        json!({"search_url": "https://e.com/?q={q}", "display_name": "x".repeat(65)}),
        json!({"search_url": "https://e.com/?q={q}", "city": "  "}),
        // Image engine and custom engines are checked like the text one.
        json!({"search_url": "https://e.com/?q={q}", "image_search_url": "http://img.example/?q={q}"}),
        json!({"search_url": "https://e.com/?q={q}", "custom_engines": [{"name": "Nothing"}]}),
        json!({"search_url": "https://e.com/?q={q}", "custom_engines": [{"name": "Bad", "image_url": "https://x.example/"}]}),
        json!({"search_url": "https://e.com/?q={q}", "custom_engines": [{"name": "", "text_url": "https://x.example/?q={q}"}]}),
    ] {
        let (st, _, v) = req(
            &t,
            Method::PUT,
            "/api/me/settings",
            Some(bad.clone()),
            Some(&admin),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad} -> {v}");
    }

    // Another user keeps their own defaults.
    let (_, _, bob) = req(
        &t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        Some(&admin),
    )
    .await;
    assert_eq!(bob["username"], "bob");
    let pw = bob["password"].as_str().unwrap();
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/login",
        Some(json!({"username": "bob", "password": pw})),
        None,
    )
    .await;
    let (_, bc, _) = req(
        &t,
        Method::POST,
        "/api/me/password",
        Some(json!({"current": pw, "new": "bobs own password"})),
        Some(&bc.unwrap()),
    )
    .await;
    let (_, _, v) = req(
        &t,
        Method::GET,
        "/api/me/settings",
        None,
        Some(&bc.unwrap()),
    )
    .await;
    assert_eq!(v["search_url"], "https://duckduckgo.com/?q={q}");
}

#[test]
fn effort_override_is_validated_and_gated() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let t = rt.block_on(start());
    // Model without effort support: the override is dropped.
    assert_eq!(
        t.master
            .agent_spec_with(None, Some("mock"), Some("high"))
            .unwrap()
            .effort,
        None
    );
    // Model with support: kept.
    assert_eq!(
        t.master
            .agent_spec_with(None, Some("thinker"), Some("high"))
            .unwrap()
            .effort
            .as_deref(),
        Some("high")
    );
    assert!(
        t.master
            .agent_spec_with(None, Some("thinker"), Some("turbo"))
            .is_err()
    );
}

/// FIX_PLAN 1.2 and 1.4: no HTTP call installs software, writes node.toml
/// or sets up a server over SSH. The routes are gone, not merely refused,
/// so they cannot come back as a side effect of an unrelated change. The
/// built-in node is wired in, which is when the old endpoints did act.
#[tokio::test]
async fn no_endpoint_installs_software_or_edits_node_toml() {
    let t = start().await;
    let node_toml = t._dir.path().join("node.toml");
    let original = "# mine\nworkspace_roots = [\"/w\"]\n";
    std::fs::write(&node_toml, original).unwrap();
    let state = harness_tools::graphify::GraphifyState::new(false, t._dir.path().join("graphify"));
    t.master.set_graphify(state.clone(), node_toml.clone());
    let (_, admin, _) = req(
        &t,
        Method::POST,
        "/api/setup",
        Some(json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"})),
        None,
    )
    .await;
    let admin = admin.unwrap();

    let (st, _, _) = req(
        &t,
        Method::PUT,
        "/api/admin/graphify",
        Some(json!({"allowed": true})),
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::METHOD_NOT_ALLOWED);
    let (st, _, _) = req(
        &t,
        Method::POST,
        "/api/admin/graphify/install",
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    for (method, path) in [
        (Method::POST, "/api/admin/provision/probe"),
        (Method::POST, "/api/admin/provision"),
        (Method::GET, "/api/admin/provision/x"),
        (Method::GET, "/api/admin/node-setups"),
        (Method::POST, "/api/admin/node-setups/x/firewall"),
    ] {
        let (st, _, _) = req(&t, method.clone(), path, Some(json!({})), Some(&admin)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{method} {path}");
    }
    assert_eq!(std::fs::read_to_string(&node_toml).unwrap(), original);
    assert!(!state.allowed());

    // Status stays readable, with what to do on the node instead.
    let (st, _, v) = req(&t, Method::GET, "/api/admin/graphify", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["local_node"], true);
    assert_eq!(v["allowed"], false);
    assert_eq!(v["node_toml"], node_toml.display().to_string());
    let steps = v["steps"].to_string();
    assert!(
        steps.contains("rizzy-harness graphify install") && steps.contains("graphify = true"),
        "{steps}"
    );
    // The join command, which stays, still answers.
    let (st, _, _) = req(&t, Method::GET, "/api/admin/nodes", None, Some(&admin)).await;
    assert_eq!(st, StatusCode::OK);
}
