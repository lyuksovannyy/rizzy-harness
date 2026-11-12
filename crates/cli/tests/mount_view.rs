//! The file view (ADOPTION_PLAN T.8c, O12): a contained command's mount
//! namespace is built from the node's grants and nothing else. These are
//! experiment B.11's `files.sh` probes as tests: what is not granted does
//! not exist for the command (absent, not "permission denied"), with
//! Landlock and without it, and a folder swapped after the node's check is
//! not what the command gets.
//!
//! "Without Landlock" is the node's own command line with `sandbox-exec`
//! taken out (the command runs straight from bubblewrap's init, so nothing
//! but the view confines it), or replaced by what a Landlock ABI 4 kernel
//! enforces (`support/review_t8a/landlock_abi4.py`), each still started
//! through the guard as the node starts it.
//!
//! Every file here is a stand-in the test makes in its own temp folder; the
//! processes a test starts are found by their unique `sleep <n>` and killed
//! by pid. They skip where bubblewrap or Landlock does not work, and fail
//! there with `HARNESS_REQUIRE_BWRAP=1`.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_tools::contain::{Binds, Contain, Form};
use harness_tools::sandbox::{Confinement, Grants, Network, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const ABI4: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/review_t8a/landlock_abi4.py"
);

const SSH_CANARY: &str = "SSH-STAND-IN-CANARY";
const SIBLING_CANARY: &str = "SIBLING-SESSION-CANARY";
const DATA_CANARY: &str = "DATA-DIR-CANARY";

/// A data directory, a session folder, a sibling session's folder, a
/// stand-in for `~/.ssh`, and links out of the session folder; all inside
/// one temp folder that the sandbox does not grant.
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
    sibling: PathBuf,
    ssh: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/code/chat-a");
    let sibling = root.join("home/code/chat-b");
    let ssh = root.join("home/.ssh");
    for d in [&data, &ws, &sibling, &ssh] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(data.join("api-token"), format!("{DATA_CANARY}\n")).unwrap();
    std::fs::write(sibling.join("notes.txt"), format!("{SIBLING_CANARY}\n")).unwrap();
    std::fs::write(ssh.join("id_test"), format!("{SSH_CANARY}\n")).unwrap();
    std::fs::write(ws.join("marker"), "checked\n").unwrap();
    std::os::unix::fs::symlink(&ssh, ws.join("link-dir")).unwrap();
    std::os::unix::fs::symlink(ssh.join("id_test"), ws.join("link-file")).unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
        sibling,
        ssh,
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

    /// The contained sandbox and the wrapper's Landlock status line, or
    /// `None` where it cannot run here.
    fn sandbox_with(&self, settings: &Settings) -> Option<(Arc<Sandbox>, String)> {
        let sb = self.plain(settings);
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

    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        self.sandbox_with(&Settings::default()).map(|(sb, _)| sb)
    }

    /// The contained sandbox in another form or with other binds than this
    /// bubblewrap would choose (the older bubblewrap's binds by path on a
    /// newer one), probed as the node probes it.
    fn sandbox_as(&self, form: Form, binds: Binds) -> Option<Arc<Sandbox>> {
        self.sandbox()?;
        let sb = self.plain(&Settings::default());
        let bwrap = harness_tools::contain::find_bwrap().unwrap();
        let c = Contain::new(bwrap, form).with_binds(binds);
        if let Err(e) = sb.probe_containment(&c, &self.data.join("sandbox-tmp/probe-as")) {
            assert!(
                !(bwrap::required() && form == Form::Full),
                "{form:?} {binds:?}: {e}"
            );
            eprintln!("SKIPPED ({form:?}, {binds:?}): {e}");
            return None;
        }
        Some(Arc::new(sb.with_containment(c)))
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

    /// The node's contained `bash -c <script>`, as the bash tool builds it.
    fn shell(&self, sb: Arc<Sandbox>, script: &str) -> tokio::process::Command {
        let conf = Confinement::Confined {
            sandbox: sb,
            tmp: None,
        }
        .for_session("s1");
        let (cmd, contained) = conf
            .shell(
                "bash".as_ref(),
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
        cmd
    }
}

/// The node's command line for `script`, with what runs after
/// bubblewrap's `--` replaced: `Some(prefix)` puts `prefix` in place of
/// the harness binary and `sandbox-exec`, keeping the wrapper's grants
/// (the Landlock ABI 4 stand-in reads them); `None` drops the wrapper and
/// its grants and runs the command straight from bubblewrap's init.
fn without_the_wrapper(
    cmd: &tokio::process::Command,
    prefix: Option<&[OsString]>,
) -> tokio::process::Command {
    let std = cmd.as_std();
    let args: Vec<OsString> = std.get_args().map(OsString::from).collect();
    let guard_end = args.iter().position(|a| a == "--").unwrap();
    let bwrap_end = guard_end
        + 1
        + args[guard_end + 1..]
            .iter()
            .position(|a| a == "--")
            .unwrap();
    assert_eq!(args[bwrap_end + 2], "sandbox-exec", "{args:?}");
    let exec_end = bwrap_end
        + 2
        + args[bwrap_end + 2..]
            .iter()
            .position(|a| a == "--")
            .unwrap();
    let mut new: Vec<OsString> = args[..=bwrap_end].to_vec();
    match prefix {
        Some(p) => {
            new.extend(p.iter().cloned());
            new.extend(args[bwrap_end + 3..].iter().cloned());
        }
        None => new.extend(args[exec_end + 1..].iter().cloned()),
    }
    let mut c = tokio::process::Command::new(std.get_program());
    c.args(new).env_clear();
    for (k, v) in std.get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    if let Some(d) = std.get_current_dir() {
        c.current_dir(d);
    }
    c
}

/// Run a contained command line through its guard, as the node does: its
/// stdout and stderr together, and whether it exited 0.
async fn run_guarded(mut cmd: tokio::process::Command) -> (String, bool) {
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = harness_tools::contain::spawn_for_test(cmd).await.unwrap();
    let out = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .expect("the command did not end")
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        out.status.success(),
    )
}

