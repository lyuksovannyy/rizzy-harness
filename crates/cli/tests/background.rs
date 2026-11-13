//! Background commands (ADOPTION_PLAN T.8d step 1, docs/background-commands.md
//! §7 tests 1 to 4 and 7 to 10, §8 step 1): a `bash` call with
//! `background: true` keeps running after its call, as the same contained
//! command, held by its session's table; its end is recorded once by the
//! session actor; it dies with its timeout, its stop, its session, its
//! owner's access and the node.
//!
//! Through the real `rizzy-harness sandbox-exec` wrapper, the real `bwrap`,
//! the real bash tool and (for the session tests) a real node runtime
//! driven by the scripted agent. Where bubblewrap does not work they skip
//! with a printed reason; with `HARNESS_REQUIRE_BWRAP=1` they fail.
//!
//! Processes are found from outside by their unique `sleep <n>` command
//! line and killed by exactly that pid on drop. Servers the tests start
//! listen on free ports of 127.0.0.1 and are reached only from here.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use harness_tools::background::{Access, Ended, Limits, Registry, SessionBg};
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, BashOutput, BashStop, Tool, ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
    }
}

impl Fixture {
    fn plain(&self, settings: &Settings) -> Sandbox {
        Sandbox::new(
            PathBuf::from(EXE),
            settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// The contained sandbox, or `None` where it cannot run here.
    fn sandbox_with(&self, settings: &Settings) -> Option<Arc<Sandbox>> {
        let sb = self.plain(settings);
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        let sb = bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain")).ok()?;
        Some(Arc::new(sb))
    }

    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        self.sandbox_with(&Settings::default())
    }

    fn ctx(&self, sb: Arc<Sandbox>, bg: &Arc<SessionBg>) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        ctx.background = Access::On(bg.clone());
        ctx
    }
}

fn table(limits: Limits) -> (Arc<SessionBg>, mpsc::UnboundedReceiver<Ended>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (Registry::new(limits).session(1, tx), rx)
}

// ---- the test's own processes, found from outside ----

fn alive(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let state = stat.rsplit(')').next().unwrap_or("").trim_start();
            !(state.starts_with('Z') || state.starts_with('X') || state.starts_with('x'))
        }
        Err(_) => false,
    }
}

fn sleeps(secs: u32) -> Vec<i32> {
    let want = format!("sleep\0{secs}\0");
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == want.as_bytes())
                && alive(*pid)
        })
        .collect()
}

struct Reaper(Vec<u32>);

impl Drop for Reaper {
    fn drop(&mut self) {
        for secs in &self.0 {
            for pid in sleeps(*secs) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }
}

/// The T.8a escapers: `setsid`, `nohup … &`, a double fork with `setsid`,
/// a child that ignores SIGTERM, and a plain background child.
fn detached(base: u32) -> (String, Reaper) {
    let s: Vec<u32> = (0..5).map(|i| base + i).collect();
    let cmd = format!(
        "setsid sleep {} & nohup sleep {} >/dev/null 2>&1 & ( setsid sleep {} & ) ; \
         sh -c 'trap \"\" TERM; exec sleep {}' & sleep {} & ",
        s[0], s[1], s[2], s[3], s[4]
    );
    (cmd, Reaper(s))
}

async fn all_started(r: &Reaper) {
    let by = Instant::now() + Duration::from_secs(20);
    while r.0.iter().any(|s| sleeps(*s).is_empty()) {
        assert!(Instant::now() < by, "the detached children never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn none_left(r: &Reaper, how: &str) {
    let by = Instant::now() + Duration::from_secs(2);
    loop {
        let left: Vec<(u32, Vec<i32>)> =
            r.0.iter()
                .map(|s| (*s, sleeps(*s)))
                .filter(|(_, p)| !p.is_empty())
                .collect();
        if left.is_empty() {
            return;
        }
        assert!(
            Instant::now() < by,
            "{how}: still running 2 s later (sleep secs, pids): {left:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn run(tool: &dyn Tool, ctx: &ToolCtx, input: Value) -> ToolOutcome {
    tool.run(ctx, input).await
}

async fn next_end(rx: &mut mpsc::UnboundedReceiver<Ended>, within: Duration) -> Ended {
    tokio::time::timeout(within, rx.recv())
        .await
        .expect("no end within the bound")
        .expect("the table closed")
}

// ---- the tool ----

/// A background start returns after its first wait with the id, the
/// deadline and the output so far, and the command keeps running; on the
/// branch head the flag was ignored and the call ran to its end.
#[tokio::test]
async fn a_background_start_returns_and_keeps_running() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, mut ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let _r = Reaper(vec![975_001]);
    let t = Instant::now();
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "echo first; sleep 975001", "background": true, "timeout_secs": 600}),
    )
    .await;
    let took = t.elapsed();
    assert!(
        took < Duration::from_secs(6),
        "the call took {took:?}: {}",
        r.output
    );
    assert!(r.ok, "{}", r.output);
    assert!(r.output.contains("first"), "{}", r.output);
    assert!(r.output.contains("bg-1"), "{}", r.output);
    assert!(
        r.output.contains("at the latest (600 s"),
        "the deadline in words: {}",
        r.output
    );
    let st = r.background.clone().expect("a background start");
    assert_eq!((st.bg_id.as_str(), st.timeout_s), ("bg-1", 600));
    assert!(!sleeps(975_001).is_empty(), "it stopped with its call");
    // bash_output reads it; the list names it.
    let o = run(&BashOutput, &ctx, json!({"id": "bg-1"})).await;
    assert!(
        o.ok && o.output.contains("first") && o.output.contains("running"),
        "{}",
        o.output
    );
    let l = run(&BashOutput, &ctx, json!({})).await;
    assert!(
        l.output.contains("bg-1") && l.output.contains("sleep 975001"),
        "{}",
        l.output
    );
    // bash_stop ends it with the guard's confirmation, recorded once.
    let s = run(&BashStop, &ctx, json!({"id": "bg-1"})).await;
    assert!(
        s.ok && s.output.contains("confirmed the end"),
        "{}",
        s.output
    );
    let e = next_end(&mut ends, Duration::from_secs(10)).await;
    assert_eq!(e.reason, BackgroundReason::StoppedByAgent);
    assert_eq!(e.confirmed, BackgroundConfirmed::Ended);
    assert!(sleeps(975_001).is_empty());
}

/// One that ends within the first wait is an ordinary result: nothing is
/// tracked, no id is given.
#[tokio::test]
async fn a_quick_background_command_is_an_ordinary_result() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, mut ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "echo nope >&2; exit 4", "background": true, "timeout_secs": 60}),
    )
    .await;
    assert_eq!(r.exit_code, Some(4), "{}", r.output);
    assert!(
        r.background.is_none() && !r.output.contains("bg-"),
        "{}",
        r.output
    );
    assert!(bg.running().is_empty());
    assert!(ends.try_recv().is_err());
}

