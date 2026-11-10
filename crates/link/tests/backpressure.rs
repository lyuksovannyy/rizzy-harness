//! Back-pressure and timeouts on the node link (FIX_PLAN 3.7, audit L1).
//!
//! These run the real TLS + WebSocket transport on loopback. What they
//! measure is queue length (messages parsed and waiting for the consumer)
//! and how far the peer got with sending, not process memory.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::SinkExt;
use harness_link::client::{self, Remote, connect_node};
use harness_link::wire::{self, ClientStream, Hello, ServerMsg, ServerStream};
use harness_link::{MasterCert, NodeKey, Timing, server};
use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

struct Listener {
    url: String,
    fp: String,
    accepted: mpsc::UnboundedReceiver<ServerStream>,
    _dir: tempfile::TempDir,
}

/// A master-side TLS + WebSocket acceptor (no node handshake: `bridge`
/// starts after it).
async fn listener() -> Listener {
    let dir = tempfile::tempdir().unwrap();
    let cert = MasterCert::load_or_create(dir.path()).unwrap();
    let tls = cert.server_config().unwrap();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://127.0.0.1:{}", l.local_addr().unwrap().port());
    let (tx, accepted) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = l.accept().await {
            let (tls, tx) = (tls.clone(), tx.clone());
            tokio::spawn(async move {
                if let Ok(ws) = wire::accept(tcp, tls).await {
                    let _ = tx.send(ws);
                }
            });
        }
    });
    Listener {
        url,
        fp: cert.fingerprint(),
        accepted,
        _dir: dir,
    }
}

fn hello(id: &str) -> String {
    serde_json::to_string(&NodeToMaster::Hello {
        protocol_version: harness_protocol::PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "v".into(),
        capabilities: vec![],
        sessions: vec![],
    })
    .unwrap()
}

/// A valid frame of about `pad` bytes.
fn ack(i: usize, pad: usize) -> String {
    serde_json::to_string(&NodeToMaster::Ack {
        req_id: i.to_string(),
        ok: false,
        error: Some("x".repeat(pad)),
    })
    .unwrap()
}

async fn connect(l: &mut Listener) -> (ClientStream, ServerStream) {
    let c = wire::connect(&l.url, &l.fp).await.unwrap();
    let s = l.accepted.recv().await.unwrap();
    (c, s)
}

/// Wait until `n` has started moving and then stops for `quiet`, at most
/// `max`. Quiet before the first count is not "settled": on a busy machine
/// the handshake alone can take longer than `quiet`.
async fn settle(n: &AtomicUsize, quiet: Duration, max: Duration) -> usize {
    let end = tokio::time::Instant::now() + max;
    let mut last = n.load(Ordering::SeqCst);
    loop {
        tokio::time::sleep(quiet).await;
        let now = n.load(Ordering::SeqCst);
        if (now == last && now > 0) || tokio::time::Instant::now() > end {
            return now;
        }
        last = now;
    }
}

const FLOOD: usize = 2000;
const PAD: usize = 32 * 1024;

/// A node that floods valid frames while the master's consumer is stalled:
/// the master stops reading its socket once a few messages are queued, the
/// node's sends stall, nothing is lost or reordered, and another node's
/// link keeps flowing meanwhile.
#[tokio::test]
async fn flooding_node_is_throttled_not_buffered() {
    let mut l = listener().await;
    let (mut client, srv) = connect(&mut l).await;
    let (_to_node, mut from_node) = server::bridge(srv, "n1".into(), CancellationToken::new());

    let sent = Arc::new(AtomicUsize::new(0));
    let s2 = sent.clone();
    tokio::spawn(async move {
        client.send(Message::text(hello("n1"))).await.unwrap();
        for i in 0..FLOOD {
            if client.send(Message::text(ack(i, PAD))).await.is_err() {
                return;
            }
            s2.fetch_add(1, Ordering::SeqCst);
        }
        // Keep the socket open until the test is done.
        std::future::pending::<()>().await;
    });

    // Nobody reads `from_node`: the store is "stalled".
    let got_out = settle(&sent, Duration::from_millis(300), Duration::from_secs(10)).await;
    let queued = from_node.len();
    assert!(
        queued <= harness_link::FROM_NODE_QUEUE,
        "the master queued {queued} messages from a node it is not consuming"
    );
    assert!(
        got_out < FLOOD,
        "the node sent all {FLOOD} frames ({} MiB) while the master read none: no back-pressure",
        (FLOOD * PAD) >> 20
    );

    // Another node is not affected by the stalled one.
    let (mut other, srv2) = connect(&mut l).await;
    let (_t2, mut from_other) = server::bridge(srv2, "n2".into(), CancellationToken::new());
    other.send(Message::text(hello("n2"))).await.unwrap();
    other.send(Message::text(ack(7, 10))).await.unwrap();
    for want in ["hello", "7"] {
        let m = tokio::time::timeout(Duration::from_secs(2), from_other.recv())
            .await
            .expect("the second node's link stalled")
            .unwrap();
        match (want, m) {
            ("hello", NodeToMaster::Hello { .. }) => {}
            ("7", NodeToMaster::Ack { req_id, .. }) if req_id == "7" => {}
            (w, m) => panic!("wanted {w}, got {m:?}"),
        }
    }

    // The consumer catches up: every frame arrives, in order.
    match from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        m => panic!("{m:?}"),
    }
    for i in 0..FLOOD {
        let m = tokio::time::timeout(Duration::from_secs(10), from_node.recv())
            .await
            .expect("link stalled while draining")
            .expect("link closed while draining");
        match m {
            NodeToMaster::Ack { req_id, .. } => assert_eq!(req_id, i.to_string()),
            m => panic!("{m:?}"),
        }
    }
}