/// One line per path: `<tag>=absent` when `lstat` says it does not exist,
/// `<tag>=there:<first line or the read's error>` otherwise.
fn look(paths: &[(&str, PathBuf)]) -> String {
    let mut s = String::new();
    for (tag, p) in paths {
        let p = p.display();
        s.push_str(&format!(
            "if [ -e '{p}' ]; then echo \"{tag}=there:$(head -n1 '{p}' 2>&1 \
             || ls '{p}' 2>&1 | head -n1)\"; else echo '{tag}=absent'; fi; "
        ));
    }
    s
}

fn outside(f: &Fixture) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("sibling", f.sibling.clone()),
        ("sibling-file", f.sibling.join("notes.txt")),
        ("ssh", f.ssh.clone()),
        ("ssh-key", f.ssh.join("id_test")),
        ("link-dir-target", f.ws.join("link-dir/id_test")),
        ("link-file-target", f.ws.join("link-file")),
        ("data-token", f.data.join("api-token")),
        ("home-code-listing", f.root.join("home/code/chat-b")),
    ]
}

fn assert_all_absent(out: &str, how: &str) {
    for canary in [SSH_CANARY, SIBLING_CANARY, DATA_CANARY] {
        assert!(!out.contains(canary), "{how}: {canary} was read:\n{out}");
    }
    for tag in [
        "sibling",
        "sibling-file",
        "ssh",
        "ssh-key",
        "link-dir-target",
        "link-file-target",
        "data-token",
        "home-code-listing",
    ] {
        assert!(
            out.lines().any(|l| l == format!("{tag}=absent")),
            "{how}: {tag} is not absent:\n{out}"
        );
    }
    assert!(
        !out.contains("Permission denied"),
        "{how}: denied is not absent:\n{out}"
    );
}

