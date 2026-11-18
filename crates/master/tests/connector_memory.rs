//! What connector checks cost the server's process (review of PR 155,
//! HIGH 1; PLAN.md T84): against hostile answers (about 1 MB of minimal
//! tools, 400 KB of them in one array, a list just under every cap), many
//! checks one after another and many at once: at most two run at a time,
//! the rest are refused at once; the peak memory growth and the memory
//! kept afterwards stay under stated bounds; no thread is left.
//!
//! Measured as the reviewer did, from this process's `/proc/self/status`
//! (`VmHWM`, `VmRSS`) and `/proc/self/task`. To keep it from being flaky,
//! this file holds this one test only (no other test of the binary runs
//! beside it), the pool's threads and the fake are made before the first
//! measure, and the bounds have room for a loaded machine (several times
//! what the fixed code was measured at, a fraction of what the old code
//! took).

mod fake_mcp;

use std::time::Duration;

use harness_master::connectors::{self, Busy, Net, Pool};

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

/// Peak growth allowed, kB (the old code: +37 MB for one check, +488 MB
/// for twenty at once).
const PEAK_KB: u64 = 48 * 1024;
/// Memory kept after all ended, kB (the old code kept 306 MB of RSS).
const KEPT_KB: u64 = 24 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checks_cost_bounded_memory_and_leave_no_thread() {
    let dir = tempfile::tempdir().unwrap();
    let fake = fake_mcp::start(dir.path()).await;
    let net = Net {
        lookup: harness_master::remote::Lookup::table(fake.table()),
        allow_private: true,
        roots: vec![fake.cert_der.clone()],
        timeout: Duration::from_secs(8),
    };
    let pool = Pool::default();
    let one = |who: String, path: &'static str| {
        let (pool, net) = (&pool, net.clone());
        async move {
            connectors::check(
                pool,
                &who,
                "conn",
                &format!("https://mcp.test/{path}"),
                None,
                &net,
            )
            .await
        }
    };
    // Warm up: the workers exist, each has run once.
    for i in 0..2 {
        assert!(one(format!("w{i}"), "ok").await.unwrap().result.is_ok());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (th0, rss0, hwm0) = (threads(), vm("VmRSS:"), vm("VmHWM:"));

    // One after another, the dearest answers.
    for i in 0..10 {
        for path in ["tiny", "dense", "smalltools"] {
            let c = one(format!("s{i}"), path).await.unwrap();
            assert_eq!(c.tasks_left, 0, "{path}");
        }
    }
    // Twenty at once, as one person may start in a window; then again.
    for round in 0..3 {
        let all = futures::future::join_all((0..20).map(|i| {
            one(
                format!("p{round}-{i}"),
                if i % 2 == 0 { "tiny" } else { "smalltools" },
            )
        }))
        .await;
        let ran = all.iter().filter(|r| r.is_ok()).count();
        let full = all.iter().filter(|r| matches!(r, Err(Busy::Full))).count();
        assert!((1..=2).contains(&ran), "round {round}: {ran} ran");
        assert_eq!(ran + full, 20);
    }
    assert!(pool.most_at_once() <= 2, "{}", pool.most_at_once());
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
