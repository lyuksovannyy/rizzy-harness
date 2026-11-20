//! Model prices are data the server's admin sets (no built-in table): per
//! model, per million tokens, each class optional, with one server-wide
//! currency. Validated and refused when bad (never dropped), stored in
//! config.toml with the model, set by server admins only, and readable by
//! every signed-in user with the model list (a member sees their own
//! chat's estimate).

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::sync::Arc;

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

struct Server {
    base: String,
    admin: String,
    dir: tempfile::TempDir,
}

async fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::load(&dir.path().join("config.toml")).unwrap();
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "CLAIM").unwrap();
    let auth =
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        });
    let master = Master::new(Store::open_in_memory().unwrap(), auth);
    master.set_agents(harness_agents::AgentSet::from_agents(
        harness_agents::builtins(),
        harness_agents::DEFAULT_AGENT,
    ));
    master.set_provider_runtime(Arc::new(rt));
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    let mut s = Server {
        base: format!("http://{addr}"),
        admin: String::new(),
        dir,
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
    cookie: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", s.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    if let Some(c) = cookie {
        rb = rb.login(c).same_origin();
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
    cookie: &str,
) -> (StatusCode, Value) {
    let (st, _, v) = send(s, method, path, body, Some(cookie)).await;
    (st, v)
}