// ---- 1. what is not granted does not exist ----

/// The node's command (Landlock inside, where it is): a sibling session's
/// folder, the `~/.ssh` stand-in, the data directory and the targets of
/// both links out of the session folder are absent. Before T.8c they were
/// there and "permission denied" (Landlock alone decided).
#[tokio::test]
async fn outside_the_grants_is_absent() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let script = format!("{} echo in-folder=$(cat marker)", look(&outside(&f)));
    let r = Bash.run(&f.ctx(sb), json!({ "command": script })).await;
    eprintln!("{}", r.output);
    assert!(r.output.contains("in-folder=checked"), "{}", r.output);
    assert_all_absent(&r.output, "with Landlock");
}

/// The same with Landlock unavailable: the node's command line with
/// `sandbox-exec` taken out, so only the view confines. Before T.8c the
/// command read everything.
#[tokio::test]
async fn outside_the_grants_is_absent_without_landlock() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let script = format!(
        "{} echo in-folder=$(cat marker); echo pid=$$",
        look(&outside(&f))
    );
    let cmd = without_the_wrapper(&f.shell(sb, &script), None);
    let (out, ok) = run_guarded(cmd).await;
    eprintln!("{out}");
    assert!(ok, "{out}");
    assert!(out.contains("in-folder=checked"), "{out}");
    assert!(out.contains("pid=2"), "not in its namespace: {out}");
    assert_all_absent(&out, "without Landlock");
}

/// The same in every way the node can contain a command here: the older
/// bubblewrap's binds by path (bubblewrap 0.9, CI's), and the form with the
/// outer `/proc` (a default rootless container), each through the bash
/// tool and with only the view.
#[tokio::test]
async fn outside_the_grants_is_absent_in_every_form_and_bind() {
    for (form, binds) in [
        (Form::Full, Binds::Path),
        (Form::SharedProc, Binds::Fd),
        (Form::SharedProc, Binds::Path),
    ] {
        let f = fixture();
        let Some(sb) = f.sandbox_as(form, binds) else {
            continue;
        };
        let how = format!("{form:?}, {binds:?}");
        let script = format!("{} echo in-folder=$(cat marker)", look(&outside(&f)));
        let r = Bash
            .run(&f.ctx(sb.clone()), json!({ "command": script }))
            .await;
        assert!(
            r.output.contains("in-folder=checked"),
            "{how}: {}",
            r.output
        );
        assert_all_absent(&r.output, &format!("{how}, with Landlock"));
        let (out, ok) = run_guarded(without_the_wrapper(&f.shell(sb, &script), None)).await;
        assert!(ok && out.contains("in-folder=checked"), "{how}: {out}");
        assert_all_absent(&out, &format!("{how}, without Landlock"));
        eprintln!("{how}: all absent");
    }
}

/// No descriptor of the view's reaches the command: bubblewrap closes those
/// it bound with `--bind-fd`, and nothing else of the guard's is inherited.
/// A descriptor of a host folder would lead out of the view (`openat(fd,
/// "..")`). The descriptor of the node's image that `sandbox-exec` is
/// started from does not reach the command either: `sandbox-exec` closes
/// every descriptor above 2 first (T.8f; `own_image.rs` checks it by
/// identity).
#[tokio::test]
async fn no_descriptor_of_a_folder_reaches_the_command() {
    for binds in [Binds::Fd, Binds::Path] {
        let f = fixture();
        let Some(sb) = f.sandbox_as(Form::Full, binds) else {
            continue;
        };
        let script = "for n in $(ls /proc/self/fd); do [ \"$n\" -gt 2 ] || continue; \
                      t=$(readlink /proc/self/fd/$n 2>&1); [ -d /proc/self/fd/$n ] && \
                      echo \"DIR-FD $n $t\"; echo \"fd $n $t\"; done; echo end";
        for cmd in [
            f.shell(sb.clone(), script),
            without_the_wrapper(&f.shell(sb.clone(), script), None),
        ] {
            let (out, ok) = run_guarded(cmd).await;
            eprintln!("{binds:?}: {out}");
            assert!(ok && out.contains("end"), "{out}");
            assert!(!out.contains("DIR-FD"), "{binds:?}: {out}");
        }
    }
}

