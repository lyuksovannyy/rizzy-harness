//! A contained command's `sandbox-exec` is the node's own running image,
//! started through a descriptor the guard holds and bubblewrap passes on,
//! never through a path (ADOPTION_PLAN T.8f; review notes T8C-N3, T8F-2,
//! T8F-3). So a harness binary replaced under a running node (a `cargo
//! build`, an upgrade) changes nothing for contained commands, whatever
//! file systems the binary and the data directory are on; no session's
//! file ever runs as `sandbox-exec`; and nothing of the image reaches the
//! command.
//!
//! Each test runs a copy of the harness binary it owns, never the build's
//! binary replaced in place. They skip where bubblewrap or Landlock does
//! not work, and fail there with `HARNESS_REQUIRE_BWRAP=1`.

#[allow(dead_code)]
#[path = "support/bwrap.rs"]
mod bwrap;

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::contain::{Binds, Contain, Form};
use harness_tools::sandbox::{Confinement, Network, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A data directory (in the tests' temp folder) and a session folder,
/// which is in `build_fs`'s temp folder when asked for: on the build's own
/// file system, so where that is another one than the temp folder's, the
/// binary in the session's folder and the data directory are on two.
struct Fixture {
    _dirs: Vec<tempfile::TempDir>,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture(build_fs: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let mut dirs = vec![dir];
    let ws = if build_fs {
        let w = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let p = w.path().canonicalize().unwrap().join("ws");
        dirs.push(w);
        p
    } else {
        root.join("ws")
    };
    for d in [&data, &ws, &data.join("sandbox-tmp")] {
        std::fs::create_dir_all(d).unwrap();
    }
    Fixture {
        _dirs: dirs,
        root,
        data,
        ws,
    }
}

impl Fixture {
    /// A copy of the harness binary at `at` (a file of its own).
    fn binary(&self, at: &Path) -> PathBuf {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::copy(EXE, at).unwrap();
        at.to_owned()
    }

    fn plain(&self, exe: &Path, network: Network) -> Sandbox {
        Sandbox::new(
            exe.to_owned(),
            &Settings {
                network,
                ..Default::default()
            },
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// `exe`'s sandbox, contained in `form` with `binds` and probed as the
    /// node probes it (the probes are `sandbox-exec`'s own checks, started
    /// as commands are), or `None` (and a printed reason) where that cannot
    /// run here.
    fn contained(&self, exe: &Path, form: Form, binds: Binds, network: Network) -> Option<Sandbox> {
        let sb = self.plain(exe, network);
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock does not work here: {e}");
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            return None;
        }
        let bw = match harness_tools::contain::find_bwrap() {
            Ok(b) => b,
            Err(e) => {
                assert!(!bwrap::required(), "no bubblewrap: {e}");
                eprintln!("SKIPPED: {e}");
                return None;
            }
        };
        // With the network off the command gets a network namespace of its
        // own (the default of `Contain`, T.8b).
        let c = Contain::new(bw, form).with_binds(binds);
        if let Err(e) = sb.probe_containment(&c, &self.data.join("sandbox-tmp/probe-contain")) {
            assert!(
                !(bwrap::required() && form == Form::Full),
                "{form:?} {binds:?} {network:?}: {e}"
            );
            eprintln!("SKIPPED ({form:?}, {binds:?}, {network:?}): {e}");
            return None;
        }
        Some(sb.with_containment(c))
    }

    fn ctx(&self, sb: Arc<Sandbox>, session: &str) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: Some(self.data.join("sandbox-tmp").join(session)),
        }
        .for_session(session);
        ctx
    }
}

async fn run(ctx: &ToolCtx, script: &str) -> String {
    Bash.run(ctx, json!({ "command": script })).await.output
}

/// A script that records how it was run in `log`, then runs the command
/// after its first `--` (as `sandbox-exec` would, unconfined): what a
/// session puts in the binary's place.
fn impostor(at: &Path, log: &Path) {
    std::fs::write(
        at,
        format!(
            "#!/bin/sh\necho \"ran as $1\" >> '{}'\nshift; while [ \"$1\" != -- ]; do shift; \
             done; shift; exec \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(at, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn ino(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).unwrap().ino()
}

/// The names in the data directory that start with `node-image` (T.8f's
/// first design made them; nothing may now).
fn image_names(data: &Path) -> Vec<String> {
    std::fs::read_dir(data)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("node-image"))
        .collect()
}

// ---- 1. a binary replaced after the start ----

/// The binary lies in the session's folder (a node run from its build
/// tree), on the build's file system, and the data directory in the temp
/// folder: here two file systems, so no hard link of the binary into the
/// data directory can be made (checked). One command removes the binary
/// and writes a recording script in its place, as a rebuild would. The next
/// command still runs, and the script never runs as `sandbox-exec`: in both
/// bubblewrap forms, both bind modes, the network on and off. Before T.8f
/// the next command was refused ("restart the node").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_binary_replaced_after_the_start_still_runs_from_the_old_image() {
    let mut ran = 0;
    for (form, binds, network) in [
        (Form::Full, Binds::Fd, Network::On),
        (Form::Full, Binds::Path, Network::On),
        (Form::Full, Binds::Fd, Network::Off),
        (Form::Full, Binds::Path, Network::Off),
        (Form::SharedProc, Binds::Fd, Network::On),
        (Form::SharedProc, Binds::Path, Network::On),
        (Form::SharedProc, Binds::Fd, Network::Off),
        (Form::SharedProc, Binds::Path, Network::Off),
    ] {
        let how = format!("{form:?} {binds:?} network {network:?}");
        let f = fixture(true);
        let exe = f.binary(&f.ws.join("target/debug/rizzy-harness"));
        let two_fs =
            std::fs::metadata(&exe).unwrap().dev() != std::fs::metadata(&f.data).unwrap().dev();
        if two_fs {
            let e = std::fs::hard_link(&exe, f.data.join("a-link")).unwrap_err();
            assert_eq!(e.raw_os_error(), Some(nix::libc::EXDEV), "{e}");
        } else {
            eprintln!("(the build and the temp folder share a file system here)");
        }
        let Some(sb) = f.contained(&exe, form, binds, network) else {
            continue;
        };
        let ctx = f.ctx(Arc::new(sb), "s1");
        let log = f.ws.join("ran-as");
        let r1 = run(
            &ctx,
            &format!(
                "rm target/debug/rizzy-harness && printf '%s\\n' '#!/bin/sh' 'echo \"ran as $1\" \
                 >> {}' 'shift; while [ \"$1\" != -- ]; do shift; done; shift; exec \"$@\"' > \
                 target/debug/rizzy-harness && chmod +x target/debug/rizzy-harness && echo replaced",
                log.display()
            ),
        )
        .await;
        assert!(r1.contains("replaced"), "{how}: {r1}");
        let r2 = run(&ctx, "echo second").await;
        let wrote = std::fs::read_to_string(&log).unwrap_or_default();
        eprintln!("{how}: command 2: {r2:?}; the replacement wrote: {wrote:?}");
        assert!(wrote.is_empty(), "{how}: the replacement ran: {wrote}");
        assert!(r2.contains("second\n"), "{how}: {r2}");
        let r3 = run(&f.ctx(ctx_sandbox(&ctx), "s2"), "echo other-session").await;
        assert!(r3.contains("other-session\n"), "{how}: {r3}");
        assert!(image_names(&f.data).is_empty());
        ran += 1;
    }
    assert!(ran > 0 || !bwrap::required(), "no form ran");
}

fn ctx_sandbox(ctx: &ToolCtx) -> Arc<Sandbox> {
    match &ctx.sandbox {
        Confinement::Confined { sandbox, .. } => sandbox.clone(),
        _ => panic!("not confined"),
    }
}

// ---- 2. nothing of the image reaches the command ----

/// The review's `a_what_a_command_can_do_to_the_image`, inverted: the binary
/// is outside every grant (an installed, user-owned binary). In both bind
/// modes the command holds no descriptor above 2 it did not open itself,
/// none of the image's device and inode, and a `chmod 000` through every
/// descriptor it has leaves the binary's mode on the host as it was; the
/// next command, of any session, runs. Before T.8f (by path) the command
/// inherited a descriptor of the image and set its mode to 000.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_of_the_image_reaches_the_command() {
    for binds in [Binds::Fd, Binds::Path] {
        let f = fixture(false);
        let exe = f.binary(&f.root.join("bin/rizzy-harness"));
        let Some(sb) = f.contained(&exe, Form::Full, binds, Network::On) else {
            return;
        };
        let ctx = f.ctx(Arc::new(sb), "s1");
        let m = std::fs::metadata(&exe).unwrap();
        let image = format!("{}:{}", m.dev(), m.ino());
        let before = m.mode();
        let r = run(
            &ctx,
            // (`ls` in a child of its own, writing to a file: a command
            // substitution would hold a pipe in the shell while it runs.)
            "ls /proc/$$/fd > \"$TMPDIR/fds\"; echo fds=$(sort -n \"$TMPDIR/fds\" | tr '\\n' ' '); \
             for f in /proc/$$/fd/*; do echo \"id ${f##*/} $(stat -L -c %d:%i \"$f\" 2>&1)\"; done; \
             for f in /proc/$$/fd/*; do chmod 000 \"$f\" 2>/dev/null; done; echo done",
        )
        .await;
        eprintln!("{binds:?}: {r}");
        assert!(r.contains("done"), "{binds:?}: {r}");
        assert!(
            !r.contains(&image),
            "{binds:?}: the command holds the image: {r}"
        );
        assert!(r.contains("fds=0 1 2\n"), "{binds:?}: {r}");
        assert_eq!(
            std::fs::metadata(&exe).unwrap().mode(),
            before,
            "{binds:?}: the binary's mode changed"
        );
        let r = run(&ctx, "echo next").await;
        assert!(r.contains("next\n"), "{binds:?}: {r}");
        let r = run(&f.ctx(ctx_sandbox(&ctx), "s2"), "echo another").await;
        assert!(r.contains("another\n"), "{binds:?}: {r}");
    }
}

// ---- 3. a session's file never runs as sandbox-exec ----

/// The review's `h_the_fallback_by_path_in_a_build_tree`, inverted: by path
/// (an older bubblewrap), in a build tree, two contained commands of two
/// sessions: one replaces the binary at a random moment, the other is
/// `echo RAN`. 200 pairs: the session's file never runs as `sandbox-exec`
/// and the command always runs. Before T.8f it ran as `sandbox-exec` in
/// about 2 % of the pairs, and every command after a replacement was
/// refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_session_replacing_the_binary_never_runs_as_sandbox_exec() {
    let rounds: usize = std::env::var("T8F_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let f = fixture(false);
    let master = f.binary(&f.root.join("master"));
    let Some(_) = f.contained(&master, Form::Full, Binds::Path, Network::On) else {
        return;
    };
    let bw = harness_tools::contain::find_bwrap().unwrap();
    let log = f.ws.join("ran-as");
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let (mut ran, mut session_file_ran, mut other) = (0, 0, Vec::new());
    for i in 0..rounds {
        let dir = f.ws.join(format!("t{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("rizzy-harness");
        std::fs::hard_link(&master, &exe).unwrap();
        impostor(&dir.join("imp"), &log);
        let sb = f
            .plain(&exe, Network::On)
            .with_containment(Contain::new(bw.clone(), Form::Full).with_binds(Binds::Path));
        let sb = Arc::new(sb);
        let (c1, c2) = (f.ctx(sb.clone(), "s1"), f.ctx(sb, "s2"));
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let spin = x % 40000;
        let script = format!(
            "while [ ! -e t{i}/go ]; do :; done; for ((k=0;k<{spin};k++)); do :; done; \
             mv -f t{i}/imp t{i}/rizzy-harness; echo swapped"
        );
        let swapper = tokio::spawn(async move { run(&c2, &script).await });
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let before = std::fs::read_to_string(&log).unwrap_or_default().len();
        std::fs::write(dir.join("go"), "").unwrap();
        let r = run(&c1, "echo RAN").await;
        let s = swapper.await.unwrap();
        let after = std::fs::read_to_string(&log).unwrap_or_default().len();
        if after > before {
            session_file_ran += 1;
        } else if r.contains("RAN\n") && s.contains("swapped") {
            ran += 1;
        } else {
            other.push(format!("{r:?} / {s:?}"));
        }
    }
    eprintln!(
        "{rounds} pairs: ran from the node's image {ran}; THE SESSION'S FILE RAN AS sandbox-exec \
         {session_file_ran}; other {} {:?}",
        other.len(),
        other.iter().take(2).collect::<Vec<_>>()
    );
    assert_eq!(session_file_ran, 0);
    assert_eq!(ran, rounds);
}

// ---- 4. the real node ----

/// The server, killed (by its pid) and reaped when dropped.
struct Server {
    child: std::process::Child,
    addr: String,
    token: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `serve` from `exe` on `data`, its log in `log`: `Ok` when it listens,
/// `Err(log)` when it ended first.
fn serve(exe: &Path, data: &Path, bind: &str, log: &Path) -> Result<Server, String> {
    use std::io::BufRead;
    let mut child = std::process::Command::new(exe)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", bind])
        .env("HARNESS_LOG", "info")
        .stdout(std::process::Stdio::piped())
        .stderr(std::fs::File::create(log).unwrap())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let addr = loop {
        let Some(Ok(line)) = lines.next() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::fs::read_to_string(log).unwrap_or_default());
        };
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.trim().to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    let token = std::fs::read_to_string(data.join("api-token"))
        .unwrap()
        .trim()
        .to_string();
    Ok(Server { child, addr, token })
}

impl Server {
    /// The output of the one tool call `text` makes, in a session of its
    /// own in `cwd` (the demo provider's `run: <command>`).
    async fn run(&self, cwd: &Path, text: &str) -> String {
        let c = reqwest::Client::new();
        let base = format!("http://{}", self.addr);
        let v: serde_json::Value = c
            .post(format!("{base}/api/sessions"))
            .bearer_auth(&self.token)
            .json(&json!({"cwd": cwd.display().to_string(), "approval_mode": "auto"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let sid = v["session_id"].as_str().unwrap_or_else(|| panic!("{v}"));
        let r = c
            .post(format!("{base}/api/sessions/{sid}/prompt"))
            .bearer_auth(&self.token)
            .json(&json!({ "text": text }))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "{}", r.status());
        let started = std::time::Instant::now();
        loop {
            let v: serde_json::Value = c
                .get(format!("{base}/api/sessions/{sid}/events?limit=5000"))
                .bearer_auth(&self.token)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            let evs = v["events"].as_array().cloned().unwrap_or_default();
            if let Some(f) = evs.iter().find(|e| e["kind"] == "tool_call_finished") {
                return f["payload"]["output"].as_str().unwrap_or("").to_string();
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(30),
                "{evs:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

fn node_files(f: &Fixture) {
    std::fs::write(
        f.data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n",
            f.ws.display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        f.data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
}

/// `serve` run from a copy of the binary in its own project folder (the
/// build tree, on the build's file system; the data directory in the temp
/// folder): a session replaces the binary as a rebuild would, and the next
/// contained command still runs. Its `file view` log line names no path of
/// the image. Before T.8f the command was refused ("restart the node"),
/// and with T.8f's first design too, where the data directory is on
/// another file system than the binary.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_run_from_its_build_tree_survives_a_rebuild() {
    let f = fixture(true);
    let exe = f.binary(&f.ws.join("target/debug/rizzy-harness"));
    node_files(&f);
    let log = f.root.join("serve.log");
    let s = serve(&exe, &f.data, "127.0.0.1:0", &log).unwrap();
    let first = s.run(&f.ws, "run: echo pid=$$").await;
    if !first.contains("pid=2\n") {
        assert!(!bwrap::required(), "the shell is not contained: {first}");
        eprintln!("SKIPPED: the shell is not contained here: {first}");
        return;
    }
    let view = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .find(|l| l.contains("file view: "))
        .unwrap_or_default()
        .to_string();
    eprintln!("{view}");
    assert!(!view.contains("/.rizzy-harness"), "{view}");
    let r = s
        .run(
            &f.ws,
            "run: rm target/debug/rizzy-harness && printf '#!/bin/sh\\necho IMPOSTOR\\n' > \
             target/debug/rizzy-harness && chmod +x target/debug/rizzy-harness && echo replaced",
        )
        .await;
    assert!(r.contains("replaced"), "{r}");
    let r = s.run(&f.ws, "run: echo second").await;
    eprintln!("after the rebuild: {r}");
    assert!(r.contains("second\n") && !r.contains("IMPOSTOR"), "{r}");
    assert!(image_names(&f.data).is_empty());
}

/// The review's `d_a_second_node_and_doctor` and `g_a_restart`, for what
/// still applies without a link: a second start of another build on the
/// same data directory (it fails on the port that is taken) and `doctor`,
/// of either build, do not stop the running node's contained commands; a
/// restart works; nothing named `node-image*` is ever made in the data
/// directory. With T.8f's first design the failed second start replaced
/// the link and the running node refused every command.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_start_doctor_and_a_restart_leave_the_node_alone() {
    let f = fixture(false);
    let a = f.binary(&f.root.join("a/rizzy-harness"));
    let b = f.binary(&f.root.join("b/rizzy-harness"));
    node_files(&f);
    let s = serve(&a, &f.data, "127.0.0.1:0", &f.root.join("a.log")).unwrap();
    let first = s.run(&f.ws, "run: echo pid=$$").await;
    if !first.contains("pid=2\n") {
        assert!(!bwrap::required(), "the shell is not contained: {first}");
        eprintln!("SKIPPED: the shell is not contained here: {first}");
        return;
    }
    let e = serve(&b, &f.data, &s.addr, &f.root.join("b.log")).err();
    assert!(e.is_some(), "B started on A's port");
    for exe in [&a, &b] {
        let out = std::process::Command::new(exe)
            .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout).into_owned();
        let v = out
            .lines()
            .find(|l| l.contains(" file view: "))
            .unwrap_or("");
        assert!(
            !v.contains("/.rizzy-harness") && !v.contains("node-image"),
            "{v}"
        );
    }
    let r = s.run(&f.ws, "run: echo A-RAN").await;
    assert!(r.contains("A-RAN\n"), "{r}");
    assert_eq!(image_names(&f.data), Vec::<String>::new());
    drop(s);
    let s = serve(&a, &f.data, "127.0.0.1:0", &f.root.join("a2.log")).unwrap();
    let r = s.run(&f.ws, "run: echo A-AGAIN").await;
    assert!(r.contains("A-AGAIN\n"), "{r}");
    assert_eq!(image_names(&f.data), Vec::<String>::new());
}
