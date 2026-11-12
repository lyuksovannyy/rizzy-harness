//! The agent's shell in bubblewrap (ADOPTION_PLAN T.8a): nothing a command
//! starts outlives it. These are experiment B.1's tests inverted: before
//! T.8a a `setsid` child survived cancel, timeout, a dropped call and a
//! `kill -9` of the node (`a_setsid_child_survives_cancel_in_the_sandbox`
//! failed on `main` once flipped: "a setsid child survived cancel").
//!
//! Through the real `rizzy-harness sandbox-exec` wrapper, the real `bwrap`
//! and the real bash tool. Where bubblewrap or Landlock does not work they
//! skip with a printed reason; with `HARNESS_REQUIRE_BWRAP=1` they fail.
//!
//! The processes are found from outside by their unique `sleep <n>`
//! command line (inside the namespace `$$` is not the host's pid), and
//! every one a test finds is killed by exactly that pid on drop.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_tools::sandbox::{Confinement, Grants, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx, ToolOutcome};
use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
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
        data,
        ws,
    }
}

impl Fixture {
    fn plain(&self) -> Sandbox {
        Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// The contained sandbox, or `None` where it cannot run here.
    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        let sb = self.plain();
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain"))
            .ok()
            .map(Arc::new)
    }

    fn ctx(&self, sb: Arc<Sandbox>) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        ctx
    }
}

// ---- finding the test's own processes from outside ----

fn alive(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let state = stat.rsplit(')').next().unwrap_or("").trim_start();
            !(state.starts_with('Z') || state.starts_with('X') || state.starts_with('x'))
        }
        Err(_) => false,
    }
}

/// Every live process whose command line is exactly `sleep <secs>`.
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

/// Kills, on drop, every process still running one of these sleeps.
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

/// The four ways a process leaves the group the tool signals (B.1, B.11):
/// `setsid`, `nohup … &`, a double fork with `setsid`, and a child that
/// ignores SIGTERM; and, as a control, a plain background child. Each runs
/// `sleep <base + i>`.
fn detached(base: u32) -> (String, Reaper) {
    let s: Vec<u32> = (0..5).map(|i| base + i).collect();
    let cmd = format!(
        "setsid sleep {} & nohup sleep {} >/dev/null 2>&1 & ( setsid sleep {} & ) ; \
         sh -c 'trap \"\" TERM; exec sleep {}' & sleep {} & ",
        s[0], s[1], s[2], s[3], s[4]
    );
    (cmd, Reaper(s))
}

