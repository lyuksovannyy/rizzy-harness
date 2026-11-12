//! The guard that watches every contained command until it is dead
//! (ADOPTION_PLAN T.8e). Each contained `bash` call runs as node, guard,
//! `bwrap`, init, `sandbox-exec`, `bash`; the guard's stdin is a channel
//! whose other end the node holds, and its closing is the kill order the
//! kernel delivers when the node dies; the guard also watches its parent,
//! for a node that dies while a child of it holds a copy of that end.
//! The `review T8E-*` sections are the proofs of
//! `docs/security-review/2026-11-T8e-guard.md`, inverted.
//!
//! Through the real `rizzy-harness` binary, the real `bwrap` and the real
//! bash tool. Where bubblewrap does not work they skip with a printed
//! reason; with `HARNESS_REQUIRE_BWRAP=1` they fail. The FUSE test also
//! needs `fusermount3` and `python3`; with `HARNESS_REQUIRE_FUSE=1` it
//! fails instead of skipping without them.
//!
//! Every process a test looks for is found from outside by a path unique
//! to that test (its session folder, which is in bubblewrap's and the
//! guard's command lines) or a unique `sleep <n>`, and killed by exactly
//! that pid when the test ends.

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
        self.plain_with(&Settings::default())
    }

    fn plain_with(&self, settings: &Settings) -> Sandbox {
        Sandbox::new(
            PathBuf::from(EXE),
            settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// The contained sandbox, or `None` (and a printed reason) where it
    /// cannot run here.
    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        self.sandbox_with(&Settings::default())
    }

    /// The same with these settings (T.8b: the network off).
    fn sandbox_with(&self, settings: &Settings) -> Option<Arc<Sandbox>> {
        let sb = self.plain_with(settings);
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain"))
            .ok()
            .map(Arc::new)
    }

    /// The contained sandbox without the probes (the helper processes; the
    /// parent test probed).
    fn sandbox_unprobed(&self) -> Arc<Sandbox> {
        Arc::new(
            self.plain()
                .with_containment(harness_tools::contain::Contain::new(
                    harness_tools::contain::find_bwrap().unwrap(),
                    harness_tools::contain::Form::Full,
                )),
        )
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

// ---- finding the test's own processes from outside ----

fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit(')').next()?.trim_start().chars().next()
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

/// Live processes whose command line holds `needle` (a test's unique
/// session folder: bubblewrap, its init and the guard carry it), or is
/// exactly `sleep <secs>`.
fn left(needle: &Path, secs: &[u32]) -> Vec<i32> {
    let n = needle.as_os_str().as_encoded_bytes();
    let me = std::process::id() as i32;
    pids()
        .into_iter()
        .filter(|p| *p != me)
        .filter(|p| {
            let c = cmdline(*p);
            c.windows(n.len()).any(|w| w == n)
                || secs.iter().any(|s| c == format!("sleep\0{s}\0").as_bytes())
        })
        .filter(|p| alive(*p))
        .collect()
}

/// Live processes whose command line is exactly `sleep <secs>`.
fn sleeps(secs: u32) -> Vec<i32> {
    let want = format!("sleep\0{secs}\0");
    pids()
        .into_iter()
        .filter(|p| cmdline(*p) == want.as_bytes() && alive(*p))
        .collect()
}

/// SIGKILL every pid in `pids` (the test's own processes, found by
/// [`left`]).
fn kill_all(pids: &[i32]) {
    for p in pids {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(*p),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}

/// Kills, on drop, whatever [`left`] still finds for this test.
struct Cleanup(PathBuf, Vec<u32>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for _ in 0..3 {
            kill_all(&left(&self.0, &self.1));
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Wait up to `within` for [`left`] to find nothing; what it still finds.
fn gone_within(needle: &Path, secs: &[u32], within: Duration) -> Vec<i32> {
    let by = Instant::now() + within;
    loop {
        let l = left(needle, secs);
        if l.is_empty() || Instant::now() >= by {
            return l;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn describe(pids: &[i32]) -> String {
    pids.iter()
        .map(|p| {
            format!(
                "{p} {:?} {}",
                state(*p),
                String::from_utf8_lossy(&cmdline(*p)).replace('\0', " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- 1. the kill window: the node SIGKILLed 0 to 1 ms after a spawn ----

/// The review's `kill_window.py node`, through the node's own code: a
/// helper process (this test binary re-run) spawns one contained command
/// the way the bash tool does and says so; the test SIGKILLs it 0 to 1 ms
/// later. On `main` before the guard, 170 of 200 namespaces survived
/// (bubblewrap's init sets its death signal late). (A first version read
/// one byte of the helper's stdout, which libtest's "running 1 test" line
/// supplied, so some kills landed before the spawn; the helper now writes
/// a line of its own.)
/// `T8E_TRIES` sets the count (default 200).
#[test]
fn a_kill_of_the_node_just_after_a_spawn_leaves_nothing() {
    const SECS: u32 = 974_801;
    let tries: u32 = std::env::var("T8E_TRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let mut survivors = Vec::new();
    for attempt in 0..tries {
        let ws = f.ws.join(format!("try-{attempt}"));
        std::fs::create_dir_all(&ws).unwrap();
        let _clean = Cleanup(ws.clone(), vec![SECS]);
        let mut helper = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "helper_node_spawns_one_command", "--nocapture"])
            .env("T8E_HELPER_DATA", &f.data)
            .env("T8E_HELPER_WS", &ws)
            .env("T8E_HELPER_SCRIPT", format!("sleep {SECS}"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // The helper writes its line after its spawn returned.
        let mut out = helper.stdout.take().unwrap();
        spawned(&mut out);
        let until = Instant::now() + Duration::from_micros(u64::from(attempt % 11) * 100);
        while Instant::now() < until {
            std::hint::spin_loop();
        }
        helper.kill().unwrap(); // SIGKILL
        let _ = helper.wait();
        let l = gone_within(&ws, &[SECS], Duration::from_secs(2));
        if !l.is_empty() {
            survivors.push((attempt, describe(&l)));
        }
    }
    eprintln!(
        "kill -9 of the node 0 to 1 ms after a spawn: {} of {tries} namespaces left running",
        survivors.len()
    );
    assert!(
        survivors.is_empty(),
        "{} of {tries} namespaces survived the node: {:?}",
        survivors.len(),
        survivors.first()
    );
}

/// The line the helper node writes once its spawn returned.
const SPAWNED: &[u8] = b"\nT8E-SPAWNED\n";

/// Read the helper's stdout up to its [`SPAWNED`] line (and keep the pipe
/// open: a helper that writes into a closed pipe dies).
fn spawned(out: &mut std::process::ChildStdout) {
    let mut seen = Vec::new();
    let mut b = [0u8; 1];
    while !seen.ends_with(SPAWNED) {
        out.read_exact(&mut b).expect("the helper never spawned");
        seen.push(b[0]);
    }
}

/// Not a test on its own: the node of the test above. It spawns one
/// contained command through the node's code, writes one byte, and waits
/// to be killed.
#[tokio::test]
async fn helper_node_spawns_one_command() {
    let (Some(data), Some(ws), Some(script)) = (
        std::env::var_os("T8E_HELPER_DATA"),
        std::env::var_os("T8E_HELPER_WS"),
        std::env::var("T8E_HELPER_SCRIPT").ok(),
    ) else {
        return;
    };
    let f = Fixture {
        _dir: tempfile::tempdir().unwrap(),
        data: data.into(),
        ws: ws.into(),
    };
    let cmd = f.shell(f.sandbox_unprobed(), &script);
    let child = Confinement::spawn(cmd, true).await.unwrap();
    // With `T8E_HELPER_HANG`, one more child started the way the node's
    // git is (`node/src/git.rs`: `current_dir(<the session's folder>)`),
    // into a folder that does not answer: it waits in `chdir`, before
    // `exec`, holding a copy of every descriptor of this node (review
    // T8E-1).
    if let Some(hang) = std::env::var_os("T8E_HELPER_HANG") {
        tokio::spawn(async move {
            let mut git = tokio::process::Command::new("true");
            git.current_dir(hang);
            let _ = git.spawn();
        });
    }
    // A line of its own: libtest writes "running 1 test" first.
    let mut out = std::io::stdout();
    out.write_all(SPAWNED).unwrap();
    out.flush().unwrap();
    tokio::time::sleep(Duration::from_secs(1000)).await;
    drop(child);
}

// ---- 2. cancel returns only after the end is confirmed ----

/// The moment a cancel returns, nothing of the command is left: not the
/// detached children (they ignore SIGTERM, so the kill after the grace is
/// what ends them), not bubblewrap, its init or the guard. No allowance
/// for a kernel that is still tearing the namespace down.
///
/// This guards the contract and proves nothing about the guard: it passes
/// on `main` before T.8e too (review N-4), where the kernel happened to be
/// done by the time the call returned. What the guard adds, the wait for a
/// confirmed end, is proved by the FUSE test below, which fails on `main`.
#[tokio::test]
async fn cancel_returns_only_after_everything_ended() {
    cancel_returns_only_after_everything_ended_with(
        Settings::default(),
        [974_811, 974_812, 974_813],
    )
    .await;
}

/// The same with the network off (T.8b): the command in a network
/// namespace of its own changes nothing about the guard, its kill and its
/// confirmation.
#[tokio::test]
async fn cancel_returns_only_after_everything_ended_with_the_network_off() {
    cancel_returns_only_after_everything_ended_with(
        Settings {
            network: harness_tools::sandbox::Network::Off,
            ..Default::default()
        },
        [974_821, 974_822, 974_823],
    )
    .await;
}

async fn cancel_returns_only_after_everything_ended_with(settings: Settings, secs: [u32; 3]) {
    let f = fixture();
    let Some(sb) = f.sandbox_with(&settings) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), secs.to_vec());
    for round in 0..5 {
        let ctx = f.ctx(sb.clone());
        let cancel = ctx.cancel.clone();
        let cmd = format!(
            "trap '' TERM; setsid sh -c 'trap \"\" TERM; exec sleep {}' & \
             (trap '' TERM; exec sleep {}) & ( setsid sleep {} & ) ; echo ready; wait",
            secs[0], secs[1], secs[2]
        );
        let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
        let by = Instant::now() + Duration::from_secs(20);
        while secs.iter().any(|s| sleeps(*s).is_empty()) {
            assert!(Instant::now() < by, "the children never started");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        cancel.cancel();
        let r = h.await.unwrap();
        let l = left(&f.ws, &secs);
        assert!(
            l.is_empty(),
            "round {round}: the cancel returned ({}) with these still running:\n{}",
            r.output.lines().last().unwrap_or_default(),
            describe(&l)
        );
        assert!(r.output.contains("[cancelled"), "{}", r.output);
    }
}

// ---- 3. what the kernel cannot kill yet is reported, not "cancelled" ----

fn have(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn fuse_required() -> bool {
    std::env::var_os("HARNESS_REQUIRE_FUSE").is_some_and(|v| v == "1")
}

/// The FUSE file system of `support/fuse_stall.py`, mounted at `mnt`:
/// every READ of `mnt/stall` is held until [`Self::release`].
struct Stall {
    server: Option<std::process::Child>,
    lines: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
    mnt: PathBuf,
}

impl Stall {
    fn mount(mnt: &Path) -> Option<Self> {
        if !(have("python3") && have("fusermount3") && Path::new("/dev/fuse").exists()) {
            assert!(!fuse_required(), "no python3, fusermount3 or /dev/fuse");
            eprintln!("SKIPPED: no python3, fusermount3 or /dev/fuse");
            return None;
        }
        std::fs::create_dir_all(mnt).unwrap();
        let mut server = std::process::Command::new("python3")
            .arg("-I")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/fuse_stall.py"
            ))
            .arg(mnt)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = std::io::BufReader::new(server.stdout.take().unwrap()).lines();
        match lines.next() {
            Some(Ok(l)) if l == "mounted" => Some(Self {
                server: Some(server),
                lines,
                mnt: mnt.to_owned(),
            }),
            other => {
                let _ = server.kill();
                let _ = server.wait();
                assert!(!fuse_required(), "FUSE does not mount here: {other:?}");
                eprintln!("SKIPPED: FUSE does not mount here: {other:?}");
                None
            }
        }
    }

    /// Wait until the server holds a READ.
    fn held(&mut self) {
        self.expect("read");
    }

    /// Wait until the server prints `what` (`read`, `lookup`).
    fn expect(&mut self, what: &str) {
        assert_eq!(
            self.lines.next().map(|l| l.unwrap()).as_deref(),
            Some(what),
            "the server never saw the {what}"
        );
    }

    /// End the server (its stdin closed, and SIGKILL: a process stuck
    /// before `exec` may hold a copy of that pipe): the kernel aborts the
    /// connection and every held request fails.
    fn release(&mut self) {
        if let Some(mut s) = self.server.take() {
            drop(s.stdin.take());
            let _ = s.kill();
            let _ = s.wait();
        }
    }
}

impl Drop for Stall {
    fn drop(&mut self) {
        if let Some(s) = &mut self.server {
            let _ = s.kill();
        }
        self.release();
        let _ = std::process::Command::new("fusermount3")
            .args(["-u", "-z", "--"])
            .arg(&self.mnt)
            .status();
    }
}

/// A command stuck in a read the kernel will not interrupt (a FUSE read
/// already in user space waits uninterruptibly even after SIGKILL) cannot
/// be ended by any kill. The call says so instead of reporting the
/// command cancelled, within the agent loop's grace for a cancelled tool,
/// and once the read is released the process is gone and nothing is left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_the_kernel_cannot_kill_yet_is_reported_still_alive() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mnt = f.ws.join("mnt");
    let Some(mut stall) = Stall::mount(&mnt) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), vec![]);
    let file = mnt.join("stall");
    let ctx = f.ctx(sb);
    let cancel = ctx.cancel.clone();
    let cmd = format!("cat {}", file.display());
    let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
    tokio::task::block_in_place(|| stall.held());
    let want = [
        b"cat\0".as_slice(),
        file.as_os_str().as_encoded_bytes(),
        b"\0",
    ]
    .concat();
    let cat = pids()
        .into_iter()
        .find(|p| cmdline(*p) == want)
        .expect("the reader");
    let t = Instant::now();
    cancel.cancel();
    let r = h.await.unwrap();
    let took = t.elapsed();
    let stuck = alive(cat);
    eprintln!(
        "cancel returned after {took:?}; reader {cat} {:?}; result: {}",
        state(cat),
        r.output.lines().last().unwrap_or_default()
    );
    assert!(
        stuck,
        "the kernel ended the reader: the case is not exercised"
    );
    assert!(
        !r.output.contains("[cancelled"),
        "reported as cancelled while {cat} still runs: {}",
        r.output
    );
    assert!(r.output.contains("still alive"), "{}", r.output);
    assert!(
        took < harness_agent_native::TOOL_CANCEL_GRACE,
        "the report came after the loop abandons the tool: {took:?}"
    );
    tokio::task::block_in_place(|| stall.release());
    let l = gone_within(&f.ws, &[], Duration::from_secs(2));
    assert!(!alive(cat), "the reader outlived its release");
    assert!(l.is_empty(), "left after the release:\n{}", describe(&l));
}

// ---- 4. the guard itself ----

/// The test's guard: a process whose command line starts `<exe>
/// sandbox-guard` and holds `needle`.
fn guard_of(needle: &Path) -> Option<i32> {
    let n = needle.as_os_str().as_encoded_bytes();
    let head = [
        EXE.as_bytes(),
        b"\0",
        harness_tools::guard::SUBCOMMAND.as_bytes(),
        b"\0",
    ]
    .concat();
    pids().into_iter().find(|p| {
        let c = cmdline(*p);
        c.starts_with(&head) && c.windows(n.len()).any(|w| w == n) && alive(*p)
    })
}

/// The children of `pid` (its main thread's).
fn children(pid: i32) -> Vec<i32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect()
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let by = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < by, "{what} never happened");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The detached children of `cli/tests/contain.rs`: `setsid`, `nohup`, a
/// double fork with `setsid`, one that ignores SIGTERM, a plain one.
fn detached(base: u32) -> (String, Vec<u32>) {
    let s: Vec<u32> = (0..5).map(|i| base + i).collect();
    let cmd = format!(
        "setsid sleep {} & nohup sleep {} >/dev/null 2>&1 & ( setsid sleep {} & ) ; \
         sh -c 'trap \"\" TERM; exec sleep {}' & sleep {} & sleep 1000",
        s[0], s[1], s[2], s[3], s[4]
    );
    (cmd, s)
}

/// The second line: a `kill -9` of the guard alone (not the node) still
/// ends the namespace, through bubblewrap's `--die-with-parent`, which
/// names the guard's only thread, and its init's own death signal.
#[tokio::test]
async fn a_kill_of_the_guard_alone_ends_the_namespace() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let (cmd, secs) = detached(974_821);
    let _clean = Cleanup(f.ws.clone(), secs.clone());
    let ctx = f.ctx(sb);
    let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
    wait_for("the detached children", || {
        secs.iter().all(|s| !sleeps(*s).is_empty())
    })
    .await;
    let guard = guard_of(&f.ws).expect("the guard");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(guard),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    let l = tokio::task::spawn_blocking({
        let ws = f.ws.clone();
        let secs = secs.clone();
        move || gone_within(&ws, &secs, Duration::from_secs(2))
    })
    .await
    .unwrap();
    assert!(
        l.is_empty(),
        "left 2 s after the guard's kill:\n{}",
        describe(&l)
    );
    let r = tokio::time::timeout(Duration::from_secs(5), h)
        .await
        .expect("the call never returned")
        .unwrap();
    // Not a plain exit code: the guard died, and the tool said so after
    // it looked for what the guard left (review N-7, T8E-2).
    assert!(
        r.output.contains("its guard was killed (signal 9)"),
        "{}",
        r.output
    );
}

/// A command cannot reach, signal or outlive its guard, and does not hold
/// its control channel: the guard is outside the PID namespace (its host
/// number names nothing inside, and `kill -KILL -1` inside reaches only
/// the namespace), the channel is not among the command's descriptors (it
/// has no socket at all), and no signal is blocked in it (the guard blocks
/// SIGCHLD for its `signalfd`; the mask must not leak). When the call
/// returns, the guard is gone too: no idle process.
#[tokio::test]
async fn a_command_cannot_reach_signal_or_outlive_its_guard() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![974_831]);
    let gpid = f.ws.join("gpid");
    let ctx = f.ctx(sb);
    let cmd = format!(
        "sleep 974831 & ls -l /proc/$$/fd; grep -E '^Sig(Blk|Ign)' /proc/self/status; \
         while [ ! -s {g} ]; do sleep 0.01; done; g=$(cat {g}); \
         test -e /proc/$g && echo GUARD-VISIBLE; \
         kill -KILL $g; echo kill-guard=$?; kill -STOP $g; echo stop-guard=$?; \
         kill -KILL -1; echo kill-all=$?; echo done",
        g = gpid.display()
    );
    let h = tokio::spawn(async move { Bash.run(&ctx, json!({"command": cmd})).await });
    let mut guard = None;
    wait_for("the guard", || {
        guard = guard_of(&f.ws);
        guard.is_some()
    })
    .await;
    let guard = guard.unwrap();
    let channel = std::fs::read_dir(format!("/proc/{guard}/fd"))
        .unwrap()
        .filter_map(|e| std::fs::read_link(e.ok()?.path()).ok())
        .map(|l| l.to_string_lossy().into_owned())
        .find(|l| l.starts_with("socket:"))
        .expect("the guard's channel");
    let rss = std::fs::read_to_string(format!("/proc/{guard}/status"))
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS"))
        .unwrap_or_default()
        .to_string();
    eprintln!("the guard's memory while its command runs: {rss}");
    std::fs::write(&gpid, guard.to_string()).unwrap();
    let r = tokio::time::timeout(Duration::from_secs(20), h)
        .await
        .expect("the call never returned")
        .unwrap();
    let out = &r.output;
    assert!(r.ok, "{out}");
    assert!(out.contains("done"), "{out}");
    assert!(
        !out.contains(&channel),
        "the command holds {channel}: {out}"
    );
    assert!(
        !out.contains("socket:"),
        "the command holds a socket: {out}"
    );
    assert!(out.contains("SigBlk:\t0000000000000000"), "{out}");
    assert!(!out.contains("GUARD-VISIBLE"), "{out}");
    assert!(
        out.contains("kill-guard=1") && out.contains("stop-guard=1"),
        "{out}"
    );
    assert!(out.contains("kill-all=0"), "{out}");
    let l = left(&f.ws, &[974_831]);
    assert!(
        l.is_empty(),
        "left when the call returned:\n{}",
        describe(&l)
    );
    assert!(!alive(guard), "the guard outlived its command");
}

// ---- 5. the sweep at the node's start ----

/// The node's contained command line for `script`, split: bubblewrap and
/// its arguments, without the guard in front (what a dead guard leaves).
fn bwrap_argv(f: &Fixture, sb: Arc<Sandbox>, script: &str) -> Vec<std::ffi::OsString> {
    let cmd = f.shell(sb, script);
    let all: Vec<std::ffi::OsString> = cmd.as_std().get_args().map(Into::into).collect();
    let at = all.iter().position(|a| a == "--").unwrap();
    all[at + 1..].to_vec()
}

/// Start `argv` orphaned (through a shell that exits at once), as a
/// namespace whose guard and node are gone; `--die-with-parent` is dropped
/// so it outlives that shell: the stand-in for a guard killed in the
/// moment before bubblewrap or its init set the death signal. Returns the
/// pid of the program started (bubblewrap), as the shell reports it.
fn orphan(argv: &[std::ffi::OsString]) -> i32 {
    let argv: Vec<_> = argv.iter().filter(|a| *a != "--die-with-parent").collect();
    let out = std::process::Command::new("sh")
        .args(["-c", "\"$@\" </dev/null >/dev/null 2>&1 & echo $!", "sh"])
        .args(argv)
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

fn replace_mark(argv: &[std::ffi::OsString], to: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut out = Vec::new();
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == harness_tools::guard::MARK || a == harness_tools::guard::MARK_SESSION {
            let v = it.next().unwrap();
            if let Some(to) = to {
                out.push(a.clone());
                out.push(if a == harness_tools::guard::MARK {
                    to.as_os_str().to_owned()
                } else {
                    v.clone()
                });
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// A restarted node ends what a doubly unlucky kill left: a bubblewrap
/// whose guard is gone, and an init whose bubblewrap is gone too, each
/// found by the marker naming this node's data directory, named with its
/// session, and gone when the sweep returns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restarted_node_sweeps_what_a_dead_guard_left() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let root = f.data.join("sandbox-tmp");
    for (secs, kill_bwrap) in [(974_841, false), (974_842, true)] {
        let _clean = Cleanup(f.ws.clone(), vec![secs]);
        let bwrap = orphan(&bwrap_argv(&f, sb.clone(), &format!("sleep {secs}")));
        wait_for("the orphan", || !sleeps(secs).is_empty()).await;
        if kill_bwrap {
            // bubblewrap gone, its init (with no death signal) left. The
            // init is taken as bubblewrap's own child, not guessed from the
            // command's parent: how many processes sit between the two
            // differs between bubblewrap versions and shells (the guess
            // killed the init itself on CI's Ubuntu).
            let inits = children(bwrap);
            assert_eq!(
                inits.len(),
                1,
                "bubblewrap {bwrap} should have one child, its init:\n{}",
                describe(&[&[bwrap][..], &inits[..]].concat())
            );
            let init = inits[0];
            let tree =
                describe(&[&[bwrap, init][..], &children(init)[..], &sleeps(secs)[..]].concat());
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(bwrap),
                nix::sys::signal::Signal::SIGKILL,
            )
            .unwrap();
            wait_for("bubblewrap's end", || !alive(bwrap)).await;
            assert!(
                alive(init) && !sleeps(secs).is_empty(),
                "the init did not survive bubblewrap; before the kill:\n{tree}\nafter:\n{}",
                describe(&[&[init][..], &sleeps(secs)[..]].concat())
            );
        }
        let swept = tokio::task::spawn_blocking({
            let root = root.clone();
            move || harness_tools::contain::sweep(&root)
        })
        .await
        .unwrap();
        // Usually one orphan; in about 2 runs of 7 the sweep finds two
        // (cause not established: both carry the mark and both ended).
        // Every one found must be this session's, and ended.
        assert!(!swept.is_empty(), "{swept:?}");
        assert!(
            swept.iter().all(|s| s.session == "s1" && s.ended),
            "{swept:?}"
        );
        let l = left(&f.ws, &[secs]);
        assert!(l.is_empty(), "left after the sweep:\n{}", describe(&l));
    }
}

/// The sweep never touches a process without the marker, one marked for
/// another data directory, or a namespace whose guard is alive (here a
/// running call): all three still run after it, and it reports nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_touches_nothing_else() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let root = f.data.join("sandbox-tmp");
    let (unmarked, other, live) = (974_851, 974_852, 974_853);
    let _clean = Cleanup(f.ws.clone(), vec![unmarked, other, live]);
    orphan(&replace_mark(
        &bwrap_argv(&f, sb.clone(), &format!("sleep {unmarked}")),
        None,
    ));
    orphan(&replace_mark(
        &bwrap_argv(&f, sb.clone(), &format!("sleep {other}")),
        Some(&f.ws.join("another-node/sandbox-tmp")),
    ));
    let ctx = f.ctx(sb);
    let cancel = ctx.cancel.clone();
    let h = tokio::spawn(async move {
        Bash.run(&ctx, json!({"command": format!("sleep {live}")}))
            .await
    });
    wait_for("all three", || {
        [unmarked, other, live]
            .iter()
            .all(|s| !sleeps(*s).is_empty())
    })
    .await;
    let swept = tokio::task::spawn_blocking({
        let root = root.clone();
        move || harness_tools::contain::sweep(&root)
    })
    .await
    .unwrap();
    assert!(swept.is_empty(), "{swept:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    for s in [unmarked, other, live] {
        assert!(!sleeps(s).is_empty(), "the sweep ended sleep {s}");
    }
    cancel.cancel();
    let r = h.await.unwrap();
    assert!(r.output.contains("[cancelled"), "{}", r.output);
}

// ---- 6. what the guard costs ----

/// Per command, the guard adds one `exec` of the harness binary and a few
/// system calls. Measured here against T.8a's command line (bubblewrap
/// started directly, the guard's part stripped), both running `true` to
/// the end and collecting the output, alternately, `T8E_RUNS` times each
/// (default 40). The bound is loose (a debug build on a loaded machine);
/// the numbers are printed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_guard_costs_little_per_command() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let runs: usize = std::env::var("T8E_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let (mut with, mut without) = (Vec::new(), Vec::new());
    for _ in 0..runs {
        let mut cmd = f.shell(sb.clone(), "true");
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let t = Instant::now();
        let child = Confinement::spawn(cmd, true).await.unwrap();
        let harness_tools::ShellChild::Contained(c) = child else {
            panic!("not contained")
        };
        let out = c.wait_with_output().await.unwrap();
        with.push(t.elapsed());
        assert!(out.status.success(), "{out:?}");

        let argv = bwrap_argv(&f, sb.clone(), "true");
        let mut direct = tokio::process::Command::new(&argv[0]);
        direct
            .args(&argv[1..])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0);
        let t = Instant::now();
        let out = direct.output().await.unwrap();
        without.push(t.elapsed());
        assert!(out.status.success(), "{out:?}");
    }
    let median = |v: &mut Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    let mean = |v: &[Duration]| v.iter().sum::<Duration>() / v.len() as u32;
    let (mw, mo) = (mean(&with), mean(&without));
    let (dw, d_o) = (median(&mut with), median(&mut without));
    eprintln!(
        "per command, {runs} runs: with the guard median {dw:?} mean {mw:?}; bubblewrap \
         directly (T.8a) median {d_o:?} mean {mo:?}; the guard adds {:?} (median)",
        dw.saturating_sub(d_o)
    );
    assert!(
        dw < d_o + Duration::from_millis(15),
        "the guard adds {:?}",
        dw.saturating_sub(d_o)
    );
}

// ---- review T8E-1: a child of the node waiting before `exec` ----

/// The reviewer's proof inverted (`docs/security-review/2026-11-T8e-guard.md`
/// T8E-1). The node has one contained command and one more child, started
/// the way its git is, waiting in `chdir` into a folder that does not
/// answer: before `exec`, so it holds a copy of the node's end of every
/// guard's channel. The node is SIGKILLed; the channel does not close. The
/// guard must learn of the node's death another way (its parent changes):
/// the command is gone within 2 s.
#[test]
fn a_node_killed_while_a_child_waits_before_exec_leaves_nothing() {
    const SECS: u32 = 974_861;
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let mnt = f.ws.parent().unwrap().join("mnt-hang");
    let Some(mut stall) = Stall::mount(&mnt) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let mut node = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "helper_node_spawns_one_command", "--nocapture"])
        .env("T8E_HELPER_DATA", &f.data)
        .env("T8E_HELPER_WS", &f.ws)
        .env("T8E_HELPER_SCRIPT", format!("sleep {SECS}"))
        .env("T8E_HELPER_HANG", mnt.join("hang"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut out = node.stdout.take().unwrap();
    spawned(&mut out);
    stall.expect("lookup");
    let by = Instant::now() + Duration::from_secs(20);
    while sleeps(SECS).is_empty() {
        assert!(Instant::now() < by, "the command never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let t = Instant::now();
    node.kill().unwrap(); // SIGKILL
    let _ = node.wait();
    let l = gone_within(&f.ws, &[SECS], Duration::from_secs(2));
    eprintln!(
        "node SIGKILLed with a child stuck before exec: {} after {:?}",
        if l.is_empty() { "nothing left" } else { "LEFT" },
        t.elapsed()
    );
    stall.release();
    assert!(
        l.is_empty(),
        "still running 2 s after the node died:\n{}",
        describe(&l)
    );
}

/// A guard's command line for `script`, started from the binary's path
/// (this process's descriptor means nothing to a shell), with its
/// `--node` value replaced by `node` (`None`: without `--node`).
fn guard_argv(
    f: &Fixture,
    sb: Arc<Sandbox>,
    script: &str,
    node: Option<&str>,
) -> Vec<std::ffi::OsString> {
    let cmd = f.shell(sb, script);
    let mut argv: Vec<std::ffi::OsString> = vec![EXE.into()];
    let mut it = cmd.as_std().get_args();
    while let Some(a) = it.next() {
        if a == "--node" {
            it.next();
            if let Some(n) = node {
                argv.push("--node".into());
                argv.push(n.into());
            }
        } else {
            argv.push(a.into());
        }
    }
    argv
}

/// Start `argv` under a shell that stays its parent (`wait`), its stdin a
/// pipe the returned handle holds open (`<&9` overrides the `/dev/null` a
/// shell gives `&`); `@NODE@` in it becomes the shell's pid. The command
/// line is in a script file, so the shell's own command line carries no
/// mark (a process that does is killed by the sweep, by design).
fn under_shell(
    dir: &Path,
    argv: &[std::ffi::OsString],
) -> (std::process::Child, Option<std::process::ChildStdin>) {
    let quote = |a: &std::ffi::OsStr| {
        let a = a.to_str().unwrap();
        if a == "@NODE@" {
            "\"$$\"".to_string()
        } else {
            format!("'{}'", a.replace('\'', "'\\''"))
        }
    };
    let line: Vec<String> = argv.iter().map(|a| quote(a)).collect();
    let script = dir.join(format!("under-shell-{}.sh", std::process::id()));
    std::fs::write(
        &script,
        format!(
            "exec 9<&0\n{} <&9 >/dev/null 2>&1 &\nwait\n",
            line.join(" ")
        ),
    )
    .unwrap();
    let mut sh = std::process::Command::new("sh")
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let held = sh.stdin.take();
    (sh, held)
}

/// The second line for T8E-1: a guard whose node died (its parent is no
/// longer the pid of its own `--node`) and that is still ending a command
/// the kernel cannot end yet is told to stop (SIGTERM, its kill order) by
/// the sweep at start, and named as still busy; once the read is released
/// everything is gone. A guard with no `--node` the sweep can read is left
/// alone and named (re-check R-1: never judged by who is starting).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restarted_node_ends_a_guard_whose_node_died() {
    const SECS: u32 = 974_862;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mnt = f.ws.join("mnt");
    let Some(mut stall) = Stall::mount(&mnt) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let root = f.data.join("sandbox-tmp");
    let sweep = |root: PathBuf| async move {
        tokio::task::spawn_blocking(move || harness_tools::contain::sweep(&root))
            .await
            .unwrap()
    };

    // A guard with no `--node`: left alone, named.
    let (mut sh, held) = under_shell(
        f.data.parent().unwrap(),
        &guard_argv(&f, sb.clone(), &format!("sleep {SECS}"), None),
    );
    wait_for("the command", || !sleeps(SECS).is_empty()).await;
    let unknown = guard_of(&f.ws).expect("the guard");
    let swept = sweep(root.clone()).await;
    eprintln!("a guard without --node: {swept:?}");
    assert!(
        swept
            .iter()
            .any(|s| s.pid == unknown && s.what == harness_tools::contain::Left::GuardUnknown),
        "{swept:?}"
    );
    assert!(!sleeps(SECS).is_empty(), "the sweep ended it");
    drop(held);
    let _ = sh.wait();
    let by = Instant::now() + Duration::from_secs(3);
    while (alive(unknown) || !sleeps(SECS).is_empty()) && Instant::now() < by {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !alive(unknown) && sleeps(SECS).is_empty(),
        "the channel's close did not end it"
    );

    // A guard whose node died, busy with a stuck read.
    let file = mnt.join("stall");
    let (mut sh, held) = under_shell(
        f.data.parent().unwrap(),
        &guard_argv(&f, sb, &format!("cat {}", file.display()), Some("@NODE@")),
    );
    tokio::task::block_in_place(|| stall.held());
    let guard = guard_of(&f.ws).expect("the guard");
    sh.kill().unwrap(); // its node dies
    let _ = sh.wait();
    let swept = sweep(root).await;
    eprintln!("a guard of a dead node: {swept:?}");
    assert!(
        swept.iter().any(|s| s.pid == guard
            && s.what == harness_tools::contain::Left::Guard
            && !s.ended
            && s.session == "s1"),
        "{swept:?}"
    );
    tokio::task::block_in_place(|| stall.release());
    drop(held);
    let l = gone_within(&f.ws, &[], Duration::from_secs(3));
    assert!(l.is_empty(), "left after the release:\n{}", describe(&l));
}

// ---- review T8E-2: a guard that dies in its first millisecond ----

/// `kill_windows.py guard` through the bash tool: the guard is SIGKILLed 0
/// to 1 ms after its bubblewrap appears, the node alive. Before, 61 of 260
/// namespaces ran on until the node's next start. Now the tool sees its
/// guard die of a signal, ends what the guard left for that session
/// before it returns, and says so: nothing is left when it has returned.
/// Meanwhile another command of the same session runs under its live
/// guard, and that sweep must not touch it. `T8E_TRIES` (default 60).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guard_killed_in_its_first_millisecond_leaves_nothing_once_the_tool_returned() {
    const SECS: u32 = 974_871;
    const OTHER: u32 = 974_872;
    let tries: u32 = std::env::var("T8E_TRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![SECS, OTHER]);
    let other_ctx = f.ctx(sb.clone());
    let other_cancel = other_ctx.cancel.clone();
    let other = tokio::spawn(async move {
        Bash.run(&other_ctx, json!({"command": format!("sleep {OTHER}")}))
            .await
    });
    wait_for("the other command", || !sleeps(OTHER).is_empty()).await;
    let other_guard = guard_of(&f.ws).expect("the other guard");
    let mut survivors = Vec::new();
    for attempt in 0..tries {
        let ctx = f.ctx(sb.clone());
        let h = tokio::spawn(async move {
            Bash.run(&ctx, json!({"command": format!("sleep {SECS}")}))
                .await
        });
        let ws = f.ws.clone();
        let killed = tokio::task::spawn_blocking(move || {
            let by = Instant::now() + Duration::from_secs(5);
            loop {
                let g = pids().into_iter().find(|p| {
                    *p != other_guard && {
                        let c = cmdline(*p);
                        c.split(|b| *b == 0).nth(1)
                            == Some(harness_tools::guard::SUBCOMMAND.as_bytes())
                            && c.windows(ws.as_os_str().len())
                                .any(|w| w == ws.as_os_str().as_encoded_bytes())
                    }
                });
                if let Some(g) = g
                    && !children(g).is_empty()
                {
                    let until =
                        Instant::now() + Duration::from_micros(u64::from(attempt % 11) * 100);
                    while Instant::now() < until {
                        std::hint::spin_loop();
                    }
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(g),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                    return true;
                }
                if Instant::now() > by {
                    return false;
                }
            }
        })
        .await
        .unwrap();
        assert!(killed, "try {attempt}: no guard to kill");
        let r = tokio::time::timeout(Duration::from_secs(10), h)
            .await
            .expect("the call never returned")
            .unwrap();
        assert!(
            r.output.contains("its guard was killed"),
            "try {attempt}: {}",
            r.output
        );
        let l: Vec<i32> = left(&f.ws, &[SECS])
            .into_iter()
            .filter(|p| *p != other_guard && !is_under(*p, other_guard))
            .collect();
        if !l.is_empty() {
            survivors.push((attempt, describe(&l)));
            kill_all(&l);
        }
    }
    let other_alive = !sleeps(OTHER).is_empty() && alive(other_guard);
    other_cancel.cancel();
    let r = other.await.unwrap();
    eprintln!(
        "a guard SIGKILLed 0 to 1 ms after its bubblewrap appeared: {} of {tries} namespaces \
         left once the tool returned",
        survivors.len()
    );
    assert!(
        other_alive,
        "the sweep ended the other command: {}",
        r.output
    );
    assert!(survivors.is_empty(), "{survivors:?}");
}

/// Whether `pid` descends from `ancestor`.
fn is_under(pid: i32, ancestor: i32) -> bool {
    let mut p = pid;
    for _ in 0..64 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{p}/stat")) else {
            return false;
        };
        let Some(pp) = stat
            .rsplit(')')
            .next()
            .and_then(|f| f.split_whitespace().nth(1))
            .and_then(|x| x.parse::<i32>().ok())
        else {
            return false;
        };
        if pp == ancestor {
            return true;
        }
        if pp <= 1 {
            return false;
        }
        p = pp;
    }
    false
}

/// A stop signal to the guard (SIGTERM, the sweep's kill order) is not the
/// command's own exit: the result says its guard was told to stop, not a
/// plain `[exit code: 137]` (review N-7).
#[tokio::test]
async fn a_stop_signal_to_the_guard_is_not_reported_as_the_commands_exit() {
    const SECS: u32 = 974_875;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let ctx = f.ctx(sb);
    let h = tokio::spawn(async move {
        Bash.run(&ctx, json!({"command": format!("sleep {SECS}")}))
            .await
    });
    wait_for("the command", || !sleeps(SECS).is_empty()).await;
    let guard = guard_of(&f.ws).unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(guard),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let r = h.await.unwrap();
    assert!(!r.output.contains("[exit code: 137]"), "{}", r.output);
    assert!(
        r.output.contains("its guard was told to stop"),
        "{}",
        r.output
    );
    assert!(left(&f.ws, &[SECS]).is_empty());
}

// ---- review T8E-3: a guard that does not answer ----

/// The node's own bound for a guard that does not answer, with the drain
/// after it and a margin, fits inside the agent loop's grace for a
/// cancelled tool: the real constants, not copies.
#[test]
fn the_stop_and_its_drain_fit_the_loops_grace() {
    let need = harness_tools::contain::STOP_BOUND
        + harness_tools::contain::SWEEP_IN_STOP
        + harness_tools::contain::STUCK_DRAIN
        + Duration::from_millis(200);
    assert!(
        need <= harness_agent_native::TOOL_CANCEL_GRACE,
        "{need:?} > {:?}",
        harness_agent_native::TOOL_CANCEL_GRACE
    );
}

/// The reviewer's proof inverted (T8E-3): the guard cannot answer (it is
/// stopped, the stand-in for one that is starved or frozen). The tool's
/// result comes inside the loop's grace, says the end is not confirmed,
/// and does not say "cancelled".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guard_that_does_not_answer_is_reported_within_the_loops_grace() {
    const SECS: u32 = 974_881;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let ctx = f.ctx(sb);
    let cancel = ctx.cancel.clone();
    let mut h = tokio::spawn(async move {
        Bash.run(&ctx, json!({"command": format!("sleep {SECS}")}))
            .await
    });
    wait_for("the command", || !sleeps(SECS).is_empty()).await;
    let guard = guard_of(&f.ws).unwrap();
    let pid = nix::unistd::Pid::from_raw(guard);
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGSTOP).unwrap();
    let t = Instant::now();
    cancel.cancel();
    let r = tokio::time::timeout(harness_agent_native::TOOL_CANCEL_GRACE, &mut h).await;
    let took = t.elapsed();
    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGCONT).unwrap();
    let r = match r {
        Ok(r) => r.unwrap(),
        Err(_) => panic!(
            "no answer inside the loop's grace ({took:?}): the loop records [cancelled]; \
             later: {}",
            h.await.unwrap().output
        ),
    };
    eprintln!(
        "answered after {took:?}: {}",
        r.output.lines().last().unwrap_or_default()
    );
    assert!(r.output.contains("not confirmed"), "{}", r.output);
    assert!(!r.output.contains("[cancelled"), "{}", r.output);
    let l = gone_within(&f.ws, &[SECS], Duration::from_secs(3));
    assert!(l.is_empty(), "left:\n{}", describe(&l));
}

