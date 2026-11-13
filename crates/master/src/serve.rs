//! The HTTP serve loop and its limits (audit S6).
//!
//! `axum::serve` has no knobs, so the loop is ours: it accepts, takes a
//! connection permit, runs the (optional) TLS handshake under its own
//! timeout, then hands the socket to hyper's HTTP/1 server with a
//! header-read timeout. HTTP/1 only, as with `axum::serve` and axum's default
//! features; there is no HTTP/2 state to bound.
//!
//! - **Slow clients.** hyper's `header_read_timeout` covers the whole head of
//!   each request (not each read), so half a request line, a silent socket
//!   and a header sent a byte a second all end at the same deadline. A TLS
//!   handshake that never completes ends at `handshake_timeout`. Neither
//!   gets as far as a handler.
//! - **Connection cap.** The permit is taken *before* `accept`: at the cap the
//!   server stops accepting and further clients wait in the kernel backlog
//!   until a slot frees (accept-and-close would turn a busy moment into
//!   errors for real users). The permit lives in the connection task, so it
//!   is released however the connection ends: finished, reset, timed out,
//!   handshake failure, panic. An upgraded connection (WebSocket) ends its
//!   hyper connection at the upgrade, which releases the permit: sockets do
//!   not count against the HTTP cap (their number is bounded per user in
//!   `ws.rs`), and they do not hold shutdown up.
//! - **Listeners.** One loop accepts from every listener it is given (the UI
//!   holds `127.0.0.1` and `[::1]` of its port, security review S2-1), in
//!   turn, so a flood on one does not starve the other; the cap and the
//!   shutdown are shared.
//! - **Shutdown.** When `shutdown` resolves the loop stops accepting, tells
//!   every connection to finish its in-flight request and close, and waits
//!   for them up to `shutdown_grace`.

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};

#[derive(Clone, Copy, Debug)]
pub struct ServeConfig {
    /// Time to receive a request's complete head, from the first byte of the
    /// wait for it (also the idle time of a keep-alive connection).
    pub header_read_timeout: Duration,
    /// Time for the TLS handshake (zero work for plain HTTP).
    pub handshake_timeout: Duration,
    /// Connections held at once.
    pub max_connections: usize,
    /// How long shutdown waits for in-flight requests.
    pub shutdown_grace: Duration,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            header_read_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(10),
            // A browser opens a handful per user; this is far above any
            // honest load and well under the usual 1024 file descriptors
            // (the node link, SQLite and tool children need theirs).
            max_connections: 512,
            shutdown_grace: Duration::from_secs(30),
        }
    }
}

/// Serve plain HTTP.
pub async fn serve(
    listeners: Vec<TcpListener>,
    app: Router,
    cfg: ServeConfig,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    serve_with(
        listeners,
        |s: TcpStream| async move { Ok(s) },
        app,
        cfg,
        shutdown,
    )
    .await
}

/// Serve over whatever `handshake` turns an accepted socket into (a TLS
/// stream). The handshake runs inside the connection's permit and under
/// `cfg.handshake_timeout`. Every listener serves the same app.
pub async fn serve_with<Io, H, Fut>(
    listeners: Vec<TcpListener>,
    handshake: H,
    app: Router,
    cfg: ServeConfig,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()>
where
    Io: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: Fn(TcpStream) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = io::Result<Io>> + Send + 'static,
{
    if listeners.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nothing to listen on",
        ));
    }
    let max = cfg.max_connections.max(1);
    let permits = Arc::new(Semaphore::new(max));
    let handshake = Arc::new(handshake);
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut next = 0usize;
    tokio::pin!(shutdown);
    loop {
        let permit = tokio::select! {
            biased;
            _ = &mut shutdown => break,
            p = permits.clone().acquire_owned() => p.expect("the semaphore is never closed"),
        };
        let (tcp, peer) = tokio::select! {
            biased;
            _ = &mut shutdown => break,
            r = accept(&listeners, &mut next) => match r {
                Ok(v) => v,
                Err(e) => {
                    // Usually out of file descriptors; back off, do not spin.
                    tracing::warn!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        let (handshake, app, mut stop) = (handshake.clone(), app.clone(), stop_rx.clone());
        tokio::spawn(async move {
            let _permit = permit;
            let io = match tokio::time::timeout(cfg.handshake_timeout, handshake(tcp)).await {
                Ok(Ok(io)) => io,
                Ok(Err(e)) => return tracing::debug!(%peer, error = %e, "handshake failed"),
                Err(_) => return tracing::debug!(%peer, "handshake timed out"),
            };
            let conn = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(cfg.header_read_timeout)
                .serve_connection(TokioIo::new(io), TowerToHyperService::new(app))
                .with_upgrades();
            tokio::pin!(conn);
            let result = tokio::select! {
                r = conn.as_mut() => r,
                _ = stop.changed() => {
                    // Finish the request in flight, then close.
                    conn.as_mut().graceful_shutdown();
                    conn.await
                }
            };
            if let Err(e) = result {
                tracing::debug!(%peer, error = %e, "connection ended");
            }
        });
    }
    drop(listeners);
    let _ = stop_tx.send(true);
    // Every connection holds one permit; all of them back means all closed.
    let all = permits.acquire_many(max as u32);
    if tokio::time::timeout(cfg.shutdown_grace, all).await.is_err() {
        tracing::warn!(
            grace_secs = cfg.shutdown_grace.as_secs(),
            "shutdown: closing connections still busy after the grace period"
        );
    }
    Ok(())
}

/// The next connection on any of `listeners`, polled from `next` on so that
/// each gets its turn.
async fn accept(
    listeners: &[TcpListener],
    next: &mut usize,
) -> io::Result<(TcpStream, std::net::SocketAddr)> {
    std::future::poll_fn(|cx| {
        let n = listeners.len();
        for i in 0..n {
            let at = (*next + i) % n;
            if let Poll::Ready(r) = listeners[at].poll_accept(cx) {
                *next = (at + 1) % n;
                return Poll::Ready(r);
            }
        }
        Poll::Pending
    })
    .await
}
