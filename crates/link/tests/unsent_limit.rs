//! The node's socket to the master holds at most `UNSENT_LIMIT` bytes
//! unsent (A.21, review A21-4): without it the kernel's send buffer, up to
//! `tcp_wmem`'s maximum, is ahead of every ack the node sends.

#![cfg(any(target_os = "linux", target_os = "android"))]

use harness_link::{MasterCert, UNSENT_LIMIT, wire};
use tokio::net::TcpListener;

/// Read back from the socket `wire::connect` made.
#[tokio::test]
async fn the_nodes_socket_holds_at_most_the_unsent_limit() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cert = MasterCert::load_or_create(dir.path()).unwrap();
    let tls = cert.server_config().unwrap();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move {
        let (tcp, _) = l.accept().await.unwrap();
        let _ws = wire::accept(tcp, tls).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });
    let ws = wire::connect(&url, &cert.fingerprint()).await.unwrap();
    let (tcp, _) = ws.get_ref().get_ref();
    let lowat = socket2::SockRef::from(tcp).tcp_notsent_lowat().unwrap();
    assert_eq!(lowat, UNSENT_LIMIT);
}
