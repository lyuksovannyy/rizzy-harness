//! What connector checks and agents' connector calls cost the server's
//! process (X.35 step 2, PLAN.md T85; review of PR 157, M5), with the fake
//! remote server in ANOTHER process (this test binary started again, its
//! `fake_server_process` only), so nothing of the fake is counted: idle
//! before any connector use, after heavy checks, after 200 heavy calls, and
//! a while later. The dearest answers each may get: about 1 MB of minimal
//! tools, a list just under the value cap, a result just under a call's
//! budget and the value cap, 50 MB cut at the budget, a JSON bomb refused
//! before the parse; one after another and twenty at once.
//!
//! `HARNESS_MEMORY_WAIT_S` sets how long "a while later" is (default 2;
//! the review's table used 60). The numbers go to stderr; the bound holds
//! what this process keeps after use. This file holds this one test only.

mod fake_mcp;

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

use harness_master::connectors::call::{self, Remote};
use harness_master::connectors::pool::Pool;
use harness_master::connectors::{self, Net};
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

/// What this process may keep after all the use below, kB (RSS after,
/// minus idle before). Measured (2026-11-19): +8.2 MB release, +11.3 MB
/// debug on the one connector thread; +18.8 MB release, +23 MB debug on
/// the six worker threads before (each kept its own allocator arena).
const KEPT_KB: u64 = 16 * 1024;

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

/// The fake, in a process of its own: run only when the measuring test
/// starts it (it prints its port and waits until its input closes).
#[tokio::test]
#[ignore = "started by calls_and_checks_cost_bounded_memory"]
async fn fake_server_process() {
    let Ok(dir) = std::env::var("HARNESS_FAKE_DIR") else {
        return;
    };
    let fake = fake_mcp::start(std::path::Path::new(&dir)).await;
    std::fs::write(std::path::Path::new(&dir).join("cert.der"), &fake.cert_der).unwrap();
    println!("PORT {}", fake.port);
    let _ = tokio::task::spawn_blocking(|| {
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn calls_and_checks_cost_bounded_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "fake_server_process", "--nocapture"])
        .env("HARNESS_FAKE_DIR", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let port: u16 = loop {
        let mut line = String::new();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "the fake did not start"
        );
        if let Some(p) = line.trim().strip_prefix("PORT ") {
            break p.parse().unwrap();
        }
    };
    let cert = std::fs::read(dir.path().join("cert.der")).unwrap();
    let a: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let table = std::collections::HashMap::from([("mcp.test".to_string(), vec![a])]);
    let net = Net {
        lookup: harness_master::remote::Lookup::table(table),
        allow_private: true,
        roots: vec![cert],
        timeout: Duration::from_secs(8),
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (th0, idle) = (threads(), vm("VmRSS:"));

    // One plain check first: what the first use costs at all (the TLS
    // stack, the connector thread and its runtime), apart from the load.
    // One connector thread for both pools, as the master has it.
    let thread = std::sync::Arc::new(harness_master::connectors::pool::Thread::default());
    let cpool = Pool::on(thread.clone(), harness_master::connectors::pool::WORKERS, 1);
    let c = connectors::check(&cpool, "first", "conn", "https://mcp.test/ok", None, &net).await;
    assert!(c.unwrap().result.is_ok());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let first = vm("VmRSS:");

    // Checks: the step-1 load.
    let check = |who: String, path: &'static str| {
        let (pool, net) = (&cpool, net.clone());
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
    for i in 0..10 {
        for path in ["tiny", "dense", "smalltools"] {
            let _ = check(format!("s{i}"), path).await;
        }
    }
    for round in 0..3 {
        futures::future::join_all((0..20).map(|i| {
            check(
                format!("p{round}-{i}"),
                if i % 2 == 0 { "tiny" } else { "smalltools" },
            )
        }))
        .await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_checks = vm("VmRSS:");

    // 200 calls: 120 one after another, 80 twenty at once.
    let pool = Pool::on(thread.clone(), call::WORKERS, call::PER_KEY);
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
    let mut ok = 0;
    for i in 0..40 {
        for path in ["callfull", "callbig", "callbomb"] {
            if matches!(one(format!("s{i}"), path, i).await, Ok(Some(Ok(_)))) {
                ok += 1;
            }
        }
    }
    assert_eq!(ok, 40, "every full result was read");
    for round in 0..4 {
        let all = futures::future::join_all((0..20).map(|i| {
            one(
                format!("p{round}-{i}"),
                if i % 2 == 0 { "callfull" } else { "callbig" },
                i,
            )
        }))
        .await;
        let ran = all.iter().filter(|r| r.is_ok()).count();
        assert!(
            (1..=call::WORKERS).contains(&ran),
            "round {round}: {ran} ran"
        );
    }
    assert!(pool.most_at_once() <= call::WORKERS);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let after_calls = vm("VmRSS:");
    let wait: u64 = std::env::var("HARNESS_MEMORY_WAIT_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    tokio::time::sleep(Duration::from_secs(wait)).await;
    let (th1, later, hwm) = (threads(), vm("VmRSS:"), vm("VmHWM:"));
    drop(child.stdin.take());
    let _ = child.wait();
    eprintln!(
        "MEMORY idle {idle} kB; after one plain check {first} (+{}); after checks {after_checks} \
         (+{}); after 200 calls {after_calls} (+{}); {wait} s later {later} (+{}); VmHWM {hwm}; \
         threads {th0} -> {th1}",
        first.saturating_sub(idle),
        after_checks.saturating_sub(idle),
        after_calls.saturating_sub(idle),
        later.saturating_sub(idle)
    );
    // The one connector thread and at most its two lookup threads.
    assert!(th1 <= th0 + 3, "threads {th0} -> {th1}");
    assert!(
        later.saturating_sub(idle) < KEPT_KB,
        "kept {} kB after use",
        later.saturating_sub(idle)
    );
}
