//! The read-only shell (ADOPTION_PLAN T.7, X.7; docs/read-only-shell.md):
//! `bash` with `ToolCtx::read_only_shell` runs contained, the chat's folder
//! and everything else bound read-only, only a sized `/tmp` of its own
//! writable. Enforced by the mounts (and Landlock inside), never by reading
//! the command: every write below is an ordinary command, and each test
//! checks the file system from outside afterwards, not the exit code alone.
//!
//! Each write is also run once with the read-only shell off (`control`), so
//! a test that passes because the command was wrong cannot pass here: the
//! same command must change the folder there.
//!
//! Through the real `rizzy-harness sandbox-exec`, the real bubblewrap and
//! the real bash tool. Where bubblewrap does not work they skip with a
//! printed reason; with `HARNESS_REQUIRE_BWRAP=1` they fail. A bubblewrap
//! without `--size` (older than this test assumes) has no read-only shell:
//! the tests say so and check that it is refused instead.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx, ToolOutcome};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    ws: PathBuf,
    /// A node.toml `[sandbox] write` entry outside the folder.
    extra: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    let extra = root.join("cache");
    for d in [&data, &ws, &extra] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(ws.join("marker"), "original\n").unwrap();
    std::fs::write(extra.join("kept"), "original\n").unwrap();
    // A repository with one commit, made here as the person would have it.
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .current_dir(&ws)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["add", "marker"]);
    git(&["commit", "-q", "-m", "first"]);
    // A change not committed, for `git diff`.
    std::fs::write(ws.join("marker"), "original\nchanged\n").unwrap();
    Fixture {
        _dir: dir,
        data,
        ws,
        extra,
    }
}

impl Fixture {
    fn settings(&self) -> Settings {
        Settings {
            write: vec![self.extra.clone()],
            ..Default::default()
        }
    }

    fn plain(&self) -> Sandbox {
        self.plain_with(self.settings())
    }

    fn plain_with(&self, settings: Settings) -> Sandbox {
        Sandbox::new(
            PathBuf::from(EXE),
            &settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// The contained sandbox, or `None` where it cannot run here.
    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        self.sandbox_with(self.settings())
    }

    fn sandbox_with(&self, settings: Settings) -> Option<Arc<Sandbox>> {
        let sb = self.plain_with(settings);
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        let sb = bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain")).ok()?;
        Some(Arc::new(sb))
    }

    fn ctx(&self, conf: Confinement, read_only: bool) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = conf.for_session("s1");
        ctx.read_only_shell = read_only;
        ctx
    }

    fn marker(&self) -> String {
        std::fs::read_to_string(self.ws.join("marker")).unwrap()
    }
}

fn confined(sb: Arc<Sandbox>) -> Confinement {
    Confinement::Confined {
        sandbox: sb,
        tmp: None,
    }
}

async fn bash(ctx: &ToolCtx, cmd: &str) -> ToolOutcome {
    Bash.run(ctx, json!({ "command": cmd, "timeout_secs": 60 }))
        .await
}

/// The contained sandbox with a read-only shell, or `None` (said) where
/// this bubblewrap has none: then the shell must be refused, never run.
async fn ready(f: &Fixture) -> Option<Arc<Sandbox>> {
    let sb = f.sandbox()?;
    ready_with(f, sb).await
}

async fn ready_with(f: &Fixture, sb: Arc<Sandbox>) -> Option<Arc<Sandbox>> {
    if let Err(why) = confined(sb.clone()).read_only_shell() {
        let out = bash(&f.ctx(confined(sb), true), "touch refused-here").await;
        assert!(!out.ok, "{}", out.output);
        assert!(!f.ws.join("refused-here").exists());
        // A runner that must contain (CI) must have it: never green by
        // skipping (review of PR 125).
        assert!(
            !bwrap::required(),
            "HARNESS_REQUIRE_BWRAP=1 but there is no read-only shell here: {why}"
        );
        eprintln!("SKIPPED (no read-only shell with this bubblewrap): {why}");
        return None;
    }
    Some(sb)
}

