//! What a person waits for after Stop when the node's uplink is slow and
//! full, end to end (ADOPTION_PLAN A.21; it rebuilds B.7's
//! `control_frames_from_the_node_arrive_within_the_grace`, whose node was a
//! model of the command loop).
//!
//! The real node (session actor, agent loop, `bash`, command loop) behind
//! the real link (TLS, WebSocket, the node's `client::run_with`, the
//! master's `server::bridge`) behind a TCP proxy that carries `RATE_KIB` a
//! second each way. Twelve other sessions run `yes` until the node's out
//! queue and the socket under it are full; then the master sends an
//! approval answer and a Stop for a session whose command prints a line
//! every 0.1 s. The master side reads as fast as it can: what is measured
//! is the node and the link, not the master's mirror.
//!
//! The owner's target: the Stop and the approval answer acknowledged within
//! the master's `ACK_TIMEOUT` (15 s), past which Stop answers 504.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, TOOL_CANCEL_GRACE, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::{ApprovalMode, Decision};
use harness_link::client::{self, Remote};
use harness_link::wire::{self, Hello, ServerMsg, ServerStream};
use harness_link::{MasterCert, NodeKey, Timing, server};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// The uplink: 256 KiB/s, where B.7 measured the cancel's ack at 26.2 s.
/// `A21_RATE_KIB` sets another rate for a measurement by hand.
const RATE_KIB: usize = 256;
/// `crates/master/src/lib.rs` `ACK_TIMEOUT` (private there).
const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// One line of output and one write of `beat` every 0.0961 s.
const TALKING: &str = "run: while :; do echo tick; date +%s%N > beat; sleep 961e-4; done";
const FLOOD: &str = "run: yes 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// Twelve, each cut at 1 MiB: enough to fill B.7's 6 MiB (the old queue
/// and this machine's loopback send buffer) and keep it full.
const FLOODERS: usize = 12;
/// The proxy's own receive buffer (fixed, so the kernel's autotuning on
/// loopback does not stand in for a router's queue).
const PROXY_BUF: u32 = 64 * 1024;

fn rate() -> usize {
    std::env::var("A21_RATE_KIB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(RATE_KIB)
        << 10
}

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

/// A token bucket with no saved-up burst.
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

#[derive(Default)]
struct Seen {
    hello: bool,
    acks: HashMap<String, Instant>,
    deltas: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
    /// Messages read since the start.
    read: usize,
}

struct Master {
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
}

impl Master {
    async fn wait(&self, what: &str, within: Duration, done: impl Fn(&Seen) -> bool) {
        let end = Instant::now() + within;
        while !done(&self.seen.lock().unwrap()) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// As the master does, but waiting a while for room (the master
    /// itself fails a command at once on a full queue).
    async fn send(&self, m: MasterToNode) {
        tokio::time::timeout(Duration::from_secs(30), self.to_node.send(m))
            .await
            .expect("the node read no command for 30 s")
            .unwrap();
    }

    async fn start(&self, ws: &Path, name: &str, text: &str) -> (SessionId, PathBuf) {
        let cwd = ws.join(name);
        std::fs::create_dir(&cwd).unwrap();
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: format!("create-{name}"),
            session_id: sid,
            title: name.into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        })
        .await;
        self.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("prompt-{name}"),
            session_id: sid,
            text: text.into(),
            images: vec![],
        })
        .await;
        (sid, cwd)
    }

    async fn cancel(&self, req: &str, sid: SessionId) {
        self.send(MasterToNode::Cancel {
            req_id: req.into(),
            session_id: sid,
        })
        .await;
    }
}

