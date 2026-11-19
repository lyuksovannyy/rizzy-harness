//! A Stop, a timeout and an approval answer on a node whose uplink is full
//! (experiment B.7, reviews B7-R1, B7-R2, B7-D1; the fix is ADOPTION_PLAN
//! A.21).
//!
//! These drive the real `NodeRuntime` (session actor, agent loop, `bash`
//! tool, default limits) over its in-process link and ask whether a command
//! has ended and when the node's answers arrive. The "uplink" is the test's
//! reader of `from_node`: it reads as fast as it can, one message every so
//! often, or nothing at all (a link between two bursts, or a dead one
//! before the link's `IDLE_LIMIT`, 60 s, closes it). Other sessions run
//! `yes` (each cut at the default `tool_stream_bytes`, 1 MiB) to fill the
//! node's out queue while the uplink takes nothing.
//!
//! "Full" is read from the node (`NodeRuntime::backlog`): the test waits
//! until every streaming session has waited for room to send for a second.
//!
//! node.db is opened `Durability::Normal`, not `Full` as `node run` opens
//! it: on the slow disk this was measured on, an fsync per event fills the
//! out queue at about 2.5 messages a second and the run outlasts the
//! commands' timeouts. What the tests show does not depend on it, only on
//! the out queue being full; how fast a real node fills it does (review
//! B7-R4).
//!
//! How a stop is seen: the command writes its shell's pid to `pid` and
//! then `beat` every 0.1 s; it has ended when that process is gone (or a
//! zombie), looked at every 10 ms. (Until 2026-11-16 a 0.4 s look for a new
//! `beat` stood for "running": on a loaded machine one round of the loop,
//! a `date` and a `sleep`, took longer, and a running command was seen as
//! stopped: "the command stopped by itself", 1 of 10 runs at load 38.)
//!
//! What a stop must do is shown by order and by what the node records, not
//! by a clock of the test's: the command ends while the uplink still takes
//! nothing (before A.21 it ran until the uplink moved), and its tool result
//! is the bash tool's own cancel (`[cancelled: process group killed]`), not
//! the agent loop's abandonment at `TOOL_CANCEL_GRACE` (a fix that only
//! ends the command at the grace, re-check B7-D2). Times are printed.
//!
//! The tests run one at a time (`SERIAL`): each runs up to thirteen shells.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::event::*;
use harness_core::policy::{ApprovalMode, Decision};
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use tokio::sync::mpsc;

/// The node's lane for acks of inline commands (`URGENT_QUEUE` in
/// `crates/node/src/outbox.rs`, private there): past it their acks are
/// dropped.
const URGENT_QUEUE: usize = 64;
/// At most this many of those acks go in a row while output waits
/// (`URGENT_BURST`, private there).
const URGENT_BURST: usize = 8;
/// One line of output and one write of `beat` every 0.0961 s.
fn talking() -> String {
    while_test_lives("echo tick; date +%s%N > beat; echo >> rounds; sleep 961e-4")
}
/// No output, one write of `beat` every 0.0961 s.
fn silent() -> String {
    while_test_lives("date +%s%N > beat; echo >> rounds; sleep 961e-4")
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
/// B.7's twelve streaming sessions (re-check B7-D1: 14 appends ahead).
const FLOODERS: usize = 12;
/// The appends ahead of the watched session's next one with `FLOODERS`
/// streaming (re-check B7-D1).
const APPENDS_AHEAD: usize = 14;
/// Enough to keep the out queue full.
const FEW: usize = 4;
/// The uplink takes nothing.
const STALLED: u64 = u64::MAX;
/// How long a test waits, with the uplink taking nothing, for a command to
/// end: a limit on waiting only (a loaded machine took 196 s for one of
/// these tests), not a bound the node promises. What it catches does not
/// end at all: before A.21 the command ran until the uplink moved.
const STALL_LIMIT: Duration = Duration::from_secs(60);

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
struct Seen {
    acks: HashMap<String, Instant>,
    /// The error of each ack that refused.
    errors: HashMap<String, String>,
    deltas: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
    /// The last tool result of each session.
    results: HashMap<SessionId, String>,
    /// Every message read, in order: `ack <req_id>` or `<kind> <session>`.
    log: Vec<String>,
}

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    /// Milliseconds between two reads of the uplink; `STALLED`: none.
    pace: Arc<AtomicU64>,
    floods: Vec<SessionId>,
}

