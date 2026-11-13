//! Experiment B.7 (ADOPTION_PLAN, final review D17): control frames while
//! a node streams at the link's cap.
//!
//! The link has one queue per direction and no lane for control frames
//! (`src/pump.rs`). These tests put the real link (TLS, WebSocket, the
//! master's `server::bridge`, the node's `client::run_with`) behind a TCP
//! proxy that carries a fixed number of bytes a second, which is "the link's
//! cap": a node whose uplink is slower than what it produces. The node side
//! is a model of `NodeRuntime::attach` (`crates/node/src/lib.rs`) with its
//! real queue depths: events go into an out queue of `OUT_QUEUE`, commands
//! come out of an in queue of `IN_QUEUE`, and the command loop takes a
//! cancel or an approval answer and then waits for room in the out queue to
//! send its ack (`publish_wait`), as the node does. It does not run a
//! session: what a Stop does to a command is measured on the real node in
//! `crates/node/tests/control_under_load.rs`. The master side
//! reads as fast as it can (its mirror is opened `Durability::Normal`, so an
//! append does not fsync).
//!
//! While another session keeps the uplink full, the master sends a cancel
//! and right after it an approval answer, and the test records when each
//! reaches the node's command loop (master to node) and when each one's ack
//! reaches the master's consumer (node to master).
//!
//! Measured on 2026-11-13 (loopback, the proxy at 1 MiB/s, 8 KiB frames,
//! one `bash` pipe read; Linux 7.2, the owner's machine; see
//! `ADOPTION_PLAN.md`, results of Phase B):
//!
//! | Frame | Direction | At 1 MiB/s | At 256 KiB/s |
//! |---|---|---|---|
//! | cancel, at the node's command loop | master to node | 0.2 ms | 0.4 ms |
//! | approval answer, at the node's command loop | master to node | 1.11 s | 4.99 s |
//! | cancel's ack, at the master | node to master | 6.6 s | 26.2 s |
//! | approval answer's ack, at the master | node to master | 7.2 s | 28.2 s |
//!
//! Ahead of the cancel's ack the master read 781 frames of 8,242 bytes
//! (6.1 MiB) at 1 MiB/s, 816 (6.4 MiB) at 256 KiB/s. Two runs at each rate
//! agreed to within 30 ms (`B7_RATE_KIB=256 cargo test -p harness-link
//! --test control_under_load -- --include-ignored --nocapture`).
//!
//! What these numbers are (review B7-R1 to B7-R6, 2026-11-13):
//!
//! - "At the node's command loop" is when this test's model of the loop
//!   received the frame, not when the node acted on it. On the real node a
//!   Stop does not end a command that streams output while the out queue
//!   is full (`crates/node/tests/control_under_load.rs`).
//! - The cancel is sent first, so it gets the idle loop. Whichever of the
//!   commands the loop answers inline (cancel, approval answer,
//!   `stop_background`) comes second waits for the first one's ack to get
//!   room: sent the other way round, the cancel reached the loop after
//!   1.12 s. While it waits, the loop reads no command of any session.
//! - That wait is the time to the socket's next writable moment, so it
//!   depends on when the frames are sent: 41 ms to 1.21 s at 1 MiB/s and
//!   41 ms to 4.99 s at 256 KiB/s in the review's runs; 1.11 s and 4.99 s
//!   above are the moment this test picks (0.5 s after the queue filled).
//!   It grows with the kernel's send buffer, so it bounds the model and
//!   this machine, not the node's code.
//! - The out queue is bounded in messages (`OUT_QUEUE`, 256), not bytes.
//!   With 8 KiB frames its share was 2 MiB; with 64 KiB frames the cancel's
//!   ack waited 20.2 s behind 18.8 MiB at 1 MiB/s. On the real node,
//!   `yes` run through `bash` made events of 8,134 bytes of text on average
//!   (1,547 events, re-check B7-D3), and a full out queue of them was
//!   1.4 MiB; but an event may be 4 MiB and a replay's snapshot batch
//!   2 MiB, so after a reconnect up to 256 x 2 MiB (by reading) can be
//!   ahead of an ack.
//! - Of the 6.1 MiB ahead, 4.1 MiB (783 - 256 frames) is the node socket's
//!   send buffer, autotuned on loopback to this machine's `tcp_wmem`
//!   maximum (4 MiB) behind the proxy; a real path at these rates need not
//!   grow it that far. So 6.6 s and 26.2 s are loopback numbers; the
//!   node's own 256 frames of 8 KiB alone would be about 2 s at 1 MiB/s.
//! - The flood is a tight loop of in-memory sends. A real node writes each
//!   event to node.db with `Durability::Full` before it publishes it; on
//!   the slow disk of this checkout it produced about 2.5 messages a second
//!   (one rough count), which no uplink faster than about 20 KiB/s backs
//!   up. On a disk with a 1 to 5 ms fsync it fills the queue. The real
//!   node's production rate on a fast disk was not measured.
//!
//! Master to node, the link holds nothing back: the direction is idle. But
//! the node's command loop waits for room in the out queue to ack, so the
//! next inline command waits for the uplink. Node to master, a control
//! frame waits behind everything already queued: `OUT_QUEUE` messages of
//! whatever size, plus what the kernel holds, so its delay is those bytes
//! divided by the uplink's rate. The control in both tests is the same
//! run's own other direction.
//!
//! The model above is B.7's node, before A.21. The node-to-master half is
//! now measured on the real node, its command loop and its lane for those
//! acks, over this same link and proxy:
//! `crates/node/tests/control_over_link.rs` (A.21; it replaces this file's
//! `control_frames_from_the_node_arrive_within_the_grace`). What is left
//! here measures the link's master-to-node direction only.

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

