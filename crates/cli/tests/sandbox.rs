//! The agent's shell in the Landlock sandbox (FIX_PLAN 2.7), through the
//! real `rizzy-harness sandbox-exec` wrapper and the real bash tool.
//!
//! On a kernel without Landlock these tests skip with a printed reason, but
//! when `CI` is set they fail: a green CI run must mean they ran.
//!
//! The file-policy tests run twice where bubblewrap works: with Landlock
//! alone (the fallback) and inside the shell's containment (T.8a), which
//! must not change what a command may open. Not twice: the start-up
//! self-test (it is `sandbox::start`'s, run through the test binary; the
//! contained start is `contain.rs` `a_started_node_contains_its_shell`) and
//! the B.1 tests at the end, which record what a node without bubblewrap
//! leaves running (contained, `contain.rs` shows them gone).

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::sandbox::{Confinement, Mode, Network, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A data directory with an API token, a fake home with an ssh key, and a
/// project folder; all inside one temp dir that the sandbox does not grant.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    home: PathBuf,
    ws: PathBuf,
    /// The shell runs inside bubblewrap (T.8a).
    contained: bool,
}

/// One fixture per way the shell can run here: Landlock alone, and inside
/// the containment where bubblewrap works.
fn fixtures() -> Vec<Fixture> {
    let mut all = vec![fixture()];
    let mut f = fixture();
    let sb = f.sandbox(Settings::default());
    let sb = Arc::try_unwrap(sb).unwrap();
    if bwrap::contained(sb, &f.data.join("sandbox-tmp/probe-contain")).is_ok() {
        f.contained = true;
        all.push(f);
    }
    all
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let home = root.join("home");
    let ws = home.join("proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(data.join("api-token"), "API-TOKEN-CANARY\n").unwrap();
    std::fs::write(home.join(".ssh/id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        home,
        ws,
        contained: false,
    }
}

impl Fixture {
    fn sandbox(&self, settings: Settings) -> Arc<Sandbox> {
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap();
        Arc::new(if self.contained {
            let bwrap = harness_tools::contain::find_bwrap().unwrap();
            sb.with_containment(harness_tools::contain::Contain::new(
                bwrap,
                harness_tools::contain::Form::Full,
            ))
        } else {
            sb
        })
    }

    fn ctx(&self, sandbox: Arc<Sandbox>, cwd: &Path) -> ToolCtx {
        let mut ctx = ToolCtx::new(cwd);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined { sandbox, tmp: None }.for_session("s1");
        ctx
    }

    async fn bash(&self, settings: Settings, cwd: &Path, cmd: &str) -> String {
        let ctx = self.ctx(self.sandbox(settings), cwd);
        Bash.run(&ctx, json!({ "command": cmd })).await.output
    }
}

/// The wrapper's status line on this kernel, or `None` (and a printed
/// reason) where Landlock does not work. Panics on CI instead.
fn landlock() -> Option<String> {
    let f = fixture();
    let sb = f.sandbox(Settings::default());
    match sb.probe(&f.data.join("sandbox-tmp/probe"), &f.data) {
        Ok(s) => Some(s),
        Err(e) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI must run the sandbox tests on a Landlock kernel: {e}"
            );
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            None
        }
    }
}

/// The Landlock ABI from the status line (`effective_abi: V4` → 4).
fn abi(status: &str) -> u32 {
    let rest = status.split("effective_abi: V").nth(1).unwrap_or("0");
    rest.chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

#[tokio::test]
async fn ssh_key_and_api_token_are_permission_denied() {
    let Some(_) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        ssh_key_and_api_token_are_permission_denied_in(f).await;
    }
}

async fn ssh_key_and_api_token_are_permission_denied_in(f: Fixture) {
    let cmd = format!(
        "export HOME={}; cat ~/.ssh/id_ed25519; cat {}/api-token; ls {}; echo end",
        f.home.display(),
        f.data.display(),
        f.data.display()
    );
    let out = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(!out.contains("SSH-KEY-CANARY"), "{out}");
    assert!(!out.contains("API-TOKEN-CANARY"), "{out}");
    assert!(
        !out.lines().any(|l| l.trim() == "api-token"),
        "listing the data dir: {out}"
    );
    if f.contained {
        // The file view (T.8c): what is not granted is not there at all.
        // The data directory exists only as the empty folder above the
        // session's temp folder, and Landlock still refuses to list it.
        assert_eq!(out.matches("No such file or directory").count(), 2, "{out}");
        assert_eq!(out.matches("Permission denied").count(), 1, "{out}");
    } else {
        assert_eq!(out.matches("Permission denied").count(), 3, "{out}");
    }
    assert!(out.contains("end"), "{out}");
}

