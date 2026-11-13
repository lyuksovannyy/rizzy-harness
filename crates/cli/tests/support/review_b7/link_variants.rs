//! Review of B.7 (not part of the suite; `run_proofs.sh link` copies this
//! into `crates/link/tests/` for one run and removes it again).
//!
//! The builder's scenario from `crates/link/tests/control_under_load.rs`,
//! unchanged except for four knobs: which control frame is sent first, how
//! long after the uplink filled it is sent, the size of the queued frames,
//! and the rate. It prints; the assertions only pin the claims the review
//! makes from the numbers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::policy::Decision;
use harness_link::client::{self, Remote};
use harness_link::wire::{self, Hello, ServerMsg, ServerStream};
use harness_link::{MasterCert, NodeKey, Timing, server};
use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const OUT_QUEUE: usize = 256;
const IN_QUEUE: usize = 64;
const GRACE: Duration = Duration::from_secs(5);
const PROXY_BUF: u32 = 64 * 1024;

#[derive(Clone, Copy, Debug)]
struct Knobs {
    /// Bytes a second each way.
    rate: usize,
    /// After the out queue was first seen full (the builder: 500 ms).
    settle: Duration,
    /// The builder sends the cancel first.
    cancel_first: bool,
    /// Padding of a queued frame (the builder: 8192).
    frame: usize,
    /// Wait for the acks at the master too (the builder does).
    to_master: bool,
}

const BUILDER: Knobs = Knobs {
    rate: 1 << 20,
    settle: Duration::from_millis(500),
    cancel_first: true,
    frame: 8192,
    to_master: true,
};

async fn throttled(target: SocketAddr, rate: usize) -> String {
    let sock = TcpSocket::new_v4().unwrap();
    sock.set_recv_buffer_size(PROXY_BUF).unwrap();
    sock.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let l = sock.listen(16).unwrap();
    let url = format!("wss://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move {
        while let Ok((inbound, _)) = l.accept().await {
            let Ok(outbound) = TcpStream::connect(target).await else {
                continue;
            };
            let (ri, wi) = inbound.into_split();
            let (ro, wo) = outbound.into_split();
            tokio::spawn(pace(ri, wo, rate));
            tokio::spawn(pace(ro, wi, rate));
        }
    });
    url
}

async fn pace(mut r: OwnedReadHalf, mut w: OwnedWriteHalf, rate: usize) {
    let mut buf = vec![0u8; 16 * 1024];
    let mut next = Instant::now();
    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if w.write_all(&buf[..n]).await.is_err() {
            return;
        }
        next = next.max(Instant::now()) + Duration::from_secs_f64(n as f64 / rate as f64);
        tokio::time::sleep_until(next).await;
    }
}

fn filler(i: usize, frame: usize) -> NodeToMaster {
    NodeToMaster::Ack {
        req_id: format!("f{i}"),
        ok: false,
        error: Some("x".repeat(frame)),
    }
}

fn ack(req_id: String) -> NodeToMaster {
    NodeToMaster::Ack {
        req_id,
        ok: true,
        error: None,
    }
}

fn hello() -> NodeToMaster {
    NodeToMaster::Hello {
        protocol_version: harness_protocol::PROTOCOL_VERSION,
        node_id: "n1".into(),
        hostname: "h".into(),
        version: "v".into(),
        capabilities: vec![],
        sessions: vec![],
    }
}

#[derive(Debug)]
struct Measured {
    /// When the first control frame sent reached the node's command loop.
    first_at_node: Duration,
    /// When the second one did.
    second_at_node: Duration,
    /// The first one's ack at the master, and what was read ahead of it.
    first_ack_at_master: Option<Duration>,
    behind: usize,
    frame_bytes: usize,
}

