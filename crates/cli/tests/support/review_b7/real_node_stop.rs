//! Review of B.7 (not part of the suite; `run_proofs.sh` copies this into
//! `crates/node/tests/` for one run and removes it again).
//!
//! B.7's test uses a model of the node and records when a cancel reaches the
//! model's command loop. This drives the real `NodeRuntime` (real session
//! actor, real agent loop, real `bash` tool, defaults) over its in-process
//! link and asks what the model cannot: does a Stop end the command while the
//! node's out queue is full?
//!
//! Session B runs a quiet command (one short line and one write of `beat`
//! every 0.1 s). Twelve other sessions run `yes`, each cut at the default
//! `tool_stream_bytes` (1 MiB). The "uplink" is the test's reader of
//! `from_node`: it reads, or it takes nothing.
//!
//! node.db is opened `Durability::Normal` unless `REVIEW_B7_FULL=1`: this
//! checkout is on a slow disk, where an fsync per event makes the run longer
//! than the quiet command's 60 s timeout. What is shown does not depend on it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// `sleep 966e-4` is 0.0966 s (the review's marker number).
const QUIET: &str = "run: while :; do echo tick; date +%s%N > beat; sleep 966e-4; done";
const FLOOD: &str = "run: yes 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const FLOODERS: usize = 12;

#[derive(Default)]
struct Seen {
    acks: HashMap<String, Instant>,
    deltas: HashMap<SessionId, usize>,
    /// Bytes of `text` in those deltas.
    delta_bytes: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
}

struct Rig {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    _node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    /// The uplink takes nothing while this is set.
    stalled: Arc<AtomicBool>,
    /// Messages waiting in the node's out queue, as of the reader's last look.
    queued: Arc<AtomicUsize>,
}

