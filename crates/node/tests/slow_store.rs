//! The node's command loop while its store is slow (review A21-1, A21-2).
//!
//! A "slow store" is the node's own `node.db` with its write lock held by a
//! second connection (`BEGIN EXCLUSIVE`): each write then waits for the
//! store's `busy_timeout` (5 s) and fails, one at a time behind the store's
//! single connection. The uplink reads as fast as it can throughout.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

/// No output, one write of `beat` every 0.0962 s.
const SILENT: &str = "run: while :; do date +%s%N > beat; sleep 962e-4; done";
/// `crates/node/src/lib.rs` `MAX_LANES` (private there).
const MAX_LANES: usize = 256;

struct Node {
    dir: tempfile::TempDir,
    ws: PathBuf,
    node: NodeRuntime,
    to_node: mpsc::Sender<MasterToNode>,
    /// Acks read, with whether they said ok.
    acks: Arc<Mutex<HashMap<String, (Instant, bool)>>>,
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
        let acks = Arc::new(Mutex::new(HashMap::new()));
        let mut from_node = link.from_node;
        let a = acks.clone();
        tokio::spawn(async move {
            while let Some(m) = from_node.recv().await {
                if let NodeToMaster::Ack { req_id, ok, .. } = m {
                    a.lock().unwrap().insert(req_id, (Instant::now(), ok));
                }
            }
        });
        Self {
            dir,
            ws,
            node,
            to_node: link.to_node,
            acks,
        }
    }

    async fn send(&self, m: MasterToNode) {
        tokio::time::timeout(Duration::from_secs(30), self.to_node.send(m))
            .await
            .expect("the node read no command for 30 s")
            .unwrap();
    }

    async fn acked(&self, req: &str, within: Duration) {
        let end = Instant::now() + within;
        while !self.acks.lock().unwrap().contains_key(req) {
            assert!(Instant::now() < end, "no ack for {req}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A session in a folder of its own running `text`.
    async fn start(&self, name: &str, text: &str) -> (SessionId, PathBuf) {
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
        self.acked(&format!("create-{name}"), Duration::from_secs(30))
            .await;
        self.send(prompt(&format!("prompt-{name}"), sid, text))
            .await;
        (sid, cwd)
    }

    /// Hold the store's write lock from a second connection.
    fn wedge(&self) -> rusqlite::Connection {
        let db = rusqlite::Connection::open(self.dir.path().join("node.db")).unwrap();
        db.execute_batch("BEGIN EXCLUSIVE").unwrap();
        db
    }
}

fn prompt(req: &str, sid: SessionId, text: &str) -> MasterToNode {
    MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: None,
        attachments: vec![],
        compare: vec![],
        req_id: req.into(),
        session_id: sid,
        text: text.into(),
        images: vec![],
    }
}

fn approval_answer(req: &str) -> MasterToNode {
    MasterToNode::ApprovalRespond {
        req_id: req.into(),
        session_id: SessionId::new(),
        approval_id: "a1".into(),
        decision: Decision::Allow,
    }
}

fn cancel(req: &str, sid: SessionId) -> MasterToNode {
    MasterToNode::Cancel {
        req_id: req.into(),
        session_id: sid,
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

fn alive() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

/// A21-1 (the review's proof, kept): with 64 audit lines waiting for a slow
/// store, a Stop acts at once and the command loop reads on. The command
/// loop acts, queues the ack, then hands the audit line to the audit
/// writer; it never waits for the store.
///
/// Failed on 8c28173: past 64 pending lines the command loop wrote the
/// next line itself before acting, so the command was still running 8 s
/// after the Stop and the next command was not read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_does_not_wait_for_a_slow_store() {
    let node = Node::new().await;
    let (sid, dir) = node.start("watched", SILENT).await;
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
    node.send(cancel("stop", sid)).await;
    node.send(approval_answer("after")).await;
    let stopped = stopped_after(&dir, t0, Duration::from_secs(8)).await;
    let (stop_ack, after_ack) = {
        let a = node.acks.lock().unwrap();
        (
            a.get("stop").map(|t| t.0 - t0),
            a.get("after").map(|t| t.0 - t0),
        )
    };
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    eprintln!(
        "A21-1, a slow store with 64 audit lines pending: no write seen {stopped:?} after the \
         Stop; its ack {stop_ack:?}, the next command's ack {after_ack:?}"
    );
    assert!(
        stopped.is_some_and(|d| d < Duration::from_secs(1)),
        "the Stop waited for the store: {stopped:?}"
    );
    assert!(
        after_ack.is_some_and(|d| d < Duration::from_secs(1)),
        "the next command waited for the store: {after_ack:?}"
    );
}

/// The audit writer keeps at most `AUDIT_QUEUE` lines; past it, and for a
/// write the store refused, it counts them and records the count in one
/// line once the store takes writes again (PLAN.md T5: every command is
/// recorded). The command loop reads on meanwhile.
///
/// The first assertion fails on 8c28173: the command loop wrote lines
/// itself once 64 were pending, so it read the flood at the store's pace.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn audit_lines_a_slow_store_cannot_take_are_counted_and_recorded() {
    let node = Node::new().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let db = node.wedge();
    const N: usize = 3_000;
    let t = Instant::now();
    for i in 0..N {
        node.send(approval_answer(&format!("a{i}"))).await;
    }
    let read_in = t.elapsed();
    // Past the store's busy timeout: the first write fails.
    tokio::time::sleep(Duration::from_secs(6)).await;
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    let end = Instant::now() + Duration::from_secs(60);
    let (kept, counted) = loop {
        let (kept, counted) = node
            .node
            .store()
            .call(|s| {
                let kept = s.audit_lines("approval.respond")?.len();
                let counted: u64 = s
                    .audit_lines("audit.not_recorded")?
                    .iter()
                    .map(|(_, d)| {
                        serde_json::from_str::<serde_json::Value>(d).unwrap()["count"]
                            .as_u64()
                            .unwrap()
                    })
                    .sum();
                Ok::<_, harness_store::StoreError>((kept, counted))
            })
            .await
            .unwrap();
        if kept as u64 + counted >= N as u64 || Instant::now() > end {
            break (kept, counted);
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    eprintln!(
        "A21-1, {N} approval answers on a slow store: read in {read_in:?}; {kept} audit lines \
         written, {counted} counted as not recorded"
    );
    assert!(
        read_in < Duration::from_secs(5),
        "the command loop read {N} commands in {read_in:?}: it waited for the store"
    );
    assert_eq!(
        kept as u64 + counted,
        N as u64,
        "lines lost without a record"
    );
}

/// A21-2 (the review's proof, kept): a Cancel for a session the node does
/// not have, and that no queued create will make, is answered at once
/// ("unknown session") in the command loop: no lane, no task.
///
/// Failed on 8c28173: each went to a lane of its own, whose audit waited
/// for the store there (20,000 cancels, 20,000 tasks alive, +199 MB).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancels_for_unknown_sessions_cost_no_task_while_the_store_is_slow() {
    let node = Node::new().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let db = node.wedge();
    let base = alive();
    const N: usize = 20_000;
    for i in 0..N {
        node.send(cancel(&format!("c{i}"), SessionId::new())).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let tasks = alive().saturating_sub(base);
    let refused = {
        let a = node.acks.lock().unwrap();
        (0..N)
            .filter(|i| a.get(&format!("c{i}")).is_some_and(|t| !t.1))
            .count()
    };
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    eprintln!(
        "A21-2, {N} cancels for unknown sessions on a slow store: {tasks} more tasks alive, \
         {refused} refused in an ack"
    );
    assert!(tasks < 100, "{tasks} tasks alive for {N} cancels");
    assert!(refused > 0, "no cancel was answered");
}

/// A21-2: commands that go to a session's lane (a prompt here) for
/// sessions that do not exist, while the store is slow: at most `MAX_LANES`
/// lanes live, and each command past them is refused at once.
///
/// Fails on 8c28173: one lane, one task, per session named.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_number_of_session_lanes_is_capped() {
    let node = Node::new().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let db = node.wedge();
    let base = alive();
    const N: usize = 2_000;
    for i in 0..N {
        node.send(prompt(&format!("p{i}"), SessionId::new(), "hi"))
            .await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let tasks = alive().saturating_sub(base);
    let refused = {
        let a = node.acks.lock().unwrap();
        (0..N)
            .filter(|i| a.get(&format!("p{i}")).is_some_and(|t| !t.1))
            .count()
    };
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    eprintln!(
        "A21-2, {N} prompts for unknown sessions on a slow store: {tasks} more tasks alive, \
         {refused} refused at once"
    );
    assert!(
        tasks < MAX_LANES + 100,
        "{tasks} tasks alive for {N} prompts"
    );
    assert!(refused >= N - MAX_LANES - 100, "only {refused} refused");
}

/// Re-check D-2: a clean stop does not leave audit lines behind. `flush`
/// waits, within its bound, until every queued line is written and every
/// counted one recorded as a count, and says false while that is not done.
///
/// Fails on c9be431: `flush` waited for the link only and returned true
/// with the store still held and 1,500 lines queued or counted in memory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_clean_stop_writes_or_counts_every_audit_line() {
    let node = Node::new().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let db = node.wedge();
    const N: usize = 1_500;
    for i in 0..N {
        node.send(approval_answer(&format!("a{i}"))).await;
    }
    let left = node.node.stop(Duration::from_secs(1)).await;
    let held = node.node.flush(Duration::from_secs(1)).await;
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    let done = node.node.flush(Duration::from_secs(30)).await;
    // What is in node.db now, read beside the store (as after the process
    // ended).
    let (kept, counted): (i64, i64) = {
        let db = rusqlite::Connection::open(node.dir.path().join("node.db")).unwrap();
        let kept = db
            .query_row(
                "SELECT COUNT(*) FROM audit WHERE kind = 'approval.respond'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let counted = db
            .query_row(
                "SELECT COALESCE(SUM(json_extract(detail, '$.count')), 0) FROM audit \
                 WHERE kind = 'audit.not_recorded'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        (kept, counted)
    };
    eprintln!(
        "D-2, {N} audit lines at a stop with the store held: flush said {held} while held, \
         {done} after; {kept} written, {counted} counted (left: {left:?})"
    );
    assert!(
        !held,
        "flush said all was out while the store held the audit lines"
    );
    assert!(done, "flush gave up with the store free");
    assert_eq!(kept + counted, N as i64, "audit lines lost at a clean stop");
}