/// The owner's rule (2026-11-13): a background command must state its
/// timeout. Refused before anything starts, naming the range.
#[tokio::test]
async fn a_background_start_without_a_timeout_is_refused() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, _ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let _r = Reaper(vec![975_011]);
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "sleep 975011", "background": true}),
    )
    .await;
    assert!(!r.ok && r.background.is_none(), "{}", r.output);
    assert!(
        r.output.contains("timeout_secs") && r.output.contains("from 5 to 3600 seconds"),
        "{}",
        r.output
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(sleeps(975_011).is_empty(), "a process started");
    assert!(bg.running().is_empty());
    // With it, it starts.
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "sleep 975011", "background": true, "timeout_secs": 30}),
    )
    .await;
    assert!(r.background.is_some(), "{}", r.output);
    bg.stop_all_within(BackgroundReason::StoppedByAgent, Duration::from_secs(10))
        .await;
}

/// §7 test 1: the T.8a escapers are all gone 2 s after the background
/// command's stop, its timeout and its session's table going (delete).
#[tokio::test]
async fn escapers_end_with_a_background_stop_timeout_and_delete() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    for (how, base) in [("stop", 975_100), ("timeout", 975_110), ("delete", 975_120)] {
        let (bg, mut ends) = table(Limits::default());
        let ctx = f.ctx(sb.clone(), &bg);
        let (cmd, reaper) = detached(base);
        let timeout = if how == "timeout" { 5 } else { 600 };
        let r = run(
            &Bash,
            &ctx,
            json!({"command": format!("{cmd} sleep 1000"), "background": true, "timeout_secs": timeout}),
        )
        .await;
        assert!(r.background.is_some(), "{how}: {}", r.output);
        all_started(&reaper).await;
        let e = match how {
            "stop" => {
                let s = run(&BashStop, &ctx, json!({"id": "bg-1"})).await;
                assert!(s.ok, "{}", s.output);
                next_end(&mut ends, Duration::from_secs(10)).await
            }
            "timeout" => next_end(&mut ends, Duration::from_secs(20)).await,
            _ => {
                drop(ctx);
                // The table goes with its session; its end is not
                // recorded (the log goes too), and the task stops it.
                let w = Arc::downgrade(&bg);
                drop(bg);
                assert!(w.upgrade().is_none());
                // No end is sent: there is no session to record it.
                let got = tokio::time::timeout(Duration::from_secs(10), ends.recv()).await;
                assert!(matches!(got, Ok(None)), "{got:?}");
                none_left(&reaper, how).await;
                continue;
            }
        };
        if how == "timeout" {
            assert_eq!(e.reason, BackgroundReason::TimedOut, "{e:?}");
        }
        assert_eq!(e.confirmed, BackgroundConfirmed::Ended, "{how}: {e:?}");
        none_left(&reaper, how).await;
    }
}

/// §7 test 1, `kill -9` of the node: this binary re-run as a helper holds a
/// background command and is SIGKILLed; nothing of it is left 2 s later.
#[tokio::test]
async fn escapers_of_a_background_command_end_when_the_node_is_killed() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let (_, reaper) = detached(975_140);
    let mut helper = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "helper_holds_a_background_command",
            "--nocapture",
        ])
        .env("T8D_HELPER", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    all_started(&reaper).await;
    helper.kill().unwrap(); // SIGKILL
    let _ = helper.wait();
    none_left(&reaper, "kill -9 of the node").await;
}