impl Node {
    async fn new() -> Self {
        Self::with(|_, _| {}).await
    }

    async fn with(edit: impl FnOnce(&mut NodePolicy, &mut NativeConfig)) -> Self {
        let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        let store = Store::open(&dir.path().join("node.db"), Durability::Normal).unwrap();
        let mut policy = NodePolicy {
            workspace_roots: vec![dir.path().into()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        };
        let mut native = NativeConfig::default();
        edit(&mut policy, &mut native);
        let node = NodeRuntime::start(NodeConfig {
            node_id: "local".into(),
            hostname: "test".into(),
            policy,
            store,
            backend: Arc::new(NativeBackend::new(
                Arc::new(Mock {
                    delay: Duration::from_millis(1),
                }),
                harness_tools::builtin(),
                native,
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
        let pace = Arc::new(AtomicU64::new(0));
        let mut from_node = link.from_node;
        let (s, p) = (seen.clone(), pace.clone());
        tokio::spawn(async move {
            loop {
                match p.load(Ordering::SeqCst) {
                    STALLED => {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        continue;
                    }
                    0 => {}
                    ms => tokio::time::sleep(Duration::from_millis(ms)).await,
                }
                let m = tokio::time::timeout(Duration::from_millis(20), from_node.recv()).await;
                let at = Instant::now();
                let mut s = s.lock().unwrap();
                match m {
                    Err(_) => {}
                    Ok(None) => return,
                    Ok(Some(NodeToMaster::Ack { req_id, error, .. })) => {
                        s.log.push(format!("ack {req_id}"));
                        if let Some(e) = error {
                            s.errors.insert(req_id.clone(), e);
                        }
                        s.acks.insert(req_id, at);
                    }
                    Ok(Some(NodeToMaster::Event { event })) => {
                        let sid = event.session_id;
                        s.log.push(format!("{} {sid}", event.kind));
                        match event.typed() {
                            Ok(EventKind::ToolOutputDelta(_)) => {
                                *s.deltas.entry(sid).or_default() += 1;
                            }
                            Ok(EventKind::TurnEnded(_)) => {
                                s.ended.insert(sid, at);
                            }
                            Ok(EventKind::ToolCallFinished(f)) => {
                                s.results.insert(sid, f.output);
                            }
                            _ => {}
                        }
                    }
                    Ok(Some(_)) => s.log.push("other".into()),
                }
            }
        });
        Self {
            dir,
            ws,
            node,
            to_node: link.to_node,
            seen,
            pace,
            floods: vec![],
        }
    }

    /// Hold the store's write lock from a second connection: every write
    /// of the node's store waits for its busy timeout (5 s), then fails.
    fn wedge(&self) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.dir.path().join("node.db")).unwrap();
        db.execute_batch("BEGIN EXCLUSIVE").unwrap();
        db
    }

    fn pace(&self, ms: u64) {
        self.pace.store(ms, Ordering::SeqCst);
    }

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

    async fn create(&self, name: &str, sid: SessionId) -> PathBuf {
        let cwd = self.ws.join(name);
        std::fs::create_dir(&cwd).unwrap();
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
        cwd
    }

    /// A session in a folder of its own whose first prompt is `text`.
    /// `ack`: wait for the create's ack (needs a reading uplink).
    async fn start(&self, name: &str, text: &str, ack: bool) -> (SessionId, PathBuf) {
        let sid = SessionId::new();
        let cwd = self.create(name, sid).await;
        if ack {
            let req = format!("create-{name}");
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
            files: vec![],
            owner: None,
            peers: vec![],
            tags: vec![],
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

    /// Stall the uplink, start `n` flooders, and wait until they and
    /// `also` each wait for room to send.
    async fn fill(&mut self, n: usize, also: Option<SessionId>) {
        self.pace(STALLED);
        for i in 0..n {
            let (sid, _) = self.start(&format!("flood{i}"), FLOOD, false).await;
            self.floods.push(sid);
        }
        let n = self.floods.len() + usize::from(also.is_some());
        full(&self.node, n).await;
    }

    /// Read again, stop the flooders and wait for every turn to end.
    async fn tidy(&self, also: Option<SessionId>) {
        self.pace(0);
        for (i, sid) in self.floods.iter().enumerate() {
            self.cancel(&format!("tidy-{i}"), *sid).await;
        }
        let all: Vec<SessionId> = self.floods.iter().copied().chain(also).collect();
        self.wait("every turn to end", Duration::from_secs(120), |s| {
            all.iter().all(|sid| s.ended.contains_key(sid))
        })
        .await;
    }
}

/// Until at least `n` senders (one per streaming session) have waited for
/// room in the node's out queue for a second (review A21-7: read from the
/// node, not inferred from heads that stop moving).
async fn full(node: &NodeRuntime, n: usize) {
    let end = Instant::now() + Duration::from_secs(120);
    let mut since = None;
    loop {
        let b = node.backlog();
        match (b.waiting >= n && b.queued > 0, since) {
            (true, None) => since = Some(Instant::now()),
            (true, Some(t)) if t.elapsed() > Duration::from_secs(1) => return,
            (false, _) => since = None,
            _ => {}
        }
        assert!(Instant::now() < end, "the out queue never filled: {b:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Whether the command in `dir` runs: the shell whose pid it wrote is alive
/// and not a zombie (the node reaps it when the tool ends).
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

/// The bash tool's own result for a cancel: it saw the Stop (not the agent
/// loop's abandonment at the grace).
fn cancelled_by_the_tool(result: &str) -> bool {
    result.contains("[cancelled") && !result.contains("abandoned")
}

/// A session running `cmd`, seen writing `beat` (and, if it talks, seen
/// streaming) while the uplink still reads.
async fn running(node: &Node, cmd: &str) -> (SessionId, PathBuf) {
    let (sid, dir) = node.start("watched", cmd, true).await;
    if cmd == talking() {
        node.wait(
            "the watched session to stream",
            Duration::from_secs(30),
            |s| s.deltas.get(&sid).copied().unwrap_or(0) > 0,
        )
        .await;
    }
    let end = Instant::now() + Duration::from_secs(60);
    while !(beating(&dir) && dir.join("beat").exists()) {
        assert!(Instant::now() < end, "the watched command never ran");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    (sid, dir)
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

/// What one run measured after the Stop.
#[derive(Debug)]
struct Stop {
    /// When the command was seen gone, while the uplink took nothing.
    stalled: Option<Duration>,
    /// How long the uplink took nothing after the Stop.
    stall: Duration,
    /// When the command was seen gone, after the uplink read again.
    after_read: Option<Duration>,
    ack: Duration,
    turn_ended: Duration,
    /// The watched call's tool result.
    result: String,
    /// Rounds the loop ran between the Stop's arrival and its end.
    rounds: usize,
}

/// Fill the out queue, send `before` if any and then a Stop for the watched
/// session running `cmd`, and look for up to `STALL_LIMIT` with the uplink
/// taking nothing.
async fn stop_while_full(cmd: &str, before: Option<MasterToNode>) -> Stop {
    let mut node = Node::new().await;
    let (sid, dir) = running(&node, cmd).await;
    node.fill(FLOODERS, (cmd == talking()).then_some(sid)).await;
    assert!(beating(&dir), "the command stopped by itself");
    let t0 = Instant::now();
    if let Some(m) = before {
        node.send(m).await;
    }
    node.cancel("stop", sid).await;
    let r0 = rounds(&dir);
    let stalled = stopped_after(&dir, t0, STALL_LIMIT).await;
    let ran = rounds(&dir) - r0;
    let stall = t0.elapsed();
    node.pace(0);
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
    let (ack, turn_ended, result) = {
        let s = node.seen.lock().unwrap();
        (
            s.acks["stop"] - t0,
            s.ended[&sid] - t0,
            s.results.get(&sid).cloned().unwrap_or_default(),
        )
    };
    node.tidy(Some(sid)).await;
    let m = Stop {
        stalled,
        stall,
        after_read,
        ack,
        turn_ended,
        result,
        rounds: ran,
    };
    eprintln!(
        "A.21 real node, Stop with the out queue full: gone {:?} after the Stop while the \
         uplink took nothing for {:?}; {:?} after the uplink read again; ack {:?} and \
         turn_ended {:?} after the Stop; result {:?}; {} rounds after the Stop",
        m.stalled,
        m.stall,
        m.after_read,
        m.ack,
        m.turn_ended,
        m.result.lines().last(),
        m.rounds
    );
    m
}

/// An approval answer the node acks with an error (no such session): the
/// command loop handles it inline.
fn approval_answer(req: &str) -> MasterToNode {
    MasterToNode::ApprovalRespond {
        req_id: req.into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
        note: None,
    }
}

/// Ended by the Stop itself while the uplink took nothing: a fix that only
/// ends the command at the grace (recorded as abandoned, re-check B7-D2),
/// or one that ends it only once the uplink moves, does not pass.
fn promptly(m: &Stop) -> bool {
    m.stalled.is_some() && cancelled_by_the_tool(&m.result) && m.rounds <= STOP_ROUNDS
}

/// The control: a command that writes nothing is not held by the full out
/// queue, so a Stop sent first ends it at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_ends_a_silent_command_while_the_uplink_takes_nothing() {
    let _one = SERIAL.lock().await;
    let m = stop_while_full(&silent(), None).await;
    assert!(promptly(&m), "{m:?}");
}

/// B7-R1: a command that writes output is stopped while the out queue is
/// full.
///
/// Failed before A.21 (2026-11-13): still writing 6.1 s after the Stop; the
/// agent loop's `coalesce` awaited the append of the next output inside a
/// `select!` arm, so neither the tool nor its cancel was polled while that
/// append waited for room.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_ends_a_streaming_command_while_the_uplink_takes_nothing() {
    let _one = SERIAL.lock().await;
    let m = stop_while_full(&talking(), None).await;
    assert!(promptly(&m), "{m:?}");
}

/// B7-R2: a Stop that comes right after another command the command loop
/// answers inline (an approval answer here; a second Stop or a
/// `stop_background` is the same) still ends a command that writes nothing.
///
/// Failed before A.21 (2026-11-13): still writing 6.0 s after the Stop. The
/// command loop waited for room to ack the approval answer and read no
/// command of any session until the uplink moved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_after_an_approval_answer_ends_a_silent_command_while_the_uplink_takes_nothing() {
    let _one = SERIAL.lock().await;
    let m = stop_while_full(&silent(), Some(approval_answer("approval"))).await;
    assert!(promptly(&m), "{m:?}");
}

/// The timeout of a foreground `bash` call that states none, here: the
/// node's ceiling (`[limits] bash_timeout_max_s`) applies to the default
/// (60 s) too, so the test does not wait a minute. It must outlast the fill
/// with room to spare: on a loaded machine filling behind twelve flooders
/// took 17 s to 26 s, past the 15 s this was (3 of 3 runs failed there).
const TIMEOUT: Duration = Duration::from_secs(30);

/// Re-check B7-D1: a call's timeout ends a command that streams output while
/// the out queue is full. No Stop; the uplink takes nothing from the fill
/// until the command is gone (looked for up to `TIMEOUT` and `STALL_LIMIT`
/// after it was seen running), and its result is the call's timeout.
///
/// `FEW` flooders, not B.7's twelve: what is shown is that the call's timer
/// runs while its output's append waits for room, which needs the queue
/// full and this session waiting, not a given number of appends ahead (that
/// is the Stop's case below); fewer sessions fill it sooner.
///
/// Failed before A.21 (2026-11-13, with the default 60 s): still writing
/// 66 s and 75 s after its prompt; it ended only when the uplink read
/// again. The bash tool's timer is in the tool future, which `coalesce` did
/// not poll while its append waited.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_timeout_ends_a_streaming_command_while_the_uplink_takes_nothing() {
    let _one = SERIAL.lock().await;
    // The fill must end before the call's timer does, or the run shows
    // nothing (the timer fires while no append waits). How long the fill
    // takes is this machine's (17 s to 26 s, once 43.7 s at load 59): a run
    // whose fill came too late is not a result, and the next one gives the
    // call twice the time.
    for timeout in [TIMEOUT, 2 * TIMEOUT, 4 * TIMEOUT] {
        if timeout_while_full(timeout).await {
            return;
        }
    }
    panic!(
        "the out queue never filled before the call's timeout, up to {:?}",
        4 * TIMEOUT
    );
}

/// One run of the test above with the node's ceiling at `timeout`; false
/// when the queue filled too late for it to show anything.
async fn timeout_while_full(timeout: Duration) -> bool {
    // As `node run` passes the ceiling to the agent loop.
    let mut node = Node::with(|p, n| {
        p.limits.bash_timeout_max_s = timeout.as_secs();
        n.bash_timeout_max = timeout;
    })
    .await;
    let (sid, dir) = running(&node, &talking()).await;
    let ran = Instant::now();
    node.fill(FEW, Some(sid)).await;
    let filled = ran.elapsed();
    if filled + Duration::from_secs(2) >= timeout {
        eprintln!(
            "A.21 real node, a {timeout:?} timeout: the out queue filled {filled:?} after the \
             command ran, too late to show anything; again with a longer one"
        );
        node.cancel("tidy-watched", sid).await;
        node.tidy(Some(sid)).await;
        return false;
    }
    assert!(beating(&dir), "the command stopped by itself");
    // Ended while the uplink still takes nothing (before A.21 it ran until
    // the uplink moved), and by the call's timer.
    let ended = stopped_after(&dir, ran, timeout + STALL_LIMIT).await;
    assert!(
        ended.is_some(),
        "still running {:?} after the call started",
        timeout + STALL_LIMIT
    );
    node.tidy(Some(sid)).await;
    let result = node
        .seen
        .lock()
        .unwrap()
        .results
        .get(&sid)
        .cloned()
        .unwrap_or_default();
    eprintln!(
        "A.21 real node, a {timeout:?} timeout with the out queue full (full {filled:?} after \
         the command ran): gone {ended:?} after it ran; result {:?}",
        result.lines().last()
    );
    assert!(
        result.contains(&format!("[timed out after {}s", timeout.as_secs())),
        "not ended by its timeout: {result}"
    );
    true
}

/// B7-D1, the case the B.7 tests did not cover: an uplink that still moves,
/// one message every 200 ms, while twelve other sessions stream. Another
/// session's Stop ends its command, by the tool's own cancel, before the
/// uplink has taken one message per append ahead of it, and its ack comes
/// before that many messages too: one session's output does not hold
/// another's Stop. (Counted in messages: it was "within a second" and
/// "within two", which the 200 ms pace and the node stretch under load.)
///
/// Fails before A.21: the stopped command runs until the uplink has taken
/// one message for every append waiting ahead of it (14 messages, 2.8 s at
/// this pace, by B7-D1), and the ack waits behind the whole queue (256
/// messages, about 51 s).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_sessions_output_does_not_hold_another_sessions_stop_on_a_trickling_uplink() {
    let _one = SERIAL.lock().await;
    let mut node = Node::new().await;
    let (sid, dir) = running(&node, &talking()).await;
    node.fill(FLOODERS, Some(sid)).await;
    assert!(beating(&dir), "the command stopped by itself");
    node.pace(200);
    let t0 = Instant::now();
    let from = node.seen.lock().unwrap().log.len();
    node.cancel("stop", sid).await;
    let r0 = rounds(&dir);
    let stopped = stopped_after(&dir, t0, STALL_LIMIT).await;
    let ran = rounds(&dir) - r0;
    let taken_by_end = node.seen.lock().unwrap().log.len() - from;
    node.wait("the stop's ack", Duration::from_secs(120), |s| {
        s.acks.contains_key("stop")
    })
    .await;
    let (ack, ack_at) = {
        let s = node.seen.lock().unwrap();
        let at = s.log[from..].iter().position(|l| l == "ack stop");
        (s.acks["stop"] - t0, at)
    };
    node.tidy(Some(sid)).await;
    let result = node
        .seen
        .lock()
        .unwrap()
        .results
        .get(&sid)
        .cloned()
        .unwrap_or_default();
    eprintln!(
        "A.21 real node, Stop on an uplink taking a message every 200 ms: gone {stopped:?} \
         after the Stop, {taken_by_end} messages taken by then; {ran} rounds after the Stop; \
         ack after {ack:?}, message {ack_at:?} after the Stop; result {:?}",
        result.lines().last()
    );
    // Counted in messages the uplink took, not seconds (the pace stretches
    // with the machine's load): before A.21 the command ran until one
    // message per append ahead of it had gone (`APPENDS_AHEAD`), and the
    // ack waited behind the whole queue (256).
    assert!(
        stopped.is_some(),
        "still running {STALL_LIMIT:?} after the Stop"
    );
    assert!(
        taken_by_end < APPENDS_AHEAD,
        "the command ran until {taken_by_end} messages had gone ({stopped:?})"
    );
    assert!(cancelled_by_the_tool(&result), "{result}");
    assert!(
        ran <= STOP_ROUNDS,
        "the loop ran {ran} rounds after the Stop ({stopped:?})"
    );
    assert!(
        ack_at.is_some_and(|i| i < APPENDS_AHEAD),
        "the stop's ack was message {ack_at:?} after the Stop ({ack:?})"
    );
}

/// The acks of the inline commands (a cancel and a `stop_background` here)
/// go ahead of queued output; a create's ack does not: `Master::create_session` relies on it arriving after the
/// session's `session_created`.
///
/// The Stop's ack fails before A.21 (it came after the whole queue); the
/// create's order held then, and is what the lane must not break.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stops_ack_goes_ahead_of_queued_output_and_a_creates_ack_does_not() {
    let _one = SERIAL.lock().await;
    let mut node = Node::new().await;
    node.fill(FEW, None).await;
    let late = SessionId::new();
    node.create("late", late).await;
    node.cancel("stop", node.floods[0]).await;
    node.send(MasterToNode::StopBackground {
        req_id: "stop-bg".into(),
        session_id: node.floods[1],
        bg_id: None,
        by: harness_protocol::StopBackgroundBy::User,
    })
    .await;
    // The create has run up to its wait for room.
    tokio::time::sleep(Duration::from_secs(1)).await;
    let from = node.seen.lock().unwrap().log.len();
    node.pace(0);
    node.wait("the acks", Duration::from_secs(60), |s| {
        ["stop", "stop-bg", "create-late"]
            .iter()
            .all(|r| s.acks.contains_key(*r))
    })
    .await;
    let log = node.seen.lock().unwrap().log.clone();
    let at = |what: &str| log.iter().position(|l| l == what);
    let (stop, stop_bg, created, acked) = (
        at("ack stop"),
        at("ack stop-bg"),
        at(&format!("session_created {late}")),
        at("ack create-late"),
    );
    node.tidy(None).await;
    eprintln!(
        "A.21 real node, out queue full: the stop's ack was message {stop:?} and \
         stop_background's {stop_bg:?} (reading again at {from}); session_created {created:?}, \
         the create's ack {acked:?}"
    );
    let stop = stop.unwrap();
    assert!(
        stop < from + 3,
        "the stop's ack waited behind {} messages",
        stop - from
    );
    let stop_bg = stop_bg.unwrap();
    assert!(
        stop_bg < from + 4,
        "stop_background's ack waited behind {} messages",
        stop_bg - from
    );
    assert!(
        created.is_some_and(|c| c < acked.unwrap()),
        "the create's ack came before its session_created: {created:?} {acked:?}"
    );
}

/// A hostile (or broken) master floods approval answers, each answered
/// inline. The node keeps reading its commands while its uplink takes
/// nothing (at most `URGENT_QUEUE` of their acks are kept; the rest are
/// dropped, so a master cannot make the node hold them without bound), and
/// with the uplink moving again the flood does not starve the sessions'
/// output: while output waits, at most `URGENT_BURST` of those acks go in
/// a row.
///
/// The first part fails before A.21: the command loop waited for room for
/// the first ack and read nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_master_flooding_inline_commands_neither_stalls_the_node_nor_starves_its_output() {
    let _one = SERIAL.lock().await;
    let mut node = Node::new().await;
    node.fill(FEW, None).await;
    const N: usize = 2_000;
    let sent = Arc::new(AtomicUsize::new(0));
    let flood = |sent: Arc<AtomicUsize>, n: usize, tag: &'static str| {
        let tx = node.to_node.clone();
        async move {
            for i in 0..n {
                if tx
                    .send(approval_answer(&format!("{tag}{i}")))
                    .await
                    .is_err()
                {
                    return;
                }
                sent.fetch_add(1, Ordering::SeqCst);
            }
        }
    };
    let read_all = tokio::time::timeout(Duration::from_secs(20), flood(sent.clone(), N, "f")).await;
    let taken = sent.load(Ordering::SeqCst);
    eprintln!(
        "A.21 real node, a master flooding approval answers: {taken} of {N} read with the uplink \
         taking nothing"
    );
    assert!(
        read_all.is_ok(),
        "the node stopped reading commands after {taken} of {N} while its uplink took nothing"
    );
    // The last of them are read by now or soon.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Part 2: read one message every 5 ms while the master keeps flooding.
    let from = node.seen.lock().unwrap().log.len();
    node.pace(5);
    let more = tokio::spawn(flood(Arc::new(AtomicUsize::new(0)), usize::MAX, "g"));
    tokio::time::sleep(Duration::from_secs(3)).await;
    more.abort();
    let log = node.seen.lock().unwrap().log.clone();
    let kept = log.iter().filter(|l| l.starts_with("ack f")).count();
    let part2 = &log[from..];
    // Everything but the flood's acks: the sessions' events and the acks
    // of their lane commands, queued in order.
    let flooded = |l: &String| l.starts_with("ack f") || l.starts_with("ack g");
    let output = part2.iter().filter(|l| !flooded(l)).count();
    node.tidy(None).await;
    eprintln!(
        "A.21 real node, the flood went on: {kept} acks of the first {N} kept; then {} messages \
         read, {output} of them output",
        part2.len()
    );
    assert!(kept <= URGENT_QUEUE, "{kept} acks kept for the flood");
    assert!(
        part2.len() > 100,
        "the uplink read only {} messages",
        part2.len()
    );
    assert!(
        output + 1 >= part2.len() / (URGENT_BURST + 1),
        "the flood starved the output: {output} of {} messages: {}",
        part2.len(),
        part2
            .iter()
            .map(|l| if flooded(l) { 'a' } else { 'o' })
            .collect::<String>()
    );
}

/// `crates/node/src/lib.rs` `MAX_LANES` (private there).
const MAX_LANES: usize = 256;

/// Re-check D-1: a command refused "at once" (past `MAX_LANES` here; past
/// a lane's depth the same) reaches the master while the uplink is full.
/// The refusal follows no event, so it goes ahead of the queued output.
///
/// Fails on c9be431: the refusal was offered to the full ordered queue
/// and dropped, and the master's request timed out after 15 s (the
/// re-check's proof: 0 of 44 reached the master).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_past_the_lane_cap_reaches_the_master_while_the_uplink_is_full() {
    let _one = SERIAL.lock().await;
    let mut node = Node::new().await;
    node.fill(FEW, None).await;
    // Lanes stay busy: each prompt's audit waits for the store.
    let db = node.wedge();
    const N: usize = MAX_LANES + 40;
    for i in 0..N {
        node.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{i}"),
            session_id: SessionId::new(),
            text: "hi".into(),
            images: vec![],
            agent_named: false,
            files: vec![],
            owner: None,
            peers: vec![],
            tags: vec![],
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let from = node.seen.lock().unwrap().log.len();
    node.pace(0);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (refused, first) = {
        let s = node.seen.lock().unwrap();
        let refused: Vec<usize> = s.log[from..]
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.strip_prefix("ack ")
                    .and_then(|r| s.errors.get(r))
                    .is_some_and(|e| e.contains("commands in progress"))
            })
            .map(|(i, _)| i)
            .collect();
        (refused.len(), refused.first().copied())
    };
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    node.tidy(None).await;
    eprintln!(
        "D-1, {N} prompts for unknown sessions with the uplink full: {refused} refusals read in \
         the first second, the first as message {first:?} after reading again"
    );
    assert!(
        refused >= N - MAX_LANES - 5,
        "only {refused} refusals reached the master"
    );
}
