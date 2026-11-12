//! The node's link to its own running image (ADOPTION_PLAN T.8f, review
//! note T8C-N3): a harness binary replaced under a running node (a `cargo
//! build`, an upgrade) no longer stops contained commands, the file now at
//! the binary's path is never what runs as `sandbox-exec`, and where the
//! link cannot be made the view is exactly T.8c's.
//!
//! Each test runs a copy of the harness binary it owns, in its own temp
//! folder, next to its own data directory (same file system), never the
//! build's binary replaced in place. They skip where bubblewrap or Landlock
//! does not work, and fail there with `HARNESS_REQUIRE_BWRAP=1`.

#[allow(dead_code)]
#[path = "support/bwrap.rs"]
mod bwrap;

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::contain::{Binds, Contain, Form};
use harness_tools::image::ImageLink;
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A temp folder holding a data directory, a session folder and a copy of
/// the harness binary (`exe`), all on one file system.
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
    let ws = root.join("ws");
    for d in [&data, &ws, &data.join("sandbox-tmp")] {
        std::fs::create_dir_all(d).unwrap();
    }
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
    }
}

impl Fixture {
    /// A copy of the harness binary at `rel` (a file of its own: one name).
    fn binary(&self, rel: &str) -> PathBuf {
        let exe = self.root.join(rel);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::copy(EXE, &exe).unwrap();
        exe
    }

    fn plain(&self, exe: &Path) -> Sandbox {
        Sandbox::new(
            exe.to_owned(),
            &Settings::default(),
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// `exe`'s sandbox, contained in `form` with `binds` and probed as the
    /// node probes it, or `None` (and a printed reason) where that cannot
    /// run here. Landlock is probed first, as the node does.
    fn contained(&self, exe: &Path, form: Form, binds: Binds) -> Option<Sandbox> {
        let sb = self.plain(exe);
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
        let c = Contain::new(bw, form).with_binds(binds);
        if let Err(e) = sb.probe_containment(&c, &self.data.join("sandbox-tmp/probe-contain")) {
            assert!(
                !(bwrap::required() && form == Form::Full),
                "{form:?} {binds:?}: {e}"
            );
            eprintln!("SKIPPED ({form:?}, {binds:?}): {e}");
            return None;
        }
        Some(sb.with_containment(c))
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

    fn link(&self) -> PathBuf {
        self.data.join(harness_tools::image::LINK)
    }
}

async fn run(ctx: &ToolCtx, script: &str) -> String {
    let r = Bash.run(ctx, json!({ "command": script })).await;
    r.output
}

/// A script that records how it was run in `log`, then runs the command
/// after its first `--` (as `sandbox-exec` would, unconfined): what a
/// session or an old build puts in the binary's place.
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
    std::fs::set_permissions(at, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

fn ino(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).unwrap().ino()
}

// ---- 1. a binary replaced after the start ----

/// The binary lies in the session's folder (a node run from its build
/// tree); one command removes it and writes a recording script in its
/// place, as a rebuild would. The next command still runs, in both
/// bubblewrap forms and both bind modes, and what runs as `sandbox-exec`
/// is the old image (its inode, through the link), never the file now at
/// the path. Before T.8f the next command was refused ("restart the node").
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_binary_replaced_after_the_start_still_runs_from_the_old_image() {
    let mut ran = 0;
    for (form, binds) in [
        (Form::Full, Binds::Fd),
        (Form::Full, Binds::Path),
        (Form::SharedProc, Binds::Fd),
        (Form::SharedProc, Binds::Path),
    ] {
        let f = fixture();
        let exe = f.binary("ws/target/debug/rizzy-harness");
        let old = ino(&exe);
        let Some(sb) = f.contained(&exe, form, binds) else {
            continue;
        };
        let link = sb.link_image(&f.data).clone();
        eprintln!("{form:?} {binds:?}: {link:?}");
        let ctx = f.ctx(Arc::new(sb));
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
        assert!(r1.contains("replaced"), "{form:?} {binds:?}: {r1}");
        assert_ne!(ino(&exe), old, "the binary was not replaced");
        let r2 = run(
            &ctx,
            "echo second; stat -c 'inode=%i' /.rizzy-harness/rizzy-harness",
        )
        .await;
        let wrote = std::fs::read_to_string(&log).unwrap_or_default();
        eprintln!("{form:?} {binds:?}: command 2: {r2:?}; the replacement wrote: {wrote:?}");
        assert!(
            !wrote.contains("sandbox-exec"),
            "{form:?} {binds:?}: the replacement ran as sandbox-exec: {wrote}"
        );
        assert!(r2.contains("second\n"), "{form:?} {binds:?}: {r2}");
        assert!(
            r2.contains(&format!("inode={old}")),
            "{form:?} {binds:?}: not the old image: {r2}"
        );
        assert_eq!(link, ImageLink::Linked(f.link()));
        ran += 1;
    }
    assert!(ran > 0 || !bwrap::required(), "no form ran");
}

// ---- 2. a binary replaced before the link ----

/// The window between the node's start (its image's descriptor is opened)
/// and the link, made deterministic: the binary is replaced by a rename
/// in between. The file now at the path is not linked (the link is made
/// through the held descriptor, whose file has no name left, and checked
/// by identity); the node is in T.8c's fallback, says so, and refuses
/// commands as T.8c does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_binary_replaced_before_the_link_is_not_used() {
    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    let Some(sb) = f.contained(&exe, Form::Full, Binds::Fd) else {
        return;
    };
    let log = f.root.join("ran-as");
    let next = f.root.join("bin/next");
    impostor(&next, &log);
    std::fs::rename(&next, &exe).unwrap();
    let link = sb.link_image(&f.data).clone();
    eprintln!("{link:?}");
    let ImageLink::NotMade(why) = &link else {
        panic!("a link was made to a replaced binary: {link:?}")
    };
    assert!(why.contains("replaced"), "{why}");
    assert!(
        std::fs::symlink_metadata(f.link()).is_err(),
        "a link was left at {}",
        f.link().display()
    );
    let view = sb.view_line();
    assert!(view.contains(why) && view.contains("restart"), "{view}");
    let ctx = f.ctx(Arc::new(sb));
    let r = run(&ctx, "echo RAN-AFTER-REPLACE").await;
    eprintln!("{r}");
    assert!(!r.contains("RAN-AFTER-REPLACE\n"), "{r}");
    assert!(r.contains("restart the node"), "{r}");
    assert_eq!(std::fs::read_to_string(&log).unwrap_or_default(), "");
}

// ---- 3. where the link cannot be made ----

/// Restores a folder's mode when dropped.
struct Mode(PathBuf, u32);

impl Drop for Mode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
    }
}

