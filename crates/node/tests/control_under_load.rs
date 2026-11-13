//! Experiment B.7 on the real node (review B7-R1, B7-R2): does a Stop end a
//! command while the node's out queue is full?
//!
//! `crates/link/tests/control_under_load.rs` measures when a control frame
//! reaches a model of the node's command loop. This drives the real
//! `NodeRuntime` (session actor, agent loop, `bash` tool, default limits)
//! over its in-process link, and asks whether the command has ended. The
//! "uplink" is the test's reader of `from_node`: it reads as fast as it can,
//! or it takes nothing (a link between two bursts, or a dead one before the
//! link's `IDLE_LIMIT`, 60 s, closes it). Twelve other sessions run `yes`
//! (each cut at the default `tool_stream_bytes`, 1 MiB) to fill the out
//! queue (`OUT_QUEUE`, 256 messages) while the uplink takes nothing.
//!
//! node.db is opened `Durability::Normal`, not `Full` as `node run` opens
//! it: on the slow disk this was measured on, an fsync per event fills the
//! out queue at about 2.5 messages a second and the run outlasts the
//! commands' timeouts. What the tests show does not depend on it, only on
//! the out queue being full; how fast a real node fills it does (review
//! B7-R4).
//!
//! Measured on 2026-11-13 (the owner's machine, Linux 7.2, debug build):
//!
//! - A command that writes output, Stop sent while the uplink takes
//!   nothing: still running 6.0 s after the Stop (the whole look), past the
//!   agent loop's 5 s `TOOL_CANCEL_GRACE`. The agent loop's `coalesce` awaits the next
//!   `tool_output_delta`'s append inside a `select!` arm, the append waits in
//!   `publish_wait` for room in the out queue, and while it waits neither the
//!   tool future nor the cancel is polled. The command ends only once the
//!   uplink takes bytes again: within 1 µs of the first read, the Stop's
//!   ack 6.03 s and the turn's end 6.03 s after the Stop. (The review's own
//!   run: 6.8 s with a 6.8 s stall; with the uplink read as fast as it can,
//!   the command stopped 11 µs after the Stop.)
//! - A command that writes nothing, Stop sent first: ended 14 µs after the
//!   Stop, its ack and the turn's end waiting for the uplink
//!   (`a_stop_ends_a_silent_command_while_the_uplink_takes_nothing`, the
//!   control; it passes).
//! - The same silent command, Stop sent right after an approval answer:
//!   still running 6.2 s after the Stop; it stopped within 1 µs of the
//!   uplink reading again, its ack 6.18 s and the turn's end 13.0 s after
//!   the Stop. The command loop answers both inline and waits in
//!   `publish_wait` to ack the first, so it does not read the Stop, nor any
//!   other command of any session, until the uplink moves.
//!
//! The two failing tests are `#[ignore]` with these results; they pass once
//! a Stop is acted on whatever the out queue holds (ADOPTION_PLAN B.7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{NativeBackend, NativeConfig, TOOL_CANCEL_GRACE, mock::Mock};
use harness_core::SessionId;
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

/// `crates/node/src/lib.rs` `OUT_QUEUE` (private there).
const OUT_QUEUE: usize = 256;
/// One line of output and one write of `beat` every 0.0961 s.
const TALKING: &str = "run: while :; do echo tick; date +%s%N > beat; sleep 961e-4; done";
/// No output, one write of `beat` every 0.0961 s.
const SILENT: &str = "run: while :; do date +%s%N > beat; sleep 961e-4; done";
const FLOOD: &str = "run: yes 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FLOODERS: usize = 12;
/// How long after a Stop the command may still run: the agent loop's grace
/// for a cancelled tool, and a second for the kill and this test's look.
fn bound() -> Duration {
    TOOL_CANCEL_GRACE + Duration::from_secs(1)
}

