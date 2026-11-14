//! Re-check of the B.7 fixes (delta review, 2026-11-13). Not part of the
//! suite: `run_recheck.sh` copies this into `crates/node/tests` for one run
//! and removes the copy. The harness is the builder's
//! (`crates/node/tests/control_under_load.rs`) with a reader that can also
//! trickle, and that keeps each message's size and the watched call's
//! `tool_call_finished` text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

const OUT_QUEUE: usize = 256;
const TALKING: &str = "run: while :; do echo tick; date +%s%N > beat; sleep 966e-4; done";
const FLOOD: &str = "run: yes 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FLOODERS: usize = 12;

const FREE: usize = 0;
const STALLED: usize = 1;
const TRICKLE: usize = 2;

#[derive(Default)]
struct Seen {
    acks: HashMap<String, Instant>,
    /// Events and bytes of text per session.
    deltas: HashMap<SessionId, (usize, usize)>,
    ended: HashMap<SessionId, Instant>,
    finished: HashMap<SessionId, (Instant, String)>,
    /// Serialized size of every message taken since the last `mark`.
    sizes: Vec<usize>,
}

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    _node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    mode: Arc<AtomicUsize>,
    trickle_ms: Arc<AtomicU64>,
    queued: Arc<AtomicUsize>,
    floods: Vec<SessionId>,
}