/// The node's private link key (`<data>/keys/node.key`, the path `node run`
/// uses) cannot be read from the agent's shell: directly, through a `..`
/// path, or through a symlink planted in the project folder.
#[tokio::test]
async fn node_key_is_unreadable_from_the_shell() {
    let Some(_) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        node_key_is_unreadable_from_the_shell_in(f).await;
    }
}

async fn node_key_is_unreadable_from_the_shell_in(f: Fixture) {
    let keys = f.data.join("keys");
    std::fs::create_dir_all(&keys).unwrap();
    let key = keys.join("node.key");
    std::fs::write(&key, "NODE-KEY-CANARY\n").unwrap();
    std::fs::write(f.ws.join("plain.txt"), "PROJECT-FILE-CANARY\n").unwrap();
    // Control 1: the key exists at this path and is readable without the sandbox.
    assert_eq!(std::fs::read_to_string(&key).unwrap(), "NODE-KEY-CANARY\n");
    // `..` from the project folder reaches the same file.
    let rel = Path::new("../../data/keys/node.key");
    assert_eq!(
        std::fs::read_to_string(f.ws.join(rel)).unwrap(),
        "NODE-KEY-CANARY\n"
    );
    std::os::unix::fs::symlink(&key, f.ws.join("link-to-key")).unwrap();
    let cmd = format!(
        "cat plain.txt; echo c=$?; cat {key}; echo d=$?; cat {rel}; echo r=$?; \
         cat link-to-key; echo l=$?; cat {keys}/../keys/node.key; echo t=$?; echo end",
        key = key.display(),
        rel = rel.display(),
        keys = keys.display()
    );
    let out = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(!out.contains("NODE-KEY-CANARY"), "key bytes leaked: {out}");
    // Control 2: a file in the project folder reads fine in the same shell.
    assert!(
        out.contains("PROJECT-FILE-CANARY") && out.contains("c=0"),
        "{out}"
    );
    for tag in ["d", "r", "l", "t"] {
        assert!(
            out.contains(&format!("{tag}=1")),
            "{tag} did not fail: {out}"
        );
    }
    let (denied, absent) = (
        out.matches("Permission denied").count(),
        out.matches("No such file or directory").count(),
    );
    if f.contained {
        // The file view (T.8c): absent, not denied.
        assert_eq!((denied, absent), (0, 4), "{out}");
    } else {
        assert_eq!(denied, 4, "{out}");
    }
    assert!(out.contains("end"), "{out}");
}

#[tokio::test]
async fn writes_outside_the_project_are_denied() {
    let Some(_) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        writes_outside_the_project_are_denied_in(f).await;
    }
}

async fn writes_outside_the_project_are_denied_in(f: Fixture) {
    let cmd = format!(
        "touch {home}/planted; truncate -s 0 {data}/api-token; rm -f {data}/api-token; \
         mv {data}/api-token ./stolen; ln {data}/api-token ./hard; echo x > /tmp/rizzy-sbx-$$; \
         echo end",
        home = f.home.display(),
        data = f.data.display()
    );
    let out = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(out.contains("end"), "{out}");
    assert!(!f.home.join("planted").exists(), "{out}");
    assert_eq!(
        std::fs::read_to_string(f.data.join("api-token")).unwrap(),
        "API-TOKEN-CANARY\n",
        "{out}"
    );
    assert!(!f.ws.join("stolen").exists() && !f.ws.join("hard").exists());
    assert!(!out.contains("API-TOKEN-CANARY"), "{out}");
}

/// What ordinary work needs: write and run things in the project, `TMPDIR`,
/// `/dev/null`, `/proc/self`, process substitution.
#[tokio::test]
async fn a_build_in_the_project_works() {
    let Some(_) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        a_build_in_the_project_works_in(f).await;
    }
}