// ---- review T8E-4: the sweep and a leftover the kernel cannot end yet ----

/// A namespace a dead node and a dead guard left, whose command is in a
/// FUSE read the kernel will not interrupt: the sweep kills it but must not
/// call it ended while the reader lives (the start logs it STILL ALIVE).
/// The next start finds nothing marked and kills nothing; once the read is
/// released, it is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sweep_does_not_call_a_stuck_leftover_ended() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let mnt = f.ws.join("mnt");
    let Some(mut stall) = Stall::mount(&mnt) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), vec![]);
    let file = mnt.join("stall");
    orphan(&bwrap_argv(&f, sb, &format!("cat {}", file.display())));
    tokio::task::block_in_place(|| stall.held());
    let root = f.data.join("sandbox-tmp");
    let sweep = |root: PathBuf| async move {
        tokio::task::spawn_blocking(move || harness_tools::contain::sweep(&root))
            .await
            .unwrap()
    };
    let first = sweep(root.clone()).await;
    eprintln!("first start: {first:?}");
    assert!(!first.is_empty(), "the sweep found nothing");
    assert!(first.iter().all(|s| !s.ended), "called ended: {first:?}");
    // The next start: the stuck init carries no mark any more; nothing is
    // named and nothing killed (re-check R-2: no file of pids is kept).
    let second = sweep(root.clone()).await;
    eprintln!("second start: {second:?}");
    assert!(second.is_empty(), "{second:?}");
    tokio::task::block_in_place(|| stall.release());
    let l = gone_within(&f.ws, &[], Duration::from_secs(2));
    assert!(l.is_empty(), "left after the release:\n{}", describe(&l));
    let third = sweep(root).await;
    assert!(third.is_empty(), "{third:?}");
}

