//! A name lookup the system resolver never answers (review of PR 155, L2;
//! PLAN.md T84): each check ends at its own time limit with a fixed error,
//! and the stuck lookups do not pile up threads: each worker has one
//! blocking thread, so later lookups wait for it instead of starting more.
//! One test in this file, so the thread count is this test's alone.

mod fake_mcp;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use harness_master::connectors::client::Failure;
use harness_master::connectors::{self, Net, Pool};

fn threads() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stuck_lookup_does_not_pile_up_threads() {
    let dir = tempfile::tempdir().unwrap();
    let fake = fake_mcp::start(dir.path()).await;
    let pool = Pool::default();
    let ok = Net {
        lookup: harness_master::remote::Lookup::table(fake.table()),
        allow_private: true,
        roots: vec![fake.cert_der.clone()],
        timeout: Duration::from_secs(5),
    };
    for i in 0..2 {
        let c = connectors::check(
            &pool,
            &format!("w{i}"),
            "conn",
            "https://mcp.test/ok",
            None,
            &ok,
        )
        .await
        .unwrap();
        assert!(c.result.is_ok());
    }
    let stuck = Net {
        lookup: harness_master::remote::Lookup::stalling(
            HashMap::new(),
            "stall.test",
            Duration::from_secs(6),
        ),
        timeout: Duration::from_millis(800),
        ..ok
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let th0 = threads();
    for i in 0..6 {
        let started = Instant::now();
        let c = connectors::check(&pool, "p", "conn", "https://stall.test/mcp", None, &stuck)
            .await
            .unwrap();
        assert_eq!(c.result.unwrap_err(), Failure::Timeout, "check {i}");
        assert!(started.elapsed() < Duration::from_secs(3), "check {i}");
    }
    let th1 = threads();
    assert!(
        th1 <= th0 + connectors::pool::WORKERS,
        "threads {th0} -> {th1}: stuck lookups piled up"
    );
}
