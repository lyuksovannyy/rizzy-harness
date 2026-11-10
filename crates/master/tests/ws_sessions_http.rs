//! UI WebSockets end with the login (FIX_PLAN 3.2, AUDIT S5): every path
//! that ends a login closes the sockets opened with it, a socket re-checks
//! its login on a timer and before every subscribe, client messages are at
//! most 64 KiB, and a user has at most 8 sockets.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_core::Secret;
use harness_master::auth::{self, Auth, Claim};
use harness_master::{Master, router};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

struct T {
    base: String,
    addr: std::net::SocketAddr,
    master: Master,
    _dir: tempfile::TempDir,
}

/// A server with no nodes; sockets re-check their login every `recheck`.
async fn start(recheck: Duration) -> T {
    let dir = tempfile::tempdir().unwrap();
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
    master.set_socket_recheck(recheck);
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        addr,
        master,
        _dir: dir,
    }
}

const LONG: Duration = Duration::from_secs(3600);

async fn req(t: &T, method: Method, path: &str, body: Option<Value>, c: &str) -> StatusCode {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    rb.header("cookie", c).send().await.unwrap().status()
}

fn cookie(r: &reqwest::Response) -> String {
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn post(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.header("cookie", c);
    }
    rb.send().await.unwrap()
}

async fn login(t: &T, user: &str, pw: &str) -> String {
    let r = post(
        t,
        "/api/login",
        json!({"username": user, "password": pw}),
        None,
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    cookie(&r)
}

/// The admin "alex" and a member "bob": (cookie, user id) each.
async fn users(t: &T) -> [(String, String); 2] {
    let alex = cookie(
        &post(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    );
    let body = json!({"username": "bob", "password": "temporary pw"});
    assert_eq!(
        req(t, Method::POST, "/api/users", Some(body), &alex).await,
        StatusCode::OK
    );
    let first = login(t, "bob", "temporary pw").await;
    let bob = cookie(
        &post(
            t,
            "/api/me/password",
            json!({"current": "temporary pw", "new": "bobs own password"}),
            Some(&first),
        )
        .await,
    );
    let mut out = Vec::new();
    for c in [alex, bob] {
        let me: Value = reqwest::Client::new()
            .get(format!("{}/api/me", t.base))
            .header("cookie", &c)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        out.push((c, me["user"]["user_id"].as_str().unwrap().to_string()));
    }
    out.try_into().unwrap()
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn try_socket(t: &T, c: &str) -> Result<Socket, WsError> {
    let mut r = format!("ws://{}/api/ws", t.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", c.parse().unwrap());
    let (mut s, _) = tokio_tungstenite::connect_async(r).await?;
    // Answered: the server's loop is running.
    s.send(Message::Text("hello".into())).await.unwrap();
    assert_eq!(next_text(&mut s).await.unwrap()["type"], "error");
    Ok(s)
}

async fn socket(t: &T, c: &str) -> Socket {
    try_socket(t, c).await.unwrap()
}

/// The next text message, or `None` when the socket ended (close frame,
/// end of stream or error). Panics if nothing happens within 5 s.
async fn next_text(s: &mut Socket) -> Option<Value> {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), s.next())
            .await
            .expect("socket went quiet");
        match msg {
            Some(Ok(Message::Text(text))) => return Some(serde_json::from_str(&text).unwrap()),
            Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
            Some(Ok(_)) => {}
        }
    }
}

/// How the socket ended within `within`: `Some(code)` for a close frame
/// (0 when it had none), `None` for an end without one. Panics when a text
/// message arrives instead, or nothing happens in time.
async fn ended(s: &mut Socket, within: Duration) -> Option<u16> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let msg = tokio::time::timeout_at(deadline, s.next())
            .await
            .expect("the socket is still open");
        match msg {
            Some(Ok(Message::Close(f))) => return Some(f.map(|f| u16::from(f.code)).unwrap_or(0)),
            None | Some(Err(_)) => return None,
            Some(Ok(Message::Text(t))) => panic!("socket still delivers: {t}"),
            Some(Ok(_)) => {}
        }
    }
}

/// The socket is still served: it answers a message (after any content-free
/// `*_changed` it was sent meanwhile).
async fn alive(s: &mut Socket) {
    s.send(Message::Text("still there?".into())).await.unwrap();
    loop {
        let v = next_text(s).await.expect("socket was closed");
        if v["type"] == "error" {
            return;
        }
        assert!(v["type"].as_str().unwrap().ends_with("_changed"), "{v}");
    }
}

const SIGNED_OUT: Option<u16> = Some(4401);
const SECOND: Duration = Duration::from_secs(1);

#[tokio::test]
async fn logout_closes_that_sessions_socket_within_a_second() {
    let t = start(LONG).await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let bob2 = login(&t, "bob", "bobs own password").await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let mut b2 = socket(&t, &bob2).await;
    assert_eq!(
        req(&t, Method::POST, "/api/logout", None, &bob).await,
        StatusCode::OK
    );
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
    // Logout ends one login: bob's other browser and alex stay.
    alive(&mut b2).await;
    alive(&mut a).await;
}

#[tokio::test]
async fn disabling_an_account_closes_its_sockets() {
    let t = start(LONG).await;
    let [(alex, _), (bob, bob_id)] = users(&t).await;
    let bob2 = login(&t, "bob", "bobs own password").await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let mut b2 = socket(&t, &bob2).await;
    let st = req(
        &t,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"disabled": true})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
    assert_eq!(ended(&mut b2, SECOND).await, SIGNED_OUT);
    alive(&mut a).await;
    // And no new one opens.
    assert!(try_socket(&t, &bob).await.is_err());
}