#[tokio::test]
async fn helper_holds_a_background_command() {
    if std::env::var_os("T8D_HELPER").is_none() {
        return;
    }
    let f = fixture();
    let sb = f.sandbox().expect("checked by the parent");
    let (bg, _ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let (cmd, reaper) = detached(975_140);
    std::mem::forget(reaper);
    let r = run(
        &Bash,
        &ctx,
        json!({"command": format!("{cmd} sleep 1000"), "background": true, "timeout_secs": 600}),
    )
    .await;
    assert!(r.background.is_some(), "{}", r.output);
    tokio::time::sleep(Duration::from_secs(600)).await;
}

/// §7 test 2: asked above the ceiling, it runs to the ceiling and no
/// further; `timeout_secs: 2` is clamped to 5 s (foreground too).
#[tokio::test]
async fn the_ceiling_and_the_floor_hold_for_both_kinds() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, mut ends) = table(Limits::default());
    let mut ctx = f.ctx(sb, &bg);
    ctx.bash_timeout_max = Duration::from_secs(6);
    let t = Instant::now();
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "sleep 1000", "background": true, "timeout_secs": 99_999}),
    )
    .await;
    assert_eq!(
        r.background.as_ref().map(|b| b.timeout_s),
        Some(6),
        "{}",
        r.output
    );
    let e = next_end(&mut ends, Duration::from_secs(20)).await;
    assert_eq!(e.reason, BackgroundReason::TimedOut);
    let ran = t.elapsed();
    assert!(
        ran >= Duration::from_secs(6) && ran < Duration::from_secs(12),
        "ran {ran:?}"
    );
    ctx.bash_timeout_max = Duration::from_secs(3600);
    let t = Instant::now();
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "sleep 1000", "timeout_secs": 2}),
    )
    .await;
    let took = t.elapsed();
    assert!(r.output.contains("timed out after 5s"), "{}", r.output);
    assert!(
        took >= Duration::from_secs(5) && took < Duration::from_secs(10),
        "{took:?}"
    );
}

/// §7 test 3: a full table refuses, naming the running ids, and nothing is
/// evicted; the node's cap spans chats and names only this chat's ids.
/// §7 test 4: no argument the model writes raises a bound.
#[tokio::test]
async fn full_tables_refuse_and_name_the_ids() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let reg = Registry::new(Limits {
        per_session: 2,
        per_node: 3,
        output_kb: 16,
    });
    let (tx, _rx) = mpsc::unbounded_channel();
    let a = reg.session(1, tx.clone());
    let b = reg.session(1, tx);
    let ctx_a = f.ctx(sb.clone(), &a);
    let ctx_b = f.ctx(sb, &b);
    let _r = Reaper(vec![975_201, 975_202, 975_203]);
    let start = |s: u32| {
        json!({"command": format!("sleep {s}"), "background": true, "timeout_secs": 600,
               // What the model might write to raise its own bounds.
               "background_per_session": 100, "background_per_node": 100,
               "limits": {"background_per_session": 100}})
    };
    assert!(
        run(&Bash, &ctx_a, start(975_201))
            .await
            .background
            .is_some()
    );
    assert!(
        run(&Bash, &ctx_a, start(975_202))
            .await
            .background
            .is_some()
    );
    let r = run(&Bash, &ctx_a, start(975_299)).await;
    assert!(!r.ok && r.background.is_none(), "{}", r.output);
    assert!(
        r.output.contains("bg-1, bg-2") && r.output.contains("background_per_session = 2"),
        "{}",
        r.output
    );
    assert!(r.output.contains("nothing was stopped"), "{}", r.output);
    assert_eq!(a.running(), vec!["bg-1", "bg-2"], "something was evicted");
    assert!(
        run(&Bash, &ctx_b, start(975_203))
            .await
            .background
            .is_some()
    );
    let r = run(&Bash, &ctx_b, start(975_298)).await;
    assert!(r.output.contains("background_per_node = 3"), "{}", r.output);
    assert!(
        r.output.contains("this chat's: bg-1") && !r.output.contains("bg-2"),
        "another chat's ids are not named: {}",
        r.output
    );
    assert_eq!(reg.running(), 3);
    for t in [&a, &b] {
        t.stop_all_within(BackgroundReason::StoppedByAgent, Duration::from_secs(10))
            .await;
    }
    assert_eq!(reg.running(), 0);
}

/// §7 test 7 (the node's half; the sub-agent's is in the loop): with the
/// feature off, on an uncontained node and without a table, a background
/// start is refused and starts nothing.
#[tokio::test]
async fn off_uncontained_and_unavailable_refuse() {
    let f = fixture();
    let _r = Reaper(vec![975_301]);
    let input = json!({"command": "sleep 975301", "background": true, "timeout_secs": 60});
    // Uncontained: the Landlock-only shell, `contain = "off"` alike.
    let (bg, _e) = table(Limits::default());
    let mut ctx = ToolCtx::new(&f.ws);
    ctx.background = Access::On(bg.clone());
    let r = run(&Bash, &ctx, input.clone()).await;
    assert!(r.output.contains("containment"), "{}", r.output);
    // Off.
    if let Some(sb) = f.sandbox() {
        let (off, _e) = table(Limits {
            per_session: 0,
            ..Default::default()
        });
        let r = run(&Bash, &f.ctx(sb, &off), input.clone()).await;
        assert!(
            r.output.contains("background_per_session = 0"),
            "{}",
            r.output
        );
    }
    // A sub-agent's session gets no table.
    let mut ctx = ToolCtx::new(&f.ws);
    ctx.background = Access::Off(harness_tools::background::SUB_AGENT.into());
    let r = run(&Bash, &ctx, input).await;
    assert!(r.output.contains("sub-agent"), "{}", r.output);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(sleeps(975_301).is_empty());
    assert!(bg.running().is_empty());
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap()
}

/// §7 test 8: a command that prints without end holds its ring and no
/// more: the ring at `background_output_kb`, this process's memory not
/// growing with the flood.
#[tokio::test(flavor = "multi_thread")]
async fn a_flood_holds_its_ring_and_no_more() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, mut ends) = table(Limits {
        output_kb: 64,
        ..Default::default()
    });
    let ctx = f.ctx(sb, &bg);
    let before = rss_kib();
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "yes 0123456789abcdef", "background": true, "timeout_secs": 15}),
    )
    .await;
    assert!(r.background.is_some(), "{}", r.output);
    let mut peak = 0;
    let by = Instant::now() + Duration::from_secs(10);
    while Instant::now() < by {
        peak = peak.max(rss_kib());
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let e = next_end(&mut ends, Duration::from_secs(20)).await;
    eprintln!(
        "flood: {} bytes printed; this process {} KiB before, {} KiB at most",
        e.output_bytes, before, peak
    );
    assert!(
        e.output_bytes > 200 << 20,
        "too slow a flood to prove anything: {}",
        e.output_bytes
    );
    assert!(e.tail.len() <= harness_tools::background::TAIL_IN_EVENT && e.tail_cut);
    assert!(e.tail.contains("0123456789abcdef"));
    let o = run(&BashOutput, &ctx, json!({"id": "bg-1"})).await;
    assert!(
        o.output.contains("bytes not shown"),
        "{}",
        &o.output[..200.min(o.output.len())]
    );
    assert!(
        peak.saturating_sub(before) < 32 * 1024,
        "memory grew {} KiB with the flood",
        peak - before
    );
}