// ---- 2. the swap between the node's check and the mount (S-2) ----

/// The session folder is swapped for a link to the `~/.ssh` stand-in after
/// the node built the command line (its check), before the command starts.
/// The command must not get the link's target: the checked folder, or
/// nothing (refused). With Landlock, and with only the view.
#[tokio::test]
async fn a_folder_swapped_for_a_link_after_the_check_is_not_mounted() {
    for wrapper in [true, false] {
        let f = fixture();
        let Some(sb) = f.sandbox() else { return };
        let script = "echo got=$(cat marker 2>&1); cat id_test 2>&1; ls 2>&1";
        let cmd = f.shell(sb, script);
        let cmd = if wrapper {
            cmd
        } else {
            without_the_wrapper(&cmd, None)
        };
        // The swap: what a process of another session would do.
        std::fs::rename(&f.ws, f.ws.with_file_name("chat-a.old")).unwrap();
        std::os::unix::fs::symlink(&f.ssh, &f.ws).unwrap();
        let (out, ok) = run_guarded(cmd).await;
        eprintln!("with the wrapper {wrapper}: ok {ok}: {out}");
        assert!(
            !out.contains(SSH_CANARY),
            "the link's target was mounted: {out}"
        );
        assert!(
            out.contains("got=checked") || (!ok && out.contains("replaced")),
            "neither the checked folder nor a refusal: {out}"
        );
    }
}

/// The same swap with the older bubblewrap's binds by path: bubblewrap
/// binds what the path leads to (the link's target), and `sandbox-exec`,
/// which opens the mounted folder without following a link, refuses the
/// command before it runs (B.11's fallback). Only with the wrapper: that
/// check is the wrapper's, and it runs before every contained command,
/// with or without Landlock.
#[tokio::test]
async fn with_binds_by_path_a_swapped_folder_refuses_the_command() {
    let f = fixture();
    let Some(sb) = f.sandbox_as(Form::Full, Binds::Path) else {
        return;
    };
    let cmd = f.shell(sb, "echo got=$(cat marker 2>&1); cat id_test 2>&1");
    std::fs::rename(&f.ws, f.ws.with_file_name("chat-a.old")).unwrap();
    std::os::unix::fs::symlink(&f.ssh, &f.ws).unwrap();
    let (out, ok) = run_guarded(cmd).await;
    eprintln!("ok {ok}: {out}");
    assert!(!ok && out.contains("replaced"), "{out}");
    assert!(!out.contains(SSH_CANARY), "{out}");
}

// ---- 3. a pathname Unix socket outside the grants (T8A-1) ----

fn have_python() -> bool {
    let ok = std::process::Command::new("python3")
        .arg("-V")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        assert!(!bwrap::required(), "these tests need python3");
        eprintln!("SKIPPED: no python3");
    }
    ok
}

/// Live processes whose command line is exactly `sleep <secs>`.
fn sleeps(secs: u32) -> Vec<i32> {
    let want = format!("sleep\0{secs}\0");
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == want.as_bytes())
                && std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
                    !s.rsplit(')')
                        .next()
                        .unwrap_or("")
                        .trim_start()
                        .starts_with('Z')
                })
        })
        .collect()
}

/// Kills, by pid, every `sleep <n>` it names when dropped.
struct Reaper(Vec<u32>);