/// Wait until all five are running (so a pass is never vacuous).
async fn all_started(r: &Reaper) {
    let by = Instant::now() + Duration::from_secs(20);
    while r.0.iter().any(|s| sleeps(*s).is_empty()) {
        assert!(Instant::now() < by, "the detached children never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// None of them is left 2 s after the call ended.
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

enum Stop {
    Exit,
    Cancel,
    Timeout,
    Drop,
}

async fn detached_children_end_with(how: Stop, base: u32) {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let ctx = f.ctx(sb);
    let cancel = ctx.cancel.clone();
    let (cmd, reaper) = detached(base);
    let input = match how {
        // Long enough for the test to see all five from outside.
        Stop::Exit => json!({"command": format!("{cmd} sleep 3; echo done")}),
        Stop::Timeout => json!({"command": format!("{cmd} sleep 1000"), "timeout_secs": 2}),
        Stop::Cancel | Stop::Drop => json!({"command": format!("{cmd} sleep 1000")}),
    };
    let h = tokio::spawn(async move { Bash.run(&ctx, input).await });
    all_started(&reaper).await;
    let name = match how {
        Stop::Exit => {
            let r = h.await.unwrap();
            assert!(r.ok && r.output.contains("done"), "{}", r.output);
            "normal exit"
        }
        Stop::Cancel => {
            cancel.cancel();
            let r = h.await.unwrap();
            assert!(r.output.contains("cancelled"), "{}", r.output);
            "cancel"
        }
        Stop::Timeout => {
            let r = h.await.unwrap();
            assert!(r.output.contains("timed out"), "{}", r.output);
            "timeout"
        }
        Stop::Drop => {
            h.abort();
            let _ = h.await;
            "dropped call"
        }
    };
    none_left(&reaper, name).await;
}

#[tokio::test]
async fn detached_children_end_with_the_command() {
    detached_children_end_with(Stop::Exit, 976_100).await;
}

#[tokio::test]
async fn detached_children_end_on_cancel() {
    detached_children_end_with(Stop::Cancel, 976_110).await;
}

#[tokio::test]
async fn detached_children_end_on_timeout() {
    detached_children_end_with(Stop::Timeout, 976_120).await;
}

#[tokio::test]
async fn detached_children_end_when_the_call_is_dropped() {
    detached_children_end_with(Stop::Drop, 976_130).await;
}

/// The node is SIGKILLed: this test binary re-run as a helper process
/// drives the real `Bash` tool as the node does, and is then killed. No
/// cleanup code runs; the namespace dies with it.
#[tokio::test]
async fn detached_children_end_when_the_node_is_killed() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let (_, reaper) = detached(976_140);
    let mut helper = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "helper_runs_detached_children", "--nocapture"])
        .env("T8A_HELPER", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    all_started(&reaper).await;
    helper.kill().unwrap(); // SIGKILL
    let _ = helper.wait();
    none_left(&reaper, "kill -9 of the node").await;
}

/// Not a test on its own: the body of the helper process above.
#[tokio::test]
async fn helper_runs_detached_children() {
    if std::env::var_os("T8A_HELPER").is_none() {
        return;
    }
    let f = fixture();
    let ctx = f.ctx(f.sandbox().expect("checked by the parent"));
    let (cmd, reaper) = detached(976_140);
    std::mem::forget(reaper); // the parent owns the cleanup
    let _ = Bash
        .run(&ctx, json!({"command": format!("{cmd} sleep 1000")}))
        .await;
}

/// `PR_SET_PDEATHSIG` fires when the *thread* that started bwrap exits.
/// A contained command started from a thread that then ends must run to
/// its end: the spawn goes through the process's spawner thread. The
/// control shows the hazard is real: the same command spawned directly
/// from that thread is killed when it ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_command_outlives_the_thread_that_started_it() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let conf = Confinement::Confined {
        sandbox: sb,
        tmp: None,
    };
    let build = || {
        let (mut c, contained) = conf
            .shell(
                "sh".as_ref(),
                &["-c".as_ref(), "sleep 1; echo survived".as_ref()],
                &f.ws,
                None,
                &Default::default(),
                std::slice::from_ref(&f.data),
                Grants {
                    workspace_write: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(contained);
        c.stdout(std::process::Stdio::piped()).kill_on_drop(true);
        c
    };
    let rt = tokio::runtime::Handle::current();
    // The real path, from a thread that ends at once.
    let cmd = build();
    let rt2 = rt.clone();
    let child = std::thread::spawn(move || {
        rt2.block_on(harness_tools::contain::spawn_for_test(cmd))
            .unwrap()
    })
    .join()
    .unwrap();
    let out = child.wait_with_output().await.unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "survived");

    // Control: spawned by the ending thread itself. It lives a moment
    // longer, past bwrap's own `prctl` (a thread that ends before bwrap
    // set its death signal is not noticed: PLAN.md T9's residual).
    let mut cmd = build();
    let rt2 = rt.clone();
    let child = std::thread::spawn(move || {
        let _g = rt2.enter();
        let c = cmd.spawn().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        c
    })
    .join()
    .unwrap();
    let out = child.wait_with_output().await.unwrap();
    assert!(
        !out.status.success() && out.stdout.is_empty(),
        "control: a command spawned from an ended thread survived ({:?}); the hazard \
         this test guards against is not reproduced",
        out.status
    );
}

async fn run(ctx: &ToolCtx, input: Value) -> ToolOutcome {
    Bash.run(ctx, input).await
}

/// Inside, `/proc` is the namespace's own: the test's process is not in
/// it, and only a handful of processes are.
#[tokio::test]
async fn the_hosts_processes_are_not_visible() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let me = std::process::id();
    let r = run(
        &f.ctx(sb),
        json!({"command": format!(
            "test -e /proc/{me} && echo HOST-VISIBLE; echo pids=$(ls -d /proc/[0-9]* | wc -l); \
             echo self=$$"
        )}),
    )
    .await;
    assert!(r.ok, "{}", r.output);
    assert!(!r.output.contains("HOST-VISIBLE"), "{}", r.output);
    let n: usize = r
        .output
        .split("pids=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap();
    assert!(n < 10, "{n} processes visible: {}", r.output);
    assert!(r.output.contains("self=2"), "{}", r.output);
}

