//! Artifacts end to end on a real worker node with the mock model: an
//! agent publishes, the master pulls, the file downloads byte for byte
//! under the T17 headers; "Save as artifact"; one Media item per image
//! lineage; who sees and changes what (T10); search and features.

mod common;
use common::{Login as _, SameOrigin as _};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_core::policy::ApprovalMode;
use harness_master::auth::{Auth, Claim};
use harness_master::config::Config;
use harness_master::providers::ProviderRuntime;
use harness_master::{Master, router};
use harness_node::artifacts::StageConfig;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CSP: &str = "default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'; sandbox";

struct T {
    base: String,
    work: PathBuf,
    _dir: tempfile::TempDir,
    _node: NodeRuntime,
}

async fn node(dir: &Path, work: &Path) -> NodeRuntime {
    let data = dir.join("node-data");
    std::fs::create_dir_all(&data).unwrap();
    NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "builder".into(),
        policy: NodePolicy {
            workspace_roots: vec![work.into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: Some(StageConfig::new(data)),
    })
    .await
    .unwrap()
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let master = Master::new(
        store,
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut cfg = Config::default();
    cfg.media.enabled = true;
    // Non-admins with the agentic grant use the embedded node here; the
    // default (admins only) has its own tests in local_node_http.rs.
    cfg.nodes.local_node_users = harness_master::config::LocalNodeUsers::Granted;
    let (rt, _) = ProviderRuntime::start(cfg, dir.path(), Default::default()).unwrap();
    master.set_provider_runtime(Arc::new(rt));
    master.set_agents(harness_agents::AgentSet::load(&dir.path().join("none")).0);
    harness_master::storage::open(&master, dir.path()).unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let n = node(dir.path(), &work).await;
    master.add_link(n.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        work,
        _dir: dir,
        _node: n,
    }
}

async fn send(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    auth: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    rb = if auth.starts_with("Bearer ") {
        rb.header("authorization", auth)
    } else {
        rb.login(auth).same_origin()
    };
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn get(t: &T, path: &str, c: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{}{path}", t.base))
        .login(c)
        .same_origin()
        .send()
        .await
        .unwrap()
}

/// The cookie a login answer set; its login key is recorded (P-1).
async fn cookie(r: reqwest::Response) -> String {
    common::signed_in(r).await
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    rb.send().await.unwrap()
}

async fn alex(t: &T) -> String {
    let body = json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"});
    cookie(post_anon(t, "/api/setup", body, None).await).await
}

/// A member account made by `admin`, signed in with its own password.
async fn account(t: &T, admin: &str, name: &str, agentic: bool) -> String {
    let body = json!({"username": name, "exec_ceiling": if agentic { "host" } else { "none" }});
    let (st, v) = send(t, Method::POST, "/api/users", Some(body), admin).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap();
    let login = json!({"username": name, "password": pw});
    let first = post_anon(t, "/api/login", login, None).await;
    let change = json!({"current": pw, "new": format!("{name}s own password")});
    cookie(post_anon(t, "/api/me/password", change, Some(&cookie(first).await)).await).await
}

/// An agentic chat of `c`'s on the worker, in `ws` (default: active).
async fn chat(t: &T, c: &str, ws: Option<&str>) -> String {
    let mut body = json!({"profile": "agentic", "cwd": t.work, "approval_mode": "auto"});
    if let Some(w) = ws {
        body["workspace_id"] = json!(w);
    }
    let (st, v) = send(t, Method::POST, "/api/sessions", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

async fn prompt(t: &T, c: &str, sid: &str, text: &str) {
    let (st, v) = send(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// The chat's staged files once `n` of them have settled.
async fn settled(t: &T, c: &str, sid: &str, n: usize) -> Vec<Value> {
    for _ in 0..600 {
        let (_, v) = send(
            t,
            Method::GET,
            &format!("/api/sessions/{sid}/artifacts"),
            None,
            c,
        )
        .await;
        let rows = v.as_array().cloned().unwrap_or_default();
        if rows.len() >= n && rows.iter().all(|r| r["status"] != "pending") {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("not settled");
}

async fn personal(t: &T, c: &str) -> String {
    let (_, v) = send(t, Method::GET, "/api/workspaces", None, c).await;
    let list = v.as_array().or_else(|| v["workspaces"].as_array()).unwrap();
    let w = list.iter().find(|w| w["personal"] == true).unwrap();
    w["workspace_id"].as_str().unwrap().to_string()
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

fn png(w: u32, h: u32, tail: &[u8]) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b.extend(tail);
    b
}

#[tokio::test]
async fn an_agent_publishes_and_the_file_downloads_byte_for_byte() {
    let t = start().await;
    let alex = alex(&t).await;
    let bytes: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(t.work.join("app.bin"), &bytes).unwrap();
    let sid = chat(&t, &alex, None).await;
    prompt(&t, &alex, &sid, "publish: app.bin as myapp").await;
    let rows = settled(&t, &alex, &sid, 1).await;
    assert_eq!(rows[0]["status"], "ready", "{rows:?}");
    assert_eq!(rows[0]["by_user"], false);
    assert!(rows[0]["call_id"].is_string());

    let (st, list) = send(&t, Method::GET, "/api/artifacts", None, &alex).await;
    assert_eq!(st, StatusCode::OK, "{list}");
    let a = &list["artifacts"][0];
    assert_eq!(a["name"], "myapp");
    assert_eq!(a["mine"], true);
    assert_eq!(a["versions"], 1);
    let latest = &a["latest"];
    assert_eq!(latest["sha256"], sha(&bytes));
    assert_eq!(latest["status"], "ready");
    assert_eq!(latest["node"]["hostname"], "builder");
    // Not an image: no header dimensions.
    assert!(
        latest["width"].is_null() && latest["height"].is_null(),
        "{latest}"
    );
    assert_eq!(latest["session"]["session_id"], sid);
    assert!(
        latest["meta"]["source_path"]
            .as_str()
            .unwrap()
            .ends_with("app.bin")
    );
    assert!(list["usage"]["used"].as_u64().unwrap() >= bytes.len() as u64);

    let id = a["artifact_id"].as_str().unwrap();
    let vid = latest["version_id"].as_str().unwrap();
    let r = get(
        &t,
        &format!("/api/artifacts/{id}/versions/{vid}/download"),
        &alex,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    for (h, v) in [
        ("content-security-policy", CSP),
        ("x-content-type-options", "nosniff"),
        ("cross-origin-resource-policy", "same-origin"),
        ("cache-control", "no-store"),
        ("content-type", "application/octet-stream"),
    ] {
        assert_eq!(r.headers()[h], v, "{h}");
    }
    let disp = r.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disp.starts_with("attachment") && disp.contains("app.bin"),
        "{disp}"
    );
    assert_eq!(r.headers()["x-content-sha256"], sha(&bytes).as_str());
    assert_eq!(r.bytes().await.unwrap().as_ref(), &bytes[..]);
    // Not a kind a browser may show: no preview.
    let r = get(
        &t,
        &format!("/api/artifacts/{id}/versions/{vid}/preview"),
        &alex,
    )
    .await;
    assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let (_, me) = send(&t, Method::GET, "/api/me/storage", None, &alex).await;
    assert_eq!(me["artifacts"]["versions"], 1);
    assert_eq!(me["artifacts"]["bytes"], bytes.len());

    let (st, found) = send(
        &t,
        Method::GET,
        "/api/search?q=myapp&in=artifacts",
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{found}");
    assert_eq!(found["results"][0]["kind"], "artifact");
    assert_eq!(found["results"][0]["id"], id);
}

#[tokio::test]
async fn save_as_artifact_checks_on_the_node_and_copies_later() {
    let t = start().await;
    let alex = alex(&t).await;
    std::fs::write(t.work.join("notes.txt"), b"release notes").unwrap();
    let sid = chat(&t, &alex, None).await;
    let path = format!("/api/sessions/{sid}/artifacts");
    let (st, v) = send(
        &t,
        Method::POST,
        &path,
        Some(json!({"path": "notes.txt", "name": "Notes"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v, json!({"requested": true}));
    let rows = settled(&t, &alex, &sid, 1).await;
    assert_eq!(
        (rows[0]["status"].as_str(), rows[0]["by_user"].as_bool()),
        (Some("ready"), Some(true))
    );
    // The node's refusal is the answer.
    let (st, v) = send(
        &t,
        Method::POST,
        &path,
        Some(json!({"path": "../../etc/passwd"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let (st, _) = send(&t, Method::POST, &path, Some(json!({"path": " "})), &alex).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = send(
        &t,
        Method::POST,
        &path,
        Some(json!({"path": "notes.txt", "name": "a/b"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Someone else's chat is invisible.
    let bob = account(&t, &alex, "bob", true).await;
    let (st, _) = send(
        &t,
        Method::POST,
        &path,
        Some(json!({"path": "notes.txt"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = send(&t, Method::GET, &path, None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_republished_image_keeps_one_media_item() {
    let t = start().await;
    let alex = alex(&t).await;
    let sid = chat(&t, &alex, None).await;
    let v1 = png(4, 3, b"first");
    std::fs::write(t.work.join("chart.png"), &v1).unwrap();
    prompt(&t, &alex, &sid, "publish: chart.png").await;
    settled(&t, &alex, &sid, 1).await;
    let v2 = png(4, 3, b"second render");
    std::fs::write(t.work.join("chart.png"), &v2).unwrap();
    prompt(&t, &alex, &sid, "publish: chart.png").await;
    let rows = settled(&t, &alex, &sid, 2).await;
    assert!(rows.iter().all(|r| r["status"] == "ready"), "{rows:?}");
    let (_, media) = send(&t, Method::GET, "/api/media", None, &alex).await;
    let items = media["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{media}");
    let item = &items[0];
    assert_eq!(item["origin"], "agent");
    assert_eq!(item["kind"], "image");
    assert_eq!(
        (item["width"].as_u64(), item["height"].as_u64()),
        (Some(4), Some(3))
    );
    assert_eq!(item["artifact"]["seq"], 2);
    assert_eq!(item["artifact"]["name"], "chart.png");
    let mid = item["media_id"].as_str().unwrap();
    let r = get(&t, &format!("/api/media/{mid}/content"), &alex).await;
    assert_eq!(
        r.bytes().await.unwrap().as_ref(),
        &v2[..],
        "shows the newest"
    );
    let (_, list) = send(&t, Method::GET, "/api/artifacts?kind=media", None, &alex).await;
    let a = &list["artifacts"][0];
    assert_eq!(a["media_id"], mid);
    assert_eq!(a["kind"], "image");
    // The version carries the header dimensions, without Media.
    assert_eq!(
        (a["latest"]["width"].clone(), a["latest"]["height"].clone()),
        (json!(4), json!(3))
    );
    // A PNG may be previewed, as its own type.
    let id = a["artifact_id"].as_str().unwrap().to_string();
    let vid = a["latest"]["version_id"].as_str().unwrap();
    let r = get(
        &t,
        &format!("/api/artifacts/{id}/versions/{vid}/preview"),
        &alex,
    )
    .await;
    assert_eq!(r.headers()["content-type"], "image/png");
    // Deleting the item leaves the artifact.
    let (st, _) = send(
        &t,
        Method::DELETE,
        &format!("/api/media/{mid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, a) = send(
        &t,
        Method::GET,
        &format!("/api/artifacts/{id}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(a["versions"].as_array().unwrap().len(), 2);
    assert!(a["media_id"].is_null());
}

#[tokio::test]
async fn who_sees_shares_and_deletes() {
    let t = start().await;
    let alex = alex(&t).await;
    let bob = account(&t, &alex, "bob", false).await;
    let carol = account(&t, &alex, "carol", false).await;
    let erin = account(&t, &alex, "erin", false).await;
    let (_, w) = send(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    for (name, role) in [("bob", "member"), ("carol", "guest"), ("erin", "admin")] {
        let (st, v) = send(
            &t,
            Method::POST,
            &format!("/api/workspaces/{acme}/members"),
            Some(json!({"username": name, "role": role})),
            &alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    std::fs::write(t.work.join("tool"), b"the tool").unwrap();
    let sid = chat(&t, &alex, Some(&acme)).await;
    prompt(&t, &alex, &sid, "publish: tool").await;
    settled(&t, &alex, &sid, 1).await;
    let list = |c: String| {
        let t = &t;
        let acme = acme.clone();
        async move {
            let (st, v) = send(
                t,
                Method::GET,
                &format!("/api/artifacts?workspace={acme}"),
                None,
                &c,
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
            v["artifacts"].as_array().unwrap().len()
        }
    };
    let (_, mine) = send(
        &t,
        Method::GET,
        &format!("/api/artifacts?workspace={acme}"),
        None,
        &alex,
    )
    .await;
    let id = mine["artifacts"][0]["artifact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let one = format!("/api/artifacts/{id}");
    // Private: nobody else, the workspace's admin included.
    for c in [&bob, &carol, &erin] {
        assert_eq!(list(c.clone()).await, 0);
        assert_eq!(
            send(&t, Method::GET, &one, None, c).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (st, all) = send(&t, Method::GET, "/api/artifacts", None, "Bearer tok").await;
    assert_eq!(
        (st, all["artifacts"].as_array().unwrap().len()),
        (StatusCode::OK, 1)
    );
    // Without the agentic grant there is nothing to show bob yet.
    let (_, f) = send(
        &t,
        Method::GET,
        &format!("/api/features?workspace={acme}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(f["artifacts"]["show"], false, "{f}");
    // Shared: every member sees and downloads it, guests too.
    let (st, v) = send(
        &t,
        Method::PATCH,
        &one,
        Some(json!({"visibility": "workspace"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["visibility"], "workspace");
    for c in [&bob, &carol, &erin] {
        assert_eq!(list(c.clone()).await, 1);
    }
    let (_, f) = send(
        &t,
        Method::GET,
        &format!("/api/features?workspace={acme}"),
        None,
        &bob,
    )
    .await;
    assert_eq!(f["artifacts"]["show"], true);
    assert!(
        f["artifacts"]["why"].as_str().unwrap().contains("Acme"),
        "{f}"
    );
    let (_, a) = send(&t, Method::GET, &one, None, &bob).await;
    assert_eq!(a["mine"], false);
    assert_eq!(a["owner"]["username"], "alex");
    assert_eq!(
        a["versions"][0]["session"],
        json!({"private": true}),
        "not bob's chat"
    );
    let vid = a["versions"][0]["version_id"].as_str().unwrap();
    // Where alex's file was on alex's node is alex's alone (AUDIT L5): not
    // in the detail, its latest, the version endpoint or the list.
    let (_, owner_view) = send(&t, Method::GET, &one, None, &alex).await;
    assert!(
        owner_view["versions"][0]["meta"]["source_path"].is_string(),
        "{owner_view}"
    );
    let (_, v1) = send(
        &t,
        Method::GET,
        &format!("{one}/versions/{vid}"),
        None,
        &bob,
    )
    .await;
    let (_, listed) = send(
        &t,
        Method::GET,
        &format!("/api/artifacts?workspace={acme}"),
        None,
        &bob,
    )
    .await;
    for (what, meta) in [
        ("detail", &a["versions"][0]["meta"]),
        ("latest", &a["latest"]["meta"]),
        ("version", &v1["meta"]),
        ("list", &listed["artifacts"][0]["latest"]["meta"]),
    ] {
        assert!(meta.is_object(), "{what}: {meta}");
        assert!(meta.get("source_path").is_none(), "{what}: {meta}");
    }
    let r = get(&t, &format!("{one}/versions/{vid}/download"), &bob).await;
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"the tool");
    // Only the owner changes it; a member cannot delete it, the admin can.
    let (st, _) = send(
        &t,
        Method::PATCH,
        &one,
        Some(json!({"name": "mine now"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send(
        &t,
        Method::PATCH,
        &format!("{one}/versions/{vid}"),
        Some(json!({"pinned": true})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send(&t, Method::DELETE, &one, None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = send(&t, Method::DELETE, &one, None, &erin).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list(alex.clone()).await, 0);
    // In Personal there is no one to share with.
    let personal = personal(&t, &alex).await;
    let sid = chat(&t, &alex, Some(&personal)).await;
    prompt(&t, &alex, &sid, "publish: tool").await;
    settled(&t, &alex, &sid, 1).await;
    let (_, mine) = send(
        &t,
        Method::GET,
        &format!("/api/artifacts?workspace={personal}"),
        None,
        &alex,
    )
    .await;
    let id = mine["artifacts"][0]["artifact_id"].as_str().unwrap();
    let (st, _) = send(
        &t,
        Method::PATCH,
        &format!("/api/artifacts/{id}"),
        Some(json!({"visibility": "workspace"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, v) = send(
        &t,
        Method::PATCH,
        &format!("/api/artifacts/{id}"),
        Some(json!({"name": "Renamed"})),
        &alex,
    )
    .await;
    assert_eq!((st, v["name"].as_str()), (StatusCode::OK, Some("Renamed")));
}