/// §7 test 8 and §8: the end is recorded exactly once whatever ends it,
/// with the exit, the timeout and the stop racing.
#[tokio::test(flavor = "multi_thread")]
async fn the_end_is_recorded_once_when_exit_timeout_and_stop_race() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, mut ends) = table(Limits::default());
    let ctx = Arc::new(f.ctx(sb, &bg));
    // It exits at 5 s; its timeout is 5 s; the stop comes at 5 s too.
    let mut n = 0;
    for _ in 0..3 {
        let r = run(
            &Bash,
            &ctx,
            json!({"command": "sleep 5", "background": true, "timeout_secs": 5}),
        )
        .await;
        let id = r.background.expect("started").bg_id;
        n += 1;
        tokio::time::sleep(Duration::from_millis(1950)).await;
        let stops: Vec<_> = (0..4)
            .map(|_| {
                let (ctx, id) = (ctx.clone(), id.clone());
                tokio::spawn(async move { BashStop.run(&ctx, json!({ "id": id })).await })
            })
            .collect();
        for s in stops {
            let _ = s.await;
        }
    }
    let mut got = Vec::new();
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(12), ends.recv()).await {
        got.push(e.bg_id);
        if got.len() > n {
            break;
        }
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    while let Ok(e) = ends.try_recv() {
        got.push(e.bg_id);
    }
    got.sort();
    assert_eq!(got, vec!["bg-1", "bg-2", "bg-3"], "each end once");
}

// ---- a node with a session, driven by the scripted agent ----

struct Node {
    _f: Fixture,
    ws: PathBuf,
    rt: harness_node::NodeRuntime,
    link: harness_node::NodeLink,
    sid: SessionId,
    seen: Vec<RawEvent>,
    n: u32,
}