/// A data directory the node cannot write (the seam for every reason the
/// link cannot be made: another file system and a binary under
/// `protected_hardlinks` take the same way): T.8c's view, unchanged. The
/// command runs; after the binary is deleted, every command is refused with
/// the reason. Where the build's binary is on another file system than the
/// temp folder, the real `EXDEV` is checked too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn where_no_link_can_be_made_the_view_is_as_before() {
    let f = fixture();
    // The real EXDEV, where this machine has it.
    let other_fs =
        std::fs::metadata(EXE).unwrap().dev() != std::fs::metadata(&f.data).unwrap().dev();
    if other_fs {
        let sb = f.plain(Path::new(EXE));
        let l = sb.link_image(&f.data).clone();
        eprintln!("EXDEV: {l:?}");
        let ImageLink::NotMade(why) = &l else {
            panic!("{l:?}")
        };
        assert!(why.contains("another file system"), "{why}");
    } else {
        eprintln!("(the build and the temp folder share a file system: no real EXDEV here)");
    }
    if nix::unistd::geteuid().is_root() {
        eprintln!("SKIPPED (read-only data directory): root writes it anyway");
        return;
    }
    let exe = f.binary("bin/rizzy-harness");
    let Some(sb) = f.contained(&exe, Form::Full, Binds::Fd) else {
        return;
    };
    let l = {
        use std::os::unix::fs::PermissionsExt;
        let _back = Mode(f.data.clone(), 0o755);
        std::fs::set_permissions(&f.data, std::fs::Permissions::from_mode(0o555)).unwrap();
        sb.link_image(&f.data).clone()
    };
    eprintln!("read-only: {l:?}");
    let ImageLink::NotMade(why) = &l else {
        panic!("{l:?}")
    };
    assert!(why.contains("cannot be written"), "{why}");
    assert!(sb.view_line().contains("restart"), "{}", sb.view_line());
    let ctx = f.ctx(Arc::new(sb));
    let r = run(&ctx, "echo before").await;
    assert!(r.contains("before\n"), "{r}");
    std::fs::remove_file(&exe).unwrap();
    std::fs::copy(EXE, &exe).unwrap();
    let r = run(&ctx, "echo RAN-AFTER-DELETE").await;
    eprintln!("{r}");
    assert!(!r.contains("RAN-AFTER-DELETE\n"), "{r}");
    assert!(r.contains("restart the node"), "{r}");
}