/// Every write the brief names, as ordinary commands, each checked from
/// outside: nothing in the folder changed. The same commands with the
/// read-only shell off do change it (the control at the end).
#[tokio::test]
async fn no_write_reaches_the_folder() {
    let f = fixture();
    let Some(sb) = ready(&f).await else { return };
    let ro = f.ctx(confined(sb.clone()), true);
    let mtime = || {
        std::fs::metadata(f.ws.join("marker"))
            .unwrap()
            .modified()
            .unwrap()
    };
    let mode = || {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(f.ws.join("marker"))
            .unwrap()
            .permissions()
            .mode()
    };
    let (before_mtime, before_mode) = (mtime(), mode());
    let python = std::process::Command::new("python3")
        .arg("-c")
        .arg("")
        .output()
        .is_ok_and(|o| o.status.success());
    let mut attempts = vec![
        ("touch", "touch new-file"),
        ("redirect", "echo x > marker"),
        ("append", "echo x >> marker"),
        ("sed -i", "sed -i s/original/gone/ marker"),
        ("here-doc", "cat > from-heredoc <<'EOF'\nwritten\nEOF"),
        ("mkdir", "mkdir newdir"),
        ("rm", "rm -f marker"),
        ("rename", "mv marker renamed"),
        ("chmod", "chmod 600 marker"),
        ("utime", "touch -d 2000-01-01 marker"),
        ("hard link inside", "ln marker hard"),
        ("symlink inside", "ln -s marker soft"),
        ("proc cwd", "echo x > /proc/self/cwd/via-proc"),
        (
            "symlink from tmp",
            "ln -s \"$PWD/marker\" \"$TMPDIR/l\" && echo x > \"$TMPDIR/l\"",
        ),
        (
            "hard link from tmp",
            "ln \"$PWD/marker\" \"$TMPDIR/h\" && echo x >> \"$TMPDIR/h\"",
        ),
        ("git add", "git add -A"),
        ("git commit", "git commit -q --allow-empty -m x"),
        ("git checkout", "git checkout -q -b other"),
        ("git ref", "git update-ref refs/heads/x HEAD"),
        ("git stash", "git stash"),
        ("mount", "mount -t tmpfs none \"$PWD\" && touch new-file"),
        (
            "remount in a user namespace",
            "unshare -Urm sh -c 'mount -o remount,rw \"$PWD\" && touch new-file'",
        ),
        ("write entry", ""),
    ];
    let extra = format!("echo x > '{}/kept'", f.extra.display());
    attempts.last_mut().unwrap().1 = &extra;
    if python {
        attempts.push((
            "python -c",
            "python3 -c \"open('from-python','w').write('x')\"",
        ));
    }
    for (what, cmd) in &attempts {
        let out = bash(&ro, cmd).await;
        assert!(
            !out.ok,
            "{what}: the write ran in the read-only shell: {}",
            out.output
        );
    }
    // From outside: nothing changed.
    assert_eq!(f.marker(), "original\nchanged\n");
    assert_eq!(mtime(), before_mtime, "utime");
    assert_eq!(mode(), before_mode, "chmod");
    assert_eq!(
        std::fs::read_to_string(f.extra.join("kept")).unwrap(),
        "original\n"
    );
    let mut names: Vec<String> = std::fs::read_dir(&f.ws)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [".git", "marker"]);
    let git_out = std::process::Command::new("git")
        .args(["branch", "--list"])
        .current_dir(&f.ws)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&git_out.stdout).trim(), "* main");
    assert!(!f.ws.join(".git/index.lock").exists());

    // The control: the same kind of command writes with the shell writable,
    // so the refusals above are the read-only shell's.
    let rw = f.ctx(confined(sb), false);
    let out = bash(&rw, "touch new-file && echo x >> marker").await;
    assert!(out.ok, "{}", out.output);
    assert!(f.ws.join("new-file").exists());
    assert_eq!(f.marker(), "original\nchanged\nx\n");
}