#[tokio::test]
async fn deleting_an_account_closes_its_sockets() {
    let t = start(LONG).await;
    let [(alex, _), (bob, bob_id)] = users(&t).await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let st = req(
        &t,
        Method::DELETE,
        &format!("/api/users/{bob_id}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
    alive(&mut a).await;
}

#[tokio::test]
async fn an_admin_password_reset_closes_the_users_sockets() {
    let t = start(LONG).await;
    let [(alex, _), (bob, bob_id)] = users(&t).await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let st = req(
        &t,
        Method::PATCH,
        &format!("/api/users/{bob_id}"),
        Some(json!({"password": "new temporary"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
    alive(&mut a).await;
}

/// The HTTP side revokes every session of the user (and hands this browser
/// a new cookie), so every socket closes, this browser's too; it reconnects
/// with the new cookie.
#[tokio::test]
async fn changing_your_password_closes_all_your_sockets() {
    let t = start(LONG).await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let bob2 = login(&t, "bob", "bobs own password").await;
    let mut a = socket(&t, &alex).await;
    let mut b = socket(&t, &bob).await;
    let mut b2 = socket(&t, &bob2).await;
    let r = post(
        &t,
        "/api/me/password",
        json!({"current": "bobs own password", "new": "another password"}),
        Some(&bob),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let fresh = cookie(&r);
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
    assert_eq!(ended(&mut b2, SECOND).await, SIGNED_OUT);
    alive(&mut a).await;
    alive(&mut socket(&t, &fresh).await).await;
}

/// A login that runs out while its socket sits open: the timer finds it,
/// and the checks did not keep it alive.
#[tokio::test]
async fn an_expired_login_closes_its_socket_on_the_timer() {
    let t = start(Duration::from_millis(100)).await;
    let [(_, alex_id), _] = users(&t).await;
    let id = "ab".repeat(32);
    let now = harness_core::now_ms();
    t.master
        .inner_mirror()
        .ui_session_create(&auth::sha256_hex(&id), &alex_id, now, now + 1_500)
        .unwrap();
    let c = format!("{}={id}", auth::cookie_name());
    let mut s = socket(&t, &c).await;
    alive(&mut s).await;
    assert_eq!(ended(&mut s, Duration::from_secs(4)).await, SIGNED_OUT);
}

/// A login removed with no message sent (a missed or future revoke path):
/// the next subscribe re-checks it and closes the socket instead of
/// serving anything.
#[tokio::test]
async fn a_subscribe_rechecks_the_login() {
    let t = start(LONG).await;
    let [_, (bob, _)] = users(&t).await;
    let mut b = socket(&t, &bob).await;
    let id = bob.split_once('=').unwrap().1;
    assert!(
        t.master
            .inner_mirror()
            .ui_session_delete(&auth::sha256_hex(id))
            .unwrap()
    );
    let sub = json!({"type": "subscribe", "session_id": harness_core::SessionId::new()});
    b.send(Message::Text(sub.to_string().into())).await.unwrap();
    assert_eq!(ended(&mut b, SECOND).await, SIGNED_OUT);
}

#[tokio::test]
async fn a_one_mib_client_frame_is_rejected() {
    let t = start(LONG).await;
    let [_, (bob, _)] = users(&t).await;
    let mut b = socket(&t, &bob).await;
    let _ = b.send(Message::Text("x".repeat(1024 * 1024).into())).await;
    // Not parsed and answered: the connection ends.
    ended(&mut b, Duration::from_secs(2)).await;
    // 64 KiB is the limit, so a message just under it is still served.
    let mut b = socket(&t, &bob).await;
    b.send(Message::Text("x".repeat(60 * 1024).into()))
        .await
        .unwrap();
    assert_eq!(next_text(&mut b).await.unwrap()["type"], "error");
}

#[tokio::test]
async fn a_ninth_socket_is_refused_until_one_closes() {
    let t = start(LONG).await;
    let [(alex, _), (bob, _)] = users(&t).await;
    let mut open = Vec::new();
    for _ in 0..8 {
        open.push(socket(&t, &bob).await);
    }
    match try_socket(&t, &bob).await {
        Err(WsError::Http(r)) => assert_eq!(r.status().as_u16(), 429),
        other => panic!("ninth socket: {:?}", other.map(|_| ())),
    }
    // Per user: alex is not affected.
    let _a = socket(&t, &alex).await;
    assert_eq!(t.master.socket_count(), 9);
    // A closed socket gives its place back...
    open.pop().unwrap().close(None).await.unwrap();
    settles(&t, 8).await;
    open.push(socket(&t, &bob).await);
    // ...and so does one that just drops, with no close frame.
    drop(open.pop());
    settles(&t, 8).await;
    open.push(socket(&t, &bob).await);
    // A revoked one too.
    assert_eq!(
        req(&t, Method::POST, "/api/logout", None, &bob).await,
        StatusCode::OK
    );
    settles(&t, 1).await;
}

/// Wait (at most 2 s) until the server holds `n` UI sockets.
async fn settles(t: &T, n: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while t.master.socket_count() != n {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} sockets, expected {n}",
            t.master.socket_count()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