/// An account created by the admin, logged in with its own password.
async fn user(s: &Server, name: &str) -> String {
    let (st, v) = call(
        s,
        Method::POST,
        "/api/users",
        Some(json!({"username": name, "exec_ceiling": "none"})),
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

/// A keyless local provider `loc` with two models, `big` priced as given.
fn provider(price: Value) -> Value {
    json!({
        "name": "Local",
        "preset": "custom_openai",
        "base_url": "http://127.0.0.1:9/v1",
        "models": [
            {"id": "big", "custom": true, "price": price},
            {"id": "free", "custom": true},
        ],
    })
}

async fn put_provider(s: &Server, who: &str, price: Value) -> (StatusCode, Value) {
    call(
        s,
        Method::PUT,
        "/api/admin/providers/loc",
        Some(provider(price)),
        who,
    )
    .await
}

async fn models(s: &Server, who: &str) -> Vec<Value> {
    let (st, v) = call(s, Method::GET, "/api/models", None, who).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["models"].as_array().unwrap().clone()
}

fn model<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter().find(|m| m["id"] == id).unwrap()
}

#[tokio::test]
async fn an_admin_sets_prices_everyone_reads_them_and_they_persist() {
    let s = start().await;
    let price = json!({"input": 3.0, "cached_input": 0.3, "cache_write": 3.75, "output": 15.0});
    let (st, v) = put_provider(&s, &s.admin, price).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The admin's page shows them on the model.
    let loc = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "loc")
        .unwrap();
    assert_eq!(loc["models"][0]["price"]["output"], 15.0);
    assert_eq!(v["currency"], "USD");

    // A member reads them with the model list, with the currency.
    let bob = user(&s, "bob").await;
    let list = models(&s, &bob).await;
    let big = model(&list, "loc::big");
    assert_eq!(
        big["price"],
        json!({"input": 3.0, "cached_input": 0.3, "cache_write": 3.75, "output": 15.0, "currency": "USD"})
    );
    // An unpriced model says nothing (never a zero).
    assert!(
        model(&list, "loc::free")
            .get("price")
            .is_none_or(Value::is_null)
    );

    // Stored where model settings are: config.toml, beside the model.
    let cfg = Config::load(&s.dir.path().join("config.toml")).unwrap();
    let m = cfg.provider("loc").unwrap().model("big").unwrap();
    assert_eq!(m.price.as_ref().unwrap().cache_write, Some(3.75));

    // Partly priced: what is set is kept, the rest stays unset.
    let (st, v) = put_provider(&s, &s.admin, json!({"input": 1.0, "output": 2.0})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let list = models(&s, &bob).await;
    assert_eq!(
        model(&list, "loc::big")["price"],
        json!({"input": 1.0, "output": 2.0, "currency": "USD"})
    );
    // All empty: no price at all.
    let (st, v) = put_provider(&s, &s.admin, json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let list = models(&s, &bob).await;
    assert!(
        model(&list, "loc::big")
            .get("price")
            .is_none_or(Value::is_null)
    );
}

#[tokio::test]
async fn a_bad_price_is_refused_and_nothing_changes() {
    let s = start().await;
    let good = json!({"input": 3.0, "output": 15.0});
    let (st, v) = put_provider(&s, &s.admin, good).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for bad in [
        json!({"input": -1.0}),
        json!({"output": 1.0e9}),
        json!({"cached_input": "cheap"}),
        json!({"input": 1.0, "per_request": 2.0}),
    ] {
        let (st, v) = put_provider(&s, &s.admin, bad.clone()).await;
        assert!(st.is_client_error(), "{bad}: {st} {v}");
        if st == StatusCode::BAD_REQUEST {
            let msg = v["error"].as_str().unwrap_or_default();
            assert!(msg.contains("big"), "names the model: {v}");
        }
    }
    let list = models(&s, &s.admin).await;
    assert_eq!(model(&list, "loc::big")["price"]["output"], 15.0);
}

#[tokio::test]
async fn the_currency_is_one_for_the_server_and_admin_only() {
    let s = start().await;
    let (st, v) = put_provider(&s, &s.admin, json!({"input": 3.0})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PUT,
        "/api/admin/pricing",
        Some(json!({"currency": " eur "})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["currency"], "EUR");
    let list = models(&s, &s.admin).await;
    assert_eq!(model(&list, "loc::big")["price"]["currency"], "EUR");
    let cfg = Config::load(&s.dir.path().join("config.toml")).unwrap();
    assert_eq!(cfg.pricing.currency, "EUR");
    for bad in ["euro", "", "E1R", "€€€"] {
        let (st, v) = call(
            &s,
            Method::PUT,
            "/api/admin/pricing",
            Some(json!({ "currency": bad })),
            &s.admin,
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad:?}: {v}");
    }
}

#[tokio::test]
async fn members_and_workspace_admins_cannot_set_prices() {
    let s = start().await;
    let (st, v) = put_provider(&s, &s.admin, json!({"input": 3.0})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let bob = user(&s, "bob").await;
    let carol = user(&s, "carol").await;
    // Carol administers a team workspace; that is not the server.
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let ws = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({"username": "carol", "role": "admin"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for who in [&bob, &carol] {
        let (st, _) = put_provider(&s, who, json!({"input": 0.0})).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        let (st, _) = call(
            &s,
            Method::PUT,
            "/api/admin/pricing",
            Some(json!({"currency": "JPY"})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
    }
    let list = models(&s, &bob).await;
    assert_eq!(model(&list, "loc::big")["price"]["input"], 3.0);
    assert_eq!(model(&list, "loc::big")["price"]["currency"], "USD");
}

#[test]
fn a_bad_price_in_config_toml_is_refused_at_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    for (bad, why) in [
        ("input = nan", "nan"),
        ("output = inf", "inf"),
        ("input = -0.5", "negative"),
        ("input = 20000.0", "too large"),
    ] {
        std::fs::write(
            &path,
            format!(
                "[[providers]]\nid = \"loc\"\nname = \"Local\"\npreset = \"custom_openai\"\n\
                 kind = \"openai\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\
                 [[providers.models]]\nid = \"big\"\nprice = {{ {bad} }}\n"
            ),
        )
        .unwrap();
        let e = Config::load(&path).unwrap_err();
        assert!(format!("{e:#}").contains("big"), "{why}: {e:#}");
    }
    std::fs::write(&path, "[pricing]\ncurrency = \"dollars\"\n").unwrap();
    assert!(Config::load(&path).is_err());
}