/// What a planning agent needs still works: listing, reading, grep, git's
/// history, diff and status (no index lock: `GIT_OPTIONAL_LOCKS=0`), and
/// writing its own `/tmp`, which is empty at the next command.
#[tokio::test]
async fn reading_still_works() {
    let f = fixture();
    let Some(sb) = ready(&f).await else { return };
    let ro = f.ctx(confined(sb), true);
    for (what, cmd, want) in [
        ("ls", "ls", "marker"),
        ("cat", "cat marker", "changed"),
        ("grep", "grep -n original marker", "1:original"),
        (
            "find",
            "find . -name marker -not -path './.git/*'",
            "./marker",
        ),
        ("git log", "git log --oneline", "first"),
        ("git diff", "git diff", "+changed"),
        ("git status", "git status --short", "M marker"),
        ("git show", "git show --stat HEAD", "marker"),
        (
            "tmp",
            "echo scratch > \"$TMPDIR/n\" && cat \"$TMPDIR/n\" && echo $TMPDIR",
            "scratch\n/tmp",
        ),
    ] {
        let out = bash(&ro, cmd).await;
        assert!(out.ok, "{what}: {}", out.output);
        assert!(out.output.contains(want), "{what}: {}", out.output);
    }
    // Another command's `/tmp` is its own: nothing of the last one.
    let out = bash(&ro, "test -e /tmp/n && echo LEFT-BEHIND; ls -A /tmp").await;
    assert!(out.ok, "{}", out.output);
    assert!(!out.output.contains("LEFT-BEHIND"), "{}", out.output);
    assert!(!f.ws.join(".git/index.lock").exists());
}

/// Filling the temp folder stops at its size (a tmpfs of node.toml's
/// `read_only_tmp_mib`, here 64, in the command's own namespace), and
/// nothing of it lands on the node's disk.
#[tokio::test]
async fn the_temp_folder_is_bounded() {
    let f = fixture();
    let Some(sb) = f.sandbox_with(Settings {
        read_only_tmp: 64 * 1024 * 1024,
        ..f.settings()
    }) else {
        return;
    };
    let Some(sb) = ready_with(&f, sb).await else {
        return;
    };
    let ro = f.ctx(confined(sb), true);
    let mib = 64 + 16;
    let out = bash(
        &ro,
        &format!("head -c {mib}M /dev/zero > \"$TMPDIR/big\"; echo rc=$?; df -k /tmp | tail -n1"),
    )
    .await;
    assert!(out.output.contains("No space left"), "{}", out.output);
    assert!(!out.output.contains("rc=0"), "{}", out.output);
    // The session's temp folder on disk got nothing.
    let tmp = f.data.join("sandbox-tmp/s1");
    let used: u64 = walk(&tmp);
    assert!(
        used < 1024 * 1024,
        "{used} bytes on disk in {}",
        tmp.display()
    );
}

