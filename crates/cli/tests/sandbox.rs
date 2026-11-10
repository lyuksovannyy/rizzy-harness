//! The agent's shell in the Landlock sandbox (FIX_PLAN 2.7), through the
//! real `rizzy-harness sandbox-exec` wrapper and the real bash tool.
//!
//! On a kernel without Landlock these tests skip with a printed reason, but
//! when `CI` is set they fail: a green CI run must mean they ran.

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
    }
}

impl Fixture {
    fn sandbox(&self, settings: Settings) -> Arc<Sandbox> {
        Arc::new(
            Sandbox::new(
                PathBuf::from(EXE),
                &settings,
                Some(self.data.join("sandbox-tmp")),
                std::slice::from_ref(&self.data),
            )
            .unwrap(),
        )
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
    let f = fixture();
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
    assert_eq!(out.matches("Permission denied").count(), 3, "{out}");
    assert!(out.contains("end"), "{out}");
}

#[tokio::test]
async fn writes_outside_the_project_are_denied() {
    let Some(_) = landlock() else { return };
    let f = fixture();
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
    let f = fixture();
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
    let Some(_) = landlock() else { return };
    let f = fixture();
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
    let f = fixture();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let cmd = format!("exec 3<>/dev/tcp/127.0.0.1/{port} && echo connected");
    let on = f.bash(Settings::default(), &f.ws, &cmd).await;
    assert!(on.contains("connected"), "network on: {on}");
    let off = Settings {
        network: Network::Off,
        ..Default::default()
    };
    if abi(&status) < 4 {
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
    let Some(_) = landlock() else { return };
    let f = fixture();
    // Let the shell see the harness binary so it can try.
    let bin_dir = PathBuf::from(EXE).canonicalize().unwrap();
    let settings = Settings {
        read: vec![bin_dir.parent().unwrap().to_path_buf()],
        ..Default::default()
    };
    let cmd = format!(
        "{EXE} sandbox-exec --status --best-effort --rw / -- cat {data}/api-token; \
         {EXE} sandbox-exec --rw / -- sh -c 'echo nested-ran; touch {home}/planted'; echo end",
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

/// A sub-agent's worktree (`<repo>/.git/harness-worktrees/<id>`) keeps its
/// git metadata in the main repository's `.git`: status and commit work, the
/// main working tree stays out of reach.
#[tokio::test]
async fn git_works_in_a_sub_agent_worktree() {
    let Some(_) = landlock() else { return };
    let f = fixture();
    let repo = &f.ws;
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "base"]);
    let wt = repo.join(".git/harness-worktrees/child1");
    git(&["worktree", "add", "--detach", "-q", wt.to_str().unwrap()]);
    let out = f
        .bash(
            Settings::default(),
            &wt,
            &format!(
                "export HOME={home}; echo b > b.txt && git status --short && git add b.txt && \
                 git -c user.name=a -c user.email=a@a commit -qm sub && git log --oneline | wc -l; \
                 echo planted > {repo}/planted.txt; echo end",
                home = f.home.display(),
                repo = repo.display()
            ),
        )
        .await;
    assert!(out.contains("?? b.txt"), "{out}");
    assert!(out.lines().any(|l| l.trim() == "2"), "commit failed: {out}");
    assert!(!repo.join("planted.txt").exists(), "{out}");
    // Control: an ordinary worktree elsewhere gets no grant on the main
    // `.git` (a `.git` file's contents never widen the sandbox).
    let other = f.home.join("wt2");
    git(&["worktree", "add", "--detach", "-q", other.to_str().unwrap()]);
    let out = f
        .bash(
            Settings::default(),
            &other,
            "git status --short; echo rc=$?",
        )
        .await;
    assert!(!out.contains("rc=0"), "{out}");
}

/// A pathname Unix socket outside the grants (a session bus, the Docker
/// socket, an ssh-agent): denied from Landlock ABI 9 on. Below it this is
/// an OPEN residual (PLAN.md R2); the test then records that it connects.
#[tokio::test]
async fn pathname_unix_sockets_need_abi_9() {
    let Some(status) = landlock() else { return };
    let f = fixture();
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
    if abi(&status) >= 9 {
        assert!(!out.contains("unix-connected"), "{out}");
        assert!(out.contains("PermissionError"), "{out}");
    } else {
        eprintln!("OPEN RESIDUAL (Landlock ABI {} < 9): {out}", abi(&status));
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
    let f = fixture();
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
        &f.data.join("node.toml"),
    )
    .unwrap();
    assert!(off.refusal.is_none() && off.summary.contains("OFF"));
}