async fn a_build_in_the_project_works_in(f: Fixture) {
    std::fs::write(
        f.ws.join("hello.c"),
        "#include <stdio.h>\nint main(void){puts(\"built-and-ran\");return 0;}\n",
    )
    .unwrap();
    let have_cc = std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let build = if have_cc {
        "cc -o hello hello.c && ./hello"
    } else {
        assert!(std::env::var_os("CI").is_none(), "CI needs a C compiler");
        eprintln!("no cc here: building a shell script instead");
        "printf '#!/bin/sh\\necho built-and-ran\\n' > hello && chmod +x hello && ./hello"
    };
    let cmd = format!(
        "set -e; {build}; mkdir -p target/sub; echo ok > target/sub/f; \
         t=$(mktemp); echo tmp-ok > \"$t\"; cat \"$t\"; echo \"TMPDIR=$TMPDIR\"; \
         echo quiet > /dev/null; head -c 4 /dev/urandom | wc -c; \
         grep -c Name /proc/self/status; cat <(echo procsub-ok)"
    );
    let out = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(out.contains("built-and-ran"), "{out}");
    assert!(out.contains("tmp-ok"), "{out}");
    assert!(out.contains("procsub-ok"), "{out}");
    assert!(out.contains("[exit code: 0]"), "{out}");
    let tmp = f.data.join("sandbox-tmp/s1");
    assert!(out.contains(&format!("TMPDIR={}", tmp.display())), "{out}");
    assert!(f.ws.join("target/sub/f").exists());
}

/// `/proc` is readable, but not the harness's own environment, its open
/// files or its folder: Landlock denies ptrace-class access outside the
/// domain. (The fd *numbers* can be listed; their targets cannot be read.)
#[tokio::test]
async fn the_harness_environment_is_not_readable_through_proc() {
    let Some(status) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        the_harness_environment_is_not_readable_through_proc_in(f, &status).await;
    }
}

async fn the_harness_environment_is_not_readable_through_proc_in(f: Fixture, _status: &str) {
    let out = f
        .bash(
            Settings::default(),
            &f.ws,
            "test $PPID = $(cut -d' ' -f4 /proc/$$/stat) && echo parent-known; \
             cat /proc/$PPID/environ | tr '\\0' '\\n' | grep -c . ; \
             for n in $(ls /proc/$PPID/fd); do readlink /proc/$PPID/fd/$n && echo LEAK; \
             head -c1 /proc/$PPID/fd/$n >/dev/null 2>&1 && echo OPENED; done; \
             ls /proc/$PPID/cwd/; cat /proc/$PPID/mem; echo end",
        )
        .await;
    assert!(out.contains("parent-known"), "{out}");
    assert!(
        !out.contains("CARGO"),
        "the harness's variables leaked: {out}"
    );
    assert!(!out.contains("LEAK") && !out.contains("OPENED"), "{out}");
    assert!(out.matches("Permission denied").count() >= 3, "{out}");
    assert!(out.contains("end"), "{out}");
}

#[tokio::test]
async fn network_off_denies_tcp_connect() {
    let Some(status) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        network_off_denies_tcp_connect_in(f, &status).await;
    }
}

async fn network_off_denies_tcp_connect_in(f: Fixture, status: &str) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let cmd = format!("exec 3<>/dev/tcp/127.0.0.1/{port} && echo connected");
    let on = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(on.contains("connected"), "network on: {on}");
    let off = Settings {
        network: Network::Off,
        ..Default::default()
    };
    if abi(status) < 4 {
        // Required + off on an old kernel: the command does not run at all.
        let out = f.bash(off, &f.ws, &cmd).await;
        assert!(out.contains("cannot enforce the sandbox"), "{out}");
        return;
    }
    let out = f.bash(off, &f.ws, &cmd).await;
    assert!(!out.contains("connected"), "network off: {out}");
    assert!(out.contains("Permission denied"), "{out}");
}

/// Landlock domains only narrow: the agent running the wrapper itself with
/// `--rw /` gains nothing.
#[tokio::test]
async fn a_nested_sandbox_exec_gains_nothing() {
    let Some(status) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        a_nested_sandbox_exec_gains_nothing_in(f, &status).await;
    }
}