impl Drop for Reaper {
    fn drop(&mut self) {
        for s in &self.0 {
            for pid in sleeps(*s) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }
}

/// A service outside every grant (the stand-in for the session bus) that
/// starts `sleep <secs>` for each connection.
fn service(path: &Path, secs: u32) {
    let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
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

fn connect_script(path: &Path) -> String {
    format!(
        "python3 -I -c 'import socket\n\
         s = socket.socket(socket.AF_UNIX)\n\
         try:\n\
         \x20   s.connect(\"{}\"); print(\"unix-\" + \"connected\")\n\
         except OSError as e: print(\"refused:\", type(e).__name__, e)\n\
         '",
        path.display()
    )
}

/// With Landlock as this kernel has it, through the bash tool, and with
/// what an ABI 4 kernel enforces (no pathname-socket right): the socket is
/// not in the view, so nothing reaches the service. Before T.8c, below ABI
/// 9 the connect went through and the service's process outlived the
/// command (`contain.rs` `below_abi_9_a_service_outside_still_outlives_the_kill`).
#[tokio::test]
async fn a_socket_outside_the_grants_cannot_be_reached() {
    let f = fixture();
    let Some((sb, status)) = f.sandbox_with(&Settings::default()) else {
        return;
    };
    if !have_python() {
        return;
    }
    let abi = harness_tools::sandbox::effective_abi(&status);
    let bus = f.root.join("home/bus");
    let reaper = Reaper(vec![976_101]);
    service(&bus, 976_101);

    let r = Bash
        .run(
            &f.ctx(sb.clone()),
            json!({ "command": connect_script(&bus) }),
        )
        .await;
    eprintln!("Landlock ABI {abi}: {}", r.output);
    assert!(!r.output.contains("unix-connected"), "{}", r.output);
    assert!(r.output.contains("FileNotFoundError"), "{}", r.output);

    // Landlock ABI 4: the stand-in must be inside the view to run.
    let stand_in = f.ws.join("landlock_abi4.py");
    std::fs::copy(ABI4, &stand_in).unwrap();
    let prefix: Vec<OsString> = vec!["python3".into(), "-I".into(), stand_in.into()];
    let cmd = without_the_wrapper(&f.shell(sb, &connect_script(&bus)), Some(&prefix));
    let (out, _) = run_guarded(cmd).await;
    eprintln!("Landlock ABI 4 stand-in: {out}");
    assert!(!out.contains("unix-connected"), "{out}");
    assert!(out.contains("FileNotFoundError"), "{out}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(sleeps(976_101).is_empty(), "the service started a process");
    drop(reaper);
}

// ---- 4. what a command makes, and the tools it runs ----

/// Files and folders a command creates in its folder belong to the node's
/// user and have ordinary modes, seen from outside (no other uid from a
/// user namespace, no mode games). Passed before T.8c too; kept so the view
/// does not change it.
#[tokio::test]
async fn files_a_command_makes_are_the_node_users() {
    use std::os::unix::fs::MetadataExt;
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let r = Bash
        .run(
            &f.ctx(sb),
            json!({ "command": "umask 022; echo x > made.txt && mkdir made.d && echo made" }),
        )
        .await;
    assert!(r.output.contains("made"), "{}", r.output);
    let (uid, gid) = (
        nix::unistd::getuid().as_raw(),
        nix::unistd::getgid().as_raw(),
    );
    for (p, mode) in [(f.ws.join("made.txt"), 0o644), (f.ws.join("made.d"), 0o755)] {
        let md = std::fs::symlink_metadata(&p).unwrap();
        assert_eq!((md.uid(), md.gid()), (uid, gid), "{}", p.display());
        assert_eq!(md.mode() & 0o7777, mode, "{}", p.display());
    }
    std::fs::remove_dir(f.ws.join("made.d")).unwrap();
    std::fs::remove_file(f.ws.join("made.txt")).unwrap();
}

/// `git` (init, status, add, commit) and a C compiler work in the folder.
/// Skips the compiler with a message where there is none.
#[tokio::test]
async fn git_and_a_compiler_work_in_the_folder() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let ctx = f.ctx(sb);
    let git = "git init -q . && git -c user.name=t -c user.email=t@example.invalid \
               commit -q --allow-empty -m first && echo hi > a.txt && git add a.txt && \
               git status --short && git -c user.name=t -c user.email=t@example.invalid \
               commit -q -m second && git log --oneline | wc -l";
    let r = Bash.run(&ctx, json!({ "command": git })).await;
    eprintln!("git: {}", r.output);
    assert!(r.output.contains("A  a.txt"), "{}", r.output);
    assert!(r.output.lines().any(|l| l.trim() == "2"), "{}", r.output);

    let have_cc = std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !have_cc {
        assert!(!bwrap::required(), "no C compiler (cc) to test with");
        eprintln!("SKIPPED (compiler): no cc on this machine");
        return;
    }
    std::fs::write(
        f.ws.join("hello.c"),
        "#include <stdio.h>\nint main(void) { puts(\"compiled-and-ran\"); return 0; }\n",
    )
    .unwrap();
    let r = Bash
        .run(&ctx, json!({ "command": "cc -o hello hello.c && ./hello" }))
        .await;
    eprintln!("cc: {}", r.output);
    assert!(r.output.contains("compiled-and-ran"), "{}", r.output);
}

// ---- 5. our own binary inside the view (T8E-5, R-3) ----

/// The reviewer's proof for T8E-5, inverted: the harness binary lies in the
/// session's folder (a node run from its build tree); one command replaces
/// it with a script that records how it was run. The next command's
/// `sandbox-exec` must still be the node's own image, and the command must
/// still run. Before T.8c the replacement ran as `sandbox-exec`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_binary_does_not_run_as_sandbox_exec() {
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
    let ctx = f.ctx(Arc::new(sb));
    let written = f.ws.join("ran-as");
    let replace = format!(
        "mv target/debug/rizzy-harness target/debug/old && printf '%s\\n' '#!/bin/sh' \
         'echo \"ran as $1\" >> {}' 'shift; while [ \"$1\" != -- ]; do shift; done; shift; \
         exec \"$@\"' > target/debug/rizzy-harness && chmod +x target/debug/rizzy-harness && \
         echo replaced",
        written.display()
    );
    let r1 = Bash.run(&ctx, json!({ "command": replace })).await;
    assert!(r1.output.contains("replaced"), "{}", r1.output);
    let r2 = Bash.run(&ctx, json!({ "command": "echo second" })).await;
    let wrote = std::fs::read_to_string(&written).unwrap_or_default();
    eprintln!(
        "command 2: {:?}; the replacement wrote: {wrote:?}",
        r2.output
    );
    assert!(
        !wrote.contains("sandbox-exec"),
        "the replaced binary ran as sandbox-exec: {wrote}"
    );
    assert!(r2.output.contains("second"), "{}", r2.output);
}

/// The binary deleted under the running node and a new one copied to its
/// path (an upgrade): contained commands still run, from the node's own
/// image (T.8f: `sandbox-exec` is started through a descriptor, so the
/// image needs no path). Before T.8f every contained command was refused
/// with "restart the node".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deleted_binary_changes_nothing_for_contained_commands() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let bin = f.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("rizzy-harness");
    std::fs::copy(EXE, &exe).unwrap();
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
    let ctx = f.ctx(Arc::new(sb));
    let r = Bash.run(&ctx, json!({ "command": "echo before" })).await;
    assert!(r.output.contains("before"), "{}", r.output);
    std::fs::remove_file(&exe).unwrap();
    std::fs::copy(EXE, &exe).unwrap();
    let r = Bash
        .run(&ctx, json!({ "command": "echo RAN-AFTER-DELETE" }))
        .await;
    eprintln!("{}", r.output);
    assert!(r.output.contains("RAN-AFTER-DELETE\n"), "{}", r.output);
}

