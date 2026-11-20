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
//! the master's `ACK_TIMEOUT` (15 s), past which Stop answers 504. The test
//! also holds them to what the node's socket may hold ahead of them: at most
//! `ack_bound_bytes()` carried from the node between the Stop and each ack,
//! which fails without the socket's cap on unsent bytes (review A21-4). That
//! bound is counted in bytes, not seconds: the proxy's pace makes one the
//! other on an idle machine (3 s at 256 KiB/s), but under load the node's
//! threads take longer to write the ack while no more bytes go ahead of it
//! (3.86 s once, load average 11).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::{EventKind, Profile};
use harness_core::policy::{ApprovalMode, Decision};
use harness_link::client::{self, Remote};
use harness_link::wire::{self, Hello, ServerMsg, ServerStream};
use harness_link::{MasterCert, NodeKey, Timing, server};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};
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
/// `crates/master/src/lib.rs` `ACK_TIMEOUT` (private there): the owner's
/// target.
const ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// The proxy's read chunk (`pace`).
const PROXY_CHUNK: usize = 16 * 1024;
/// What the test holds the node to: at most this many bytes carried from
/// the node between the Stop and its ack. What can be ahead of the ack is
/// what the node's socket holds unsent (`harness_link::UNSENT_LIMIT`, the
/// product's cap, A21-4), what is in flight to the proxy (its receive
/// window, `PROXY_BUF`) and the chunk the proxy is pacing (`PROXY_CHUNK`):
/// 208 KiB today. Measured 216 to 301 KiB with the cap (the TLS and
/// WebSocket framing and what the node writes while it handles the Stop),
/// 2,658 to 2,694 KiB without it. The margin is one more `UNSENT_LIMIT`,
/// so the bound moves with the product's cap: 336 KiB today.
fn ack_bound_bytes() -> u64 {
    let unsent = u64::from(harness_link::UNSENT_LIMIT);
    unsent + u64::from(PROXY_BUF) + PROXY_CHUNK as u64 + unsent
}
/// One line of output and one write of `beat` every 0.0961 s.
fn talking() -> String {
    while_test_lives("echo tick; date +%s%N > beat; echo >> rounds; sleep 961e-4")
}
/// `run: ` a loop of `body` that runs while this test process lives and 15
/// minutes at most. The command is the test's child (in-process node, no
/// containment), so a SIGKILL of the test, which no drop guard survives,
/// reparents it and the loop ends within one round; before this, a loop
/// orphaned that way was found writing 22.5 hours later. A loop that writes
/// output already dies of SIGPIPE when the test does; a silent one did not.
fn while_test_lives(body: &str) -> String {
    format!(
        "run: echo $$ > pid; while read -r _ _ _ p _ </proc/$$/stat && [ \"$p\" = {} ] && [ $SECONDS -lt 900 ]; \
         do {body}; done",
        std::process::id()
    )
}

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

/// Forward bytes both ways at `rate` bytes a second each; while `hold` is
/// set, nothing goes from the node to the master (the uplink takes
/// nothing, as `control_under_load`'s stalled reader).
async fn throttled(target: SocketAddr, rate: usize, hold: Arc<AtomicBool>) -> String {
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
            tokio::spawn(pace(ri, wo, rate, Some(hold.clone())));
            tokio::spawn(pace(ro, wi, rate, None));
        }
    });
    url
}

/// Bytes the proxy has carried from the node to the master.
static UPLINK_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A token bucket with no saved-up burst; it reads nothing while `hold`
/// is set. What it carries from the node is counted in [`UPLINK_BYTES`].
async fn pace(
    mut r: OwnedReadHalf,
    mut w: OwnedWriteHalf,
    rate: usize,
    hold: Option<Arc<AtomicBool>>,
) {
    let mut buf = vec![0u8; PROXY_CHUNK];
    let mut next = Instant::now();
    loop {
        while hold.as_ref().is_some_and(|h| h.load(Ordering::SeqCst)) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let n = match r.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if w.write_all(&buf[..n]).await.is_err() {
            return;
        }
        if hold.is_some() {
            UPLINK_BYTES.fetch_add(n as u64, Ordering::SeqCst);
        }
        next = next.max(Instant::now()) + Duration::from_secs_f64(n as f64 / rate as f64);
        tokio::time::sleep_until(next).await;
    }
}

