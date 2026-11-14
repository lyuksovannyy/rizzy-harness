//! The MCP client against a fake server (`support/fake_mcp.py`), started
//! the way a chat starts one but without a sandbox (`Confinement::
//! Unconfined`; the contained path is `cli/tests/mcp_sandbox.rs`): its tools
//! are listed, named and bounded; calls go through; a server that hangs,
//! floods, dies or lists too much is cut with a note; it sees only the
//! allow-listed environment; it is gone after a stop.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::policy::ToolRisk;
use harness_tools::mcp::{self, ServerConfig, SessionMcp};
use harness_tools::{Tool, ToolCtx};
use serde_json::json;

fn script() -> String {
    format!("{}/tests/support/fake_mcp.py", env!("CARGO_MANIFEST_DIR"))
}

fn server(args: &[&str]) -> ServerConfig {
    let mut all = vec![script()];
    all.extend(args.iter().map(|a| a.to_string()));
    ServerConfig {
        args: all,
        ..ServerConfig::new("python3", &[])
    }
}

fn set(cfg: ServerConfig) -> Arc<SessionMcp> {
    let mut m = BTreeMap::new();
    m.insert("fake".to_string(), cfg);
    SessionMcp::new(&m).unwrap()
}

fn ctx(dir: &tempfile::TempDir) -> ToolCtx {
    ToolCtx::new(dir.path())
}

fn tool<'a>(tools: &'a [Arc<dyn Tool>], name: &str) -> &'a Arc<dyn Tool> {
    tools
        .iter()
        .find(|t| t.name() == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        !s.rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z')
    })
}

fn pids(started: &std::path::Path) -> Vec<i32> {
    std::fs::read_to_string(started)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("started ")?.parse().ok())
        .collect()
}

#[tokio::test]
async fn tools_are_listed_under_prefixed_names_and_always_ask() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&[]));
    let l = s.load(&ctx(&dir)).await;
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    let echo = tool(&l.tools, "mcp__fake__echo");
    assert!(echo.description().contains("Repeat the text."));
    assert!(echo.description().contains("MCP server `fake`"));
    assert_eq!(echo.schema()["properties"]["text"]["type"], "string");
    // The server says `readOnlyHint: true`; the harness does not believe it.
    assert_eq!(echo.risk(), ToolRisk::Mutating);
    for t in &l.tools {
        assert!(t.name().starts_with(mcp::PREFIX), "{}", t.name());
        assert_eq!(t.risk(), ToolRisk::Mutating);
    }
    let r = echo.run(&ctx(&dir), json!({"text": "hi there"})).await;
    assert!(r.ok, "{}", r.output);
    assert_eq!(r.output, "hi there");
    let f = tool(&l.tools, "mcp__fake__fail")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(!f.ok && f.output.contains("it went wrong"), "{}", f.output);
    let i = tool(&l.tools, "mcp__fake__image")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(
        i.output.contains("image from MCP server `fake` (image/png"),
        "{}",
        i.output
    );
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

#[tokio::test]
async fn a_huge_tool_list_and_a_huge_schema_are_cut_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&["--tools", "100", "--big-schema", "--page", "7"]));
    let l = s.load(&ctx(&dir)).await;
    assert_eq!(l.tools.len(), mcp::MAX_TOOLS);
    let all = l.notes.join("\n");
    assert!(all.contains("are not offered in this chat"), "{all}");
    assert!(all.contains("enabled_tools"), "{all}");
    assert!(
        all.contains("their input schema is over 8000 bytes"),
        "{all}"
    );
    assert!(all.contains("big"), "{all}");
    assert!(!l.tools.iter().any(|t| t.name() == "mcp__fake__big"));
    // Shown once; the next turn's load does not repeat them.
    assert!(s.load(&ctx(&dir)).await.notes.is_empty());
    // node.toml's lists choose.
    let mut only = server(&["--tools", "100"]);
    only.enabled_tools = Some(vec!["t99".into(), "echo".into()]);
    only.disabled_tools = vec!["echo".into()];
    let l = set(only).load(&ctx(&dir)).await;
    let names: Vec<&str> = l.tools.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["mcp__fake__t99"]);
}

#[tokio::test]
async fn a_server_that_hangs_at_start_is_cut_at_its_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let mut cfg = server(&["--hang-init", "--started", started.to_str().unwrap()]);
    cfg.startup_timeout_sec = 1;
    let s = set(cfg);
    let t = Instant::now();
    let l = s.load(&ctx(&dir)).await;
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(l.tools.is_empty());
    assert!(
        l.notes.join("").contains("did not answer within 1 s"),
        "{:?}",
        l.notes
    );
    // The half-started process is gone.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for p in pids(&started) {
        assert!(!alive(p), "{p} still runs");
    }
}