fn quick() -> Timing {
    Timing {
        ping_every: Duration::from_millis(100),
        idle_limit: Duration::from_millis(300),
        connect: Duration::from_millis(300),
        handshake: Duration::from_millis(300),
    }
}

/// A master whose store stalls for longer than the idle limit does not
/// call a node that keeps sending dead: silence is counted only while the
/// master is actually reading.
#[tokio::test]
async fn busy_link_is_not_declared_dead() {
    let mut l = listener().await;
    let (mut client, srv) = connect(&mut l).await;
    let (_to_node, mut from_node) =
        server::bridge_with(srv, "n1".into(), CancellationToken::new(), quick());
    let n = 400;
    tokio::spawn(async move {
        client.send(Message::text(hello("n1"))).await.unwrap();
        for i in 0..n {
            if client.send(Message::text(ack(i, PAD))).await.is_err() {
                return;
            }
        }
        std::future::pending::<()>().await;
    });
    // The consumer stalls for five idle limits.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(matches!(
        from_node.recv().await,
        Some(NodeToMaster::Hello { .. })
    ));
    for i in 0..n {
        match from_node.recv().await {
            Some(NodeToMaster::Ack { req_id, .. }) => assert_eq!(req_id, i.to_string()),
            m => panic!("frame {i}: {m:?} (the link was closed while busy)"),
        }
    }
}

/// A node that goes quiet (sends nothing, answers no ping) is still
/// dropped after the idle limit.
#[tokio::test]
async fn silent_node_is_dropped() {
    let mut l = listener().await;
    let (mut client, srv) = connect(&mut l).await;
    let (_to_node, mut from_node) =
        server::bridge_with(srv, "n1".into(), CancellationToken::new(), quick());
    client.send(Message::text(hello("n1"))).await.unwrap();
    assert!(matches!(
        from_node.recv().await,
        Some(NodeToMaster::Hello { .. })
    ));
    // `client` is kept but never read or written again.
    let closed = tokio::time::timeout(Duration::from_secs(3), from_node.recv()).await;
    assert!(matches!(closed, Ok(None)), "{closed:?}");
    drop(client);
}

/// A frame over the limit closes the link; it is refused from its header.
#[tokio::test]
async fn oversized_frame_closes_the_link() {
    let mut l = listener().await;
    let (mut client, srv) = connect(&mut l).await;
    let (_to_node, mut from_node) = server::bridge(srv, "n1".into(), CancellationToken::new());
    client.send(Message::text(hello("n1"))).await.unwrap();
    assert!(matches!(
        from_node.recv().await,
        Some(NodeToMaster::Hello { .. })
    ));
    // The limit is on reading; the sender's config lets it write this.
    let _ = client
        .send(Message::text(ack(0, harness_link::MAX_MESSAGE + 1)))
        .await;
    let closed = tokio::time::timeout(Duration::from_secs(5), from_node.recv()).await;
    assert!(matches!(closed, Ok(None)), "{closed:?}");
}

fn remote(url: String, fp: String) -> Remote {
    Remote {
        url,
        fingerprint: fp,
        node_id: "n1".into(),
    }
}

