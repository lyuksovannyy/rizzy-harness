//! Independent review of A.21: proofs on the real node. Not part of the
//! suite: `run_proofs.sh` copies this file into `crates/node/tests` for one
//! run and removes it again.
//!
//! A "slow disk" here is the node's own `node.db` with its write lock held
//! by a second connection (`BEGIN EXCLUSIVE`): every write of the node's
//! store then takes the store's `busy_timeout` (5 s) and fails, one at a
//! time behind the store's single connection. Nothing else is changed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

const SILENT: &str = "run: while :; do date +%s%N > beat; sleep 978e-4; done";
const FLOOD: &str = "run: yes 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const STALLED: u64 = u64::MAX;

#[derive(Default)]
struct Seen {
    acks: HashMap<String, Instant>,
    errors: HashMap<String, String>,
    /// Largest assistant delta per session, in bytes.
    biggest_delta: HashMap<SessionId, usize>,
    deltas: HashMap<SessionId, usize>,
    ended: HashMap<SessionId, Instant>,
}

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    seen: Arc<Mutex<Seen>>,
    pace: Arc<AtomicU64>,
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
        let pace = Arc::new(AtomicU64::new(0));
        let mut from_node = link.from_node;
        let (s, p) = (seen.clone(), pace.clone());
        tokio::spawn(async move {
            loop {
                if p.load(Ordering::SeqCst) == STALLED {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    continue;
                }
                let m = tokio::time::timeout(Duration::from_millis(20), from_node.recv()).await;
                let at = Instant::now();
                let mut s = s.lock().unwrap();
                match m {
                    Err(_) => {}
                    Ok(None) => return,
                    Ok(Some(NodeToMaster::Ack { req_id, error, .. })) => {
                        if let Some(e) = error {
                            s.errors.insert(req_id.clone(), e);
                        }
                        s.acks.insert(req_id, at);
                    }
                    Ok(Some(NodeToMaster::Event { event })) => {
                        let sid = event.session_id;
                        match event.typed() {
                            Ok(EventKind::ToolOutputDelta(_)) => {
                                *s.deltas.entry(sid).or_default() += 1;
                            }
                            Ok(EventKind::AssistantDelta(d)) => {
                                *s.deltas.entry(sid).or_default() += 1;
                                let b = s.biggest_delta.entry(sid).or_default();
                                *b = (*b).max(d.text.len());
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
            dir,
            ws,
            node,
            to_node: link.to_node,
            seen,
            pace,
            floods: vec![],
        }
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

    async fn send(&self, m: MasterToNode) {
        tokio::time::timeout(Duration::from_secs(30), self.to_node.send(m))
            .await
            .expect("the node read no command for 30 s")
            .unwrap();
    }

    async fn start(&self, name: &str, text: &str, ack: bool) -> (SessionId, PathBuf) {
        let sid = SessionId::new();
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

    /// The builder's fill: stall, start `n` flooders, wait until no
    /// watched head moved for a second.
    async fn fill(&mut self, n: usize, also: Option<SessionId>) {
        self.pace(STALLED);
        for i in 0..n {
            let (sid, _) = self.start(&format!("flood{i}"), FLOOD, false).await;
            self.floods.push(sid);
        }
        let watch: Vec<SessionId> = self.floods.iter().copied().chain(also).collect();
        let end = Instant::now() + Duration::from_secs(120);
        let mut last = Vec::new();
        let mut since = Instant::now();
        loop {
            let heads = self.node.store().call(|s| s.heads()).await.unwrap();
            let now: Vec<u64> = watch
                .iter()
                .map(|w| heads.iter().find(|(s, _)| s == w).map_or(0, |h| h.1))
                .collect();
            let total: u64 = now.iter().sum();
            if now != last {
                last = now;
                since = Instant::now();
            } else if since.elapsed() > Duration::from_secs(1) && total >= 64 {
                return;
            }
            assert!(Instant::now() < end, "the out queue never filled: {last:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Hold the store's write lock from a second connection.
    fn wedge(&self) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.dir.path().join("node.db")).unwrap();
        db.execute_batch("BEGIN EXCLUSIVE").unwrap();
        db
    }
}

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

async fn stopped_after(dir: &Path, from: Instant, within: Duration) -> Option<Duration> {
    while from.elapsed() < within {
        let at = from.elapsed();
        if !beating(dir).await {
            return Some(at);
        }
    }
    None
}

fn approval_answer(req: &str) -> MasterToNode {
    MasterToNode::ApprovalRespond {
        req_id: req.into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
    }
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or(0)
}

fn alive() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

/// A21 review: once `AUDITS_PENDING` (64) audit lines wait for a slow
/// store, a Stop's own audit is written by the command loop *before* the
/// Stop acts. The Stop then waits for the disk, and the command loop reads
/// nothing else meanwhile (B.7's defect, through the disk instead of the
/// link). Before A.21 `audit_later` always spawned, so the Stop acted at
/// once. Expected to FAIL on 8c28173.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_does_not_wait_for_the_store_when_64_audit_lines_are_pending() {
    let node = Node::new().await;
    let (sid, dir) = node.start("watched", SILENT, true).await;
    let end = Instant::now() + Duration::from_secs(30);
    while !beating(&dir).await {
        assert!(Instant::now() < end, "the watched command never ran");
    }
    let db = node.wedge();
    for i in 0..64 {
        node.send(approval_answer(&format!("a{i}"))).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let t0 = Instant::now();
    node.cancel("stop", sid).await;
    node.send(approval_answer("after")).await;
    let stopped = stopped_after(&dir, t0, Duration::from_secs(8)).await;
    let (stop_ack, after_ack) = {
        let s = node.seen.lock().unwrap();
        (
            s.acks.get("stop").map(|t| *t - t0),
            s.acks.get("after").map(|t| *t - t0),
        )
    };
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    let released = Instant::now();
    let later = stopped_after(&dir, released, Duration::from_secs(60)).await;
    eprintln!(
        "A21 review, slow store with 64 audit lines pending: no write seen {stopped:?} after the \
         Stop (looked 8 s); stop ack {stop_ack:?}, the next approval answer's ack {after_ack:?}; \
         stopped {later:?} after the store was released"
    );
    assert!(
        stopped.is_some_and(|d| d < Duration::from_secs(1)),
        "the Stop waited for the store: still writing 8 s after it"
    );
}

/// A21 review: the claim is that the audit lines of a flood of inline
/// commands make work "bounded by the disk, not tasks without bound". A
/// Cancel for a session the node does not have goes to a lane of its own
/// (a task), whose audit then waits for the store there, not in the
/// command loop. So a master naming fresh session ids makes the node read
/// on and pile up one task per Cancel for as long as the store is slow.
/// The control: approval answers for unknown sessions are answered inline,
/// and the command loop stops reading once the 64 audit slots are taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancels_for_unknown_sessions_do_not_pile_up_tasks_while_the_store_is_slow() {
    let node = Node::new().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let db = node.wedge();
    let (base, rss0) = (alive(), rss_kib());
    const N: usize = 20_000;
    let t = Instant::now();
    for i in 0..N {
        node.cancel(&format!("c{i}"), SessionId::new()).await;
    }
    let read_in = t.elapsed();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let (tasks, rss1) = (alive() - base, rss_kib());
    // Control: inline commands block the loop once the audit slots are full.
    let mut taken = 0;
    for i in 0..1000 {
        match tokio::time::timeout(
            Duration::from_millis(200),
            node.to_node.send(approval_answer(&format!("x{i}"))),
        )
        .await
        {
            Ok(Ok(())) => taken += 1,
            _ => break,
        }
    }
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    eprintln!(
        "A21 review, slow store: {N} cancels for unknown sessions read in {read_in:?}; {tasks} \
         more tasks alive, RSS +{} KiB; then {taken} approval answers taken before the command \
         loop stopped reading",
        rss1.saturating_sub(rss0)
    );
    // Let the backlog drain before the runtime goes.
    let end = Instant::now() + Duration::from_secs(120);
    while alive() > base + 8 && Instant::now() < end {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        tasks < 1_000,
        "{tasks} tasks alive for {N} cancels of unknown sessions: not bounded by the disk"
    );
}

/// A21 review: while an append waits for room, `coalesce_led` keeps
/// polling the model provider but reads none of its deltas. The provider's
/// channel is unbounded, so the rest of the answer piles up in memory, and
/// when the provider finishes first it all goes out as one event.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_model_answer_is_not_lumped_into_one_event_behind_a_full_uplink() {
    let mut node = Node::new().await;
    // About 8 bytes a chunk, one every 1-2 ms: a 15 s answer of ~80 KiB.
    let (sid, _) = node.start("model", "slow: 8000", true).await;
    node.wait("the model's first delta", Duration::from_secs(30), |s| {
        s.deltas.get(&sid).copied().unwrap_or(0) > 0
    })
    .await;
    node.fill(4, None).await;
    // The provider finishes while the session's append waits.
    tokio::time::sleep(Duration::from_secs(25)).await;
    node.pace(0);
    node.wait("the model's turn to end", Duration::from_secs(120), |s| {
        s.ended.contains_key(&sid)
    })
    .await;
    let (biggest, n) = {
        let s = node.seen.lock().unwrap();
        (
            s.biggest_delta.get(&sid).copied().unwrap_or(0),
            s.deltas.get(&sid).copied().unwrap_or(0),
        )
    };
    for (i, f) in node.floods.iter().enumerate() {
        let _ = node
            .to_node
            .send(MasterToNode::Cancel {
                req_id: format!("tidy-{i}"),
                session_id: *f,
            })
            .await;
    }
    eprintln!(
        "A21 review, model answer behind a full uplink: {n} assistant deltas, the largest \
         {biggest} bytes"
    );
    assert!(
        biggest < 64 * 1024,
        "one assistant delta of {biggest} bytes: the answer piled up while the append waited"
    );
}

/// Delta re-check: a command past `MAX_LANES` (256) is refused "at once",
/// but the refusal goes into the ordered queue with `try_send`: while the
/// uplink is full it is dropped, and the master's request times out after
/// 15 s instead of getting the refusal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refusal_past_max_lanes_reaches_the_master_while_the_uplink_is_full() {
    let mut node = Node::new().await;
    node.fill(4, None).await;
    let db = node.wedge();
    for i in 0..300 {
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
        })
        .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    node.pace(0);
    tokio::time::sleep(Duration::from_secs(5)).await;
    let refused = node
        .seen
        .lock()
        .unwrap()
        .errors
        .values()
        .filter(|e| e.contains("commands in progress"))
        .count();
    for (i, f) in node.floods.iter().enumerate() {
        let _ = node
            .to_node
            .send(MasterToNode::Cancel {
                req_id: format!("tidy-{i}"),
                session_id: *f,
            })
            .await;
    }
    let answered = {
        let s = node.seen.lock().unwrap();
        (0..300).filter(|i| s.acks.contains_key(&format!("p{i}"))).count()
    };
    eprintln!("A21 delta, 300 prompts past MAX_LANES with the uplink full: {refused} refusals reached the master; {answered} of the 300 prompts answered at all");
    assert!(refused > 0, "every refusal past MAX_LANES was dropped");
}