#[derive(Default)]
struct Seen {
    acks: HashMap<String, Instant>,
    deltas: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
}

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    _node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    /// The uplink takes nothing while set.
    stalled: Arc<AtomicBool>,
    /// Messages in the out queue at the reader's last look.
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
        let stalled = Arc::new(AtomicBool::new(false));
        let queued = Arc::new(AtomicUsize::new(0));
        let mut from_node = link.from_node;
        let (s, st, q) = (seen.clone(), stalled.clone(), queued.clone());
        tokio::spawn(async move {
            loop {
                q.store(from_node.len(), Ordering::SeqCst);
                if st.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    continue;
                }
                let m = tokio::time::timeout(Duration::from_millis(20), from_node.recv()).await;
                let at = Instant::now();
                let mut s = s.lock().unwrap();
                match m {
                    Err(_) => {}
                    Ok(None) => return,
                    Ok(Some(NodeToMaster::Ack { req_id, .. })) => {
                        s.acks.insert(req_id, at);
                    }
                    Ok(Some(NodeToMaster::Event { event })) => {
                        let sid = event.session_id;
                        match event.typed() {
                            Ok(EventKind::ToolOutputDelta(_)) => {
                                *s.deltas.entry(sid).or_default() += 1;
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
            stalled,
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

    /// A session in a folder of its own whose first prompt is `text`.
    /// `ack`: wait for the create's ack (needs a reading uplink).
    async fn start(&self, name: &str, text: &str, ack: bool) -> (SessionId, PathBuf) {
        let cwd = self.ws.join(name);
        std::fs::create_dir(&cwd).unwrap();
        let sid = SessionId::new();
        let req = format!("create-{name}");
        self.send(MasterToNode::Create {
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
        });
        (sid, cwd)
    }

    fn cancel(&self, req: &str, sid: SessionId) {
        self.send(MasterToNode::Cancel {
            req_id: req.into(),
            session_id: sid,
        });
    }

    /// Stall the uplink, start the flooders, and wait until the out queue
    /// has been full for a second: every streaming session then waits for
    /// room.
    async fn fill(&mut self) {
        self.stalled.store(true, Ordering::SeqCst);
        for i in 0..FLOODERS {
            let (sid, _) = self.start(&format!("flood{i}"), FLOOD, false).await;
            self.floods.push(sid);
        }
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

    /// Read again, stop the flooders and wait for every turn to end.
    async fn tidy(&self, also: SessionId) {
        self.stalled.store(false, Ordering::SeqCst);
        for (i, sid) in self.floods.iter().enumerate() {
            self.cancel(&format!("tidy-{i}"), *sid);
        }
        let all: Vec<SessionId> = self.floods.iter().copied().chain([also]).collect();
        self.wait("every turn to end", Duration::from_secs(120), |s| {
            all.iter().all(|sid| s.ended.contains_key(sid))
        })
        .await;
    }
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

/// How long after `from` the command stopped writing, looking until
/// `within` has passed since `from`.
async fn stopped_after(dir: &Path, from: Instant, within: Duration) -> Option<Duration> {
    while from.elapsed() < within {
        let at = from.elapsed();
        if !beating(dir).await {
            return Some(at);
        }
    }
    None
}

/// A session running `cmd`, seen writing `beat` (and, if it talks, seen
/// streaming) while the uplink still reads.
async fn running(node: &Node, cmd: &str) -> (SessionId, PathBuf) {
    let (sid, dir) = node.start("watched", cmd, true).await;
    if cmd == TALKING {
        node.wait(
            "the watched session to stream",
            Duration::from_secs(30),
            |s| s.deltas.get(&sid).copied().unwrap_or(0) > 0,
        )
        .await;
    }
    let end = Instant::now() + Duration::from_secs(30);
    while !beating(&dir).await {
        assert!(Instant::now() < end, "the watched command never ran");
    }
    (sid, dir)
}

/// What one run measured after the Stop.
#[derive(Debug)]
struct Stop {
    /// When the command stopped writing, while the uplink took nothing.
    stalled: Option<Duration>,
    /// How long the uplink took nothing after the Stop.
    stall: Duration,
    /// When the command stopped writing after the uplink read again.
    after_read: Option<Duration>,
    ack: Duration,
    turn_ended: Duration,
}

/// Fill the out queue, send `before` if any and then a Stop for the watched
/// session running `cmd`, and look for `bound()`.
async fn stop_while_full(cmd: &str, before: Option<MasterToNode>) -> Stop {
    let mut node = Node::new().await;
    let (sid, dir) = running(&node, cmd).await;
    node.fill().await;
    assert!(beating(&dir).await, "the command stopped by itself");
    let t0 = Instant::now();
    if let Some(m) = before {
        node.send(m);
    }
    node.cancel("stop", sid);
    let stalled = stopped_after(&dir, t0, bound()).await;
    let stall = t0.elapsed();
    node.stalled.store(false, Ordering::SeqCst);
    let t1 = Instant::now();
    let after_read = match stalled {
        Some(_) => None,
        None => stopped_after(&dir, t1, Duration::from_secs(60)).await,
    };
    node.wait("the stop's ack", Duration::from_secs(60), |s| {
        s.acks.contains_key("stop")
    })
    .await;
    node.wait("the watched turn's end", Duration::from_secs(60), |s| {
        s.ended.contains_key(&sid)
    })
    .await;
    let (ack, turn_ended) = {
        let s = node.seen.lock().unwrap();
        (s.acks["stop"] - t0, s.ended[&sid] - t0)
    };
    node.tidy(sid).await;
    let m = Stop {
        stalled,
        stall,
        after_read,
        ack,
        turn_ended,
    };
    eprintln!(
        "B.7 real node, Stop with the out queue full: command stopped {:?} after the Stop \
         while the uplink took nothing for {:?}; {:?} after the uplink read again; \
         ack {:?} and turn_ended {:?} after the Stop",
        m.stalled, m.stall, m.after_read, m.ack, m.turn_ended
    );
    m
}

/// An approval answer the node acks with an error (no such session): the
/// command loop handles it inline and waits for room to ack it.
fn approval_answer() -> MasterToNode {
    MasterToNode::ApprovalRespond {
        req_id: "approval".into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
    }
}

/// The control: a command that writes nothing is not held by the full out
/// queue, so a Stop sent first ends it at once although its ack waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_ends_a_silent_command_while_the_uplink_takes_nothing() {
    let m = stop_while_full(SILENT, None).await;
    assert!(
        m.stalled.is_some_and(|d| d < Duration::from_secs(1)),
        "{m:?}"
    );
}

/// B7-R1: a command that writes output is not stopped while the out queue
/// is full.
///
/// Fails (2026-11-13): still running 6.0 s after the Stop; it stopped
/// within 1 µs of the uplink reading again, and the Stop's ack and the
/// turn's end came 6.03 s after the Stop. The hold is the agent loop's `coalesce` awaiting an append
/// inside a `select!` arm. One workspace's output can keep another
/// workspace's stopped command running for as long as the uplink takes
/// nothing (up to `IDLE_LIMIT`, 60 s, by reading) or until it next has
/// room (seconds on a slow uplink).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "B.7 review B7-R1: fails, a Stop does not end a streaming command while the out queue is full"]
async fn a_stop_ends_a_streaming_command_while_the_uplink_takes_nothing() {
    let m = stop_while_full(TALKING, None).await;
    assert!(m.stalled.is_some(), "{m:?}");
}

/// B7-R2: a Stop that comes right after another command the command loop
/// answers inline (an approval answer here; a second Stop or a
/// `stop_background` is the same) still ends a command that writes nothing.
///
/// Fails (2026-11-13): still running 6.2 s after the Stop. The command loop
/// waits for room to ack the approval answer and reads no command of any
/// session until the uplink moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "B.7 review B7-R2: fails, a Stop after an approval answer is not read while the out queue is full"]
async fn a_stop_after_an_approval_answer_ends_a_silent_command_while_the_uplink_takes_nothing() {
    let m = stop_while_full(SILENT, Some(approval_answer())).await;
    assert!(m.stalled.is_some(), "{m:?}");
}