async fn node_with(f: Fixture, sb: Arc<Sandbox>, secrets: Vec<String>) -> Node {
    use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
    use harness_protocol::{MasterToNode, NodeToMaster};
    let store =
        harness_store::Store::open(&f.data.join("node.db"), harness_store::Durability::Full)
            .unwrap();
    let secrets =
        harness_core::SecretSet::new(secrets.into_iter().map(harness_core::Secret::new).collect());
    let policy = harness_node::NodePolicy {
        workspace_roots: vec![f.root.clone()],
        max_approval_mode: harness_core::policy::ApprovalMode::Auto,
        ..Default::default()
    };
    let backend = NativeBackend::new(
        Arc::new(Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig {
            sandbox: Confinement::Confined {
                sandbox: sb,
                tmp: None,
            },
            secrets: secrets.clone(),
            ..Default::default()
        },
    );
    let rt = harness_node::NodeRuntime::start(harness_node::NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy,
        store,
        backend: Arc::new(backend),
        default_model: "mock".into(),
        secrets,
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(f.data.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = rt.attach().await;
    assert!(matches!(
        link.from_node.recv().await.unwrap(),
        NodeToMaster::Hello { .. }
    ));
    let sid = SessionId::new();
    let ws = f.ws.clone();
    let mut node = Node {
        _f: f,
        ws,
        rt,
        link,
        sid,
        seen: vec![],
        n: 0,
    };
    node.send(MasterToNode::Create {
        profile: None,
        incognito: false,
        agent: None,
        req_id: "c".into(),
        session_id: sid,
        title: "t".into(),
        cwd: node.ws.display().to_string(),
        model: None,
        approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
    });
    node.until(|k| matches!(k, EventKind::SessionCreated(_)))
        .await;
    node
}

impl Node {
    fn send(&self, m: harness_protocol::MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn until(
        &mut self,
        pred: impl Fn(&EventKind) -> bool,
    ) -> Vec<harness_protocol::NodeToMaster> {
        use harness_protocol::NodeToMaster;
        let mut acks = vec![];
        loop {
            let m = tokio::time::timeout(Duration::from_secs(90), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            match m {
                NodeToMaster::Event { event } => {
                    let k = event.typed().unwrap();
                    self.seen.push(event);
                    if pred(&k) {
                        return acks;
                    }
                }
                other => acks.push(other),
            }
        }
    }

    /// One turn: the scripted agent makes these calls, one per step.
    async fn turn(&mut self, calls: Value) {
        self.prompt(&format!("script: {calls}"));
        self.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    }

    fn prompt(&mut self, text: &str) {
        self.n += 1;
        self.send(harness_protocol::MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{}", self.n),
            session_id: self.sid,
            text: text.into(),
            images: vec![],
        });
    }

    fn kinds(&self) -> Vec<EventKind> {
        self.seen.iter().map(|e| e.typed().unwrap()).collect()
    }

    fn results(&self) -> Vec<String> {
        self.kinds()
            .into_iter()
            .filter_map(|k| match k {
                EventKind::ToolCallFinished(f) => Some(f.output),
                _ => None,
            })
            .collect()
    }

    fn ends(&self) -> Vec<BackgroundEnded> {
        self.kinds()
            .into_iter()
            .filter_map(|k| match k {
                EventKind::BackgroundEnded(b) => Some(b),
                _ => None,
            })
            .collect()
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A tiny HTTP server in Python, if there is one (else the tests that need
/// it say so and skip).
fn server_cmd(port: u16) -> Option<String> {
    let py = ["/usr/bin/python3", "/bin/python3"]
        .into_iter()
        .find(|p| Path::new(p).exists())?;
    Some(format!("exec {py} -m http.server {port} --bind 127.0.0.1"))
}

/// §8: a dev server started in one step answers a request in the next
/// step of the same turn, and in a later turn (the network on). The end
/// comes as a note at a step boundary of the turn running then, recorded
/// once, after `background_started` and after that call's result.
#[tokio::test(flavor = "multi_thread")]
async fn a_dev_server_answers_in_the_next_step_and_a_later_turn() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let port = free_port();
    let Some(server) = server_cmd(port) else {
        assert!(!bwrap::required(), "no python3 for the test server");
        eprintln!("SKIPPED: no python3 for the test server");
        return;
    };
    std::fs::write(f.ws.join("hello.txt"), "hello from the folder\n").unwrap();
    let mut n = node_with(f, sb, vec![]).await;
    let curl = format!(
        "for i in $(seq 50); do exec 3<>/dev/tcp/127.0.0.1/{port} 2>/dev/null && break; sleep 0.1; done; \
         printf 'GET /hello.txt HTTP/1.0\\r\\n\\r\\n' >&3; cat <&3"
    );
    n.turn(json!([
        {"tool": "bash", "input": {"command": server, "background": true, "timeout_secs": 120}},
        {"tool": "bash", "input": {"command": curl}},
    ]))
    .await;
    let r = n.results();
    assert!(
        r[0].contains("running in the background as bg-1"),
        "{}",
        r[0]
    );
    assert!(
        r[1].contains("hello from the folder"),
        "the next step: {}",
        r[1]
    );
    // A later turn.
    n.turn(json!([{"tool": "bash", "input": {"command": curl}}]))
        .await;
    let r = n.results();
    assert!(
        r[2].contains("hello from the folder"),
        "a later turn: {}",
        r[2]
    );
    // The log: started before the start call's result.
    let kinds = n.kinds();
    let started = kinds
        .iter()
        .position(|k| matches!(k, EventKind::BackgroundStarted(b) if b.bg_id == "bg-1"))
        .expect("background_started");
    let finished = kinds
        .iter()
        .position(|k| matches!(k, EventKind::ToolCallFinished(f) if f.output.contains("bg-1")))
        .unwrap();
    assert!(started < finished);
    // A restore is refused while it runs (§7, R17).
    let turn = kinds
        .iter()
        .find_map(|k| match k {
            EventKind::TurnStarted(t) => Some(t.turn_id.clone()),
            _ => None,
        })
        .unwrap();
    n.send(harness_protocol::MasterToNode::Restore {
        req_id: "r1".into(),
        session_id: n.sid,
        turn_id: turn,
        force: true,
        rewind: true,
    });
    let acks = n.until_ack().await;
    assert!(acks.contains("bg-1") && acks.contains("running"), "{acks}");
    // The agent stops it in a third turn; its end is recorded, and the
    // model is not told again (its stop's result was the end).
    n.turn(json!([{"tool": "bash_stop", "input": {"id": "bg-1"}}]))
        .await;
    let r = n.results();
    assert!(r[3].contains("confirmed the end"), "{}", r[3]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    n.drain().await;
    let ends = n.ends();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0].reason, BackgroundReason::StoppedByAgent);
}

impl Node {
    async fn until_ack(&mut self) -> String {
        use harness_protocol::NodeToMaster;
        loop {
            let m = tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
                .await
                .expect("timed out")
                .expect("link closed");
            match m {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::Ack { ok, error, .. } if !ok => return error.unwrap_or_default(),
                NodeToMaster::Ack { req_id, .. }
                    if req_id.starts_with('r') || req_id.starts_with('s') =>
                {
                    return String::from("ok");
                }
                _ => {}
            }
        }
    }

    /// Take what the node sent so far.
    async fn drain(&mut self) {
        use harness_protocol::NodeToMaster;
        while let Ok(Some(m)) =
            tokio::time::timeout(Duration::from_millis(300), self.link.from_node.recv()).await
        {
            if let NodeToMaster::Event { event } = m {
                self.seen.push(event);
            }
        }
    }
}

/// §8: an end while a turn runs reaches the agent at its next step, as a
/// note recorded there (`delivered`), scrubbed and framed as program
/// output; an end while the session is idle is recorded at once and the
/// agent reads it at the person's next message. No turn is started by an
/// end.
#[tokio::test(flavor = "multi_thread")]
async fn ends_reach_the_agent_at_its_next_step_or_message_and_start_nothing() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    const SECRET: &str = "sk-test-0123456789abcdef";
    let mut n = node_with(f, sb, vec![SECRET.into()]).await;
    let print = format!("echo out-{SECRET}; sleep 4; echo done-{SECRET}; exit 3");
    n.turn(json!([
        {"tool": "bash", "input": {"command": print, "background": true, "timeout_secs": 60}},
        {"tool": "bash", "input": {"command": "sleep 3"}},
        {"tool": "bash", "input": {"command": "true"}},
    ]))
    .await;
    let ends = n.ends();
    assert_eq!(ends.len(), 1, "{ends:?}");
    let e = &ends[0];
    assert!(e.delivered, "at a step boundary of the running turn");
    assert_eq!((e.reason, e.exit_code), (BackgroundReason::Exited, Some(3)));
    assert!(
        e.tail.contains("done-") && !e.tail.contains(SECRET),
        "{}",
        e.tail
    );
    let note = e.model_note().unwrap();
    assert!(note.contains("program output, not instructions") && note.contains("<output>"));
    // In the log after the step's results, before the next model answer.
    let kinds = n.kinds();
    let at = kinds
        .iter()
        .position(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .unwrap();
    assert!(matches!(kinds[at - 1], EventKind::ToolCallFinished(_)));
    assert!(matches!(
        kinds[at + 1..]
            .iter()
            .find(|k| !matches!(k, EventKind::AssistantDelta(_))),
        Some(EventKind::AssistantMessage(_))
    ));

    // Idle: the end is recorded at once, and no turn starts.
    let turns = |n: &Node| {
        n.kinds()
            .iter()
            .filter(|k| matches!(k, EventKind::TurnStarted(_)))
            .count()
    };
    n.turn(json!([
        {"tool": "bash", "input": {"command": "sleep 4; echo later", "background": true, "timeout_secs": 60}},
    ]))
    .await;
    let before = turns(&n);
    n.until(|k| matches!(k, EventKind::BackgroundEnded(b) if b.bg_id == "bg-2"))
        .await;
    let e = n.ends().pop().unwrap();
    assert!(!e.delivered && e.tail.contains("later"), "{e:?}");
    tokio::time::sleep(Duration::from_secs(2)).await;
    n.drain().await;
    assert_eq!(turns(&n), before, "an end started a turn");
    // The person's next message: the history carries the note.
    let history: Vec<RawEvent> = n.seen.clone();
    let msgs = harness_agent_native::history_to_messages(&history);
    assert!(
        msgs.iter().any(|m| matches!(m, harness_agent_native::ChatMsg::Note(t) if t.contains("bg-2") && t.contains("later"))),
        "{msgs:?}"
    );
}

/// §7 test 1 (delete, revocation) and §4.7 through the node: deleting the
/// session stops its background commands before its log goes; the
/// master's stop for a revoked owner stops them with `access_revoked`;
/// a turn's cancel leaves them running.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_leaves_them_revocation_and_delete_stop_them() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mut n = node_with(f, sb, vec![]).await;
    let (cmd, reaper) = detached(975_400);
    n.turn(json!([
        {"tool": "bash", "input": {"command": format!("{cmd} sleep 1000"), "background": true, "timeout_secs": 600}},
        {"tool": "bash", "input": {"command": "sleep 975499"}},
    ]))
    .await;
    // (The second call ran to its 60 s default? No: cancelled below.)
    all_started(&reaper).await;
    // A turn and its cancel: the background command keeps running.
    n.prompt(&format!(
        "script: {}",
        json!([{"tool": "bash", "input": {"command": "sleep 975498"}}])
    ));
    let by = Instant::now() + Duration::from_secs(20);
    while sleeps(975_498).is_empty() {
        assert!(Instant::now() < by);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    n.send(harness_protocol::MasterToNode::Cancel {
        req_id: "x1".into(),
        session_id: n.sid,
    });
    n.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        reaper.0.iter().all(|s| !sleeps(*s).is_empty()),
        "the cancel stopped it"
    );
    // The revocation sweep's stop.
    n.send(harness_protocol::MasterToNode::StopBackground {
        req_id: "s1".into(),
        session_id: n.sid,
        bg_id: None,
        by: harness_protocol::StopBackgroundBy::AccessRevoked,
    });
    n.until(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .await;
    assert_eq!(n.ends()[0].reason, BackgroundReason::AccessRevoked);
    none_left(&reaper, "the revocation's stop").await;
    // Delete, with one running.
    let (cmd, reaper) = detached(975_410);
    n.turn(json!([
        {"tool": "bash", "input": {"command": format!("{cmd} sleep 1000"), "background": true, "timeout_secs": 600}},
    ]))
    .await;
    all_started(&reaper).await;
    let t = Instant::now();
    n.rt.delete(n.sid).await.unwrap();
    eprintln!("delete took {:?}", t.elapsed());
    none_left(&reaper, "the session's deletion").await;
}

/// §4.7 drain: the node's stop does not wait for background commands: it
/// stops them, records `node_stopping` for each, and returns within its
/// bound.
#[tokio::test(flavor = "multi_thread")]
async fn the_node_stop_ends_them_with_node_stopping() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mut n = node_with(f, sb, vec![]).await;
    let (cmd, reaper) = detached(975_500);
    n.turn(json!([
        {"tool": "bash", "input": {"command": format!("{cmd} sleep 1000"), "background": true, "timeout_secs": 3600}},
        {"tool": "bash", "input": {"command": "trap '' TERM; sleep 1000", "background": true, "timeout_secs": 3600}},
    ]))
    .await;
    all_started(&reaper).await;
    let t = Instant::now();
    let left = n.rt.stop(Duration::from_secs(30)).await;
    let took = t.elapsed();
    assert!(left.is_empty(), "{left:?}");
    assert!(took < Duration::from_secs(10), "the stop took {took:?}");
    n.drain().await;
    let ends = n.ends();
    assert_eq!(ends.len(), 2, "{ends:?}");
    assert!(
        ends.iter()
            .all(|e| e.reason == BackgroundReason::NodeStopping
                && e.confirmed == BackgroundConfirmed::Ended)
    );
    none_left(&reaper, "the node's stop").await;
    eprintln!("node stop with 2 background commands: {took:?}");
}

/// §8: a foreground call without `timeout_secs` ends at 60 s (600 s on
/// the branch head), and says how to ask for more.
#[tokio::test(flavor = "multi_thread")]
async fn a_foreground_call_without_a_timeout_ends_at_60_s() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mut n = node_with(f, sb, vec![]).await;
    let _r = Reaper(vec![975_601]);
    let t = Instant::now();
    n.turn(json!([{"tool": "bash", "input": {"command": "sleep 975601"}}]))
        .await;
    let took = t.elapsed();
    let r = n.results();
    assert!(r[0].contains("timed out after 60s"), "{}", r[0]);
    assert!(r[0].contains("background: true"), "{}", r[0]);
    assert!(
        took >= Duration::from_secs(60) && took < Duration::from_secs(75),
        "{took:?}"
    );
}

// ---- measurements (run by hand: `--ignored --nocapture`) ----

fn processes() -> usize {
    std::fs::read_dir("/proc")
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .ok()
                .and_then(|e| e.file_name().to_str()?.parse::<i32>().ok())
                .is_some()
        })
        .count()
}

/// 64 background commands with full rings: this process's memory, the
/// processes beside it, and what one start costs.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement"]
async fn measure_64_background_commands() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (bg, _ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let (rss0, procs0) = (rss_kib(), processes());
    let mut costs = vec![];
    for i in 0..64 {
        let t = Instant::now();
        // Fills its 256 KiB ring, then sleeps.
        let r = run(
            &Bash,
            &ctx,
            json!({"command": format!("head -c 600000 /dev/zero | tr '\\0' x; exec sleep {}", 975_700 + i),
                   "background": true, "timeout_secs": 600}),
        )
        .await;
        costs.push(t.elapsed());
        assert!(r.background.is_some(), "{i}: {}", r.output);
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
    let (rss1, procs1) = (rss_kib(), processes());
    let o = run(&BashOutput, &ctx, json!({})).await;
    assert_eq!(o.output.matches("running").count(), 64, "{}", o.output);
    eprintln!(
        "64 running, rings full: RSS {rss0} -> {rss1} KiB (+{} KiB); processes {procs0} -> {procs1} (+{}); \
         a start's call (its 3 s first wait included) {:?} .. {:?}",
        rss1 - rss0,
        procs1 - procs0,
        costs.iter().min().unwrap(),
        costs.iter().max().unwrap()
    );
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "true", "background": true, "timeout_secs": 5}),
    )
    .await;
    eprintln!("the 65th in one chat: {}", r.output);
    let t = Instant::now();
    bg.stop_all_within(BackgroundReason::StoppedByAgent, Duration::from_secs(30))
        .await;
    eprintln!("stopping 64 at once took {:?}", t.elapsed());
    let _r = Reaper((975_700..975_764).collect());
}