/// Helper for the next test (does nothing unless its variable is set): a
/// node whose network is off and whose `[sandbox] write` entry holds a
/// folder on its `PATH` probes its containment; it prints the result.
#[test]
fn helper_probe_with_a_planted_path() {
    let Some(w) = std::env::var_os("T8C_HELPER_WRITE") else {
        return;
    };
    let data = PathBuf::from(std::env::var_os("T8C_HELPER_DATA").unwrap());
    let settings = Settings {
        network: Network::Off,
        write: vec![PathBuf::from(w)],
        ..Default::default()
    };
    let sb = Sandbox::new(
        PathBuf::from(EXE),
        &settings,
        Some(data.join("sandbox-tmp")),
        std::slice::from_ref(&data),
    )
    .unwrap();
    let bwrap = harness_tools::contain::find_bwrap().unwrap();
    let c = Contain::new(bwrap, Form::Full);
    match sb.probe_containment(&c, &data.join("sandbox-tmp/probe-contain")) {
        Ok(()) => println!("PROBE-RESULT ok"),
        Err(e) => println!("PROBE-RESULT failed: {e}"),
    }
}

/// R-3's proof, inverted: a `readlink` and a `cat` that exit 1, planted in
/// a `[sandbox] write` entry that is first on the node's `PATH`, do not
/// change the probe's result (it runs no program by name). Before T.8c the
/// probe failed ("the command's /proc/self/ns/net was the node's").
#[test]
fn a_planted_program_on_path_does_not_decide_the_probe() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let w = f.root.join("home/.cargo");
    let bin = w.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["readlink", "cat"] {
        use std::os::unix::fs::PermissionsExt;
        let p = bin.join(name);
        std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut path = OsString::from(&bin);
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let probe = |path: &std::ffi::OsStr| {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "helper_probe_with_a_planted_path", "--nocapture"])
            .env("T8C_HELPER_WRITE", &w)
            .env("T8C_HELPER_DATA", &f.data)
            .env("PATH", path)
            .output()
            .unwrap();
        let s = String::from_utf8_lossy(&out.stdout).into_owned();
        s.lines()
            .find(|l| l.starts_with("PROBE-RESULT"))
            .unwrap_or_else(|| panic!("no result: {s} {}", String::from_utf8_lossy(&out.stderr)))
            .to_string()
    };
    let clean = probe(&std::env::var_os("PATH").unwrap_or_default());
    let planted = probe(&path);
    eprintln!("clean PATH: {clean}; planted: {planted}");
    assert_eq!(clean, "PROBE-RESULT ok");
    assert_eq!(planted, clean, "a program on PATH changed the probe");
}