// ---- review T8E-5: the binary a command may replace ----

/// The reviewer's proof inverted (T8E-5): the harness binary lies in the
/// session's folder (a node run from its build tree). One command
/// replaces it; the next command's guard must still be the node's own
/// image, not the agent's file. (Its `sandbox-exec`, started inside the
/// namespace from the path, is the replacement: that is the residual
/// `doctor` names, closed by T.8c's mount view.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_binary_does_not_run_as_the_guard() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let bin = f.ws.join("target/debug");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("rizzy-harness");
    if std::fs::hard_link(EXE, &exe).is_err() {
        std::fs::copy(EXE, &exe).unwrap();
    }
    let sb = Sandbox::new(
        exe.clone(),
        &Settings::default(),
        Some(f.data.join("sandbox-tmp")),
        std::slice::from_ref(&f.data),
    )
    .unwrap();
    let Ok(sb) = bwrap::contained(sb, &f.data.join("sandbox-tmp/probe-contain")) else {
        panic!("bubblewrap worked a moment ago")
    };
    let sb = Arc::new(sb);
    let _clean = Cleanup(f.ws.clone(), vec![]);
    let outside = f.data.parent().unwrap().join("written-outside-the-sandbox");
    let ctx = f.ctx(sb);
    let replace = format!(
        "mv target/debug/rizzy-harness target/debug/old && printf '%s\\n' '#!/bin/sh' \
         'echo \"ran as $1\" >> {}' 'exit 0' > target/debug/rizzy-harness && \
         chmod +x target/debug/rizzy-harness && echo replaced",
        outside.display()
    );
    let r1 = Bash.run(&ctx, json!({"command": replace})).await;
    assert!(r1.output.contains("replaced"), "{}", r1.output);
    let r2 = Bash.run(&ctx, json!({"command": "echo second"})).await;
    let wrote = std::fs::read_to_string(&outside).unwrap_or_default();
    eprintln!(
        "command 2: {}; written outside: {wrote:?}",
        r2.output.replace('\n', " | ")
    );
    assert!(
        !wrote.contains("sandbox-guard"),
        "the replaced binary ran as the guard: {wrote}"
    );
}