/// The caller sees what it saw before: exit codes, both streams, the
/// timeout report; and a cancel still gives the command its SIGTERM.
#[tokio::test]
async fn exit_codes_output_and_the_graceful_stop_are_unchanged() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let ctx = f.ctx(sb.clone());
    let r = run(&ctx, json!({"command": "echo out; echo err >&2; exit 3"})).await;
    assert_eq!(r.exit_code, Some(3), "{}", r.output);
    assert!(r.output.contains("out") && r.output.contains("err"));
    assert!(r.output.contains("[exit code: 3]"), "{}", r.output);

    let r = run(&ctx, json!({"command": "sleep 100", "timeout_secs": 1})).await;
    assert!(
        r.output
            .contains("[timed out after 1s: process group killed]"),
        "{}",
        r.output
    );

    // Cancel: the command's TERM trap runs, and the call ends well inside
    // the grace period.
    let mark = f.ws.join("got-term");
    let ctx = f.ctx(sb);
    let cancel = ctx.cancel.clone();
    let cmd = format!(
        "trap 'echo term > {}; exit 0' TERM; echo ready; sleep 1000 & wait",
        mark.display()
    );
    let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
    tokio::time::sleep(Duration::from_millis(700)).await;
    let t = Instant::now();
    cancel.cancel();
    let r = h.await.unwrap();
    assert!(r.output.contains("cancelled"), "{}", r.output);
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap_or_default().trim(),
        "term",
        "the TERM trap did not run: {}",
        r.output
    );
    assert!(
        t.elapsed() < Duration::from_millis(1900),
        "{:?}",
        t.elapsed()
    );
}

/// Nothing in a call's input, nor a session's own confinement, switches
/// the containment off: it is the node's.
#[tokio::test]
async fn nothing_in_a_call_or_a_session_turns_containment_off() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mut ctx = f.ctx(sb);
    ctx.sandbox = ctx.sandbox.for_session("another-session");
    let r = run(
        &ctx,
        json!({
            "command": "echo self=$$",
            "contain": false,
            "sandbox": "off",
            "bwrap": false,
            "unshare": "none",
        }),
    )
    .await;
    assert!(r.output.contains("self=2"), "{}", r.output);
}

/// The other users of `sandbox-exec` keep running without bubblewrap:
/// only the agent's shell is contained.
#[test]
fn only_the_shell_is_contained() {
    let f = fixture();
    let Ok(sb) = bwrap::contained(f.plain(), &f.data.join("probe-contain")) else {
        return;
    };
    let conf = Confinement::Confined {
        sandbox: Arc::new(sb),
        tmp: None,
    };
    let args = |c: &tokio::process::Command| c.as_std().get_program().to_owned();
    let plain = conf
        .command(
            "true".as_ref(),
            &[],
            &f.ws,
            None,
            &Default::default(),
            &[],
            Grants::default(),
        )
        .unwrap();
    assert_eq!(args(&plain), Path::new(EXE).as_os_str());
    let (shell, contained) = conf
        .shell(
            "true".as_ref(),
            &[],
            &f.ws,
            None,
            &Default::default(),
            &[],
            Grants::default(),
        )
        .unwrap();
    assert!(contained);
    assert!(
        Path::new(&args(&shell)).ends_with("bwrap"),
        "{:?}",
        args(&shell)
    );
}

/// Where only the shared-`/proc` form works (nested in a container), the
/// kill guarantee still holds; this runs that form on purpose.
#[tokio::test]
async fn the_shared_proc_form_still_contains() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let shared = harness_tools::contain::Contain::new(
        harness_tools::contain::find_bwrap().unwrap(),
        harness_tools::contain::Form::SharedProc,
    );
    let sb = f.plain();
    sb.probe_containment(&shared, &f.data.join("probe-shared"))
        .unwrap();
    let ctx = f.ctx(Arc::new(sb.with_containment(shared)));
    let cancel = ctx.cancel.clone();
    let (cmd, reaper) = detached(976_150);
    let input = json!({"command": format!("{cmd} sleep 1000")});
    let h = tokio::spawn(async move { Bash.run(&ctx, input).await });
    all_started(&reaper).await;
    cancel.cancel();
    let _ = h.await.unwrap();
    none_left(&reaper, "cancel, shared /proc").await;
}