// ---- 4. what an earlier start left ----

/// Whatever an earlier start (or anything else) left at the link's name is
/// replaced at start, without following it: another build's file, a
/// symlink (its target is left alone), a folder. And a link replaced after
/// the start (a second node on the same data directory, task A.20) is never
/// what runs: the guard checks it against its own image and refuses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_left_by_an_earlier_start_is_never_what_runs() {
    for left in ["other-build", "symlink", "folder"] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        let log = f.root.join("ran-as");
        let other = f.root.join("other-build");
        impostor(&other, &log);
        match left {
            "other-build" => std::fs::hard_link(&other, f.link()).unwrap(),
            "symlink" => std::os::unix::fs::symlink(&other, f.link()).unwrap(),
            _ => {
                std::fs::create_dir_all(f.link().join("inside")).unwrap();
                std::fs::write(f.link().join("inside/file"), "x").unwrap();
            }
        }
        let Some(sb) = f.contained(&exe, Form::Full, Binds::Fd) else {
            return;
        };
        let l = sb.link_image(&f.data).clone();
        eprintln!("{left}: {l:?}");
        assert_eq!(l, ImageLink::Linked(f.link()), "{left}");
        assert_eq!(ino(&f.link()), ino(&exe), "{left}: not the running image");
        assert!(
            std::fs::read_to_string(&other)
                .unwrap()
                .starts_with("#!/bin/sh"),
            "{left}: the old file was touched through the name"
        );
        let ctx = f.ctx(Arc::new(sb));
        let r = run(&ctx, "echo first").await;
        assert!(r.contains("first\n"), "{left}: {r}");
        assert_eq!(std::fs::read_to_string(&log).unwrap_or_default(), "");

        // A second start's link put over this one's.
        std::fs::remove_file(f.link()).unwrap();
        std::fs::hard_link(&other, f.link()).unwrap();
        let r = run(&ctx, "echo RAN-WITH-ANOTHER-LINK").await;
        eprintln!("{left}: {r}");
        assert!(!r.contains("RAN-WITH-ANOTHER-LINK\n"), "{left}: {r}");
        assert!(
            r.contains("restart the node") && r.contains(&f.link().display().to_string()),
            "{left}: {r}"
        );
        assert_eq!(std::fs::read_to_string(&log).unwrap_or_default(), "");
        // And none at all.
        std::fs::remove_file(f.link()).unwrap();
        let r = run(&ctx, "echo RAN-WITHOUT-A-LINK").await;
        assert!(!r.contains("RAN-WITHOUT-A-LINK\n"), "{left}: {r}");
        assert!(r.contains("restart the node"), "{left}: {r}");
    }
}

// ---- 5. a session and the link ----

/// A contained command cannot see, write, replace or remove the link: it is
/// in the data directory, which no grant holds (in the view the data
/// directory is only the parent of the session's temp folder, read-only).
/// Checked in both bind modes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_contained_command_cannot_touch_the_link() {
    for binds in [Binds::Fd, Binds::Path] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        let Some(sb) = f.contained(&exe, Form::Full, binds) else {
            return;
        };
        assert_eq!(sb.link_image(&f.data), &ImageLink::Linked(f.link()));
        let before = (ino(&f.link()), std::fs::read(f.link()).unwrap());
        let ctx = f.ctx(Arc::new(sb));
        let l = f.link().display().to_string();
        let d = f.data.display().to_string();
        let r = run(
            &ctx,
            &format!(
                "if [ -e '{l}' ]; then echo link=there; else echo link=absent; fi; \
                 echo data-holds=$(ls -A '{d}' 2>&1 | tr '\\n' ' '); \
                 echo x > '{l}' 2>/dev/null && echo WROTE; rm -f '{l}' 2>/dev/null; \
                 mv '{l}' moved 2>/dev/null && echo MOVED; ln -sf /bin/sh '{l}' 2>/dev/null \
                 && echo LINKED; echo done"
            ),
        )
        .await;
        eprintln!("{binds:?}: {r}");
        // The data directory is there only as the empty parent of the
        // session's temp folder's mount point.
        assert!(r.contains("link=absent"), "{r}");
        // (Landlock inside, where it is, refuses even the listing.)
        assert!(
            r.contains("data-holds=sandbox-tmp \n") || r.contains("cannot open directory"),
            "{r}"
        );
        assert!(
            r.contains("done") && !r.contains("WROTE") && !r.contains("MOVED"),
            "{r}"
        );
        assert!(!r.contains("LINKED"), "{r}");
        assert_eq!(
            (ino(&f.link()), std::fs::read(f.link()).unwrap()),
            before,
            "{binds:?}: the link changed"
        );
        let r = run(&ctx, "echo still").await;
        assert!(r.contains("still\n"), "{r}");
    }
}