/// The master: the real handshake shape and bridge; a consumer that reads
/// as fast as it can. The `Master` comes once the node has connected.
async fn master(
    stop: &CancellationToken,
) -> (
    String,
    String,
    impl std::future::Future<Output = Master> + use<>,
) {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cert = MasterCert::load_or_create(dir.path()).unwrap();
    let tls = cert.server_config().unwrap();
    let fp = cert.fingerprint();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = l.local_addr().unwrap();
    let (acc_tx, mut accepted) = mpsc::unbounded_channel::<ServerStream>();
    tokio::spawn(async move {
        let _dir = dir;
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
    let seen = Arc::new(Mutex::new(Seen::default()));
    let (to_tx, to_rx) = tokio::sync::oneshot::channel();
    let (s, st) = (seen.clone(), stop.clone());
    tokio::spawn(async move {
        let srv = accepted.recv().await.unwrap();
        let (to_node, mut from_node) = server::bridge(srv, "n1".into(), st);
        let _ = to_tx.send(to_node);
        while let Some(m) = from_node.recv().await {
            let at = Instant::now();
            let mut s = s.lock().unwrap();
            s.read += 1;
            match m {
                NodeToMaster::Hello { .. } => s.hello = true,
                NodeToMaster::Ack { req_id, .. } => {
                    s.acks.insert(req_id, at);
                }
                NodeToMaster::Event { event } => match event.typed() {
                    Ok(EventKind::ToolOutputDelta(_)) => {
                        *s.deltas.entry(event.session_id).or_default() += 1;
                    }
                    Ok(EventKind::TurnEnded(_)) => {
                        s.ended.insert(event.session_id, at);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    });
    (url, fp, async move {
        Master {
            to_node: to_rx.await.unwrap(),
            seen,
        }
    })
}

async fn node(dir: &Path) -> NodeRuntime {
    let store = Store::open(&dir.join("node.db"), Durability::Normal).unwrap();
    NodeRuntime::start(NodeConfig {
        node_id: "n1".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store,
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
        artifacts: None,
    })
    .await
    .unwrap()
}

/// Whether the command still writes `beat` (looks for 0.4 s).
async fn beating(dir: &Path) -> bool {
    let read = || std::fs::read_to_string(dir.join("beat")).unwrap_or_default();
    let a = read();
    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let b = read();
        if !b.is_empty() && b != a {
            return true;
        }
    }
    false
}

/// When, after `from`, a look first saw no write (the start of that look).
async fn stopped_after(dir: &Path, from: Instant, within: Duration) -> Option<Duration> {
    while from.elapsed() < within {
        let at = from.elapsed();
        if !beating(dir).await {
            return Some(at);
        }
    }
    None
}

/// Until the uplink holds the sessions back: for two seconds running, all
/// of `sids` together appended something, but no more than about what the
/// uplink carries (twice its rate in 8 KiB events, plus small ones). A
/// node producing freely appends far more; one whose flooders reached
/// their stream budget appends nothing. So every place a byte can wait
/// on the way out (the node's queue, the kernel's buffers) is full.
async fn full(node: &NodeRuntime, sids: &[SessionId]) {
    let pace = (2 * rate() / 8192 + 8) as u64;
    let end = Instant::now() + Duration::from_secs(180);
    let mut last = 0;
    let mut held = 0;
    loop {
        let heads = node.store().call(|s| s.heads()).await.unwrap();
        let total: u64 = sids
            .iter()
            .map(|w| heads.iter().find(|(s, _)| s == w).map_or(0, |h| h.1))
            .sum();
        let grew = total - last;
        last = total;
        held = if grew > 0 && grew <= pace && total >= 64 {
            held + 1
        } else {
            0
        };
        if held >= 2 {
            return;
        }
        assert!(
            Instant::now() < end,
            "the uplink never held the sessions back: {grew} events in the last second, {total} in all"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_and_an_approval_answer_are_acknowledged_within_the_ack_timeout_on_a_slow_full_uplink()
 {
    let stop = CancellationToken::new();
    let (url, fp, connected) = master(&stop).await;
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ws = dir.path().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let rt = node(dir.path()).await;
    {
        let key = NodeKey::load_or_create(&dir.path().join("key")).unwrap();
        let remote = Remote {
            url,
            fingerprint: fp,
            node_id: "n1".into(),
        };
        let (rt, st) = (rt.clone(), stop.clone());
        tokio::spawn(async move {
            let attach = move || {
                let rt = rt.clone();
                async move {
                    let l = rt.attach().await;
                    (l.to_node, l.from_node)
                }
            };
            let _ = client::run_with(remote, key, st, Timing::default(), attach).await;
        });
    }
    let m = tokio::time::timeout(Duration::from_secs(30), connected)
        .await
        .expect("the node never connected");
    m.wait("the hello", Duration::from_secs(30), |s| s.hello)
        .await;
    eprintln!("A.21 over the link: connected");

    let (sid, beat) = m.start(&ws, "watched", TALKING).await;
    m.wait(
        "the watched session to stream",
        Duration::from_secs(30),
        |s| s.deltas.get(&sid).copied().unwrap_or(0) > 0,
    )
    .await;
    let mut floods = vec![];
    for i in 0..FLOODERS {
        floods.push(m.start(&ws, &format!("flood{i}"), FLOOD).await.0);
    }
    let filling = Instant::now();
    let all: Vec<SessionId> = floods.iter().copied().chain([sid]).collect();
    full(&rt, &all).await;
    let filled = filling.elapsed();
    eprintln!("A.21 over the link: held back {filled:?} after the flood started");
    assert!(beating(&beat).await, "the command stopped by itself");

    let (read0, t0) = (m.seen.lock().unwrap().read, Instant::now());
    m.send(MasterToNode::ApprovalRespond {
        req_id: "approval".into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
    })
    .await;
    m.cancel("stop", sid).await;
    let stopped = stopped_after(&beat, t0, TOOL_CANCEL_GRACE + Duration::from_secs(1)).await;
    m.wait("both acks", Duration::from_secs(120), |s| {
        s.acks.contains_key("stop") && s.acks.contains_key("approval")
    })
    .await;
    eprintln!("A.21 over the link: both acks in");
    let (ack, approval, ahead) = {
        let s = m.seen.lock().unwrap();
        (s.acks["stop"] - t0, s.acks["approval"] - t0, s.read - read0)
    };

    for (i, f) in floods.iter().enumerate() {
        m.cancel(&format!("tidy-{i}"), *f).await;
    }
    m.wait("every turn to end", Duration::from_secs(180), |s| {
        all.iter().all(|x| s.ended.contains_key(x))
    })
    .await;
    stop.cancel();
    eprintln!(
        "A.21 over the link at {} KiB/s (full {filled:?} after the flood started): no write seen \
         from {stopped:?} after the Stop; the Stop's ack after {ack:?}, the approval answer's \
         after {approval:?}; {ahead} messages read meanwhile",
        rate() >> 10
    );
    assert!(
        stopped.is_some_and(|d| d < Duration::from_secs(1)),
        "still writing: {stopped:?}"
    );
    assert!(ack < ACK_TIMEOUT, "the Stop's ack took {ack:?}");
    assert!(
        approval < ACK_TIMEOUT,
        "the approval answer's ack took {approval:?}"
    );
}