fn walk(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => walk(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Without a containment there is no read-only shell: the host mode's
/// Landlock-only twin and a node without its sandbox refuse the command
/// with the reason, and nothing runs (Landlock alone leaves `chmod`,
/// `chown`, times and extended attributes open).
#[tokio::test]
async fn without_a_containment_nothing_runs() {
    let f = fixture();
    let plain = Arc::new(f.plain());
    for (what, conf) in [
        ("landlock alone", confined(plain)),
        ("no sandbox", Confinement::Unconfined),
        (
            "refused",
            Confinement::Refused("the sandbox mode is not offered".into()),
        ),
    ] {
        assert!(conf.read_only_shell().is_err(), "{what}");
        let out = bash(&f.ctx(conf, true), "touch new-file").await;
        assert!(!out.ok, "{what}: {}", out.output);
        assert!(
            out.output
                .contains(harness_tools::sandbox::READ_ONLY_REFUSED),
            "{what}: {}",
            out.output
        );
        assert!(!f.ws.join("new-file").exists(), "{what}");
    }
}

/// A background command in the read-only shell is the same shell: it keeps
/// running after its call and still cannot write (its end says why).
#[tokio::test]
async fn a_background_command_cannot_write_either() {
    use harness_tools::background::{Access, Limits, Registry};
    let f = fixture();
    let Some(sb) = ready(&f).await else { return };
    let (tx, mut ends) = tokio::sync::mpsc::unbounded_channel();
    let bg = Registry::new(
        Limits {
            per_session: 8,
            ..Default::default()
        },
        Default::default(),
    )
    .session(1, tx);
    let mut ro = f.ctx(confined(sb), true);
    ro.background = Access::On(bg);
    let out = Bash
        .run(
            &ro,
            json!({
                "command": "sleep 4; touch from-background; echo x >> marker; echo rc=$?",
                "background": true,
                "timeout_secs": 60
            }),
        )
        .await;
    assert!(out.ok, "{}", out.output);
    assert!(out.background.is_some(), "{}", out.output);
    let end = tokio::time::timeout(std::time::Duration::from_secs(60), ends.recv())
        .await
        .expect("no end within the bound")
        .expect("the table closed");
    assert!(end.tail.contains("Read-only file system"), "{}", end.tail);
    assert!(!f.ws.join("from-background").exists());
    assert_eq!(f.marker(), "original\nchanged\n");
}

/// The read-only binds alone hold: the node's command line with
/// `sandbox-exec` (and so Landlock) taken out, the command run straight
/// from bubblewrap's init through the guard, still cannot write the folder
/// or a `write` entry, and still has its `/tmp`. Landlock is the second
/// layer, not the one this rests on.
#[tokio::test]
async fn the_binds_hold_without_landlock() {
    use std::ffi::OsString;
    let f = fixture();
    let Some(sb) = ready(&f).await else { return };
    let script = format!(
        "touch new-file; echo x >> marker; chmod 600 marker; echo x > '{}/kept'; \
         echo t > /tmp/t && cat /tmp/t",
        f.extra.display()
    );
    let (cmd, contained) = confined(sb)
        .for_session("s1")
        .shell(
            "bash".as_ref(),
            &["-c".as_ref(), script.as_ref()],
            &f.ws,
            None,
            &Default::default(),
            std::slice::from_ref(&f.data),
            harness_tools::sandbox::Grants {
                workspace_write: true,
                read_only: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(contained);
    // bubblewrap's `--` is the second one; the program after it is
    // `sandbox-exec` (through the image token) up to its own `--`.
    let std = cmd.as_std();
    let args: Vec<OsString> = std.get_args().map(OsString::from).collect();
    let dashes: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--")
        .map(|(i, _)| i)
        .collect();
    let (bwrap_end, exec_end) = (dashes[1], dashes[2]);
    assert_eq!(args[bwrap_end + 2], "sandbox-exec", "{args:?}");
    let mut new: Vec<OsString> = args[..=bwrap_end].to_vec();
    new.extend(args[exec_end + 1..].iter().cloned());
    let mut c = tokio::process::Command::new(std.get_program());
    c.args(new).env_clear();
    for (k, v) in std.get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    c.current_dir("/")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = harness_tools::contain::spawn_for_test(c).await.unwrap();
    let out = tokio::time::timeout(std::time::Duration::from_secs(60), child.wait_with_output())
        .await
        .expect("the command did not end")
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("Read-only file system"), "{text}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "t\n",
        "its /tmp: {text}"
    );
    assert!(!f.ws.join("new-file").exists());
    assert_eq!(f.marker(), "original\nchanged\n");
    assert_eq!(
        std::fs::read_to_string(f.extra.join("kept")).unwrap(),
        "original\n"
    );
}

/// Review of PR 125: a sandboxed command gets no core dumps (the host's
/// core pattern may pipe them to a handler outside the sandbox), and
/// cannot raise the limit; the same for the writable shell.
#[tokio::test]
async fn sandboxed_commands_dump_no_core() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    for read_only in [false, true] {
        if read_only && confined(sb.clone()).read_only_shell().is_err() {
            continue;
        }
        let ctx = f.ctx(confined(sb.clone()), read_only);
        let out = bash(
            &ctx,
            "echo soft=$(ulimit -Sc) hard=$(ulimit -Hc); ulimit -c 1000",
        )
        .await;
        assert!(
            out.output.contains("soft=0 hard=0"),
            "{read_only}: {}",
            out.output
        );
        assert!(!out.ok, "{read_only}: raised it: {}", out.output);
    }
}

/// Review of PR 125: under the containment form that shares this machine's
/// `/proc` (a rootless container), `/proc/<pid>/root` and `cwd` of the
/// node's own processes lead past the binds; Landlock (required on this
/// node) keeps the read-only shell from them. A best-effort node offers no
/// read-only shell in that form.
#[tokio::test]
async fn the_shared_proc_form_holds_through_landlock() {
    use harness_tools::contain::{Contain, Form};
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let sb = f.plain();
    let bwrap = harness_tools::contain::find_bwrap().unwrap();
    let c = Contain::new(bwrap.clone(), Form::SharedProc);
    if let Err(e) = sb.probe_containment(&c, &f.data.join("sandbox-tmp/probe-shared")) {
        eprintln!("SKIPPED (shared /proc form): {e}");
        return;
    }
    let sb = Arc::new(sb.with_containment(c));
    if let Err(why) = confined(sb.clone()).read_only_shell() {
        assert!(!bwrap::required(), "{why}");
        eprintln!("SKIPPED (shared /proc form): {why}");
        return;
    }
    eprintln!("RAN (shared /proc form)");
    let ro = f.ctx(confined(sb), true);
    let me = std::process::id();
    for cmd in [
        format!("echo x >> /proc/{me}/root$PWD/marker"),
        format!("echo x > /proc/{me}/cwd/via-node-cwd"),
        "for p in /proc/[0-9]*; do echo x >> $p/root$PWD/marker 2>/dev/null && echo WROTE $p \
         && exit 0; done; exit 1"
            .to_string(),
    ] {
        let out = bash(&ro, &cmd).await;
        assert!(!out.output.contains("WROTE"), "{cmd}: {}", out.output);
        assert!(!out.ok, "{cmd}: {}", out.output);
    }
    assert_eq!(f.marker(), "original\nchanged\n");
    // Best effort: no read-only shell in this form.
    let best = Sandbox::new(
        PathBuf::from(EXE),
        &Settings {
            mode: harness_tools::sandbox::Mode::BestEffort,
            ..f.settings()
        },
        Some(f.data.join("sandbox-tmp")),
        std::slice::from_ref(&f.data),
    )
    .unwrap()
    .with_containment(Contain::new(bwrap, Form::SharedProc));
    let why = confined(Arc::new(best)).read_only_shell().unwrap_err();
    assert!(why.contains("/proc"), "{why}");
}

/// A test run in the read-only shell (PLAN.md T70): `cargo test` of a
/// crate in the folder builds into the shell's own `/tmp`
/// (`CARGO_TARGET_DIR` is set there) with the toolchain folders granted
/// read, and leaves the folder as it was. The toolchain is the one running
/// this test (`CARGO`, `RUSTC` beside it); its folder is granted read, as
/// node.toml's `[sandbox] read` would.
#[tokio::test]
async fn cargo_test_runs_in_the_read_only_shell() {
    let f = fixture();
    let cargo = PathBuf::from(std::env::var("CARGO").expect("run under cargo"));
    let cargo = cargo.canonicalize().unwrap();
    let bin = cargo.parent().unwrap().to_path_buf();
    // The toolchain's root (`.../bin/cargo` -> `...`), and the cargo home
    // when there is one (its config and registry).
    let mut read = vec![bin.parent().unwrap().to_path_buf()];
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .filter(|p| p.is_dir());
    read.extend(home.clone());
    let Some(sb) = f.sandbox_with(Settings {
        read,
        ..f.settings()
    }) else {
        return;
    };
    let Some(sb) = ready_with(&f, sb).await else {
        return;
    };
    let krate = f.ws.join("krate");
    std::fs::create_dir_all(krate.join("src")).unwrap();
    std::fs::write(
        krate.join("Cargo.toml"),
        "[package]\nname = \"krate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(
        krate.join("src/lib.rs"),
        "pub fn two() -> u8 { 2 }\n#[test]\nfn adds() { assert_eq!(two(), 2); }\n",
    )
    .unwrap();
    // The lock file, as a repository has it (cargo writes one otherwise).
    let ok = std::process::Command::new(&cargo)
        .args(["generate-lockfile", "--offline"])
        .current_dir(&krate)
        .status()
        .unwrap()
        .success();
    assert!(ok, "cargo generate-lockfile");
    let before: Vec<_> = walkdir(&f.ws);
    let mut ctx = f.ctx(confined(sb), true);
    ctx.env = harness_tools::ToolEnv::new(
        &["PATH".to_string(), "HOME".to_string()],
        Vec::<String>::new(),
    )
    .unwrap();
    let cmd = format!(
        "export PATH={bin}:$PATH; {home} cd krate && cargo test --offline 2>&1 | tail -n 20; \
         ls target 2>&1 | head -n 1",
        bin = bin.display(),
        home = home
            .map(|h| format!("export CARGO_HOME={};", h.display()))
            .unwrap_or_default(),
    );
    let out = bash(&ctx, &cmd).await;
    assert!(out.output.contains("test adds ... ok"), "{}", out.output);
    assert!(
        out.output.contains("No such file"),
        "a target/ in the folder: {}",
        out.output
    );
    assert_eq!(walkdir(&f.ws), before, "the folder changed");
}

fn walkdir(p: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    out.sort();
    out
}