async fn a_nested_sandbox_exec_gains_nothing_in(f: Fixture, _status: &str) {
    // Let the shell see the harness binary so it can try.
    let bin_dir = PathBuf::from(EXE).canonicalize().unwrap();
    let settings = Settings {
        read: vec![bin_dir.parent().unwrap().to_path_buf()],
        ..Default::default()
    };
    let cmd = format!(
        "{EXE} sandbox-exec --status --best-effort --rw system / -- cat {data}/api-token; \
         {EXE} sandbox-exec --rw system / -- sh -c 'echo nested-ran; touch {home}/planted'; echo end",
        data = f.data.display(),
        home = f.home.display()
    );
    let out = f.bash(settings, &f.ws, &cmd).await;
    assert!(out.contains("end"), "{out}");
    // The nested wrapper really ran (and confined itself further).
    assert!(out.contains("sandbox status:"), "{out}");
    assert!(out.contains("nested-ran"), "{out}");
    assert!(!out.contains("API-TOKEN-CANARY"), "{out}");
    assert!(!f.home.join("planted").exists(), "{out}");
}

/// A pathname Unix socket outside the grants (a session bus, the Docker
/// socket, an ssh-agent): contained, not there at all (the file view,
/// T.8c) on every ABI; with Landlock alone, denied from ABI 9 on, and below
/// it an OPEN residual of that fallback (PLAN.md R2): the test then records
/// that it connects.
#[tokio::test]
async fn pathname_unix_sockets_need_abi_9() {
    let Some(status) = landlock() else { return };
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        pathname_unix_sockets_need_abi_9_in(f, &status).await;
    }
}

async fn pathname_unix_sockets_need_abi_9_in(f: Fixture, status: &str) {
    let sock = f.root.join("agent.sock");
    let _l = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let have_py = std::process::Command::new("python3")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success());
    if !have_py {
        eprintln!("no python3: cannot connect to a unix socket from bash");
        return;
    }
    let cmd = format!(
        "python3 -c 'import socket; s=socket.socket(socket.AF_UNIX); s.connect(\"{}\"); \
         print(\"unix-\" + \"connected\")'",
        sock.display()
    );
    let out = f.bash(Settings::default(), &f.ws, &cmd).await;
    if f.contained {
        // The file view (T.8c): the socket is not there, on every ABI.
        assert!(!out.contains("unix-connected"), "{out}");
        assert!(out.contains("FileNotFoundError"), "{out}");
    } else if abi(status) >= 9 {
        assert!(!out.contains("unix-connected"), "{out}");
        assert!(out.contains("PermissionError"), "{out}");
    } else {
        eprintln!("OPEN RESIDUAL (Landlock ABI {} < 9): {out}", abi(status));
        assert!(out.contains("unix-connected"), "{out}");
    }
}

/// Signals stay inside the sandbox from ABI 6 on: the shell cannot signal
/// the harness.
#[tokio::test]
async fn signals_to_the_harness_are_denied() {
    let Some(status) = landlock() else { return };
    if abi(&status) < 6 {
        eprintln!("Landlock ABI < 6: no signal scoping here");
        return;
    }
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        signals_to_the_harness_are_denied_in(f).await;
    }
}

/// Contained, `$PPID` is the namespace's init, outside the domain too.
async fn signals_to_the_harness_are_denied_in(f: Fixture) {
    let out = f
        .bash(Settings::default(), &f.ws, "kill -0 $PPID; echo rc=$?")
        .await;
    assert!(out.contains("rc=1"), "{out}");
}

/// The start-up self-test refuses on a kernel that cannot do what node.toml
/// asks; here it works and returns a confined runtime.
#[test]
fn start_reports_enforced_and_names_the_abi() {
    let Some(status) = landlock() else { return };
    assert!(status.contains("effective_abi"), "{status}");
    // `start` resolves the wrapper with current_exe, which in this test is
    // the test binary: a non-sandbox command line makes the probe fail, so
    // `required` must refuse, naming the setting.
    let f = fixture();
    let s = harness_tools::sandbox::start(
        &Settings::default(),
        &f.data,
        std::slice::from_ref(&f.data),
        &[],
        &f.data.join("node.toml"),
    )
    .unwrap();
    let why = s.refusal.expect("the test binary is no sandbox wrapper");
    assert!(
        why.contains("[sandbox]") && why.contains("node.toml"),
        "{why}"
    );
    assert!(matches!(s.confinement, Confinement::Refused(_)));
    let off = harness_tools::sandbox::start(
        &Settings {
            mode: Mode::Off,
            ..Default::default()
        },
        &f.data,
        std::slice::from_ref(&f.data),
        &[],
        &f.data.join("node.toml"),
    )
    .unwrap();
    assert!(off.refusal.is_none() && off.summary.contains("OFF"));
}

