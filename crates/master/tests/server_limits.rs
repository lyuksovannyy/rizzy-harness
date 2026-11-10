//! The serve loop's limits (audit S6): slow clients, the connection cap,
//! graceful shutdown, and that long-lived routes are not cut by the request
//! timeout. Timeouts are shortened so each test takes about a second.

use std::future::pending;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::{get, post};
use futures::{SinkExt, StreamExt};
use harness_core::Secret;
use harness_master::auth::{Auth, Claim};
use harness_master::limits::{Timeouts, enforce};
use harness_master::serve::{ServeConfig, serve, serve_with};
use harness_master::{Master, router_with};
use harness_store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const HEADER: Duration = Duration::from_millis(600);

fn cfg() -> ServeConfig {
    ServeConfig {
        header_read_timeout: HEADER,
        handshake_timeout: HEADER,
        max_connections: 512,
        shutdown_grace: Duration::from_millis(800),
    }
}

struct Server {
    addr: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    done: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Server {
    async fn stop(mut self) -> Duration {
        let t = Instant::now();
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(5), self.done)
            .await
            .expect("serve returned after shutdown")
            .unwrap()
            .unwrap();
        t.elapsed()
    }
}

async fn start(app: Router, cfg: ServeConfig) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, rx) = oneshot::channel::<()>();
    let done = tokio::spawn(serve(listener, app, cfg, async {
        let _ = rx.await;
    }));
    Server {
        addr,
        stop: Some(stop),
        done,
    }
}

fn hello() -> Router {
    Router::new().route("/", get(|| async { "hello" }))
}

/// True when the server closes the connection (EOF or reset) within `within`.
async fn closed_within(s: &mut TcpStream, within: Duration) -> bool {
    let mut b = [0u8; 256];
    match tokio::time::timeout(within, s.read(&mut b)).await {
        Err(_) => false,
        Ok(Ok(0)) | Ok(Err(_)) => true,
        // A reply (408) before the close is fine; keep reading to the end.
        Ok(Ok(_)) => matches!(
            tokio::time::timeout(within, s.read(&mut b)).await,
            Ok(Ok(0)) | Ok(Err(_))
        ),
    }
}

async fn get_hello(addr: SocketAddr) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn half_a_request_line_is_disconnected() {
    let srv = start(hello(), cfg()).await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    s.write_all(b"GET /api/se").await.unwrap();
    assert!(
        closed_within(&mut s, HEADER * 4).await,
        "a half request line must not hold the connection open"
    );
    srv.stop().await;
}

#[tokio::test]
async fn a_silent_connection_is_disconnected() {
    let srv = start(hello(), cfg()).await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    assert!(closed_within(&mut s, HEADER * 4).await);
    srv.stop().await;
}

#[tokio::test]
async fn headers_one_byte_at_a_time_are_cut_off_in_total() {
    // Each byte arrives well inside the timeout; only a limit on the whole
    // header read stops it.
    let srv = start(hello(), cfg()).await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    let t = Instant::now();
    let slow = b"GET / HTTP/1.1\r\nhost: x\r\nx-a: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    for b in slow {
        if s.write_all(&[*b]).await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        if t.elapsed() > HEADER * 4 {
            break;
        }
    }
    assert!(closed_within(&mut s, Duration::from_millis(500)).await);
    assert!(t.elapsed() < HEADER * 4, "cut off near the header timeout");
    srv.stop().await;
}

#[tokio::test]
async fn a_keep_alive_connection_serves_a_second_request() {
    let srv = start(hello(), cfg()).await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    for _ in 0..2 {
        s.write_all(b"GET / HTTP/1.1\r\nhost: x\r\n\r\n")
            .await
            .unwrap();
        let mut b = [0u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(2), s.read(&mut b))
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&b[..n]).starts_with("HTTP/1.1 200"));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    srv.stop().await;
}