/// `doctor` warns when the harness binary lies where a session can write
/// (a `[sandbox] write` entry here), and `contain = "required"` refuses.
#[test]
fn doctor_warns_when_a_session_can_replace_the_binary() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let dir = Path::new(EXE).parent().unwrap().canonicalize().unwrap();
    let doctor = |toml: String| {
        std::fs::write(f.data.join("node.toml"), toml).unwrap();
        let out = std::process::Command::new(EXE)
            .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.contains(" shell containment: "))
            .unwrap_or_default()
            .to_string()
    };
    let line = doctor(format!("[sandbox]\nwrite = [\"{}\"]\n", dir.display()));
    assert!(line.starts_with("warn "), "{line}");
    assert!(line.contains("can replace the harness binary"), "{line}");
    let line = doctor(format!(
        "[sandbox]\ncontain = \"required\"\nwrite = [\"{}\"]\n",
        dir.display()
    ));
    assert!(line.starts_with("FAIL "), "{line}");
}

// ---- review N-4: the sweep through the node's own start ----

/// A namespace left for this data directory is ended by a real node start
/// (`serve`, which runs `start_sandbox`), and the log names it.
#[test]
fn the_node_start_sweeps_what_a_dead_node_left() {
    const SECS: u32 = 974_891;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    orphan(&bwrap_argv(&f, sb, &format!("sleep {SECS}")));
    let by = Instant::now() + Duration::from_secs(20);
    while sleeps(SECS).is_empty() {
        assert!(Instant::now() < by, "the leftover never started");
        std::thread::sleep(Duration::from_millis(10));
    }
    let (listening, log) = serve_once(&f.data);
    let gone = sleeps(SECS).is_empty();
    let log = log.join("\n");
    assert!(listening, "the node never served: {log}");
    assert!(gone, "the leftover still runs after the start: {log}");
    assert!(
        log.contains("ended a command a previous run of this node left running")
            && log.contains("session \"s1\""),
        "{log}"
    );
}