impl Node {
    async fn new() -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Normal).unwrap();
        let node = NodeRuntime::start(NodeConfig {
            node_id: "local".into(),
            hostname: "test".into(),
            policy: NodePolicy {
                workspace_roots: vec![dir.path().into()],
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
        .unwrap();
        let link = node.attach().await;
        let seen = Arc::new(Mutex::new(Seen::default()));
        let mode = Arc::new(AtomicUsize::new(FREE));
        let trickle_ms = Arc::new(AtomicU64::new(500));
        let queued = Arc::new(AtomicUsize::new(0));
        let mut from_node = link.from_node;
        let (s, md, tr, q) = (seen.clone(), mode.clone(), trickle_ms.clone(), queued.clone());
        tokio::spawn(async move {
            loop {
                q.store(from_node.len(), Ordering::SeqCst);
                let m = match md.load(Ordering::SeqCst) {
                    STALLED => {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        continue;
                    }
                    TRICKLE => {
                        tokio::time::sleep(Duration::from_millis(tr.load(Ordering::SeqCst))).await;
                        if md.load(Ordering::SeqCst) == STALLED {
                            continue;
                        }
                        match from_node.try_recv() {
                            Ok(m) => Ok(Some(m)),
                            Err(_) => continue,
                        }
                    }
                    _ => tokio::time::timeout(Duration::from_millis(20), from_node.recv()).await,
                };
                let at = Instant::now();
                let mut s = s.lock().unwrap();
                if let Ok(Some(m)) = &m {
                    s.sizes.push(serde_json::to_string(m).map(|t| t.len()).unwrap_or(0));
                }
                match m {
                    Err(_) => {}
                    Ok(None) => return,
                    Ok(Some(NodeToMaster::Ack { req_id, .. })) => {
                        s.acks.insert(req_id, at);
                    }
                    Ok(Some(NodeToMaster::Event { event })) => {
                        let sid = event.session_id;
                        match event.typed() {
                            Ok(EventKind::ToolOutputDelta(d)) => {
                                let e = s.deltas.entry(sid).or_default();
                                e.0 += 1;
                                e.1 += d.text.len();
                            }
                            Ok(EventKind::ToolCallFinished(f)) => {
                                s.finished.insert(sid, (at, f.output));
                            }
                            Ok(EventKind::TurnEnded(_)) => {
                                s.ended.insert(sid, at);
                            }
                            _ => {}
                        }
                    }
                    Ok(Some(_)) => {}
                }
            }
        });
        Self {
            _dir: dir,
            ws,
            _node: node,
            to_node: link.to_node,
            seen,
            mode,
            trickle_ms,
            queued,
            floods: vec![],
        }
    }

    async fn wait(&self, what: &str, within: Duration, done: impl Fn(&Seen) -> bool) {
        let end = Instant::now() + within;
        while !done(&self.seen.lock().unwrap()) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn send(&self, m: MasterToNode) {
        self.to_node.try_send(m).unwrap();
    }

    async fn start(&self, name: &str, text: &str, ack: bool) -> (SessionId, PathBuf) {
        let cwd = self.ws.join(name);
        std::fs::create_dir(&cwd).unwrap();
        let sid = SessionId::new();
        let req = format!("create-{name}");
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: req.clone(),
            session_id: sid,
            title: name.into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
        });
        if ack {
            self.wait("a create's ack", Duration::from_secs(30), |s| {
                s.acks.contains_key(&req)
            })
            .await;
        }
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
            agent_named: false,
        });
        (sid, cwd)
    }

    fn cancel(&self, req: &str, sid: SessionId) {
        self.send(MasterToNode::Cancel {
            req_id: req.into(),
            session_id: sid,
        });
    }

    fn mark(&self) {
        self.seen.lock().unwrap().sizes.clear();
    }

    fn taken(&self) -> usize {
        self.seen.lock().unwrap().sizes.len()
    }

    async fn flood(&mut self) {
        for i in 0..FLOODERS {
            let (sid, _) = self.start(&format!("flood{i}"), FLOOD, false).await;
            self.floods.push(sid);
        }
    }

    /// Stall, flood, and wait until the out queue has been full for 1 s.
    async fn fill(&mut self) {
        self.mode.store(STALLED, Ordering::SeqCst);
        self.flood().await;
        let end = Instant::now() + Duration::from_secs(120);
        let mut since = None;
        loop {
            let q = self.queued.load(Ordering::SeqCst);
            match (q >= OUT_QUEUE, since) {
                (true, None) => since = Some(Instant::now()),
                (true, Some(t)) if t.elapsed() > Duration::from_secs(1) => return,
                (false, _) => since = None,
                _ => {}
            }
            assert!(Instant::now() < end, "the out queue never filled: {q}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn tidy(&self, also: Option<SessionId>) {
        self.mode.store(FREE, Ordering::SeqCst);
        for (i, sid) in self.floods.iter().enumerate() {
            self.cancel(&format!("tidy-{i}"), *sid);
        }
        let all: Vec<SessionId> = self.floods.iter().copied().chain(also).collect();
        self.wait("every turn to end", Duration::from_secs(120), |s| {
            all.iter().all(|sid| s.ended.contains_key(sid))
        })
        .await;
    }
}

/// Whether the command still writes `beat` (looks for up to 0.4 s).
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

/// The last time the command wrote `beat`, as the command's own clock
/// (`date +%s%N`) said, in ns since the epoch.
fn last_beat(dir: &Path) -> u128 {
    std::fs::read_to_string(dir.join("beat"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn now_ns() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

async fn running(node: &Node) -> (SessionId, PathBuf) {
    let (sid, dir) = node.start("watched", TALKING, true).await;
    node.wait("the watched session to stream", Duration::from_secs(30), |s| {
        s.deltas.get(&sid).is_some_and(|d| d.0 > 0)
    })
    .await;
    let end = Instant::now() + Duration::from_secs(30);
    while !beating(&dir).await {
        assert!(Instant::now() < end, "the watched command never ran");
    }
    (sid, dir)
}

/// Stop with the queue full, keep the uplink shut for `stall`, then read.
/// Returns whether the command still ran at the end of the stall, the
/// call's `tool_call_finished` text, and the bytes of the first 256
/// messages read afterwards (what the out queue held).
async fn stop_then_stall(stall: Duration) -> (bool, String, usize) {
    let mut node = Node::new().await;
    let (sid, dir) = running(&node).await;
    node.fill().await;
    assert!(beating(&dir).await, "the command stopped by itself");
    node.cancel("stop", sid);
    tokio::time::sleep(stall.saturating_sub(Duration::from_millis(400))).await;
    let still = beating(&dir).await;
    node.mark();
    node.mode.store(FREE, Ordering::SeqCst);
    node.wait("the call's end", Duration::from_secs(60), |s| {
        s.finished.contains_key(&sid)
    })
    .await;
    let (out, held) = {
        let s = node.seen.lock().unwrap();
        (
            s.finished[&sid].1.clone(),
            s.sizes.iter().take(OUT_QUEUE).sum::<usize>(),
        )
    };
    node.tidy(Some(sid)).await;
    (still, out, held)
}

fn tail(s: &str) -> String {
    let t: String = s.chars().rev().take(260).collect::<Vec<_>>().into_iter().rev().collect();
    t.replace('\n', " | ")
}

/// PLAN.md T9 (new residual): "the 5 s grace does not start, until the
/// uplink has room again". Does it start?
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n1_the_grace_runs_out_while_the_emit_waits() {
    for stall in [2u64, 7] {
        let (still, out, held) = stop_then_stall(Duration::from_secs(stall)).await;
        eprintln!(
            "recheck N1, uplink shut {stall} s after the Stop: command still running at the \
             end: {still}; the out queue's 256 messages were {held} bytes; \
             tool_call_finished ends: {}",
            tail(&out)
        );
    }
}

/// "Until the uplink has room again ... up to the link's 60 s idle limit":
/// how many messages must the uplink take before the stopped command ends?
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n2_a_trickling_uplink_holds_the_stop_for_many_frames() {
    for ms in [200u64, 1000] {
        let mut node = Node::new().await;
        let (sid, dir) = running(&node).await;
        node.fill().await;
        assert!(beating(&dir).await, "the command stopped by itself");
        node.mark();
        let stop_ns = now_ns();
        let t0 = Instant::now();
        node.cancel("stop", sid);
        node.trickle_ms.store(ms, Ordering::SeqCst);
        node.mode.store(TRICKLE, Ordering::SeqCst);
        let mut taken_then = 0;
        while beating(&dir).await {
            taken_then = node.taken();
            assert!(t0.elapsed() < Duration::from_secs(180), "never stopped");
        }
        let ran_on = Duration::from_nanos((last_beat(&dir).saturating_sub(stop_ns)) as u64);
        let ack = node.seen.lock().unwrap().acks.get("stop").map(|a| *a - t0);
        eprintln!(
            "recheck N2, uplink takes one message every {ms} ms: the command's last write was \
             {ran_on:?} after the Stop; the uplink had taken {taken_then} to {} messages by \
             then; the Stop's ack had arrived: {ack:?}",
            node.taken()
        );
        node.tidy(Some(sid)).await;
    }
}

/// "A tool's timeout ... take the same path (by reading; not run)": run it.
/// The default timeout of a foreground call is 60 s.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n3_a_timeout_does_not_end_a_streaming_command_on_a_full_queue() {
    let mut node = Node::new().await;
    let started = Instant::now();
    let (sid, dir) = running(&node).await;
    node.fill().await;
    let mut looks = vec![];
    for at in [55u64, 66, 75] {
        tokio::time::sleep(Duration::from_secs(at).saturating_sub(started.elapsed())).await;
        looks.push((at, beating(&dir).await));
    }
    let open_ns = now_ns();
    node.mode.store(FREE, Ordering::SeqCst);
    node.wait("the call's end", Duration::from_secs(60), |s| {
        s.finished.contains_key(&sid)
    })
    .await;
    let out = node.seen.lock().unwrap().finished[&sid].1.clone();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let last = last_beat(&dir);
    eprintln!(
        "recheck N3, no Stop, uplink shut: command still writing at (s after its prompt) \
         {looks:?}; its last write was {} ms before / {} ms after the uplink read again; \
         tool_call_finished ends: {}",
        open_ns.saturating_sub(last) / 1_000_000,
        last.saturating_sub(open_ns) / 1_000_000,
        tail(&out)
    );
    node.tidy(Some(sid)).await;
}

/// How many messages can the builder's twelve flooders produce at all (each
/// is cut at `tool_stream_bytes`, 1 MiB)? `fill` needs 256.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn n4_what_the_flooders_can_produce() {
    for stalled_first in [false, true] {
        let mut node = Node::new().await;
        if stalled_first {
            node.mode.store(STALLED, Ordering::SeqCst);
        }
        node.mark();
        node.flood().await;
        if stalled_first {
            tokio::time::sleep(Duration::from_secs(8)).await;
            node.mode.store(FREE, Ordering::SeqCst);
        }
        let total = |n: &Node| -> (usize, usize) {
            let s = n.seen.lock().unwrap();
            s.deltas.values().fold((0, 0), |a, d| (a.0 + d.0, a.1 + d.1))
        };
        let end = Instant::now() + Duration::from_secs(150);
        let (mut last, mut same) = ((0, 0), 0);
        while same < 8 && Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let t = total(&node);
            same = if t == last && t.0 > 0 { same + 1 } else { 0 };
            last = t;
        }
        let per: Vec<(usize, usize)> = {
            let s = node.seen.lock().unwrap();
            node.floods.iter().map(|f| s.deltas.get(f).copied().unwrap_or_default()).collect()
        };
        eprintln!(
            "recheck N4, uplink {}: 12 flooders made {} tool_output_delta events, {} bytes of \
             text ({} bytes an event), {} messages in all; per flooder (events, bytes): {per:?}",
            if stalled_first { "shut for the first 8 s" } else { "read throughout" },
            last.0,
            last.1,
            last.1.checked_div(last.0).unwrap_or(0),
            node.taken()
        );
        node.tidy(None).await;
    }
}