#[tokio::test]
async fn a_call_that_hangs_is_cut_at_its_timeout_and_the_server_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let mut cfg = server(&["--started", started.to_str().unwrap()]);
    cfg.tool_timeout_sec = 1;
    let s = set(cfg);
    let l = s.load(&ctx(&dir)).await;
    let t = Instant::now();
    let r = tool(&l.tools, "mcp__fake__hang")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(!r.ok);
    assert!(r.output.contains("no answer within 1 s"), "{}", r.output);
    let first = pids(&started)[0];
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!alive(first));
    // The next call starts it again.
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "back"}))
        .await;
    assert!(r.ok && r.output == "back", "{}", r.output);
    assert_eq!(pids(&started).len(), 2);
}

#[tokio::test]
async fn a_huge_result_is_cut_and_a_message_over_the_cap_fails_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&[]));
    let l = s.load(&ctx(&dir)).await;
    let r = tool(&l.tools, "mcp__fake__huge")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(r.ok && r.truncated, "{}", r.output.len());
    assert!(r.output.contains("[... output truncated ...]"));
    assert!(r.output.chars().count() < harness_tools::MAX_OUTPUT_CHARS + 100);
    let t = Instant::now();
    let r = tool(&l.tools, "mcp__fake__flood")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
    assert!(!r.ok);
    assert!(r.output.contains("larger than 8 MiB"), "{}", r.output);
}

#[tokio::test]
async fn a_server_that_dies_is_an_error_and_is_restarted_a_bounded_number_of_times() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let s = set(server(&["--started", started.to_str().unwrap()]));
    let l = s.load(&ctx(&dir)).await;
    let die = tool(&l.tools, "mcp__fake__die");
    let mut last = String::new();
    for _ in 0..=mcp::MAX_RESTARTS {
        let r = die.run(&ctx(&dir), json!({})).await;
        assert!(!r.ok);
        last = r.output;
    }
    assert!(last.contains("told to die"), "{last}");
    assert!(last.contains("not started again"), "{last}");
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "x"}))
        .await;
    assert!(
        !r.ok && r.output.contains("not started again"),
        "{}",
        r.output
    );
    assert_eq!(pids(&started).len() as u32, mcp::MAX_RESTARTS + 1);
}

#[tokio::test]
async fn the_server_sees_only_the_allow_listed_environment() {
    // Cargo sets these for the test process; none is on the core list.
    assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
    assert!(std::env::var("CARGO_PKG_VERSION").is_ok());
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = server(&[]);
    cfg.env_vars = vec!["CARGO_PKG_*".into()];
    cfg.env = serde_json::from_value(json!({"FAKE_TOKEN": "tok-value-1"})).unwrap();
    let s = set(cfg);
    // The node's deny-list (provider keys, `.env` names) wins over
    // `env_vars`, as over `env_pass`.
    let c = ToolCtx {
        env: harness_tools::ToolEnv::new(&[], ["CARGO_PKG_VERSION".to_string()]).unwrap(),
        ..ctx(&dir)
    };
    let l = s.load(&c).await;
    let r = tool(&l.tools, "mcp__fake__env").run(&c, json!({})).await;
    assert!(r.ok, "{}", r.output);
    let names: Vec<&str> = r
        .output
        .lines()
        .filter_map(|l| l.split('=').next())
        .collect();
    assert!(names.contains(&"PATH"), "{names:?}");
    assert!(names.contains(&"CARGO_PKG_NAME"), "{names:?}");
    assert!(r.output.contains("FAKE_TOKEN=tok-value-1"));
    assert!(!names.contains(&"CARGO_MANIFEST_DIR"), "{names:?}");
    assert!(!names.contains(&"CARGO_PKG_VERSION"), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("HARNESS_")),
        "{names:?}"
    );
}

#[tokio::test]
async fn a_stop_or_idle_time_ends_the_process_and_a_closed_set_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let s = set(server(&["--started", started.to_str().unwrap()]));
    let l = s.load(&ctx(&dir)).await;
    assert_eq!(s.running(), 1);
    let first = pids(&started)[0];
    assert!(alive(first));
    // Idle: stopped, started again at the next use, not counted.
    s.stop_idle(Duration::ZERO);
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
    assert!(!alive(first));
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "again"}))
        .await;
    assert!(r.ok, "{}", r.output);
    let second = pids(&started)[1];
    assert!(alive(second));
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
    assert!(!alive(second));
    assert_eq!(s.running(), 0);
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "x"}))
        .await;
    assert!(!r.ok, "{}", r.output);
    assert_eq!(pids(&started).len(), 2);
    // Dropping a chat's set stops its server too.
    let started2 = dir.path().join("started2");
    let s2 = set(server(&["--started", started2.to_str().unwrap()]));
    s2.load(&ctx(&dir)).await;
    let p = pids(&started2)[0];
    drop(s2);
    let by = Instant::now() + mcp::STOP_WAIT;
    while alive(p) && Instant::now() < by {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(p));
}
