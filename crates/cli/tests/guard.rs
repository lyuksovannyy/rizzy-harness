//! The guard that watches every contained command until it is dead
//! (ADOPTION_PLAN T.8e). Each contained `bash` call runs as node, guard,
//! `bwrap`, init, `sandbox-exec`, `bash`; the guard's stdin is a channel
//! whose other end only the node holds, and its closing is the kill order
//! the kernel delivers even when the node dies.
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
        Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// The contained sandbox, or `None` (and a printed reason) where it
    /// cannot run here.
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
/// later. Before the guard, 20 of 20 namespaces survived a kill 0 to
/// 500 us after the spawn (bubblewrap's init sets its death signal late).
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
        let mut out = helper.stdout.take().unwrap();
        let mut b = [0u8; 1];
        // The helper writes one byte after its spawn returned.
        out.read_exact(&mut b).expect("the helper never spawned");
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
    let mut out = std::io::stdout();
    out.write_all(b"x").unwrap();
    out.flush().unwrap();
    tokio::time::sleep(Duration::from_secs(1000)).await;
    drop(child);
}

// ---- 2. cancel returns only after the end is confirmed ----

/// The moment a cancel returns, nothing of the command is left: not the
/// detached children (they ignore SIGTERM, so the kill after the grace is
/// what ends them), not bubblewrap, its init or the guard. No allowance
/// for a kernel that is still tearing the namespace down: the guard
/// reaped it before the call returned.
#[tokio::test]
async fn cancel_returns_only_after_everything_ended() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let secs = [974_811, 974_812, 974_813];
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
        assert_eq!(
            self.lines.next().map(|l| l.unwrap()).as_deref(),
            Some("read"),
            "the server never saw the read"
        );
    }

    /// Close the server's stdin: it exits, the kernel aborts the
    /// connection and the held READ fails.
    fn release(&mut self) {
        if let Some(mut s) = self.server.take() {
            drop(s.stdin.take());
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
    assert!(r.output.contains("[exit code: signal]"), "{}", r.output);
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
/// moment before bubblewrap or its init set the death signal.
fn orphan(argv: &[std::ffi::OsString]) {
    let argv: Vec<_> = argv.iter().filter(|a| *a != "--die-with-parent").collect();
    let st = std::process::Command::new("sh")
        .args(["-c", "\"$@\" </dev/null >/dev/null 2>&1 &", "sh"])
        .args(argv)
        .status()
        .unwrap();
    assert!(st.success());
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
        orphan(&bwrap_argv(&f, sb.clone(), &format!("sleep {secs}")));
        wait_for("the orphan", || !sleeps(secs).is_empty()).await;
        if kill_bwrap {
            // bubblewrap gone, its init (with no death signal) left.
            let init = pids()
                .into_iter()
                .find(|p| {
                    children(*p)
                        .iter()
                        .any(|c| cmdline(*c) == format!("sleep\0{secs}\0").as_bytes())
                })
                .expect("the namespace's init");
            let bwrap = std::fs::read_to_string(format!("/proc/{init}/stat"))
                .unwrap()
                .rsplit(')')
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<i32>()
                .unwrap();
            assert!(cmdline(bwrap).starts_with(b"/"), "{}", describe(&[bwrap]));
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(bwrap),
                nix::sys::signal::Signal::SIGKILL,
            )
            .unwrap();
            wait_for("bubblewrap's end", || !alive(bwrap)).await;
            assert!(
                !sleeps(secs).is_empty(),
                "the init did not survive bubblewrap"
            );
        }
        let swept = tokio::task::spawn_blocking({
            let root = root.clone();
            move || harness_tools::contain::sweep(&root)
        })
        .await
        .unwrap();
        assert_eq!(swept.len(), 1, "{swept:?}");
        assert_eq!(swept[0].session, "s1");
        assert!(swept[0].ended, "{swept:?}");
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