/// One real node start (`serve` on `data`, loopback, any port), stopped as
/// soon as it serves: whether it served, and its log.
fn serve_once(data: &Path) -> (bool, Vec<String>) {
    let mut serve = std::process::Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .env("HARNESS_LOG", "warn")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(serve.stdout.take().unwrap()).lines();
    let err = serve.stderr.take().unwrap();
    let log = std::thread::spawn(move || {
        std::io::BufReader::new(err)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<_>>()
    });
    let listening = lines
        .by_ref()
        .map_while(Result::ok)
        .any(|l| l.contains("listening on"));
    let _ = serve.kill();
    let _ = serve.wait();
    (listening, log.join().unwrap())
}

/// An ordinary `sleep <secs>` of this test, no mark: the victim the sweep
/// must never touch. Its pid and start time (ticks since boot).
fn victim(secs: u32) -> (std::process::Child, i32, u64) {
    let c = std::process::Command::new("sleep")
        .arg(secs.to_string())
        .spawn()
        .unwrap();
    let pid = c.id() as i32;
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let start = stat
        .rsplit(')')
        .next()
        .unwrap()
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse()
        .unwrap();
    (c, pid, start)
}

// ---- re-check R-1: a second start on a live node's data directory ----

/// The reviewer's proof inverted (re-check R-1): a node is alive with one
/// command; the real binary is started on the same data directory. Its
/// sweep must not tell the live node's guard to stop (its parent is still
/// the pid in its own `--node`): the command runs to its own end, and the
/// second start logs nothing about it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_start_leaves_the_live_nodes_commands_alone() {
    const SECS: u32 = 974_901;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let ctx = f.ctx(sb);
    let h = tokio::spawn(async move {
        Bash.run(
            &ctx,
            json!({"command": format!("sleep {SECS} & s=$!; sleep 4; kill $s; echo finished")}),
        )
        .await
    });
    wait_for("the command", || !sleeps(SECS).is_empty()).await;
    let data = f.data.clone();
    let (listening, log) = tokio::task::spawn_blocking(move || serve_once(&data))
        .await
        .unwrap();
    let log = log.join("\n");
    assert!(listening, "the second start never served: {log}");
    let r = tokio::time::timeout(Duration::from_secs(20), h)
        .await
        .expect("the command never ended")
        .unwrap();
    assert!(
        r.ok && r.output.contains("finished"),
        "the live node's command was stopped: {} | log: {log}",
        r.output
    );
    assert!(!log.contains("previous run"), "{log}");
}

