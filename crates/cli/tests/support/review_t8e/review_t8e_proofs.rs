//! T.8e review (`docs/security-review/2026-11-T8e-guard.md`): the proofs
//! that need the node's own code (`Confinement::spawn`, the bash tool,
//! `contain::sweep`). Not part of the test suite: `run_rust_proofs.sh` beside
//! this file copies it to `crates/cli/tests/`, runs it and removes the copy.
//!
//! Each test PASSES WHEN THE FINDING REPRODUCES and prints what it saw. A
//! fix turns the named assertion around.
//!
//! Scratch folders are under `<worktree>/target/review_t8e/`. Every process
//! is found by a `sleep <n>` or a folder unique to the test and killed by
//! that pid; the FUSE mounts carry `fsname=t8e-review`.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_tools::sandbox::{Confinement, Grants, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const HANG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/review_t8e/fuse_hang.py"
);

fn scratch_root() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/review_t8e");
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

struct Fixture {
    _dir: Option<tempfile::TempDir>,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir_in(scratch_root()).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    Fixture {
        _dir: Some(dir),
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

    fn sandbox(&self) -> Arc<Sandbox> {
        let sb = self.plain();
        sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data)
            .expect("Landlock");
        Arc::new(
            bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain"))
                .ok()
                .expect("bubblewrap"),
        )
    }

    fn sandbox_unprobed(&self) -> Arc<Sandbox> {
        Arc::new(
            self.plain()
                .with_containment(harness_tools::contain::Contain::new(
                    harness_tools::contain::find_bwrap().unwrap(),
                    harness_tools::contain::Form::Full,
                )),
        )
    }

    fn ctx(&self, sb: Arc<Sandbox>, ws: &Path) -> ToolCtx {
        let mut ctx = ToolCtx::new(ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        ctx
    }

    /// The contained `sh -c <script>` as the bash tool builds it.
    fn shell(&self, sb: Arc<Sandbox>, script: &str) -> tokio::process::Command {
        let conf = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        let (mut cmd, contained) = conf
            .shell(
                "sh".as_ref(),
                &["-c".as_ref(), script.as_ref()],
                &self.ws,
                None,
                &Default::default(),
                std::slice::from_ref(&self.data),
                Grants {
                    workspace_write: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(contained);
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        cmd
    }
}

fn stat(pid: i32) -> Option<Vec<String>> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    Some(
        s.rsplit(')')
            .next()?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    )
}

fn state(pid: i32) -> Option<char> {
    stat(pid)?.first()?.chars().next()
}

fn alive(pid: i32) -> bool {
    state(pid).is_some_and(|s| !matches!(s, 'Z' | 'X' | 'x'))
}

fn pids() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .collect()
}

fn cmdline(pid: i32) -> Vec<u8> {
    std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default()
}

fn sleeps(secs: u32) -> Vec<i32> {
    let want = format!("sleep\0{secs}\0");
    pids()
        .into_iter()
        .filter(|p| cmdline(*p) == want.as_bytes() && alive(*p))
        .collect()
}

/// Live processes whose command line holds `needle`.
fn holding(needle: &Path) -> Vec<i32> {
    let n = needle.as_os_str().as_encoded_bytes();
    let me = std::process::id() as i32;
    pids()
        .into_iter()
        .filter(|p| *p != me && cmdline(*p).windows(n.len()).any(|w| w == n) && alive(*p))
        .collect()
}

fn guard_of(needle: &Path) -> Option<i32> {
    holding(needle).into_iter().find(|p| {
        cmdline(*p).split(|b| *b == 0).nth(1) == Some(harness_tools::guard::SUBCOMMAND.as_bytes())
    })
}

fn signal(pid: i32, sig: nix::sys::signal::Signal) {
    let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), sig);
}