// ---- 6. doctor ----

/// `doctor` says in its `file view` line whether the node keeps its image
/// through the link, or why not, and does not touch a running node's link
/// (it tries under another name, and removes it).
#[test]
fn doctor_names_the_link_and_leaves_it_alone() {
    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    if f.contained(&exe, Form::Full, Binds::Fd).is_none() {
        return;
    }
    let log = f.root.join("ran-as");
    let other = f.root.join("other-build");
    impostor(&other, &log);
    std::fs::hard_link(&other, f.link()).unwrap();
    std::fs::write(f.data.join("node.toml"), "").unwrap();
    let out = std::process::Command::new(&exe)
        .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    let v = out
        .lines()
        .find(|l| l.contains(" file view: "))
        .unwrap_or_else(|| panic!("no file view line: {out}"));
    eprintln!("{v}");
    assert!(v.contains(harness_tools::image::LINK), "{v}");
    assert!(v.contains("rebuild"), "{v}");
    assert_eq!(
        ino(&f.link()),
        ino(&other),
        "doctor replaced the node's link"
    );
    let left: Vec<String> = std::fs::read_dir(&f.data)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with(harness_tools::image::LINK) && n != harness_tools::image::LINK)
        .collect();
    assert!(left.is_empty(), "doctor left {left:?}");
}

// ---- 7. the real node ----

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

/// `serve` run from a copy of the binary inside its own project folder
/// (the build tree): at start it links its image into the data directory
/// and says so in its `file view` log line; a session then replaces the
/// binary as a rebuild would, and the next contained command still runs.
/// Before T.8f it was refused ("restart the node").
#[tokio::test(flavor = "multi_thread")]
async fn a_node_run_from_its_build_tree_survives_a_rebuild() {
    use std::io::BufRead;
    let f = fixture();
    let exe = f.binary("ws/target/debug/rizzy-harness");
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
    let log = f.root.join("serve.log");
    let mut child = std::process::Command::new(&exe)
        .args(["--data-dir", f.data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .env("HARNESS_LOG", "info")
        .stdout(std::process::Stdio::piped())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let addr = loop {
        let Some(Ok(line)) = lines.next() else {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "the server ended: {}",
                std::fs::read_to_string(&log).unwrap()
            );
        };
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.trim().to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    let token = std::fs::read_to_string(f.data.join("api-token"))
        .unwrap()
        .trim()
        .to_string();
    let s = Server { child, addr, token };
    let old = ino(&exe);
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
    assert!(
        view.contains(&format!("kept by its link {}", f.link().display())),
        "{view}"
    );
    assert_eq!(ino(&f.link()), old);
    let r = s
        .run(
            &f.ws,
            "run: rm target/debug/rizzy-harness && printf '#!/bin/sh\\necho IMPOSTOR\\n' > \
             target/debug/rizzy-harness && chmod +x target/debug/rizzy-harness && echo replaced",
        )
        .await;
    assert!(r.contains("replaced"), "{r}");
    let r = s
        .run(
            &f.ws,
            "run: echo second; stat -c inode=%i /.rizzy-harness/rizzy-harness",
        )
        .await;
    eprintln!("after the rebuild: {r}");
    assert!(r.contains("second\n") && !r.contains("IMPOSTOR"), "{r}");
    assert!(r.contains(&format!("inode={old}")), "{r}");
}
