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
        self.sandbox_and_status().map(|(sb, _)| sb)
    }

    /// The contained sandbox and the wrapper's Landlock status line.
    fn sandbox_and_status(&self) -> Option<(Arc<Sandbox>, String)> {
        let sb = self.plain();
        let status = match sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            Ok(s) => s,
            Err(e) => {
                assert!(!bwrap::required(), "Landlock does not work here: {e}");
                eprintln!("SKIPPED: Landlock does not work here: {e}");
                return None;
            }
        };
        let sb = bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain")).ok()?;
        Some((Arc::new(sb), status))
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

// ---- T8A-1 (docs/security-review/2026-11-T8a-containment.md) ----
//
// bubblewrap ends every process in its namespace, but a command can ask a
// service of the node user outside it to start one. Whether it can reach
// such a service is Landlock's (pathname Unix sockets from ABI 9, abstract
// ones from ABI 6), so the containment's claim holds in full only from ABI
// 9, and `doctor` says so (`sandbox::contained_line`).

fn have_python() -> bool {
    let ok = std::process::Command::new("python3")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        assert!(!bwrap::required(), "the T8A-1 tests need python3");
        eprintln!("SKIPPED: no python3");
    }
    ok
}

/// A service of the test outside the sandbox that starts `sleep <secs>`
/// for each connection: one listener on a pathname socket outside every
/// grant, one on an abstract socket (the stand-in for the session bus).
fn service(path: &Path, abstract_name: &str, secs_path: u32, secs_abstract: u32) {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};
    let by_path = UnixListener::bind(path).unwrap();
    let by_name =
        UnixListener::bind_addr(&SocketAddr::from_abstract_name(abstract_name).unwrap()).unwrap();
    for (listener, secs) in [(by_path, secs_path), (by_name, secs_abstract)] {
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if conn.is_ok() {
                    let _ = std::process::Command::new("sleep")
                        .arg(secs.to_string())
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
        });
    }
}

/// One connect to each socket.
fn connect_script(path: &Path, abstract_name: &str) -> String {
    format!(
        "python3 -c 'import socket\n\
         for a in (\"{}\", \"\\0{abstract_name}\"):\n\
         \x20   s = socket.socket(socket.AF_UNIX)\n\
         \x20   try:\n\
         \x20       s.connect(a); print(\"unix-\" + \"connected\")\n\
         \x20   except OSError as e: print(\"refused:\", e)\n\
         '",
        path.display()
    )
}

/// The review's proof through the real wrapper and the real `bash` tool,
/// kept to record which side of the line this kernel is on (CI's kernel is
/// expected below ABI 9): from ABI 9 nothing reaches the pathname socket,
/// from ABI 6 nothing the abstract one; below, what the service starts
/// outlives the command, and `doctor` warns instead of claiming it
/// (`the_containment_line_depends_on_the_landlock_abi`).
#[tokio::test]
async fn a_service_outside_is_out_of_reach_only_from_landlock_abi_9() {
    let f = fixture();
    let Some((sb, status)) = f.sandbox_and_status() else {
        return;
    };
    if !have_python() {
        return;
    }
    let abi = harness_tools::sandbox::effective_abi(&status);
    let (bus, name) = (f.root.join("bus"), format!("t8a-{}", std::process::id()));
    let reaper = Reaper(vec![975_101, 975_102]);
    service(&bus, &name, 975_101, 975_102);
    let r = run(
        &f.ctx(sb),
        json!({"command": format!("echo self=$$; {}", connect_script(&bus, &name))}),
    )
    .await;
    assert!(r.output.contains("self=2"), "not contained: {}", r.output);
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (by_path, by_name) = (!sleeps(975_101).is_empty(), !sleeps(975_102).is_empty());
    eprintln!(
        "Landlock ABI {abi}: after the contained command ended, the process started through \
         the pathname socket is {}, through the abstract socket {}",
        if by_path { "RUNNING" } else { "not there" },
        if by_name { "RUNNING" } else { "not there" },
    );
    assert_eq!(by_path, abi < 9, "{}", r.output);
    assert_eq!(by_name, abi < 6, "{}", r.output);
    drop(reaper);
}

/// RESIDUAL BELOW LANDLOCK ABI 9 (PLAN.md T9), shown on any kernel: the
/// node's exact bubblewrap command line with the wrapper replaced by what
/// an ABI 4 kernel enforces (`support/review_t8a/landlock_abi4.py`). The
/// command is in its namespace and the file policy holds; one connect to
/// each socket makes the service start a process, and the node's kill
/// leaves those two. Closed by T.8c's file view (and a network namespace
/// for abstract sockets); until then `doctor` names it and `contain =
/// "required"` refuses on such a kernel. This test flips when it closes.
#[tokio::test]
async fn below_abi_9_a_service_outside_still_outlives_the_kill() {
    use std::ffi::OsString;
    const ABI4: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/review_t8a/landlock_abi4.py"
    );
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    if !have_python() {
        return;
    }
    let (bus, name) = (
        f.root.join("bus"),
        format!("t8a-old-{}", std::process::id()),
    );
    let secret = f.root.join("home/secret");
    std::fs::write(&secret, "outside the session folder").unwrap();
    let reaper = Reaper(vec![975_111, 975_112, 975_113, 975_114]);
    service(&bus, &name, 975_111, 975_112);
    let script = format!(
        "cat {} && echo FILE-POLICY-BROKEN; {}; setsid sleep 975113 & sleep 975114",
        secret.display(),
        connect_script(&bus, &name)
    );
    let conf = Confinement::Confined {
        sandbox: sb,
        tmp: None,
    };
    let (node, contained) = conf
        .shell(
            "bash".as_ref(),
            &["-c".as_ref(), script.as_ref()],
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
    let node = node.as_std();
    let argv: Vec<OsString> = node.get_args().map(OsString::from).collect();
    let at = argv.iter().position(|a| a == EXE).unwrap();
    assert_eq!(argv[at + 1], "sandbox-exec");
    let mut old = std::process::Command::new(node.get_program());
    old.args(&argv[..at])
        .args(["python3", "-I", ABI4])
        .args(&argv[at + 2..])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(&f.ws)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    std::os::unix::process::CommandExt::process_group(&mut old, 0);
    let child = old.spawn().unwrap();
    let bwrap_pid = child.id() as i32;
    let by = Instant::now() + Duration::from_secs(20);
    while reaper.0.iter().any(|s| sleeps(*s).is_empty()) {
        assert!(Instant::now() < by, "the four processes never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The node's kill at the end of a cancel or a timeout.
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(bwrap_pid),
        nix::sys::signal::Signal::SIGKILL,
    );
    let out = child.wait_with_output().unwrap();
    let out =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(!out.contains("FILE-POLICY-BROKEN"), "{out}");
    none_left(&Reaper(vec![975_113, 975_114]), "inside the namespace").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !sleeps(975_111).is_empty() && !sleeps(975_112).is_empty(),
        "closed: nothing reached the sockets; flip this test and PLAN.md T9: {out}"
    );
    drop(reaper);
}