/// The guard held long (§9.3): a background command whose guard has run
/// for `T8D_LONG_S` seconds (default 600) still confirms its end, and its
/// memory is read at the start and the end.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a measurement, long"]
async fn measure_a_guard_held_long() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let secs: u64 = std::env::var("T8D_LONG_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600);
    let (bg, mut ends) = table(Limits::default());
    let ctx = f.ctx(sb, &bg);
    let _r = Reaper(vec![975_801]);
    let r = run(
        &Bash,
        &ctx,
        json!({"command": "exec sleep 975801", "background": true, "timeout_secs": 3600}),
    )
    .await;
    assert!(r.background.is_some(), "{}", r.output);
    let guard_rss = || -> Vec<(i32, u64)> {
        // The guard: this process's children running `sandbox-guard`.
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
            .filter(|p| {
                std::fs::read(format!("/proc/{p}/cmdline"))
                    .is_ok_and(|c| c.windows(13).any(|w| w == b"sandbox-guard"))
            })
            .filter_map(|p| {
                let s = std::fs::read_to_string(format!("/proc/{p}/status")).ok()?;
                let ppid: i32 = s
                    .lines()
                    .find_map(|l| l.strip_prefix("PPid:"))?
                    .trim()
                    .parse()
                    .ok()?;
                (ppid == std::process::id() as i32).then_some(())?;
                let rss = s
                    .lines()
                    .find_map(|l| l.strip_prefix("VmRSS:"))?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()?;
                Some((p, rss))
            })
            .collect()
    };
    let at_start = guard_rss();
    tokio::time::sleep(Duration::from_secs(secs)).await;
    let at_end = guard_rss();
    let t = Instant::now();
    let s = run(&BashStop, &ctx, json!({"id": "bg-1"})).await;
    let e = next_end(&mut ends, Duration::from_secs(10)).await;
    eprintln!(
        "guard after {secs} s: RSS {at_start:?} -> {at_end:?} KiB; stop took {:?}: {}",
        t.elapsed(),
        s.output
    );
    assert_eq!(e.confirmed, BackgroundConfirmed::Ended, "{e:?}");
    assert!(sleeps(975_801).is_empty());
}