// ---- 6. doctor ----

fn doctor(data: &Path, node_toml: &str, path: Option<&std::ffi::OsStr>) -> String {
    std::fs::write(data.join("node.toml"), node_toml).unwrap();
    let mut c = std::process::Command::new(EXE);
    c.args(["--data-dir", data.to_str().unwrap(), "doctor"]);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    let out = c.output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn line<'a>(out: &'a str, name: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(&format!(" {name}: ")))
        .unwrap_or_else(|| panic!("no {name} line: {out}"))
}

/// `doctor` names what the view binds, in a form a person can check, and
/// warns about a `[sandbox] write` entry that holds a folder on the node's
/// `PATH` (the node's own helpers, and bubblewrap itself, are found there by
/// name, outside the sandbox).
#[test]
fn doctor_shows_the_view_and_warns_about_a_writable_path_folder() {
    let f = fixture();
    if f.sandbox().is_none() {
        return;
    }
    let cache = f.root.join("home/.cache");
    let tools = f.root.join("home/.local/share/tools");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(tools.join("bin")).unwrap();
    let toml = format!(
        "[sandbox]\nread = [\"{}\"]\nwrite = [\"{}\"]\n",
        tools.display(),
        cache.display()
    );
    let out = doctor(&f.data, &toml, None);
    let v = line(&out, "file view");
    eprintln!("{v}");
    for want in [
        "/usr",
        "/etc",
        &tools.display().to_string(),
        &cache.display().to_string(),
    ] {
        assert!(v.contains(want), "{want} not named: {v}");
    }
    assert!(
        v.contains("session"),
        "the session's own grants not named: {v}"
    );

    // The same write entry, now holding a folder on PATH.
    let mut path = OsString::from(cache.join("bin"));
    std::fs::create_dir_all(cache.join("bin")).unwrap();
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let out = doctor(&f.data, &toml, Some(&path));
    let c = line(&out, "shell containment");
    assert!(c.starts_with("warn "), "{c}");
    assert!(
        c.contains(&cache.join("bin").display().to_string()) && c.contains("PATH"),
        "{c}"
    );
}