// ---- Experiment B.1 (ADOPTION_PLAN): processes that leave the group ----
//
// Landlock restricts file access, not process control, so the sandbox is
// expected to change nothing about what the tool's `killpg` can reach. Every
// process a test expects to survive is recorded by pid and killed by exactly
// that pid, after checking its command line, by `Reaper`.

fn pid_alive(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat
            .rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z'),
        Err(_) => false,
    }
}

fn is_our_sleep(pid: i32, secs: u32) -> bool {
    pid_alive(pid)
        && std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|c| c == format!("sleep\0{secs}\0").as_bytes())
            .unwrap_or(false)
}

struct Reaper {
    pid: i32,
    secs: u32,
}

impl Drop for Reaper {
    fn drop(&mut self) {
        if is_our_sleep(self.pid, self.secs) {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(self.pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

/// Run `command` in the sandbox, wait for the recorded `sleep <secs>`, end
/// the call by cancel or by a 1 s timeout, wait two seconds, and say whether
/// that pid is still alive.
async fn sandboxed_stop_and_look(command: &str, secs: u32, timeout: bool) -> (Reaper, bool) {
    let f = fixture();
    let pidfile = f.ws.join("pid");
    let command = command.replace("PIDFILE", &pidfile.display().to_string());
    let ctx = f.ctx(f.sandbox(Settings::default()), &f.ws);
    let cancel = ctx.cancel.clone();
    let input = if timeout {
        json!({"command": command, "timeout_secs": 1})
    } else {
        json!({"command": command})
    };
    let h = tokio::spawn(async move { Bash.run(&ctx, input).await });
    let mut reaper = None;
    for _ in 0..250 {
        if let Ok(s) = std::fs::read_to_string(&pidfile)
            && let Ok(pid) = s.trim().parse::<i32>()
            && is_our_sleep(pid, secs)
        {
            reaper = Some(Reaper { pid, secs });
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let Some(reaper) = reaper else {
        h.abort();
        panic!("the sandboxed command never recorded a `sleep {secs}` pid");
    };
    if !timeout {
        cancel.cancel();
    }
    let r = h.await.unwrap();
    assert!(
        r.output
            .contains(if timeout { "timed out" } else { "cancelled" }),
        "{}",
        r.output
    );
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let alive = is_our_sleep(reaper.pid, secs);
    (reaper, alive)
}

/// Control: inside the sandbox a same-group background child is killed.
#[tokio::test]
async fn b1_control_sandboxed_same_group_child_dies_on_cancel() {
    let Some(_) = landlock() else { return };
    let (_r, alive) = sandboxed_stop_and_look(
        "sh -c 'echo $$ > PIDFILE; exec sleep 987611' & wait",
        987_611,
        false,
    )
    .await;
    assert!(!alive, "control failed: a same-group child survived");
}

/// RESIDUAL WITHOUT CONTAINMENT (PLAN.md T9): Landlock alone (no
/// bubblewrap, `contain = "auto"`'s fallback) does not change it; a child
/// that called `setsid` survives a cancel. With the containment it dies:
/// `tests/contain.rs`, whose flipped form of this test failed on `main`.
#[tokio::test]
async fn a_setsid_child_survives_cancel_in_the_sandbox() {
    let Some(_) = landlock() else { return };
    let (_r, alive) = sandboxed_stop_and_look(
        "setsid sh -c 'echo $$ > PIDFILE; exec sleep 987612' & wait",
        987_612,
        false,
    )
    .await;
    assert!(
        alive,
        "the setsid child died without the containment: update PLAN.md T9"
    );
}

/// RESIDUAL WITHOUT CONTAINMENT: the same, ended by the timeout.
#[tokio::test]
async fn a_setsid_child_survives_timeout_in_the_sandbox() {
    let Some(_) = landlock() else { return };
    let (_r, alive) = sandboxed_stop_and_look(
        "setsid sh -c 'echo $$ > PIDFILE; exec sleep 987613' & wait",
        987_613,
        true,
    )
    .await;
    assert!(
        alive,
        "the setsid child died without the containment: update PLAN.md T9"
    );
}