/// How long a `connect_node` that must fail took to give up.
async fn must_fail(r: Remote) -> Duration {
    let dir = tempfile::tempdir().unwrap();
    let key = NodeKey::load_or_create(&dir.path().join("k")).unwrap();
    let t0 = tokio::time::Instant::now();
    let res = tokio::time::timeout(Duration::from_secs(5), connect_node(&r, &key, quick()))
        .await
        .expect("connect_node did not give up");
    assert!(res.is_err());
    t0.elapsed()
}

fn any_fp() -> String {
    format!("sha256:{}", "0".repeat(64))
}

/// A "master" that accepts TCP and then says nothing.
#[tokio::test]
async fn node_gives_up_on_a_silent_master() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move {
        let _keep = l.accept().await;
        std::future::pending::<()>().await
    });
    let took = must_fail(remote(url, any_fp())).await;
    assert!(took < Duration::from_secs(1), "{took:?}");
}

/// A "master" that drips a TLS record a byte at a time: the deadline is for
/// the whole step, not per byte.
#[tokio::test]
async fn node_gives_up_on_a_drip_fed_handshake() {
    use tokio::io::AsyncWriteExt;
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("wss://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move {
        let (mut tcp, _) = l.accept().await.unwrap();
        // A handshake record header announcing 16 KiB, then the body slowly.
        let _ = tcp.write_all(&[0x16, 0x03, 0x03, 0x40, 0x00]).await;
        loop {
            if tcp.write_all(&[0]).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let took = must_fail(remote(url, any_fp())).await;
    assert!(took < Duration::from_secs(1), "{took:?}");
}

/// A master that completes TLS and the upgrade but never sends the
/// challenge.
#[tokio::test]
async fn node_gives_up_when_no_challenge_comes() {
    let mut l = listener().await;
    let (url, fp) = (l.url.clone(), l.fp.clone());
    tokio::spawn(async move {
        let _ws = l.accepted.recv().await;
        std::future::pending::<()>().await
    });
    let took = must_fail(remote(url, fp)).await;
    assert!(took < Duration::from_secs(1), "{took:?}");
}

/// A master that floods commands at a node whose command loop is not
/// keeping up: the node stops reading, its queue stays at its bound, and
/// the master's writes stall instead of the node buffering them.
#[tokio::test]
async fn flooding_master_is_throttled_by_the_node() {
    const IN_QUEUE: usize = 4;
    let mut l = listener().await;
    let (url, fp) = (l.url.clone(), l.fp.clone());
    let sent = Arc::new(AtomicUsize::new(0));
    let s2 = sent.clone();
    // A fake master: the real handshake, then a flood.
    tokio::spawn(async move {
        let mut ws = l.accepted.recv().await.unwrap();
        let nonce = wire::new_nonce();
        wire::send_json(&mut ws, &ServerMsg::Challenge { nonce })
            .await
            .unwrap();
        let _: Option<Hello> = wire::recv_json(&mut ws).await.unwrap();
        let ok = ServerMsg::Accepted {
            node_id: "n1".into(),
        };
        wire::send_json(&mut ws, &ok).await.unwrap();
        let pad = "x".repeat(PAD);
        for _ in 0..FLOOD {
            let frame =
                format!(r#"{{"type":"master.welcome","protocol_version":1,"pad":"{pad}"}}"#);
            if ws.send(Message::text(frame)).await.is_err() {
                return;
            }
            s2.fetch_add(1, Ordering::SeqCst);
        }
        std::future::pending::<()>().await
    });
    let dir = tempfile::tempdir().unwrap();
    let key = NodeKey::load_or_create(&dir.path().join("k")).unwrap();
    // The "runtime": a bounded command queue nobody drains.
    let (in_tx, in_rx) = mpsc::channel::<MasterToNode>(IN_QUEUE);
    let (_out_tx, out_rx) = mpsc::channel::<NodeToMaster>(4);
    let parts = Arc::new(std::sync::Mutex::new(Some((in_tx, out_rx))));
    let stop = CancellationToken::new();
    let st = stop.clone();
    tokio::spawn(async move {
        let attach = move || {
            let p = parts.lock().unwrap().take().expect("attached once");
            async move { p }
        };
        let _ = client::run(remote(url, fp), key, st, attach).await;
    });
    let out = settle(&sent, Duration::from_millis(300), Duration::from_secs(10)).await;
    assert!(out > 0, "the handshake did not complete");
    assert!(in_rx.len() <= IN_QUEUE);
    assert!(
        out < FLOOD,
        "the master wrote all {FLOOD} commands to a node that read none"
    );
    stop.cancel();
}