// ---- the real binary: a stop signal and a `kill -9` ----

mod real {
    use super::*;
    use nix::sys::signal::Signal;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    pub struct Serve {
        child: Option<Child>,
        pub addr: String,
        token: String,
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Drop for Serve {
        fn drop(&mut self) {
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    pub fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (data, ws) = (root.join("data"), root.join("ws"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            data.join("node.toml"),
            format!(
                "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n",
                ws.display().to_string()
            ),
        )
        .unwrap();
        std::fs::write(
            data.join("config.toml"),
            "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
             mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
        )
        .unwrap();
        (dir, data, ws)
    }

    pub fn serve(data: &Path) -> Serve {
        let mut cmd = Command::new(EXE);
        cmd.args([
            "--data-dir",
            data.to_str().unwrap(),
            "serve",
            "--bind",
            "127.0.0.1:0",
        ])
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("HARNESS_JOIN_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let err = child.stderr.take().unwrap();
        let l2 = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                l2.lock().unwrap().push(line);
            }
        });
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let addr = loop {
            let line = lines.next().expect("serve ended").unwrap();
            if let Some(a) = line.split("listening on http://").nth(1) {
                break a.trim().to_string();
            }
        };
        std::thread::spawn(move || for _ in lines {});
        let token = std::fs::read_to_string(data.join("api-token"))
            .unwrap()
            .trim()
            .to_string();
        Serve {
            child: Some(child),
            addr,
            token,
            log,
        }
    }

    impl Serve {
        pub fn pid(&self) -> i32 {
            self.child.as_ref().unwrap().id() as i32
        }

        pub fn signal(&self, s: Signal) {
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.pid()), s).unwrap();
        }