#[derive(Default)]
struct Seen {
    hello: bool,
    acks: HashMap<String, Instant>,
    /// [`UPLINK_BYTES`] when each ack was read.
    ack_bytes: HashMap<String, u64>,
    deltas: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
    /// The last tool result of each session.
    results: HashMap<SessionId, String>,
    /// Messages read since the start.
    read: usize,
    /// The master's end of the link closed.
    closed: bool,
}

struct Master {
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    /// Set: the proxy carries nothing from the node (see [`throttled`]).
    hold: Arc<AtomicBool>,
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
        self.send(MasterToNode::Prompt(PromptMsg {
            req_id: format!("prompt-{name}"),
            ..PromptMsg::test(sid, text)
        }))
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
    let hold = Arc::new(AtomicBool::new(false));
    let url = throttled(target, rate(), hold.clone()).await;
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
                    s.ack_bytes
                        .insert(req_id.clone(), UPLINK_BYTES.load(Ordering::SeqCst));
                    s.acks.insert(req_id, at);
                }
                NodeToMaster::Event { event } => match event.typed() {
                    Ok(EventKind::ToolOutputDelta(_)) => {
                        *s.deltas.entry(event.session_id).or_default() += 1;
                    }
                    Ok(EventKind::TurnEnded(_)) => {
                        s.ended.insert(event.session_id, at);
                    }
                    Ok(EventKind::ToolCallFinished(f)) => {
                        s.results.insert(event.session_id, f.output);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        s.lock().unwrap().closed = true;
    });
    (url, fp, async move {
        Master {
            to_node: to_rx.await.unwrap(),
            seen,
            hold,
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

/// Rounds the watched loop has run: it appends one line to `rounds` each.
fn rounds(dir: &Path) -> usize {
    std::fs::read(dir.join("rounds")).map_or(0, |b| b.iter().filter(|c| **c == b'\n').count())
}

/// The most rounds the watched loop may run between the Stop's arrival and its
/// shell's end. What it protects: a Stop ends the command at once, within a
/// fraction of a second (PLAN.md, "Flow control", the Stop's target: the bash
/// tool kills the command's group when it sees the cancel; the agent loop's
/// `TOOL_CANCEL_GRACE`, 5 s, is only the worst case, a tool that does not end
/// by itself). Counted in rounds of the loop (about 0.1 s each on an idle
/// machine), not in seconds: load slows the loop as it slows the node, so the
/// count does not grow with load, while a Stop that takes a second more on an
/// idle machine adds about ten. Measured (2026-11-17, every Stop test, 3 runs
/// idle and 20 at load 18 to 24): 0, twice 1; with the bash tool made to wait 4
/// s after its cancel, 41, and 22 for the loop that prints (its output waits
/// for the full queue).
const STOP_ROUNDS: usize = 5;

/// Whether the command in `dir` runs: the shell whose pid it wrote is alive
/// and not a zombie. (A 0.4 s look for a new `beat` stood for this until
/// 2026-11-16; on a loaded machine one round of the loop took longer.)
fn beating(dir: &Path) -> bool {
    let Some(pid) = std::fs::read_to_string(dir.join("pid"))
        .ok()
        .and_then(|p| p.trim().parse::<u32>().ok())
    else {
        return false;
    };
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit(')')
            .next()
            .and_then(|r| r.trim_start().chars().next())
            .is_some_and(|c| !matches!(c, 'Z' | 'X' | 'x'))
    })
}

/// How long after `from` the command was first seen gone, looking every
/// 10 ms until `within` has passed since `from`.
async fn stopped_after(dir: &Path, from: Instant, within: Duration) -> Option<Duration> {
    while from.elapsed() < within {
        if !beating(dir) {
            return Some(from.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    None
}

/// The node's out queue in bytes (`OUT_BYTES` in `crates/node/src/outbox.rs`,
/// private there).
const OUT_BYTES: usize = 1 << 20;

/// Until the node's out queue has been full (all but 64 KiB of its bytes
/// taken, a sender waiting for room) for two seconds: the link's channel
/// and the socket's buffer ahead of it are full too (review A21-7: read
/// from the node, not inferred from the store). Not "every session
/// waits": on a link that moves, a session released by the link is
/// briefly not waiting while it reads its next output.
async fn full(node: &NodeRuntime, m: &Master) {
    let end = Instant::now() + Duration::from_secs(180);
    let mut since: Option<Instant> = None;
    loop {
        let b = node.backlog();
        match (b.waiting > 0 && b.bytes + (64 << 10) >= OUT_BYTES, since) {
            (true, None) => since = Some(Instant::now()),
            (true, Some(t)) if t.elapsed() > Duration::from_secs(2) => return,
            (false, _) => since = None,
            _ => {}
        }
        if Instant::now() > end {
            let s = m.seen.lock().unwrap();
            panic!(
                "the uplink never filled: {b:?}; the master read {} messages, its link closed: {}",
                s.read, s.closed
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
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

    let (sid, beat) = m.start(&ws, "watched", &talking()).await;
    m.wait(
        "the watched session to stream",
        Duration::from_secs(30),
        |s| s.deltas.get(&sid).copied().unwrap_or(0) > 0,
    )
    .await;
    // Fill with the uplink held, then let it move at its rate again: the
    // flooders' 12 MiB fill the queue however slowly a loaded machine runs
    // them, where a moving uplink could drain them as fast as they write
    // (the old "the uplink never filled"). The Stop below goes out on the
    // same slow, full uplink either way.
    m.hold.store(true, Ordering::SeqCst);
    let mut floods = vec![];
    for i in 0..FLOODERS {
        floods.push(m.start(&ws, &format!("flood{i}"), FLOOD).await.0);
    }
    let filling = Instant::now();
    let all: Vec<SessionId> = floods.iter().copied().chain([sid]).collect();
    full(&rt, &m).await;
    let filled = filling.elapsed();
    eprintln!("A.21 over the link: held back {filled:?} after the flood started");
    assert!(beating(&beat), "the command stopped by itself");
    m.hold.store(false, Ordering::SeqCst);

    let (read0, t0) = (m.seen.lock().unwrap().read, Instant::now());
    let bytes0 = UPLINK_BYTES.load(Ordering::SeqCst);
    m.send(MasterToNode::ApprovalRespond {
        req_id: "approval".into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
        note: None,
    })
    .await;
    m.cancel("stop", sid).await;
    let r0 = rounds(&beat);
    // A limit on waiting, not a bound: the Stop's effect is checked below
    // by what the tool recorded. Far past a loaded machine: it costs nothing
    // when the node is prompt.
    let stopped = stopped_after(&beat, t0, Duration::from_secs(180)).await;
    let ran = rounds(&beat) - r0;
    m.wait("both acks", Duration::from_secs(120), |s| {
        s.acks.contains_key("stop") && s.acks.contains_key("approval")
    })
    .await;
    eprintln!("A.21 over the link: both acks in");
    let (ack, approval, ahead) = {
        let s = m.seen.lock().unwrap();
        (s.acks["stop"] - t0, s.acks["approval"] - t0, s.read - read0)
    };
    let (ack_bytes, approval_bytes) = {
        let s = m.seen.lock().unwrap();
        (
            s.ack_bytes["stop"] - bytes0,
            s.ack_bytes["approval"] - bytes0,
        )
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
        "A.21 over the link at {} KiB/s (full {filled:?} after the flood started): gone \
         {stopped:?} after the Stop; the Stop's ack after {ack:?} and {} KiB, the approval \
         answer's after {approval:?} and {} KiB; {ahead} messages read meanwhile",
        rate() >> 10,
        ack_bytes >> 10,
        approval_bytes >> 10
    );
    // Ended by the bash tool's own cancel, not abandoned at
    // `TOOL_CANCEL_GRACE` (it was "within 1 s", a clock of this machine).
    let result = m
        .seen
        .lock()
        .unwrap()
        .results
        .get(&sid)
        .cloned()
        .unwrap_or_default();
    assert!(stopped.is_some(), "still running 180 s after the Stop");
    eprintln!("A.21 over the link: {ran} rounds after the Stop");
    assert!(
        ran <= STOP_ROUNDS,
        "the loop ran {ran} rounds after the Stop ({stopped:?})"
    );
    assert!(
        result.contains("[cancelled") && !result.contains("abandoned"),
        "not ended by the Stop's cancel ({stopped:?}): {result}"
    );
    // The owner's target, in the master's own clock (its 504).
    assert!(ack < ACK_TIMEOUT, "the Stop's ack took {ack:?}");
    assert!(
        approval < ACK_TIMEOUT,
        "the approval answer's ack took {approval:?}"
    );
    // The socket's cap, in what went ahead (bytes: the rate does not
    // change what can be ahead of the ack).
    let bound = ack_bound_bytes();
    assert!(
        ack_bytes < bound,
        "{} KiB went ahead of the Stop's ack ({ack:?})",
        ack_bytes >> 10
    );
    assert!(
        approval_bytes < bound,
        "{} KiB went ahead of the approval answer's ack ({approval:?})",
        approval_bytes >> 10
    );
}