// ---- re-check R-2: nothing outside the marks is killed ----

/// The reviewer's `leftovers_file.py` inverted (re-check R-2): a file at
/// `<data>/sandbox-leftovers` naming an unmarked process (directly, then
/// as a link to a file elsewhere) makes no start kill anything: the victim
/// survives two starts.
#[test]
fn a_planted_leftovers_file_kills_nothing() {
    const SECS: u32 = 974_911;
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let (mut v, pid, start) = victim(SECS);
    let file = f.data.join("sandbox-leftovers");
    let elsewhere = f.ws.join("elsewhere");
    for link in [false, true] {
        let _ = std::fs::remove_file(&file);
        let text = format!("{pid} {start} some-session\n");
        if link {
            std::fs::write(&elsewhere, &text).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        } else {
            std::fs::write(&file, &text).unwrap();
        }
        for n in 1..=2 {
            let (listening, log) = serve_once(&f.data);
            assert!(listening, "{log:?}");
            assert!(
                v.try_wait().unwrap().is_none(),
                "start {n} (file as a link: {link}) killed the victim: {log:?}"
            );
        }
    }
    let _ = v.kill();
    let _ = v.wait();
}

/// The reviewer's `leftovers_inject.py` inverted (re-check R-2): a marked
/// process stuck in a read the kernel will not interrupt, whose session
/// text holds a line break and a victim's pid and start time. Two starts:
/// the victim survives, and the line naming the marked process is one
/// line, its session escaped.
#[test]
fn a_session_with_a_line_break_injects_nothing() {
    const SECS: u32 = 974_912;
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let mnt = f.ws.join("mnt");
    let Some(mut stall) = Stall::mount(&mnt) else {
        return;
    };
    let _clean = Cleanup(f.ws.clone(), vec![SECS]);
    let (mut v, pid, start) = victim(SECS);
    let session = format!("s1\n{pid} {start} injected");
    let root = f.data.join("sandbox-tmp");
    let mut reader = std::process::Command::new("python3")
        .args(["-I", "-c", "import sys; open(sys.argv[1], 'rb').read()"])
        .arg(mnt.join("stall"))
        .arg("--mark")
        .arg(&root)
        .arg("--mark-session")
        .arg(&session)
        .spawn()
        .unwrap();
    stall.held();
    for n in 1..=2 {
        let (listening, log) = serve_once(&f.data);
        assert!(listening, "{log:?}");
        assert!(
            v.try_wait().unwrap().is_none(),
            "start {n} killed the victim: {log:?}"
        );
        for l in &log {
            assert!(
                !l.contains("injected") || l.contains("shell containment"),
                "start {n}: a line of its own from the session text: {log:?}"
            );
        }
        if n == 1 {
            assert!(
                log.iter()
                    .any(|l| l.contains("STILL ALIVE") && l.contains("\\n")),
                "start 1: {log:?}"
            );
        }
    }
    stall.release();
    let _ = reader.wait();
    let _ = v.kill();
    let _ = v.wait();
}