impl Rig {
    async fn new() -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        // As `node run` opens it (`crates/cli/src/main.rs`).
        let durability = if std::env::var_os("REVIEW_B7_FULL").is_some() {
            Durability::Full
        } else {
            Durability::Normal
        };
        let store = Store::open(&dir.path().join("node.db"), durability).unwrap();
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
        {
            let (seen, stalled, queued) = (seen.clone(), stalled.clone(), queued.clone());
            tokio::spawn(async move {
                loop {
                    queued.store(from_node.len(), Ordering::SeqCst);
                    if stalled.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        continue;
                    }
                    let m = tokio::time::timeout(Duration::from_millis(20), from_node.recv()).await;
                    let at = Instant::now();
                    match m {
                        Err(_) => {}
                        Ok(None) => return,
                        Ok(Some(NodeToMaster::Ack { req_id, .. })) => {
                            seen.lock().unwrap().acks.insert(req_id, at);
                        }
                        Ok(Some(NodeToMaster::Event { event })) => {
                            let sid = event.session_id;
                            let mut s = seen.lock().unwrap();
                            match event.typed().unwrap() {
                                EventKind::ToolOutputDelta(d) => {
                                    *s.deltas.entry(sid).or_default() += 1;
                                    *s.delta_bytes.entry(sid).or_default() += d.text.len();
                                }
                                EventKind::TurnEnded(_) => {
                                    s.ended.insert(sid, at);
                                }
                                _ => {}
                            }
                        }
                        Ok(Some(_)) => {}
                    }
                }
            });
        }
        Self {
            _dir: dir,
            ws,
            _node: node,
            to_node: link.to_node,
            seen,
            stalled,
            queued,
        }
    }

    async fn wait(&self, what: &str, within: Duration, done: impl Fn(&Seen) -> bool) {
        let end = Instant::now() + within;
        loop {
            if done(&self.seen.lock().unwrap()) {
                return;
            }
            assert!(Instant::now() < end, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A session in a folder of its own, with `text` as its first prompt.
    /// `wait_ack`: wait for the create's ack (needs a reading uplink).
    async fn start(&self, name: &str, text: &str, wait_ack: bool) -> (SessionId, PathBuf) {
        let cwd = self.ws.join(name);
        std::fs::create_dir(&cwd).unwrap();
        let sid = SessionId::new();
        let req = format!("create-{name}");
        self.to_node
            .try_send(MasterToNode::Create {
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
            })
            .unwrap();
        if wait_ack {
            self.wait("a create", Duration::from_secs(20), |s| {
                s.acks.contains_key(&req)
            })
            .await;
        }
        self.to_node
            .try_send(MasterToNode::Prompt {
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
            })
            .unwrap();
        (sid, cwd)
    }

    fn cancel(&self, req: &str, sid: SessionId) {
        self.to_node
            .try_send(MasterToNode::Cancel {
                req_id: req.into(),
                session_id: sid,
            })
            .unwrap();
    }
}

/// Whether the quiet command still writes `beat` (two looks 0.4 s apart).
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

/// How long after `from` the quiet command stopped writing, looking for at
/// most `within`.
async fn stopped_after(dir: &Path, from: Instant, within: Duration) -> Option<Duration> {
    while from.elapsed() < within {
        let at = from.elapsed();
        if !beating(dir).await {
            return Some(at);
        }
    }
    None
}

struct Run {
    rig: Rig,
    quiet: (SessionId, PathBuf),
    floods: Vec<SessionId>,
}

/// The quiet session, streaming and writing `beat`.
async fn quiet_running() -> Run {
    let rig = Rig::new().await;
    let quiet = rig.start("quiet", QUIET, true).await;
    let sid = quiet.0;
    rig.wait("the quiet session to stream", Duration::from_secs(30), |s| {
        s.deltas.get(&sid).copied().unwrap_or(0) > 0
    })
    .await;
    assert!(beating(&quiet.1).await, "the quiet command is not running");
    Run {
        rig,
        quiet,
        floods: vec![],
    }
}

/// Start the flooders, without waiting for anything from the node.
async fn flood(run: &mut Run) {
    for i in 0..FLOODERS {
        let (sid, _) = run.rig.start(&format!("flood{i}"), FLOOD, false).await;
        run.floods.push(sid);
    }
}

async fn tidy(run: &Run) {
    run.rig.stalled.store(false, Ordering::SeqCst);
    for (i, sid) in run.floods.iter().enumerate() {
        run.rig.cancel(&format!("tidy-{i}"), *sid);
    }
    let all: Vec<SessionId> = run.floods.iter().copied().chain([run.quiet.0]).collect();
    run.rig
        .wait("every turn to end", Duration::from_secs(60), |s| {
            all.iter().all(|sid| s.ended.contains_key(sid))
        })
        .await;
}

/// The control: the same five sessions, the uplink reading as fast as it
/// can. A Stop ends the quiet session's command within a second.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn control_with_a_reading_uplink_a_stop_ends_the_command_at_once() {
    let mut run = quiet_running().await;
    flood(&mut run).await;
    let floods = run.floods.clone();
    run.rig
        .wait("every flooder to stream", Duration::from_secs(30), |s| {
            floods.iter().all(|sid| s.deltas.get(sid).copied().unwrap_or(0) > 0)
        })
        .await;
    assert!(beating(&run.quiet.1).await, "the quiet command is not running");
    let t0 = Instant::now();
    run.rig.cancel("stop", run.quiet.0);
    let dead = stopped_after(&run.quiet.1, t0, Duration::from_secs(10)).await;
    run.rig
        .wait("the stop's ack", Duration::from_secs(10), |s| {
            s.acks.contains_key("stop")
        })
        .await;
    let ack = run.rig.seen.lock().unwrap().acks["stop"] - t0;
    eprintln!("B.7 review, control: command stopped {dead:?} after the Stop, ack after {ack:?}");
    tidy(&run).await;
    {
        let s = run.rig.seen.lock().unwrap();
        let (n, bytes): (usize, usize) = floods
            .iter()
            .map(|sid| (s.deltas[sid], s.delta_bytes[sid]))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        eprintln!(
            "B.7 review, control: {} flooders streamed {n} tool_output_delta events, {bytes} bytes \
             of text, {} bytes an event on average",
            floods.len(),
            bytes / n.max(1)
        );
    }
    assert!(
        dead.is_some_and(|d| d < Duration::from_secs(1)),
        "the command ran on with a reading uplink: {dead:?}"
    );
}

/// The uplink takes nothing for 8 s (a link between two bursts, or a dead
/// one before `IDLE_LIMIT`, 60 s, closes it). The node's out queue is full,
/// so the quiet session's next `tool_output_delta` waits in `publish_wait`
/// inside the agent loop's `coalesce`, which is the task that polls `bash`.
/// The Stop cancels the turn's token, and nothing looks at it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_a_full_uplink_a_stop_does_not_end_the_command() {
    let mut run = quiet_running().await;
    run.rig.stalled.store(true, Ordering::SeqCst);
    flood(&mut run).await;
    // Full, and still full a second later: every streaming session is now
    // waiting for room.
    let end = Instant::now() + Duration::from_secs(60);
    let mut full_since = None;
    loop {
        let q = run.rig.queued.load(Ordering::SeqCst);
        match (q >= 256, full_since) {
            (true, None) => full_since = Some(Instant::now()),
            (true, Some(t)) if t.elapsed() > Duration::from_secs(1) => break,
            (false, _) => full_since = None,
            _ => {}
        }
        assert!(Instant::now() < end, "the out queue never filled: {q}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        beating(&run.quiet.1).await,
        "the quiet command stopped by itself once the queue was full"
    );

    let t0 = Instant::now();
    run.rig.cancel("stop", run.quiet.0);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let at_3s = beating(&run.quiet.1).await;
    tokio::time::sleep(Duration::from_millis(3600)).await;
    // Past the agent loop's 5 s `TOOL_CANCEL_GRACE` too.
    let at_7s = beating(&run.quiet.1).await;
    let stall = t0.elapsed();

    run.rig.stalled.store(false, Ordering::SeqCst);
    let t1 = Instant::now();
    let dead = stopped_after(&run.quiet.1, t1, Duration::from_secs(20)).await;
    run.rig
        .wait("the stop's ack", Duration::from_secs(20), |s| {
            s.acks.contains_key("stop")
        })
        .await;
    let quiet = run.quiet.0;
    run.rig
        .wait("the quiet turn's end", Duration::from_secs(30), |s| {
            s.ended.contains_key(&quiet)
        })
        .await;
    let (ack, ended) = {
        let s = run.rig.seen.lock().unwrap();
        (s.acks["stop"] - t0, s.ended[&quiet] - t0)
    };
    eprintln!(
        "B.7 review, full uplink: command still running 3 s after the Stop: {at_3s}; \
         7 s after: {at_7s}; uplink read again after {stall:?}; command stopped {dead:?} \
         after that; ack {ack:?} and turn_ended {ended:?} after the Stop"
    );
    tidy(&run).await;
    assert!(
        at_3s && at_7s,
        "the Stop ended the command with a full uplink (3 s: {at_3s}, 7 s: {at_7s})"
    );
}