/// `crates/node/src/lib.rs` `OUT_QUEUE` (private there).
const OUT_QUEUE: usize = 256;
/// `crates/node/src/lib.rs` `IN_QUEUE` (private there).
const IN_QUEUE: usize = 64;
/// One `read` of a command's pipe (`crates/tools/src/bash.rs`,
/// `chunk = [0u8; 8192]`). The agent loop coalesces reads into events of
/// `FLUSH_BYTES` (2 KiB) or more, so real events vary (review B7-R3).
const DELTA: usize = 8192;
/// The uplink: 1 MiB/s, about a 10 Mbit/s home or office uplink.
/// `B7_RATE_KIB` sets another rate for a measurement by hand.
const RATE: usize = 1 << 20;

fn rate() -> usize {
    std::env::var("B7_RATE_KIB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(RATE, |k| k << 10)
}
/// The proxy's own receive buffer (fixed, so the kernel's autotuning on
/// loopback does not stand in for a router's queue).
const PROXY_BUF: u32 = 64 * 1024;

/// Forward bytes both ways at `rate` bytes a second each.
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

/// A token bucket with no saved-up burst: an idle direction may send its
/// next chunk at once, a busy one gets `rate`.
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

/// A filler event of about `DELTA` bytes on the wire (an `ack` with a
/// padded error, as in `backpressure.rs`: the link only sees bytes).
fn filler(i: usize) -> NodeToMaster {
    NodeToMaster::Ack {
        req_id: format!("f{i}"),
        ok: false,
        error: Some("x".repeat(DELTA)),
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

/// What one run measured. Times are from the moment the master queued the
/// frame; "behind" is the fillers the master read between sending the
/// cancel and reading its ack.
#[derive(Debug)]
struct Measured {
    cancel_at_node: Duration,
    /// When the command loop got room in the out queue for the cancel's ack.
    cancel_ack_queued: Duration,
    approval_at_node: Duration,
    cancel_ack_at_master: Duration,
    approval_ack_at_master: Duration,
    behind: usize,
    frame_bytes: usize,
}

impl Measured {
    fn bytes_behind(&self) -> usize {
        self.behind * self.frame_bytes
    }

    fn print(&self) {
        eprintln!(
            "B.7 at {} KiB/s: cancel at node {:?}, its ack queued {:?}, approval at node {:?}; \
             cancel ack at master {:?}, approval ack at master {:?}; \
             {} frames ({:.2} MiB) ahead of the cancel's ack",
            rate() >> 10,
            self.cancel_at_node,
            self.cancel_ack_queued,
            self.approval_at_node,
            self.cancel_ack_at_master,
            self.approval_ack_at_master,
            self.behind,
            self.bytes_behind() as f64 / (1 << 20) as f64,
        );
    }
}

/// One run: saturate the uplink, then send a cancel and an approval answer.
async fn scenario() -> Measured {
    // The master: the real handshake shape, then the real bridge.
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
    let url = throttled(target, rate()).await;

    // The node: the real client loop over the throttled proxy.
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

    // Another session streams for the whole run, as fast as it may.
    let flooding = Arc::new(AtomicBool::new(true));
    {
        let (out, flooding) = (out_tx.clone(), flooding.clone());
        tokio::spawn(async move {
            let mut i = 0;
            while flooding.load(Ordering::SeqCst) {
                if out.send(filler(i)).await.is_err() {
                    return;
                }
                i += 1;
            }
        });
    }

    // The node's command loop: act at once, then wait for room to ack.
    let (seen_tx, mut seen_rx) = mpsc::unbounded_channel::<(&'static str, Instant)>();
    {
        let (out, seen) = (out_tx.clone(), seen_tx.clone());
        tokio::spawn(async move {
            while let Some(m) = in_rx.recv().await {
                let at = Instant::now();
                let (what, req_id) = match m {
                    MasterToNode::Cancel { req_id, .. } => ("cancel at node", req_id),
                    MasterToNode::ApprovalRespond { req_id, .. } => ("approval at node", req_id),
                    _ => continue,
                };
                let _ = seen.send((what, at));
                let _ = out.send(ack(req_id)).await;
                if what == "cancel at node" {
                    let _ = seen.send(("cancel ack queued", Instant::now()));
                }
            }
        });
    }

    // The master's consumer, as fast as it can read.
    let srv = accepted.recv().await.unwrap();
    let (to_node, mut from_node) = server::bridge(srv, "n1".into(), stop.clone());
    let fillers = Arc::new(AtomicUsize::new(0));
    // Fillers read when the cancel's ack was read.
    let at_cancel_ack = Arc::new(AtomicUsize::new(0));
    {
        let (fillers, seen) = (fillers.clone(), seen_tx.clone());
        let at_cancel_ack = at_cancel_ack.clone();
        tokio::spawn(async move {
            while let Some(m) = from_node.recv().await {
                if let NodeToMaster::Ack { req_id, .. } = m {
                    let at = Instant::now();
                    match req_id.as_str() {
                        "cancel" => {
                            at_cancel_ack.store(fillers.load(Ordering::SeqCst), Ordering::SeqCst);
                            let _ = seen.send(("cancel ack at master", at));
                        }
                        "approval" => {
                            let _ = seen.send(("approval ack at master", at));
                        }
                        _ => {
                            fillers.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            }
        });
    }

    // Saturated: the master has read some of the stream and the node's out
    // queue is full, so the socket under it is full too.
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        if fillers.load(Ordering::SeqCst) > 0 && out_tx.capacity() == 0 {
            break;
        }
        assert!(Instant::now() < end, "the uplink never filled");
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(out_tx.capacity(), 0, "the uplink did not stay full");

    let sid = SessionId::new();
    let t0 = Instant::now();
    let before = fillers.load(Ordering::SeqCst);
    to_node
        .send(MasterToNode::Cancel {
            req_id: "cancel".into(),
            session_id: sid,
        })
        .await
        .unwrap();
    to_node
        .send(MasterToNode::ApprovalRespond {
            req_id: "approval".into(),
            session_id: SessionId::new(),
            approval_id: "a1".into(),
            decision: Decision::Allow,
        })
        .await
        .unwrap();

    let mut got = std::collections::HashMap::new();
    while got.len() < 5 {
        let (what, at) = tokio::time::timeout(Duration::from_secs(120), seen_rx.recv())
            .await
            .expect("a control frame never arrived")
            .unwrap();
        got.insert(what, at.saturating_duration_since(t0));
    }
    flooding.store(false, Ordering::SeqCst);
    stop.cancel();
    let frame_bytes = serde_json::to_string(&filler(0)).unwrap().len();
    Measured {
        cancel_at_node: got["cancel at node"],
        cancel_ack_queued: got["cancel ack queued"],
        approval_at_node: got["approval at node"],
        cancel_ack_at_master: got["cancel ack at master"],
        approval_ack_at_master: got["approval ack at master"],
        behind: at_cancel_ack.load(Ordering::SeqCst) - before,
        frame_bytes,
    }
}

/// Master to node: the first control frame the master sends reaches the
/// node's command loop (this test's model of it) within a second while the
/// uplink is full: the link holds nothing back in that direction. The
/// control: in the same run the cancel's own ack, going the full direction,
/// had at least the node's whole out queue ahead of it.
///
/// It does not say the node acts on it (on the real node a streaming
/// command is not stopped, `crates/node/tests/control_under_load.rs`), nor
/// anything of the second frame: the approval answer sent right after it
/// reached the model's loop after 1.11 s, when the cancel's ack got room in
/// the out queue. That wait is made by the model's own `send(ack).await`
/// and the kernel, so it is printed, not asserted.
///
/// A measurement, not a guard (re-check, weak tests): the pump reads while
/// its writes are blocked by construction (`crates/link/src/pump.rs`), and
/// no change in `crates/node` can turn this red or green, so it does not
/// spend 9 s in every run of the crate. Run it with `--include-ignored`.
#[tokio::test]
#[ignore = "B.7 measurement: the master-to-node direction under a full uplink (9 s)"]
async fn the_first_control_frame_reaches_the_node_while_its_uplink_is_full() {
    let m = scenario().await;
    m.print();
    assert!(m.behind >= OUT_QUEUE - 2, "the uplink was not full: {m:?}");
    assert!(m.cancel_at_node < Duration::from_secs(1), "{m:?}");
}
