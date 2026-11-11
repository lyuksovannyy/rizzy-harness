//! Temporary passwords (AUDIT L14, FIX_PLAN 3.10): the server makes them,
//! hands each out once in the answer to the admin who asked, stores only its
//! hash, and the account must change it at first login. An admin cannot
//! choose one any more.

mod common;
use common::SameOrigin as _;

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_store::{Durability, Store};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

const CLAIM: &str = "ABCDE-FGHJK-MNPQR-STUVW";

/// Everything the process logs, at every level, for every test in this file.
fn logs() -> Arc<Mutex<Vec<u8>>> {
    static BUF: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    BUF.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one subscriber");
        buf
    })
    .clone()
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Server {
    base: String,
    store: Store,
    dir: tempfile::TempDir,
}

async fn start() -> Server {
    logs();
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, CLAIM).unwrap();
    // On disk, so the test can search the database files for the plaintext.
    let store = Store::open(&dir.path().join("master.db"), Durability::Normal).unwrap();
    let auth = Auth::new(
        Secret::new("api-secret".into()),
        vec![addr.to_string()],
        false,
    )
    .with_claim(Claim {
        code: Secret::new(CLAIM.into()),
        path: claim_path,
    });
    let master = Master::new(store.clone(), auth);
    tokio::spawn(async move { axum::serve(listener, router(master)).await.unwrap() });
    Server {
        base: format!("http://{addr}"),
        store,
        dir,
    }
}

async fn req(
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
        rb = rb.header("cookie", c).same_origin();
    }
    let r = rb.send().await.unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    (status, cookie, r.json().await.unwrap_or(Value::Null))
}

async fn admin(s: &Server) -> String {
    let body = json!({"claim_code": CLAIM, "username": "alex", "password": "admin password"});
    let (st, c, v) = req(s, Method::POST, "/api/setup", Some(body), None).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    c.unwrap()
}

async fn login(s: &Server, user: &str, pw: &str) -> (StatusCode, Option<String>) {
    let body = json!({"username": user, "password": pw});
    let (st, c, _) = req(s, Method::POST, "/api/login", Some(body), None).await;
    (st, c)
}

/// Create `name`; returns its id and the password the server made.
async fn create(s: &Server, admin: &str, name: &str) -> (String, String) {
    let body = json!({"username": name});
    let (st, _, v) = req(s, Method::POST, "/api/users", Some(body), Some(admin)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"]
        .as_str()
        .expect("a password in the answer")
        .to_string();
    (v["user_id"].as_str().unwrap().to_string(), pw)
}

/// 4 groups of 5 from a 32-symbol alphabet: 100 bits.
fn assert_generated(pw: &str) {
    assert_eq!(pw.len(), 23, "{pw}");
    let groups: Vec<&str> = pw.split('-').collect();
    assert_eq!(groups.len(), 4, "{pw}");
    for g in groups {
        assert_eq!(g.len(), 5, "{pw}");
        assert!(
            g.bytes()
                .all(|b| b"0123456789abcdefghjkmnpqrstvwxyz".contains(&b)),
            "{pw}"
        );
    }
}

/// The plaintext is in no log line and in no file of the data directory.
fn assert_nowhere(s: &Server, pw: &str) {
    let logged = String::from_utf8_lossy(&logs().lock().unwrap()).to_string();
    assert!(!logged.is_empty(), "the capture sees the server's logs");
    assert!(!logged.contains(pw), "the password was logged");
    for entry in std::fs::read_dir(s.dir.path()).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.windows(pw.len()).any(|w| w == pw.as_bytes()),
            "the password is in {}",
            path.display()
        );
    }
}

async fn stored_hash(s: &Server, name: &str) -> (String, bool) {
    let name = name.to_string();
    let u = s
        .store
        .call(move |st| st.user_by_username(&name))
        .await
        .unwrap()
        .unwrap();
    (u.password_hash, u.must_change_password)
}

