//! What agents' connector calls cost the server's process (X.35 step 2,
//! PLAN.md T85), as `connector_memory.rs` measures checks: against the
//! dearest answers a call may get (a result just under its budget and the
//! value cap, 50 MB cut at the budget, a JSON bomb refused before the
//! parse), many calls one after another and many at once on the call
//! workers: at most [`call::WORKERS`] run at a time, the rest are refused
//! at once; the peak growth and what is kept afterwards stay under stated
//! bounds; no thread is left. This file holds this one test only.

mod fake_mcp;

use std::time::Duration;

use harness_master::connectors::Net;
use harness_master::connectors::call::{self, Remote};
use harness_master::connectors::pool::{Busy, Pool};
use harness_store::custom_connectors::ConnectorRow;

fn vm(field: &str) -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    s.lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn threads() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

/// Peak growth allowed, kB: four calls at once, each bounded by its
/// 256 KiB and 8,192 values.
const PEAK_KB: u64 = 64 * 1024;
/// Memory kept after all ended, kB (the workers' allocator arenas).
const KEPT_KB: u64 = 32 * 1024;

fn row(path: &str, i: usize) -> ConnectorRow {
    ConnectorRow {
        connector_id: format!("c{i}"),
        workspace_id: "w".into(),
        name: "conn".into(),
        url: format!("https://mcp.test/{path}"),
        auth: "none".into(),
        header: None,
        created_ms: 0,
        updated_ms: 0,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_cost_bounded_memory_and_leave_no_thread() {
    let dir = tempfile::tempdir().unwrap();
    let fake = fake_mcp::start(dir.path()).await;
    let net = Net {
        lookup: harness_master::remote::Lookup::table(fake.table()),
        allow_private: true,
        roots: vec![fake.cert_der.clone()],
        timeout: Duration::from_secs(8),
    };
    let pool = Pool::new("connector-call", call::WORKERS, call::PER_KEY);
    let one = |who: String, path: &'static str, i: usize| {
        let (pool, net) = (&pool, net.clone());
        async move {
            call::run_on(
                pool,
                &who,
                &row(path, i),
                Remote {
                    token: None,
                    raw: "tool_0".into(),
                    arguments: serde_json::json!({"q": "x"}),
                    net,
                },
            )
            .await
        }
    };
    // Warm up: every worker exists and has run once.
    let warm =
        futures::future::join_all((0..call::WORKERS).map(|i| one(format!("w{i}"), "plain", i)))
            .await;
    assert!(
        warm.iter().all(|r| matches!(r, Ok(Some(Ok(_))))),
        "{warm:?}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (th0, rss0, hwm0) = (threads(), vm("VmRSS:"), vm("VmHWM:"));

    for i in 0..10 {
        let full = one(format!("s{i}"), "callfull", i).await.unwrap().unwrap();
        let a = full.expect("parsed");
        assert!(a.text.len() > 20_000, "{}", a.text.len());
        let big = one(format!("s{i}"), "callbig", i).await.unwrap().unwrap();
        assert!(big.is_err());
        let bomb = one(format!("s{i}"), "callbomb", i).await.unwrap().unwrap();
        assert!(bomb.is_err());
    }
    // Twenty at once, three rounds: at most the workers run, the rest are
    // refused at once (never queued).
    for round in 0..3 {
        let all = futures::future::join_all((0..20).map(|i| {
            one(
                format!("p{round}-{i}"),
                if i % 2 == 0 { "callfull" } else { "callbig" },
                i,
            )
        }))
        .await;
        let ran = all.iter().filter(|r| r.is_ok()).count();
        let full = all.iter().filter(|r| matches!(r, Err(Busy::Full))).count();
        assert!(
            (1..=call::WORKERS).contains(&ran),
            "round {round}: {ran} ran"
        );
        assert_eq!(ran + full, 20);
    }
    assert!(pool.most_at_once() <= call::WORKERS);
    assert_eq!(fake.settle().await, 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (th1, rss1, hwm1) = (threads(), vm("VmRSS:"), vm("VmHWM:"));
    eprintln!(
        "threads {th0} -> {th1}; VmHWM {hwm0} -> {hwm1} kB (+{}); VmRSS {rss0} -> {rss1} kB",
        hwm1.saturating_sub(hwm0)
    );
    assert_eq!(th1, th0, "threads after = threads before");
    assert!(
        hwm1.saturating_sub(hwm0) < PEAK_KB,
        "peak grew {} kB",
        hwm1 - hwm0
    );
    assert!(
        rss1.saturating_sub(rss0) < KEPT_KB,
        "kept {} kB",
        rss1.saturating_sub(rss0)
    );
}