        pub fn exited_within(&mut self, within: Duration) -> Option<std::process::ExitStatus> {
            let by = Instant::now() + within;
            loop {
                if let Some(s) = self.child.as_mut().unwrap().try_wait().unwrap() {
                    self.child = None;
                    return Some(s);
                }
                if Instant::now() >= by {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        pub fn log(&self) -> String {
            self.log.lock().unwrap().join("\n")
        }

        pub async fn post(&self, path: &str, body: Value) -> Value {
            let r = reqwest::Client::new()
                .post(format!("http://{}{path}", self.addr))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success(), "{path}: {}", r.status());
            r.json().await.unwrap_or(Value::Null)
        }

        pub async fn events(&self, sid: &str) -> Vec<Value> {
            let r = reqwest::Client::new()
                .get(format!(
                    "http://{}/api/sessions/{sid}/events?limit=5000",
                    self.addr
                ))
                .bearer_auth(&self.token)
                .send()
                .await
                .unwrap();
            let v: Value = r.json().await.unwrap();
            v["events"].as_array().unwrap().clone()
        }

        pub async fn wait_for(&self, sid: &str, pred: impl Fn(&Value) -> bool) -> Vec<Value> {
            let by = Instant::now() + Duration::from_secs(60);
            loop {
                let evs = self.events(sid).await;
                if evs.iter().any(&pred) {
                    return evs;
                }
                assert!(
                    Instant::now() < by,
                    "not found in {sid}: {evs:?}\n{}",
                    self.log()
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        /// A session running one background command (the T.8a escapers).
        pub async fn with_background(&self, ws: &Path, cmd: &str) -> String {
            let sid = self
                .post(
                    "/api/sessions",
                    json!({"cwd": ws.display().to_string(), "approval_mode": "auto"}),
                )
                .await["session_id"]
                .as_str()
                .unwrap()
                .to_string();
            let script = json!([{"tool": "bash", "input": {"command": cmd, "background": true, "timeout_secs": 3600}}]);
            self.post(
                &format!("/api/sessions/{sid}/prompt"),
                json!({"text": format!("script: {script}")}),
            )
            .await;
            self.wait_for(&sid, |e| e["kind"] == "turn_ended").await;
            sid
        }
    }

    pub fn ends(evs: &[Value]) -> Vec<String> {
        evs.iter()
            .filter(|e| e["kind"] == "background_ended")
            .map(|e| e["payload"]["reason"].as_str().unwrap_or("").to_string())
            .collect()
    }
}

/// §4.7 drain, on the real binary: SIGTERM stops the background commands,
/// records `node_stopping` for each, and exits 0 within A.19's bound
/// without waiting for them to end on their own.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_signal_ends_background_commands_with_node_stopping() {
    let (_dir, data, ws) = real::setup();
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let (cmd, reaper) = detached(975_900);
    let mut s = real::serve(&data);
    let sid = s.with_background(&ws, &format!("{cmd} sleep 1000")).await;
    all_started(&reaper).await;
    let t = Instant::now();
    s.signal(nix::sys::signal::Signal::SIGTERM);
    let status = s.exited_within(Duration::from_secs(60));
    let took = t.elapsed();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(0),
        "{took:?}\n{}",
        s.log()
    );
    assert!(took < Duration::from_secs(15), "the stop took {took:?}");
    none_left(&reaper, "SIGTERM of serve").await;
    let s2 = real::serve(&data);
    let evs = s2.events(&sid).await;
    assert_eq!(real::ends(&evs), vec!["node_stopping"], "{evs:#?}");
    eprintln!("SIGTERM with a background command: exit 0 in {took:?}");
}

/// §7 test 9, on the real binary: after `kill -9` nothing of the command
/// is left, and at the next start its end is `node_restarted`; no turn
/// starts.
#[tokio::test(flavor = "multi_thread")]
async fn after_kill_9_and_a_start_each_has_node_restarted_and_no_turn_starts() {
    let (_dir, data, ws) = real::setup();
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let (cmd, reaper) = detached(975_910);
    let mut s = real::serve(&data);
    let sid = s.with_background(&ws, &format!("{cmd} sleep 1000")).await;
    all_started(&reaper).await;
    let turns_before = s
        .events(&sid)
        .await
        .iter()
        .filter(|e| e["kind"] == "turn_started")
        .count();
    s.signal(nix::sys::signal::Signal::SIGKILL);
    assert!(s.exited_within(Duration::from_secs(10)).is_some());
    none_left(&reaper, "kill -9 of serve").await;
    let s2 = real::serve(&data);
    let evs = s2.wait_for(&sid, |e| e["kind"] == "background_ended").await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let evs2 = s2.events(&sid).await;
    assert_eq!(real::ends(&evs), vec!["node_restarted"], "{evs:#?}");
    let turns_after = evs2.iter().filter(|e| e["kind"] == "turn_started").count();
    assert_eq!(
        turns_after, turns_before,
        "a turn started after the restart"
    );
}