/// Kills, on drop, what still carries this test's folder or sleeps.
struct Cleanup(PathBuf, Vec<u32>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for _ in 0..3 {
            for p in holding(&self.0) {
                signal(p, nix::sys::signal::Signal::SIGCONT);
                signal(p, nix::sys::signal::Signal::SIGKILL);
            }
            for s in &self.1 {
                for p in sleeps(*s) {
                    signal(p, nix::sys::signal::Signal::SIGKILL);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn gone_within(secs: u32, within: Duration) -> Vec<i32> {
    let by = Instant::now() + within;
    loop {
        let l = sleeps(secs);
        if l.is_empty() || Instant::now() >= by {
            return l;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// `fuse_hang.py` mounted at `mnt`: its folder `d` stops answering after
/// [`Self::stall`].
struct Hang {
    server: Option<std::process::Child>,
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    mnt: PathBuf,
}

impl Hang {
    fn mount(mnt: &Path) -> Self {
        std::fs::create_dir_all(mnt).unwrap();
        let mut server = std::process::Command::new("python3")
            .arg("-I")
            .arg(HANG)
            .arg(mnt)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = std::io::BufReader::new(server.stdout.take().unwrap()).lines();
        assert_eq!(
            lines.next().map(|l| l.unwrap()).as_deref(),
            Some("mounted"),
            "FUSE does not mount here"
        );
        Self {
            server: Some(server),
            lines,
            mnt: mnt.to_owned(),
        }
    }

    fn stall(&mut self) {
        let s = self.server.as_mut().unwrap();
        s.stdin.as_mut().unwrap().write_all(b"stall\n").unwrap();
        assert_eq!(
            self.lines.next().map(|l| l.unwrap()).as_deref(),
            Some("stalling")
        );
    }

    /// Wait until the server holds a request on `d`.
    fn held(&mut self) {
        assert_eq!(
            self.lines.next().map(|l| l.unwrap()).as_deref(),
            Some("held"),
            "nothing reached the stalled folder"
        );
    }

    /// End the server: the kernel aborts the connection, every held request
    /// fails. (SIGKILL, not only the closed stdin: a process stuck before
    /// `exec` may hold a copy of that pipe too, which is this review's
    /// finding in small.)
    fn release(&mut self) {
        if let Some(mut s) = self.server.take() {
            drop(s.stdin.take());
            let _ = s.kill();
            let _ = s.wait();
        }
    }
}

impl Drop for Hang {
    fn drop(&mut self) {
        self.release();
        let _ = std::process::Command::new("fusermount3")
            .args(["-u", "-z", "--"])
            .arg(&self.mnt)
            .status();
    }
}

// ---- T8E-1: a child of the node that has not reached exec holds the channel ----

/// Not a test on its own: the node. One contained `sleep` through the
/// node's code; then, with `T8E_R_HANG`, one more child the way the node's
/// git is started (`node/src/git.rs:371`, `exec`: a tokio `Command` with
/// `current_dir(<the session's folder>)`), whose folder does not answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn helper_node() {
    let (Some(data), Some(ws), Some(secs)) = (
        std::env::var_os("T8E_R_DATA"),
        std::env::var_os("T8E_R_WS"),
        std::env::var("T8E_R_SECS").ok(),
    ) else {
        return;
    };
    let f = Fixture {
        _dir: None,
        data: data.into(),
        ws: ws.into(),
    };
    let cmd = f.shell(f.sandbox_unprobed(), &format!("sleep {secs}"));
    let child = Confinement::spawn(cmd, true).await.unwrap();
    let secs: u32 = secs.parse().unwrap();
    while sleeps(secs).is_empty() {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    if let Some(hang) = std::env::var_os("T8E_R_HANG") {
        tokio::spawn(async move {
            let mut git = tokio::process::Command::new("true");
            git.current_dir(hang);
            let _ = git.spawn();
        });
    }
    let mut out = std::io::stdout();
    out.write_all(b"x").unwrap();
    out.flush().unwrap();
    tokio::time::sleep(Duration::from_secs(1000)).await;
    drop(child);
}

fn start_node(f: &Fixture, secs: u32, hang: Option<&Path>) -> std::process::Child {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "helper_node", "--nocapture"])
        .env("T8E_R_DATA", &f.data)
        .env("T8E_R_WS", &f.ws)
        .env("T8E_R_SECS", secs.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(h) = hang {
        c.env("T8E_R_HANG", h);
    }
    let mut node = c.spawn().unwrap();
    let mut b = [0u8; 1];
    node.stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut b)
        .expect("the node never spawned");
    node
}

/// Claim 1 ("a node killed in any way, at any moment, leaves no command
/// running") and claim 3 ("a restarted node sweeps anything left").
///
/// Control: the node with one contained command is SIGKILLed; the command
/// is gone at once. Then the same node has also started its git in a
/// session folder that does not answer: that child waits in `chdir`, before
/// `exec`, with a copy of every descriptor of the node, the node's end of
/// the guard's channel among them. The node is SIGKILLed: the channel does
/// not close, the guard is not told, the command runs on with nobody
/// enforcing its timeout. The sweep of the next start finds nothing (the
/// guard is alive, so nothing is an orphan). On `origin/main` the same
/// kill ended the command (bubblewrap's death signal named the node's
/// spawner thread, whoever held descriptors).
#[test]
fn proof_t8e1_a_child_stuck_before_exec_keeps_the_channel_open() {
    const SECS: u32 = 977_201;
    let f = fixture();
    let _ = f.sandbox(); // the probes, as a node start runs them
    let _clean = Cleanup(f.data.clone(), vec![SECS, SECS + 1]);
    let root = f.data.join("sandbox-tmp");

    // Control.
    let mut node = start_node(&f, SECS, None);
    let t = Instant::now();
    node.kill().unwrap();
    let _ = node.wait();
    let left = gone_within(SECS, Duration::from_secs(3));
    eprintln!(
        "control: node SIGKILLed; command {} after {:?}",
        if left.is_empty() { "gone" } else { "LEFT" },
        t.elapsed()
    );
    assert!(left.is_empty(), "the control left its command");

    // The same, with one child stuck before exec.
    let mnt = f.ws.parent().unwrap().join("mnt");
    let mut hang = Hang::mount(&mnt);
    hang.stall();
    let mut node = start_node(&f, SECS + 1, Some(&mnt.join("d")));
    hang.held();
    let guard = guard_of(&f.data).expect("the guard");
    let pgrp = |p: i32| stat(p).map(|s| s[2].clone()).unwrap_or_default();
    eprintln!(
        "process groups: node {} guard {} bubblewrap {:?} (a terminal's Ctrl-C reaches the node's only)",
        pgrp(node.id() as i32),
        pgrp(guard),
        std::fs::read_to_string(format!("/proc/{guard}/task/{guard}/children"))
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|c| c.parse::<i32>().ok())
            .map(pgrp)
            .collect::<Vec<_>>()
    );
    node.kill().unwrap();
    let _ = node.wait();
    let left = gone_within(SECS + 1, Duration::from_secs(5));
    eprintln!(
        "stalled: node SIGKILLed; 5 s later the command is {}; its guard {guard} is {}",
        if left.is_empty() {
            "gone".to_string()
        } else {
            format!("STILL RUNNING {left:?}")
        },
        if alive(guard) { "alive" } else { "gone" }
    );
    assert!(
        !left.is_empty() && alive(guard),
        "FIXED: the command did not outlive the node"
    );
    let swept = harness_tools::contain::sweep(&root);
    std::thread::sleep(Duration::from_millis(300));
    eprintln!(
        "the next start's sweep: {swept:?}; the command is {}",
        if sleeps(SECS + 1).is_empty() {
            "gone"
        } else {
            "still running"
        }
    );
    assert!(
        swept.is_empty() && !sleeps(SECS + 1).is_empty(),
        "FIXED: the sweep ended it"
    );
    hang.release();
    let left = gone_within(SECS + 1, Duration::from_secs(5));
    eprintln!(
        "the folder answers again: the command is {}",
        if left.is_empty() { "gone" } else { "LEFT" }
    );
    assert!(left.is_empty());
}

// ---- T8E-3: an answer that takes as long as the loop's grace is never seen ----

/// Claim 2 ("a cancel … returns only after the guard confirmed, or reports
/// 'still alive' instead of 'cancelled'"). The node waits 2 s + 2 s + 1 s
/// for the guard's answer, then reports "the end is not confirmed". The
/// agent loop abandons a cancelled tool after `TOOL_CANCEL_GRACE`, the same
/// 5 s, and records `[cancelled]` (`agent-native/src/lib.rs:1927`). So the
/// node's own bound can never produce a result: here the guard does not
/// answer (it is stopped, the stand-in for a guard that is starved or
/// frozen), the tool's answer comes after the loop's grace, and the command
/// is alive at that moment.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proof_t8e3_an_unconfirmed_stop_is_recorded_as_cancelled() {
    const SECS: u32 = 977_211;
    let f = fixture();
    let sb = f.sandbox();
    let _clean = Cleanup(f.data.clone(), vec![SECS]);
    let ctx = f.ctx(sb, &f.ws);
    let cancel = ctx.cancel.clone();
    let mut h = tokio::spawn(async move {
        Bash.run(&ctx, json!({"command": format!("sleep {SECS}")}))
            .await
    });
    while sleeps(SECS).is_empty() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let guard = guard_of(&f.data).expect("the guard");
    signal(guard, nix::sys::signal::Signal::SIGSTOP);
    let t = Instant::now();
    cancel.cancel();
    // What `run_tool` does: the tool has TOOL_CANCEL_GRACE, then it is
    // dropped and the call is recorded as "[cancelled]".
    let in_grace = tokio::time::timeout(harness_agent_native::TOOL_CANCEL_GRACE, &mut h).await;
    let alive_then = !sleeps(SECS).is_empty();
    eprintln!(
        "at the loop's grace ({:?}): the tool has {}; the command is {}",
        t.elapsed(),
        if in_grace.is_ok() {
            "answered"
        } else {
            "not answered: the loop records \"[cancelled]\""
        },
        if alive_then { "ALIVE" } else { "gone" }
    );
    let r = h.await.unwrap();
    eprintln!(
        "the tool's own answer came after {:?}: {}",
        t.elapsed(),
        r.output.lines().last().unwrap_or_default()
    );
    assert!(
        in_grace.is_err() && alive_then,
        "FIXED: the answer came inside the loop's grace"
    );
    assert!(r.output.contains("not confirmed"), "{}", r.output);
    signal(guard, nix::sys::signal::Signal::SIGCONT);
    assert!(gone_within(SECS, Duration::from_secs(3)).is_empty());
}

// ---- N: a session folder that hangs ----

/// PLAN.md T9 now says "the guard starts in `/` (bubblewrap enters the
/// folder), so a folder that hangs stalls the guard, not the node's spawn".
/// What a bash call does when its session folder does not answer: printed,
/// with whether a guard was ever started and whether the cancel returned.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observe_a_bash_call_in_a_folder_that_hangs() {
    let f = fixture();
    let sb = f.sandbox();
    let _clean = Cleanup(f.data.clone(), vec![]);
    let mnt = f.ws.parent().unwrap().join("mnt");
    let mut hang = Hang::mount(&mnt);
    let ws = mnt.join("d");
    let ctx = f.ctx(sb, &ws);
    let cancel = ctx.cancel.clone();
    hang.stall();
    let t = Instant::now();
    let mut h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": "echo hi"})).await });
    tokio::time::sleep(Duration::from_secs(2)).await;
    let guard = guard_of(&f.data);
    eprintln!(
        "2 s into the call: guard {guard:?}; children of the guard: {:?}",
        guard.map(|g| std::fs::read_to_string(format!("/proc/{g}/task/{g}/children"))
            .unwrap_or_default())
    );
    cancel.cancel();
    let r = tokio::time::timeout(Duration::from_secs(8), &mut h).await;
    match &r {
        Ok(r) => eprintln!(
            "the cancel returned after {:?}: {}",
            t.elapsed(),
            r.as_ref().unwrap().output.lines().last().unwrap_or_default()
        ),
        Err(_) => eprintln!(
            "8 s after the cancel the call has NOT returned: it waits in the node, where no \
             cancel reaches it"
        ),
    }
    hang.release();
    if r.is_err() {
        let r = tokio::time::timeout(Duration::from_secs(10), h).await;
        eprintln!(
            "after the folder answered: {:?}",
            r.map(|r| r.unwrap().output)
        );
    }
}

// ---- T8E-5: the guard is a second program outside the sandbox ----

/// The guard is `<the harness binary> sandbox-guard`, started outside
/// bubblewrap and outside Landlock for every command. bubblewrap is only
/// taken where the node's user cannot replace it (T.8a review N-1); the
/// harness binary has no such check. Where a session's folder holds it (a
/// node run from its own build tree, with a session on that tree), one
/// command replaces it and the next command's "guard" is the agent's
/// program: unconfined, in the host's PID namespace, the node's control
/// channel on its stdin. (Before T.8e the same replacement gave the agent
/// an unconfined `sandbox-exec`, but inside the PID namespace.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proof_t8e5_a_replaced_binary_runs_as_the_guard_outside_everything() {
    let f = fixture();
    let bin = f.ws.join("target/debug");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("rizzy-harness");
    if std::fs::hard_link(EXE, &exe).is_err() {
        std::fs::copy(EXE, &exe).unwrap();
    }
    let sb = Sandbox::new(
        exe,
        &Settings::default(),
        Some(f.data.join("sandbox-tmp")),
        std::slice::from_ref(&f.data),
    )
    .unwrap();
    sb.probe(&f.data.join("sandbox-tmp/probe"), &f.data)
        .expect("Landlock");
    let sb = Arc::new(
        bwrap::contained(sb, &f.data.join("sandbox-tmp/probe-contain"))
            .ok()
            .expect("bubblewrap"),
    );
    let _clean = Cleanup(f.data.clone(), vec![]);
    // Not in any grant: beside the data directory and the session folder.
    let outside = f.data.parent().unwrap().join("written-outside-the-sandbox");
    let ctx = f.ctx(sb, &f.ws);
    let direct = Bash
        .run(
            &ctx,
            json!({"command": format!("echo x > {} ; echo direct-write=$?", outside.display())}),
        )
        .await;
    eprintln!("a contained command writing there itself: {}", direct.output.replace('\n', " | "));
    assert!(!outside.exists(), "the control is not confined");
    let replace = format!(
        "mv target/debug/rizzy-harness target/debug/old && printf '%s\\n' '#!/bin/sh' \
         'echo \"ran as $1: pid $$, $(ls /proc | grep -c \"^[0-9]\") processes in /proc, stdin \
         $(readlink /proc/$$/fd/0)\" > {}' 'exit 0' > target/debug/rizzy-harness && \
         chmod +x target/debug/rizzy-harness && echo replaced",
        outside.display()
    );
    let r1 = Bash.run(&ctx, json!({"command": replace})).await;
    eprintln!("command 1: {}", r1.output.replace('\n', " | "));
    let r2 = Bash.run(&ctx, json!({"command": "echo second"})).await;
    eprintln!("command 2: {}", r2.output.replace('\n', " | "));
    let wrote = std::fs::read_to_string(&outside).unwrap_or_default();
    eprintln!("outside every grant, written by command 2's \"guard\": {wrote}");
    assert!(
        wrote.contains("sandbox-guard"),
        "FIXED: the replaced binary did not run as the guard"
    );
}