// ---- 7. what the view costs ----

/// Per command: the node's command line (guard, view, `sandbox-exec`,
/// `true`) against the same with the view replaced by T.8a's whole host
/// (`--dev-bind / /`), alternately, `T8C_RUNS` times each (default 30).
/// Printed; the bound is loose (a debug build on a loaded machine).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_view_costs_little_per_command() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    let runs: usize = std::env::var("T8C_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let (mut with, mut without) = (Vec::new(), Vec::new());
    for _ in 0..runs {
        let cmd = f.shell(sb.clone(), "true");
        let host = host_view(&cmd);
        let t = Instant::now();
        let (out, ok) = run_guarded(cmd).await;
        with.push(t.elapsed());
        assert!(ok, "{out}");
        let t = Instant::now();
        let (out, ok) = run_guarded(host).await;
        without.push(t.elapsed());
        assert!(ok, "{out}");
    }
    let median = |v: &mut Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    let (dw, d_o) = (median(&mut with), median(&mut without));
    eprintln!(
        "per command, {runs} runs: with the view median {dw:?}; with the whole host \
         (T.8a's --dev-bind / /) median {d_o:?}; the view adds {:?}",
        dw.saturating_sub(d_o)
    );
    assert!(
        dw < d_o + Duration::from_millis(25),
        "the view adds {:?}",
        dw.saturating_sub(d_o)
    );
}

/// `cmd` (the node's guarded command line) with bubblewrap's file options
/// replaced by T.8a's: the host's whole file system, `/proc` its own, and
/// the harness binary by its path.
fn host_view(cmd: &tokio::process::Command) -> tokio::process::Command {
    let std = cmd.as_std();
    let args: Vec<OsString> = std.get_args().map(OsString::from).collect();
    let guard_end = args.iter().position(|a| a == "--").unwrap();
    let bwrap_end = guard_end
        + 1
        + args[guard_end + 1..]
            .iter()
            .position(|a| a == "--")
            .unwrap();
    let chdir = args[..bwrap_end]
        .iter()
        .position(|a| a == "--chdir")
        .unwrap();
    let mut new: Vec<OsString> = args[..=guard_end + 1].to_vec();
    new.extend(
        [
            "--die-with-parent",
            "--new-session",
            "--unshare-pid",
            "--dev-bind",
            "/",
            "/",
            "--proc",
            "/proc",
        ]
        .map(OsString::from),
    );
    new.extend(args[chdir..=bwrap_end].iter().cloned());
    new.push(EXE.into());
    new.extend(args[bwrap_end + 2..].iter().cloned());
    let mut c = tokio::process::Command::new(std.get_program());
    c.args(new).env_clear();
    for (k, v) in std.get_envs() {
        if let Some(v) = v {
            c.env(k, v);
        }
    }
    if let Some(d) = std.get_current_dir() {
        c.current_dir(d);
    }
    c
}