async fn scenario(k: Knobs) -> Measured {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cert = MasterCert::load_or_create(dir.path()).unwrap();
    let tls = cert.server_config().unwrap();
    let fp = cert.fingerprint();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = l.local_addr().unwrap();
    let (acc_tx, mut accepted) = mpsc::unbounded_channel::<ServerStream>();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = l.accept().await {
            if let Ok(mut ws) = wire::accept(tcp, tls.clone()).await {
                let nonce = wire::new_nonce();
                wire::send_json(&mut ws, &ServerMsg::Challenge { nonce })
                    .await
                    .unwrap();
                let _: Option<Hello> = wire::recv_json(&mut ws).await.unwrap();
                let ok = ServerMsg::Accepted {
                    node_id: "n1".into(),
                };
                wire::send_json(&mut ws, &ok).await.unwrap();
                let _ = acc_tx.send(ws);
            }
        }
    });
    let url = throttled(target, k.rate).await;

    let (in_tx, mut in_rx) = mpsc::channel::<MasterToNode>(IN_QUEUE);
    let (out_tx, out_rx) = mpsc::channel::<NodeToMaster>(OUT_QUEUE);
    out_tx.try_send(hello()).unwrap();
    let parts = Arc::new(Mutex::new(Some((in_tx, out_rx))));
    let stop = CancellationToken::new();
    let key_dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let key = NodeKey::load_or_create(&key_dir.path().join("k")).unwrap();
    let remote = Remote {
        url,
        fingerprint: fp,
        node_id: "n1".into(),
    };
    let st = stop.clone();
    tokio::spawn(async move {
        let attach = move || {
            let p = parts.lock().unwrap().take().expect("attached once");
            async move { p }
        };
        let _ = client::run_with(remote, key, st, Timing::default(), attach).await;
    });

    let flooding = Arc::new(AtomicBool::new(true));
    {
        let (out, flooding) = (out_tx.clone(), flooding.clone());
        tokio::spawn(async move {
            let mut i = 0;
            while flooding.load(Ordering::SeqCst) {
                if out.send(filler(i, k.frame)).await.is_err() {
                    return;
                }
                i += 1;
            }
        });
    }

    // The node's command loop, as the builder modelled it.
    let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<(String, Instant)>();
    {
        let (out, seen) = (out_tx.clone(), seen_tx.clone());
        tokio::spawn(async move {
            while let Some(m) = in_rx.recv().await {
                let at = Instant::now();
                let req_id = match m {
                    MasterToNode::Cancel { req_id, .. }
                    | MasterToNode::ApprovalRespond { req_id, .. } => req_id,
                    _ => continue,
                };
                let _ = seen.send((format!("{req_id} at node"), at));
                let _ = out.send(ack(req_id)).await;
            }
        });
    }

    let srv = accepted.recv().await.unwrap();
    let (to_node, mut from_node) = server::bridge(srv, "n1".into(), stop.clone());
    let fillers = Arc::new(AtomicUsize::new(0));
    let at_first_ack = Arc::new(AtomicUsize::new(0));
    {
        let (fillers, seen) = (fillers.clone(), seen_tx.clone());
        let at_first_ack = at_first_ack.clone();
        tokio::spawn(async move {
            while let Some(m) = from_node.recv().await {
                if let NodeToMaster::Ack { req_id, .. } = m {
                    let at = Instant::now();
                    match req_id.as_str() {
                        "first" => {
                            at_first_ack.store(fillers.load(Ordering::SeqCst), Ordering::SeqCst);
                            let _ = seen.send(("first ack at master".into(), at));
                        }
                        "second" => {}
                        _ => {
                            fillers.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
        });
    }

    let end = Instant::now() + Duration::from_secs(120);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if fillers.load(Ordering::SeqCst) > 0 && out_tx.capacity() == 0 {
            break;
        }
        assert!(Instant::now() < end, "the uplink never filled");
    }
    tokio::time::sleep(k.settle).await;

    let cancel = |req: &str| MasterToNode::Cancel {
        req_id: req.into(),
        session_id: SessionId::new(),
    };
    let approval = |req: &str| MasterToNode::ApprovalRespond {
        req_id: req.into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
    };
    let t0 = Instant::now();
    let before = fillers.load(Ordering::SeqCst);
    let (first, second) = if k.cancel_first {
        (cancel("first"), approval("second"))
    } else {
        (approval("first"), cancel("second"))
    };
    to_node.send(first).await.unwrap();
    to_node.send(second).await.unwrap();

    let want = if k.to_master { 3 } else { 2 };
    let mut got = HashMap::new();
    while got.len() < want {
        let (what, at) = tokio::time::timeout(Duration::from_secs(300), seen_rx.recv())
            .await
            .expect("a control frame never arrived")
            .unwrap();
        got.insert(what, at.saturating_duration_since(t0));
    }
    flooding.store(false, Ordering::SeqCst);
    stop.cancel();
    Measured {
        first_at_node: got["first at node"],
        second_at_node: got["second at node"],
        first_ack_at_master: got.get("first ack at master").copied(),
        behind: at_first_ack.load(Ordering::SeqCst).saturating_sub(before),
        frame_bytes: serde_json::to_string(&filler(0, k.frame)).unwrap().len(),
    }
}

/// The builder's order the other way round: an approval answer, then the
/// cancel. The cancel now waits on the node as the approval answer did in
/// the builder's run, so "a cancel acts on the node at once" holds only for
/// a cancel that is the first command the loop sees.
#[tokio::test]
async fn a_cancel_sent_after_another_answer_waits_on_the_node() {
    let m = scenario(Knobs {
        cancel_first: false,
        to_master: false,
        ..BUILDER
    })
    .await;
    eprintln!(
        "B.7 review, approval answer then cancel at 1024 KiB/s: approval answer at node {:?}, \
         CANCEL at node {:?}",
        m.first_at_node, m.second_at_node
    );
    assert!(m.first_at_node < Duration::from_millis(100), "{m:?}");
    assert!(m.second_at_node > Duration::from_millis(300), "{m:?}");
}

/// The wait of the second command on the node is the time to the uplink's
/// next writable moment, so it depends on when the frames are sent. The
/// builder's 1.11 s and 4.99 s are one fixed moment (0.5 s after the queue
/// first filled).
#[tokio::test]
async fn the_wait_on_the_node_depends_on_the_moment() {
    for rate_kib in [1024usize, 256] {
        let mut worst = Duration::ZERO;
        let step = if rate_kib == 1024 { 400 } else { 1300 };
        for i in 0..5u64 {
            let settle = Duration::from_millis(i * step);
            let m = scenario(Knobs {
                rate: rate_kib << 10,
                settle,
                to_master: false,
                ..BUILDER
            })
            .await;
            eprintln!(
                "B.7 review, {rate_kib} KiB/s, sent {settle:?} after the queue filled: \
                 cancel at node {:?}, approval answer at node {:?}",
                m.first_at_node, m.second_at_node
            );
            worst = worst.max(m.second_at_node);
        }
        eprintln!(
            "B.7 review, {rate_kib} KiB/s: longest wait of the second command {worst:?} \
             (the grace is {GRACE:?})"
        );
    }
}

/// The out queue is bounded in messages, not bytes. With 64 KiB frames (a
/// tool result, a snapshot batch of a replay is up to 2 MiB) the same 256
/// messages are 16 MiB ahead of a control frame.
#[tokio::test]
async fn the_bytes_ahead_grow_with_the_frame_size() {
    let m = scenario(Knobs {
        frame: 64 * 1024,
        ..BUILDER
    })
    .await;
    let bytes = m.behind * m.frame_bytes;
    eprintln!(
        "B.7 review, 64 KiB frames at 1024 KiB/s: cancel's ack at master {:?}, {} frames \
         ({:.1} MiB) ahead of it",
        m.first_ack_at_master,
        m.behind,
        bytes as f64 / (1 << 20) as f64
    );
    assert!(m.behind >= OUT_QUEUE - 2, "{m:?}");
    assert!(bytes > 16 << 20, "{m:?}");
}

/// At 256 KiB/s the builder's passing test measured 4.99 s against its 5 s
/// limit, sending 0.5 s after the queue filled. Sent 0.15 s after it, the
/// review expected a wait past the grace and did NOT get one: 41 ms (the
/// socket was still taking bytes). The longest wait the review saw at this
/// rate is 4.19 s (`the_wait_on_the_node_depends_on_the_moment`).
#[tokio::test]
async fn at_256_kib_sent_150_ms_after_the_fill() {
    let m = scenario(Knobs {
        rate: 256 << 10,
        settle: Duration::from_millis(150),
        to_master: false,
        ..BUILDER
    })
    .await;
    eprintln!(
        "B.7 review, 256 KiB/s, sent 150 ms after the queue filled: cancel at node {:?}, \
         approval answer at node {:?} (the grace is {GRACE:?})",
        m.first_at_node, m.second_at_node
    );
}