#[tokio::test]
async fn creating_a_user_answers_a_generated_password_once() {
    let s = start().await;
    let a = admin(&s).await;
    let (_, pw) = create(&s, &a, "bob").await;
    assert_generated(&pw);
    let (_, pw2) = create(&s, &a, "carol").await;
    assert_ne!(pw, pw2, "each account gets its own");

    // Only its hash is stored, and the account must change it.
    let (hash, must) = stored_hash(&s, "bob").await;
    assert!(hash.starts_with("$argon2id$"), "{hash}");
    assert!(!hash.contains(&pw));
    assert!(must);

    // Never again: not in the list, not in the user's own answer.
    let (_, _, list) = req(&s, Method::GET, "/api/users", None, Some(&a)).await;
    assert!(!list.to_string().contains(&pw));
    assert!(
        list["users"]
            .as_array()
            .unwrap()
            .iter()
            .all(|u| u.get("password").is_none())
    );

    // It works once, and only to set a password of the user's own.
    let (st, bc) = login(&s, "bob", &pw).await;
    assert_eq!(st, StatusCode::OK);
    let bc = bc.unwrap();
    let (_, _, me) = req(&s, Method::GET, "/api/me", None, Some(&bc)).await;
    assert_eq!(me["user"]["must_change_password"], true);
    assert!(!me.to_string().contains(&pw));
    let (st, _, _) = req(&s, Method::GET, "/api/sessions", None, Some(&bc)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let change = json!({"current": pw, "new": "bobs own password"});
    let (st, _, v) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(change),
        Some(&bc),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(login(&s, "bob", &pw).await.0, StatusCode::UNAUTHORIZED);
    assert_nowhere(&s, &pw);
    assert_nowhere(&s, &pw2);
}

#[tokio::test]
async fn an_admin_cannot_choose_a_password() {
    let s = start().await;
    let a = admin(&s).await;
    let chosen = json!({"username": "bob", "password": "the admins pick"});
    let (st, _, v) = req(&s, Method::POST, "/api/users", Some(chosen), Some(&a)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (_, _, list) = req(&s, Method::GET, "/api/users", None, Some(&a)).await;
    assert_eq!(
        list["users"].as_array().unwrap().len(),
        1,
        "no account made: {list}"
    );

    let (id, pw) = create(&s, &a, "bob").await;
    let chosen = json!({"password": "the admins pick"});
    let path = format!("/api/users/{id}");
    let (st, _, v) = req(&s, Method::PATCH, &path, Some(chosen), Some(&a)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(
        login(&s, "bob", "the admins pick").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(&s, "bob", &pw).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_reset_answers_a_new_password_and_ends_the_old_logins() {
    let s = start().await;
    let a = admin(&s).await;
    let (id, first) = create(&s, &a, "bob").await;
    let (_, bc) = login(&s, "bob", &first).await;
    let bc = bc.unwrap();
    let change = json!({"current": first, "new": "bobs own password"});
    let (_, bc2, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(change),
        Some(&bc),
    )
    .await;
    let bc2 = bc2.unwrap();

    let path = format!("/api/users/{id}/password");
    let (st, _, v) = req(&s, Method::POST, &path, None, Some(&a)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pw = v["password"].as_str().unwrap().to_string();
    assert_generated(&pw);
    assert_ne!(pw, first);
    assert_eq!(v["must_change_password"], true);
    let (hash, must) = stored_hash(&s, "bob").await;
    assert!(hash.starts_with("$argon2id$") && must);

    // Signed out everywhere; the old password is gone.
    let (st, _, _) = req(&s, Method::GET, "/api/me", None, Some(&bc2)).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert_eq!(
        login(&s, "bob", "bobs own password").await.0,
        StatusCode::UNAUTHORIZED
    );
    let (st, nc) = login(&s, "bob", &pw).await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, me) = req(&s, Method::GET, "/api/me", None, nc.as_deref()).await;
    assert_eq!(me["user"]["must_change_password"], true);

    // An unknown account, and the admin's own (they have /api/me/password).
    let (st, _, _) = req(
        &s,
        Method::POST,
        "/api/users/nobody/password",
        None,
        Some(&a),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (_, _, me) = req(&s, Method::GET, "/api/me", None, Some(&a)).await;
    let own = format!(
        "/api/users/{}/password",
        me["user"]["user_id"].as_str().unwrap()
    );
    let (st, _, _) = req(&s, Method::POST, &own, None, Some(&a)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    assert_nowhere(&s, &pw);
    assert_nowhere(&s, &first);
}

#[tokio::test]
async fn only_admins_make_or_reset_passwords() {
    let s = start().await;
    let a = admin(&s).await;
    let (bob_id, pw) = create(&s, &a, "bob").await;
    let (carol_id, _) = create(&s, &a, "carol").await;
    let (_, bc) = login(&s, "bob", &pw).await;
    let change = json!({"current": pw, "new": "bobs own password"});
    let (_, bc, _) = req(
        &s,
        Method::POST,
        "/api/me/password",
        Some(change),
        bc.as_deref(),
    )
    .await;
    let bc = bc.unwrap();

    let body = json!({"username": "mallory"});
    let (st, _, v) = req(&s, Method::POST, "/api/users", Some(body), Some(&bc)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.get("password").is_none());
    for id in [&carol_id, &bob_id] {
        let path = format!("/api/users/{id}/password");
        let (st, _, v) = req(&s, Method::POST, &path, None, Some(&bc)).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
        assert!(v.get("password").is_none());
    }
    // Signed out: 401, and nothing is made.
    let (st, _, _) = req(
        &s,
        Method::POST,
        &format!("/api/users/{carol_id}/password"),
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_login_throttle_covers_a_temporary_password() {
    let s = start().await;
    let a = admin(&s).await;
    let (_, pw) = create(&s, &a, "bob").await;
    for _ in 0..5 {
        assert_eq!(
            login(&s, "bob", "a wrong guess").await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    // Locked: even the right temporary password is refused.
    assert_eq!(login(&s, "bob", &pw).await.0, StatusCode::TOO_MANY_REQUESTS);
}