#[tokio::test]
async fn at_the_cap_a_further_connection_waits_and_a_freed_slot_is_reused() {
    let mut c = cfg();
    c.max_connections = 1;
    let srv = start(hello(), c).await;
    // The first connection takes the only slot and says nothing.
    let mut a = TcpStream::connect(srv.addr).await.unwrap();
    // The second is not served while the first holds the slot...
    let second = tokio::spawn(get_hello(srv.addr));
    tokio::time::sleep(HEADER / 3).await;
    assert!(!second.is_finished(), "served past the cap");
    // ...and is served once the first is cut off by the header timeout
    // (the permit is released when a connection ends in any way).
    assert!(closed_within(&mut a, HEADER * 4).await);
    let body = tokio::time::timeout(Duration::from_secs(3), second)
        .await
        .expect("slot reused")
        .unwrap();
    assert!(body.starts_with("HTTP/1.1 200"), "{body}");
    // A client that just hangs up also frees its slot.
    let a = TcpStream::connect(srv.addr).await.unwrap();
    drop(a);
    assert!(get_hello(srv.addr).await.starts_with("HTTP/1.1 200"));
    srv.stop().await;
}

#[tokio::test]
async fn a_handshake_that_never_completes_is_dropped_and_frees_its_slot() {
    let mut c = cfg();
    c.max_connections = 1;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let first = Arc::new(AtomicBool::new(true));
    // The first connection's handshake hangs; later ones pass straight through.
    let handshake = move |s: TcpStream| {
        let hang = first.swap(false, Ordering::SeqCst);
        async move {
            if hang {
                pending::<()>().await;
            }
            Ok::<_, std::io::Error>(s)
        }
    };
    let (stop, rx) = oneshot::channel::<()>();
    tokio::spawn(serve_with(listener, handshake, hello(), c, async {
        let _ = rx.await;
    }));
    let mut a = TcpStream::connect(addr).await.unwrap();
    let t = Instant::now();
    assert!(closed_within(&mut a, HEADER * 4).await);
    assert!(t.elapsed() < HEADER * 4);
    assert!(
        tokio::time::timeout(Duration::from_secs(3), get_hello(addr))
            .await
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn shutdown_lets_an_in_flight_request_finish_then_stops() {
    let app = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(400)).await;
            "done"
        }),
    );
    let srv = start(app, cfg()).await;
    let addr = srv.addr;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET /slow HTTP/1.1\r\nhost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let took = srv.stop().await;
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    assert!(
        out.starts_with("HTTP/1.1 200") && out.ends_with("done"),
        "{out}"
    );
    assert!(
        took < Duration::from_millis(790),
        "no need to wait the whole grace: {took:?}"
    );
    // The listener is gone: nobody new is accepted.
    assert!(TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn shutdown_is_bounded_when_a_request_never_finishes() {
    let app = Router::new().route("/hang", get(pending::<&'static str>));
    let srv = start(app, cfg()).await;
    let mut s = TcpStream::connect(srv.addr).await.unwrap();
    s.write_all(b"GET /hang HTTP/1.1\r\nhost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let took = srv.stop().await;
    assert!(
        took >= Duration::from_millis(700),
        "waited for the grace: {took:?}"
    );
    assert!(took < Duration::from_secs(3), "{took:?}");
}

// ---- the request timeout and its exemptions ----

struct Flag(Arc<AtomicBool>);
impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Routes named like real ones that all sleep for 5 s, behind `enforce`
/// with a 300 ms short and a 1.2 s long limit.
fn sleepers(dropped: Arc<AtomicBool>) -> Router {
    let sleeper = move || {
        let d = dropped.clone();
        async move {
            let _f = Flag(d);
            tokio::time::sleep(Duration::from_secs(5)).await;
            "late"
        }
    };
    Router::new()
        .route("/api/me", get(sleeper.clone()))
        .route("/api/agent-summary", post(sleeper.clone()))
        .route("/api/attachments", post(sleeper.clone()))
        .route("/api/media/{id}/content", get(sleeper))
        .route_layer(axum::middleware::from_fn_with_state(
            Timeouts {
                short: Duration::from_millis(300),
                long: Duration::from_millis(1200),
            },
            enforce,
        ))
}

async fn call(
    addr: SocketAddr,
    method: reqwest::Method,
    path: &str,
    wait: Duration,
) -> Option<(u16, String)> {
    let c = reqwest::Client::new();
    let r = tokio::time::timeout(
        wait,
        c.request(method, format!("http://{addr}{path}")).send(),
    )
    .await
    .ok()?
    .unwrap();
    let st = r.status().as_u16();
    Some((st, r.text().await.unwrap()))
}

#[tokio::test]
async fn a_sleeping_handler_is_cut_and_dropped() {
    let dropped = Arc::new(AtomicBool::new(false));
    let srv = start(sleepers(dropped.clone()), cfg()).await;
    let t = Instant::now();
    let (st, body) = call(
        srv.addr,
        reqwest::Method::GET,
        "/api/me",
        Duration::from_secs(3),
    )
    .await
    .expect("cut before the handler finished");
    assert!(t.elapsed() < Duration::from_secs(2));
    assert_eq!(st, 504);
    assert_eq!(body, r#"{"error":"request timed out"}"#);
    assert!(
        dropped.load(Ordering::SeqCst),
        "the handler's future was dropped"
    );
    srv.stop().await;
}

#[tokio::test]
async fn long_routes_get_the_longer_limit() {
    let srv = start(sleepers(Arc::default()), cfg()).await;
    // Past the short limit and not cut...
    let m = reqwest::Method::POST;
    assert!(
        call(
            srv.addr,
            m.clone(),
            "/api/agent-summary",
            Duration::from_millis(700)
        )
        .await
        .is_none()
    );
    // ...but cut by the long one.
    let (st, _) = call(srv.addr, m, "/api/agent-summary", Duration::from_secs(3))
        .await
        .unwrap();
    assert_eq!(st, 504);
    srv.stop().await;
}

#[tokio::test]
async fn uploads_and_blob_downloads_are_not_cut() {
    let srv = start(sleepers(Arc::default()), cfg()).await;
    for (m, p) in [
        (reqwest::Method::POST, "/api/attachments"),
        (reqwest::Method::GET, "/api/media/x/content"),
    ] {
        assert!(
            call(srv.addr, m, p, Duration::from_millis(1800))
                .await
                .is_none(),
            "{p} was cut"
        );
    }
    srv.stop().await;
}

// ---- the real router ----

const REQUEST: Duration = Duration::from_millis(500);

async fn master_server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    // The Host allowlist needs the address; bind first.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let app = router_with(
        master,
        Timeouts {
            short: REQUEST,
            long: REQUEST * 4,
        },
    );
    let (stop, rx) = oneshot::channel::<()>();
    let done = tokio::spawn(async move {
        let _keep = dir;
        serve(listener, app, cfg(), async {
            let _ = rx.await;
        })
        .await
    });
    Server {
        addr,
        stop: Some(stop),
        done,
    }
}

async fn admin_cookie(addr: SocketAddr) -> String {
    let r = reqwest::Client::new()
        .post(format!("http://{addr}/api/setup"))
        .json(&serde_json::json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_websocket_outlives_the_request_timeout_and_does_not_hold_shutdown() {
    let srv = master_server().await;
    let cookie = admin_cookie(srv.addr).await;
    let mut r = format!("ws://{}/api/ws", srv.addr)
        .into_client_request()
        .unwrap();
    r.headers_mut().insert("cookie", cookie.parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(r).await.unwrap();
    tokio::time::sleep(REQUEST * 3).await;
    ws.send(Message::Text("hello".into())).await.unwrap();
    let got = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("the socket still answers");
    assert!(matches!(got, Some(Ok(Message::Text(_)))), "{got:?}");
    let took = srv.stop().await;
    assert!(took < Duration::from_millis(700), "{took:?}");
}

#[tokio::test]
async fn a_slow_upload_through_the_real_router_is_not_cut() {
    let srv = master_server().await;
    let cookie = admin_cookie(srv.addr).await;
    // Chunks arrive further apart than the request timeout.
    let body = futures::stream::unfold(0u8, |i| async move {
        if i == 3 {
            return None;
        }
        tokio::time::sleep(REQUEST * 2 / 3).await;
        Some((
            Ok::<_, std::io::Error>(format!("--b\r\nchunk {i}\r\n").into_bytes()),
            i + 1,
        ))
    });
    let r = reqwest::Client::new()
        .post(format!("http://{}/api/attachments", srv.addr))
        .header("cookie", cookie)
        .header("content-type", "multipart/form-data; boundary=b")
        .body(reqwest::Body::wrap_stream(body))
        .send()
        .await
        .unwrap();
    // The (malformed) body is the handler's to reject; the timeout is not.
    assert_ne!(r.status(), 504, "the upload was cut by the request timeout");
    srv.stop().await;
}
