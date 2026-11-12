//! T.8f review (2026-11): proofs and observations that need the node's own
//! code. Not part of the test suite: `run_rust_proofs.sh` copies this file
//! to `crates/cli/tests/` for the run and removes the copy again.
//!
//! Every test here PRINTS what it observed; the asserts are only the facts
//! the report quotes. Each works on copies of the harness binary and on
//! data directories of its own under `TMPDIR`.

#[allow(dead_code)]
#[path = "support/bwrap.rs"]
mod bwrap;

use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::contain::{Binds, Contain, Form};
use harness_tools::image::ImageLink;
use harness_tools::sandbox::{Confinement, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

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

    fn contained(&self, exe: &Path, form: Form, binds: Binds) -> Sandbox {
        let sb = self.plain(exe);
        sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data)
            .expect("Landlock");
        let bw = harness_tools::contain::find_bwrap().expect("bubblewrap");
        let c = Contain::new(bw, form).with_binds(binds);
        sb.probe_containment(&c, &self.data.join("sandbox-tmp/probe-contain"))
            .expect("containment");
        sb.with_containment(c)
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

    fn link(&self) -> PathBuf {
        self.data.join(harness_tools::image::LINK)
    }
}

async fn run(ctx: &ToolCtx, script: &str) -> String {
    Bash.run(ctx, json!({ "command": script })).await.output
}

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

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().mode() & 0o7777
}

fn names(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(d)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

/// What a contained command does with any inherited descriptor of the image.
const PROBE_PY: &str = r#"
import os, sys, errno
img = os.stat('/.rizzy-harness/rizzy-harness')
print(f"view image: nlink={img.st_nlink} mode={oct(img.st_mode & 0o7777)}")
found = None
for fd in sorted(os.listdir('/proc/self/fd'), key=int):
    p = '/proc/self/fd/' + fd
    try:
        st = os.stat(p)
    except OSError:
        continue
    if (st.st_dev, st.st_ino) == (img.st_dev, img.st_ino):
        found = p
        print(f"inherited descriptor {fd} IS the image; it names: {os.path.basename(os.readlink(p))}")
def t(name, f):
    try:
        f()
        print(f"{name}: DONE")
    except OSError as e:
        print(f"{name}: refused ({errno.errorcode.get(e.errno)})")
view = '/.rizzy-harness/rizzy-harness'
t("view path: chmod 000", lambda: os.chmod(view, 0))
t("view path: open for writing", lambda: os.close(os.open(view, os.O_WRONLY)))
t("view path: link into the temp folder", lambda: os.link(view, os.environ['TMPDIR'] + '/keep-v'))
if not found:
    print("no inherited descriptor of the image")
    sys.exit(0)
t("fd: open for writing", lambda: os.close(os.open(found, os.O_WRONLY)))
t("fd: truncate", lambda: os.truncate(found, 0))
t("fd: link into the temp folder (same file system as the data directory)",
  lambda: os.link(found, os.environ['TMPDIR'] + '/keep', follow_symlinks=True))
t("fd: link into the session folder", lambda: os.link(found, 'keep', follow_symlinks=True))
t("fd: setxattr user.review", lambda: os.setxattr(found, 'user.review', b'1'))
t("fd: utime", lambda: os.utime(found, (0, 0)))
t("fd: chmod 000", lambda: os.chmod(found, 0))
"#;

// ---- A. what a command can do to the image's inode (T8C-N2 with a link) ----

/// The binary is OUTSIDE every grant (an installed, user-owned binary). In
/// each bind mode, with the link (T.8f) and without (T.8c's fallback): what
/// a contained command can do to the image, and what the next command gets.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_what_a_command_can_do_to_the_image() {
    for (binds, linked) in [(Binds::Fd, true), (Binds::Path, true), (Binds::Path, false)] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        std::fs::write(f.ws.join("probe.py"), PROBE_PY).unwrap();
        let sb = f.contained(&exe, Form::Full, binds);
        let l = if linked {
            format!("{:?}", sb.link_image(&f.data))
                .chars()
                .take(6)
                .collect()
        } else {
            "no link (T.8c's view)".to_string()
        };
        println!("==== {binds:?}, {l} ====");
        let ctx = f.ctx(Arc::new(sb), "s1");
        let before = (ino(&exe), mode(&exe));
        let r = run(&ctx, "python3 probe.py").await;
        println!("{}", r.replace(f.root.to_str().unwrap(), "<root>"));
        let after = (ino(&exe), mode(&exe));
        println!(
            "on the host: the binary's mode before {:o}, after {:o}; size {}",
            before.1,
            after.1,
            std::fs::symlink_metadata(&exe).unwrap().len()
        );
        let r2 = run(&ctx, "echo NEXT-COMMAND-RAN").await;
        println!(
            "the next command: {}",
            r2.replace(f.root.to_str().unwrap(), "<root>")
        );
        let r3 = run(&f.ctx(ctx_sandbox(&ctx), "s2"), "echo ANOTHER-SESSION-RAN").await;
        println!(
            "another session's command: {}",
            r3.replace(f.root.to_str().unwrap(), "<root>")
        );
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
}

fn ctx_sandbox(ctx: &ToolCtx) -> Arc<Sandbox> {
    match &ctx.sandbox {
        Confinement::Confined { sandbox, .. } => sandbox.clone(),
        _ => panic!("not confined"),
    }
}

// ---- C. the link replaced after the guard's check ----

/// Something that can write the data directory (a second node's start, task
/// A.20) replaces the link at a random moment while commands run. Counted:
/// commands that ran from the node's image, commands refused, and commands
/// for which the OTHER file ran as `sandbox-exec`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn c_a_link_replaced_after_the_guards_check() {
    let rounds: usize = std::env::var("REVIEW_T8F_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    for binds in [Binds::Path, Binds::Fd] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        let log = f.ws.join("ran-as");
        let other = f.root.join("other-build");
        impostor(&other, &log);
        let sb = f.contained(&exe, Form::Full, binds);
        assert_eq!(sb.link_image(&f.data), &ImageLink::Linked(f.link()));
        let ctx = f.ctx(Arc::new(sb), "s1");
        // The copy of the binary is written out first: on a slow disk the
        // swapper's link and rename otherwise wait seconds for the journal.
        std::fs::File::open(&exe).unwrap().sync_all().unwrap();
        std::fs::File::open(&f.data).unwrap().sync_all().unwrap();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let swaps = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (data, link, exe2, other2, stop2, swaps2) = (
            f.data.clone(),
            f.link(),
            exe.clone(),
            other.clone(),
            stop.clone(),
            swaps.clone(),
        );
        let swapper = std::thread::spawn(move || {
            let mut x: u64 = 0x9E3779B97F4A7C15;
            let mut nap = || {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                std::thread::sleep(std::time::Duration::from_micros(x % 7000));
            };
            while !stop2.load(std::sync::atomic::Ordering::SeqCst) {
                for src in [&other2, &exe2] {
                    let t = data.join("swap");
                    let _ = std::fs::remove_file(&t);
                    std::fs::hard_link(src, &t).unwrap();
                    std::fs::rename(&t, &link).unwrap();
                    swaps2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    nap();
                }
            }
        });
        let (mut ok, mut refused_guard, mut refused_bwrap, mut other_ran, mut odd) =
            (0, 0, 0, 0, Vec::new());
        for i in 0..rounds {
            let before = std::fs::read_to_string(&log).unwrap_or_default().len();
            let r = run(&ctx, "echo RAN").await;
            let after = std::fs::read_to_string(&log).unwrap_or_default().len();
            if after > before {
                other_ran += 1;
                if other_ran == 1 {
                    println!(
                        "{binds:?}: round {i}: THE OTHER FILE RAN AS sandbox-exec; the command's \
                         output: {r:?}; its record: {:?}",
                        std::fs::read_to_string(&log).unwrap_or_default()
                    );
                }
            } else if r.contains("RAN\n") {
                ok += 1;
            } else if r.contains("is not the image this node runs") {
                refused_guard += 1;
            } else if r.contains("bwrap:") {
                refused_bwrap += 1;
                if refused_bwrap <= 2 {
                    println!(
                        "{binds:?}: bubblewrap's refusal: {}",
                        r.replace(f.root.to_str().unwrap(), "<root>").trim()
                    );
                }
            } else {
                odd.push(r);
            }
        }
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        swapper.join().unwrap();
        println!(
            "{binds:?}: {rounds} commands, {} swaps of the link: ran from the node's image {ok}; \
             refused by the guard {refused_guard}; refused by bubblewrap {refused_bwrap}; THE \
             OTHER FILE RAN AS sandbox-exec {other_ran}; other outcomes {}",
            swaps.load(std::sync::atomic::Ordering::SeqCst),
            odd.len()
        );
        for o in odd.iter().take(3) {
            println!(
                "  other outcome: {:?}",
                o.replace(f.root.to_str().unwrap(), "<root>")
            );
        }
        if binds == Binds::Fd {
            assert_eq!(other_ran, 0, "fd mode ran another file");
        }
    }
}

// ---- E. what a start finds, and what it leaves ----

fn held(p: &Path) -> std::fs::File {
    std::fs::File::open(p).unwrap()
}

#[test]
fn e_what_a_start_finds_and_leaves() {
    use harness_tools::image::{LINK, make, trial};
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let bin = root.join("bin");
    std::fs::write(&bin, "image").unwrap();
    let f = held(&bin);
    let outside = root.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("kept"), "kept").unwrap();
    let pid = std::process::id();

    // E1: a FIFO, a dangling symlink, a socket-like special file at the name.
    nix::unistd::mkfifo(
        &data.join(LINK),
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .unwrap();
    let r = make(f.as_raw_fd(), &data);
    println!(
        "E1 a FIFO at the name: {r:?}; now a regular file with the image's inode: {}",
        ino(&data.join(LINK)) == ino(&bin)
    );
    std::fs::remove_file(data.join(LINK)).unwrap();
    std::os::unix::fs::symlink("/nonexistent/x", data.join(LINK)).unwrap();
    let r = make(f.as_raw_fd(), &data);
    println!(
        "E1 a dangling symlink at the name: {r:?}; the image's inode: {}",
        ino(&data.join(LINK)) == ino(&bin)
    );

    // E2: a folder at the name holding links out.
    std::fs::remove_file(data.join(LINK)).unwrap();
    std::fs::create_dir_all(data.join(LINK).join("deep")).unwrap();
    std::os::unix::fs::symlink(&outside, data.join(LINK).join("to-folder")).unwrap();
    std::os::unix::fs::symlink(outside.join("kept"), data.join(LINK).join("deep/to-file")).unwrap();
    std::fs::hard_link(outside.join("kept"), data.join(LINK).join("deep/hard")).unwrap();
    let r = make(f.as_raw_fd(), &data);
    println!(
        "E2 a folder with links out at the name: {r:?}; outside still holds {:?} = {:?}",
        names(&outside),
        std::fs::read_to_string(outside.join("kept")).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("kept")).unwrap(),
        "kept"
    );

    // E3: the temporary name occupied: a symlink out, then a folder.
    let tmp = data.join(format!("{LINK}.new-{pid}"));
    std::os::unix::fs::symlink(outside.join("kept"), &tmp).unwrap();
    let r = make(f.as_raw_fd(), &data);
    println!(
        "E3 a symlink at the temporary name: {r:?}; its target: {:?}; the data directory: {:?}",
        std::fs::read_to_string(outside.join("kept")).unwrap(),
        names(&data)
    );
    // (What the line above shows left at the temporary name is finding
    // T8F-1: see `g_a_restart_leaves_its_temporary_link`.)
    let _ = std::fs::remove_file(&tmp);
    std::fs::create_dir_all(tmp.join("x")).unwrap();
    let r = make(f.as_raw_fd(), &data);
    println!(
        "E3 a folder at the temporary name: {r:?}; the data directory: {:?}",
        names(&data)
    );
    let _ = std::fs::remove_file(&tmp);

    // E4: what a start that crashed between the link and the rename left
    // (another pid's temporary name): is it ever removed?
    let stale = data.join(format!("{LINK}.new-{}", pid.wrapping_add(100_000)));
    let old = root.join("old-build");
    std::fs::write(&old, "an old build").unwrap();
    std::fs::hard_link(&old, &stale).unwrap();
    std::fs::remove_file(&old).unwrap();
    let r = make(f.as_raw_fd(), &data);
    let _ = std::fs::remove_file(&tmp);
    let t = trial(f.as_raw_fd(), &data);
    println!(
        "E4 a crashed start's temporary link (another pid): after a start ({r:?}) and a doctor \
         ({t:?}) the data directory holds {:?}",
        names(&data)
    );
    std::fs::remove_file(&stale).unwrap();

    // E5: a data directory that cannot be written, with an earlier start's
    // link at the name: is anything left at the name?
    if !nix::unistd::geteuid().is_root() {
        std::fs::remove_file(data.join(LINK)).unwrap();
        std::fs::write(data.join(LINK), "an earlier build").unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = make(f.as_raw_fd(), &data);
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
        println!(
            "E5 an unwritable data directory with an earlier link: {r:?}; the data directory \
             holds {:?}; the name holds {:?}",
            names(&data),
            std::fs::read_to_string(data.join(LINK)).ok()
        );
    }

    // E6: the data directory reached through a symlink.
    let via = root.join("data-link");
    std::os::unix::fs::symlink(&data, &via).unwrap();
    let _ = std::fs::remove_file(data.join(LINK));
    let r = make(f.as_raw_fd(), &via);
    println!("E6 the data directory through a symlink: {r:?}");

    // E7: a binary with a second name, its first name replaced (a cargo
    // build tree: target/debug/x is a hard link of deps/x-<hash>).
    let bin2 = root.join("bin2");
    std::fs::write(&bin2, "image two").unwrap();
    std::fs::hard_link(&bin2, root.join("bin2-deps")).unwrap();
    let f2 = held(&bin2);
    std::fs::write(root.join("next"), "next").unwrap();
    std::fs::rename(root.join("next"), &bin2).unwrap();
    let r = make(f2.as_raw_fd(), &data);
    println!(
        "E7 a binary replaced at its path that has another name: {r:?}; the link is the held \
         image: {}; is the file now at the path: {}",
        ino(&data.join(LINK)) == ino(&root.join("bin2-deps")),
        ino(&data.join(LINK)) == ino(&bin2)
    );
    assert_eq!(ino(&data.join(LINK)), ino(&root.join("bin2-deps")));

    // E8: doctor's trial with its own names occupied by someone else's things.
    let dn = data.join(format!("{LINK}.doctor-{pid}"));
    std::fs::create_dir_all(dn.join("someone-elses")).unwrap();
    std::fs::write(dn.join("someone-elses/file"), "x").unwrap();
    let _ = std::fs::remove_file(&tmp);
    let before = ino(&data.join(LINK));
    let t = trial(f.as_raw_fd(), &data);
    println!(
        "E8 doctor's trial with a folder at its own name: {t:?}; the node's link untouched: {}; \
         the data directory holds {:?}",
        ino(&data.join(LINK)) == before,
        names(&data)
    );
}

/// Two starts racing on one data directory with different images (A.20).
/// For each outcome: what each start believes, and what the name holds.
#[test]
fn e9_two_starts_racing() {
    use harness_tools::image::{LINK, make};
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::write(&a, "image a").unwrap();
    std::fs::write(&b, "image b").unwrap();
    let (fa, fb) = (held(&a), held(&b));
    let mut tally = std::collections::BTreeMap::<String, usize>::new();
    for _ in 0..3000 {
        let _ = std::fs::remove_file(data.join(LINK));
        let barrier = std::sync::Barrier::new(2);
        let (ra, rb) = std::thread::scope(|s| {
            let ta = s.spawn(|| {
                barrier.wait();
                // The same pid in both threads: give each its own temporary
                // name by racing only the final steps is not possible from
                // outside; two processes differ in pid. See the note printed.
                make(fa.as_raw_fd(), &data)
            });
            barrier.wait();
            let rb = make(fb.as_raw_fd(), &data);
            (ta.join().unwrap(), rb)
        });
        let holds = match std::fs::symlink_metadata(data.join(LINK)) {
            Ok(m) if m.ino() == ino(&a) => "a",
            Ok(m) if m.ino() == ino(&b) => "b",
            Ok(_) => "another file",
            Err(_) => "nothing",
        };
        let short = |r: &ImageLink| match r {
            ImageLink::Linked(_) => "Linked".to_string(),
            ImageLink::NotMade(w) => format!(
                "NotMade({})",
                w.chars()
                    .rev()
                    .take(48)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            ),
            o => format!("{o:?}"),
        };
        *tally
            .entry(format!(
                "A: {} | B: {} | the name holds {holds}",
                short(&ra),
                short(&rb)
            ))
            .or_default() += 1;
        // The property that matters: a start that says Linked while the
        // name holds the other image is caught by the guard (refused); a
        // start must never be told Linked for a name it then finds gone
        // AND later filled by the other. Recorded, not asserted.
    }
    println!(
        "(both threads share one pid here, so they also share the temporary name: this is \
              harsher than two processes)"
    );
    for (k, v) in tally {
        println!("{v:5}  {k}");
    }
    println!("left in the data directory: {:?}", names(&data));
}

// ---- real nodes ----

struct Server {
    child: std::process::Child,
    addr: String,
    token: String,
    log: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(exe: &Path, data: &Path, ws: &Path, log: &Path, bind: &str) -> Result<Server, String> {
        use std::io::BufRead;
        if !data.join("node.toml").exists() {
            std::fs::write(
                data.join("node.toml"),
                format!(
                    "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n",
                    ws.display().to_string()
                ),
            )
            .unwrap();
            std::fs::write(
                data.join("config.toml"),
                "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \
                 \"mock\"\nmock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
            )
            .unwrap();
        }
        let mut child = std::process::Command::new(exe)
            .args(["--data-dir", data.to_str().unwrap()])
            .args(["serve", "--bind", bind])
            .env("HARNESS_LOG", "info")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::fs::File::create(log).unwrap())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", exe.display()))?;
        let mut lines = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        let addr = loop {
            let Some(Ok(line)) = lines.next() else {
                let _ = child.kill();
                let st = child.wait();
                return Err(format!(
                    "the server ended ({st:?}): {}",
                    std::fs::read_to_string(log).unwrap_or_default()
                ));
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
        Ok(Server {
            child,
            addr,
            token,
            log: log.to_owned(),
        })
    }

    fn view_line(&self) -> String {
        std::fs::read_to_string(&self.log)
            .unwrap()
            .lines()
            .find(|l| l.contains("file view: "))
            .map(|l| {
                let at = l.find("the harness binary at").unwrap_or(0);
                l[at..].to_string()
            })
            .unwrap_or_default()
    }

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
            if started.elapsed() > std::time::Duration::from_secs(40) {
                return format!("NO tool_call_finished IN 40 s; events: {evs:?}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

fn doctor_view(exe: &Path, data: &Path) -> String {
    let out = std::process::Command::new(exe)
        .args(["--data-dir", data.to_str().unwrap(), "doctor"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    out.lines()
        .find(|l| l.contains(" file view: "))
        .map(|l| {
            let at = l.find("the harness binary at").unwrap_or(0);
            l[at..].to_string()
        })
        .unwrap_or_else(|| format!("NO file view LINE: {out}"))
}

// ---- B. a node run from its build tree: what a session can do to the image ----

#[tokio::test(flavor = "multi_thread")]
async fn b_a_build_tree_session_and_the_running_image() {
    let f = fixture();
    let exe = f.binary("ws/target/debug/rizzy-harness");
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    println!("{}", cut(s.view_line()));
    let old = ino(&exe);
    assert_eq!(ino(&f.link()), old);
    // 1: content. The image is the node's running program.
    let r = s
        .run(
            &f.ws,
            "run: ln target/debug/rizzy-harness .keep && echo third-link-made; \
             stat -c 'nlink=%h mode=%a' .keep; \
             echo x >> target/debug/rizzy-harness; echo append=$?; \
             : > .keep; echo truncate=$?; \
             cp /usr/bin/true target/debug/rizzy-harness; echo cp-over=$?; \
             python3 -c \"import os; os.truncate('.keep', 0)\"; echo truncate-syscall=$?",
        )
        .await;
    println!("1 (content, while the node runs): {}", cut(r));
    println!(
        "  on the host: the link's size {} (the binary copied was {})",
        std::fs::metadata(f.link()).unwrap().len(),
        std::fs::metadata(EXE).unwrap().len()
    );
    assert_eq!(
        std::fs::metadata(f.link()).unwrap().len(),
        std::fs::metadata(EXE).unwrap().len()
    );
    // 2: a rebuild.
    let r = s
        .run(
            &f.ws,
            "run: rm target/debug/rizzy-harness && echo next-build > target/debug/rizzy-harness \
             && echo rebuilt; stat -c 'kept: nlink=%h' .keep",
        )
        .await;
    println!("2 (a rebuild): {}", cut(r));
    let r = s.run(&f.ws, "run: echo AFTER-THE-REBUILD").await;
    println!("3 (after the rebuild): {}", cut(r.clone()));
    assert!(r.contains("AFTER-THE-REBUILD\n"));
    // 4: the mode of the shared inode.
    let r = s.run(&f.ws, "run: chmod 000 .keep; echo chmod=$?").await;
    println!(
        "4 (chmod 000 on the session's own link to the image): {}",
        cut(r)
    );
    println!(
        "  on the host: the node's link has mode {:o}",
        mode(&f.link())
    );
    let r = s.run(&f.ws, "run: echo AFTER-CHMOD").await;
    println!("5 (the next command, any session): {}", cut(r));
    let log = std::fs::read_to_string(&s.log).unwrap();
    for l in log
        .lines()
        .filter(|l| l.contains("ermission denied"))
        .take(2)
    {
        println!("  node log: {}", cut(l.to_string()));
    }
    let _ = std::fs::set_permissions(f.link(), std::fs::Permissions::from_mode(0o755));
    let r = s
        .run(&f.ws, "run: echo AFTER-THE-OWNER-RESTORED-THE-MODE")
        .await;
    println!("6 (mode restored on the host): {}", cut(r));
}

// ---- D. a second node on the same data directory, and doctor ----

#[tokio::test(flavor = "multi_thread")]
async fn d_a_second_node_and_doctor() {
    let f = fixture();
    let a = f.binary("bin-a/rizzy-harness");
    let b = f.binary("bin-b/rizzy-harness");
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    let sa = Server::start(&a, &f.data, &f.ws, &f.root.join("a.log"), "127.0.0.1:0").unwrap();
    println!("A: {}", cut(sa.view_line()));
    println!(
        "A, first command: {}",
        cut(sa.run(&f.ws, "run: echo A-RAN").await)
    );
    assert_eq!(ino(&f.link()), ino(&a));

    // doctor, same build and another build, against the running node.
    let before = names(&f.data);
    println!("doctor (A's binary): {}", cut(doctor_view(&a, &f.data)));
    println!("doctor (another build): {}", cut(doctor_view(&b, &f.data)));
    println!(
        "after both doctors: the link is still A's image: {}; new names in the data directory: {:?}",
        ino(&f.link()) == ino(&a),
        names(&f.data)
            .into_iter()
            .filter(|n| !before.contains(n))
            .collect::<Vec<_>>()
    );
    assert_eq!(ino(&f.link()), ino(&a));
    println!(
        "A, after the doctors: {}",
        cut(sa.run(&f.ws, "run: echo A-RAN-2").await)
    );

    // A second start of ANOTHER build that cannot take the port.
    let r = Server::start(&b, &f.data, &f.ws, &f.root.join("b-fail.log"), &sa.addr);
    println!(
        "B started on A's port: {}",
        match &r {
            Ok(_) => "it runs".to_string(),
            Err(e) => cut(e
                .lines()
                .filter(|l| l.contains("rror") || l.contains("ended"))
                .take(3)
                .collect::<Vec<_>>()
                .join(" / ")),
        }
    );
    drop(r);
    println!(
        "after B's failed start the link is: {}",
        if ino(&f.link()) == ino(&a) {
            "A's image"
        } else if ino(&f.link()) == ino(&b) {
            "B's image"
        } else {
            "something else"
        }
    );
    println!(
        "A, after B's failed start: {}",
        cut(sa.run(&f.ws, "run: echo A-RAN-3").await)
    );

    // A second node that does start.
    match Server::start(&b, &f.data, &f.ws, &f.root.join("b.log"), "127.0.0.1:0") {
        Ok(sb) => {
            println!("B: {}", cut(sb.view_line()));
            println!(
                "with B running the link is: {}",
                if ino(&f.link()) == ino(&b) {
                    "B's image"
                } else {
                    "not B's image"
                }
            );
            let ra = sa
                .run(
                    &f.ws,
                    "run: echo A-RAN-4; stat -c inode=%i /.rizzy-harness/rizzy-harness",
                )
                .await;
            println!(
                "A, with B running (A's image is inode {}, B's {}): {}",
                ino(&a),
                ino(&b),
                cut(ra.clone())
            );
            assert!(
                !ra.contains(&format!("inode={}", ino(&b))),
                "A bound B's build"
            );
            let rb = sb
                .run(
                    &f.ws,
                    "run: echo B-RAN; stat -c inode=%i /.rizzy-harness/rizzy-harness",
                )
                .await;
            println!("B: {}", cut(rb));
            drop(sb);
            let ra = sa.run(&f.ws, "run: echo A-RAN-5").await;
            println!("A, after B stopped: {}", cut(ra));
        }
        Err(e) => println!("B did not start: {}", cut(e)),
    }
}

// ---- F. the real fallbacks (run by real_fallbacks.sh) ----

/// `REVIEW_T8F_DATA_PARENT`: a folder on ANOTHER file system than `TMPDIR`
/// (EXDEV); `REVIEW_T8F_FOREIGN_EXE`: a copy of the binary owned by another
/// user, mode 755 (EPERM under protected_hardlinks). One of them.
#[tokio::test(flavor = "multi_thread")]
async fn f_a_real_fallback() {
    let f = fixture();
    let (exe, data, what) = if let Some(p) = std::env::var_os("REVIEW_T8F_DATA_PARENT") {
        let data = PathBuf::from(p).join("data");
        std::fs::create_dir_all(&data).unwrap();
        (
            f.binary("bin/rizzy-harness"),
            data.canonicalize().unwrap(),
            "EXDEV",
        )
    } else if let Some(p) = std::env::var_os("REVIEW_T8F_FOREIGN_EXE") {
        (PathBuf::from(p), f.data.clone(), "EPERM")
    } else {
        println!("SKIPPED: run through real_fallbacks.sh");
        return;
    };
    let m = std::fs::metadata(&exe).unwrap();
    println!(
        "{what}: the binary: owner uid {} (this user is {}), mode {:o}, device {}; the data \
         directory's device {}",
        m.uid(),
        nix::unistd::geteuid(),
        m.mode() & 0o7777,
        m.dev(),
        std::fs::metadata(&data).unwrap().dev()
    );
    let cut = |r: String| {
        r.replace(f.root.to_str().unwrap(), "<root>")
            .replace(data.to_str().unwrap(), "<data>")
            .replace(exe.parent().unwrap().to_str().unwrap(), "<bin>")
    };
    std::fs::write(
        data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n",
            f.ws.display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
    if what == "EXDEV" {
        // The link itself, with the node's own code: the build's binary
        // (on the disk) and this data directory (on the other file system).
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &Settings::default(),
            Some(data.join("sandbox-tmp")),
            std::slice::from_ref(&data),
        )
        .unwrap();
        let l = sb.link_image(&data).clone();
        println!("link_image: {}", cut(format!("{l:?}")));
        assert!(matches!(&l, ImageLink::NotMade(w) if w.contains("another file system")));
        println!(
            "the view line: {}",
            cut(sb
                .view_line()
                .split("the harness binary at")
                .nth(1)
                .unwrap_or("")
                .to_string())
        );
        println!("the data directory holds: {:?}", names(&data));
        if harness_tools::contain::find_bwrap().is_err() {
            // In this private namespace bubblewrap is not root's file (root
            // is not mapped): the node is told to use it anyway.
            let mut t = std::fs::read_to_string(data.join("node.toml")).unwrap();
            t.push_str("\n[sandbox]\nbwrap = \"/usr/bin/bwrap\"\n");
            std::fs::write(data.join("node.toml"), t).unwrap();
            println!("(node.toml names /usr/bin/bwrap: root is not mapped in this namespace)");
        }
    }
    println!("doctor: {}", cut(doctor_view(&exe, &data)));
    println!(
        "after doctor, node-image* in the data directory: {:?}",
        names(&data)
            .into_iter()
            .filter(|n| n.starts_with("node-image"))
            .collect::<Vec<_>>()
    );
    let s = Server::start(&exe, &data, &f.ws, &f.root.join("serve.log"), "127.0.0.1:0").unwrap();
    println!("node log: {}", cut(s.view_line()));
    println!(
        "after the start, node-image* in the data directory: {:?}",
        names(&data)
            .into_iter()
            .filter(|n| n.starts_with("node-image"))
            .collect::<Vec<_>>()
    );
    println!(
        "first command: {}",
        cut(s.run(&f.ws, "run: echo FIRST-RAN").await)
    );
    // Replaced by a rename, as an upgrade does.
    let next = exe.with_extension("next");
    std::fs::copy(EXE, &next).unwrap();
    std::fs::rename(&next, &exe).unwrap();
    let r = s.run(&f.ws, "run: echo RAN-AFTER-THE-UPGRADE").await;
    println!("after the upgrade: {}", cut(r.clone()));
    assert!(!r.contains("RAN-AFTER-THE-UPGRADE\n"));
    assert!(r.contains("restart the node"));
}

// ---- G. a restart of the same binary ----

/// T8F-1. The link is made under a temporary name and renamed over the
/// fixed one. `rename(2)` does nothing when both names are already the same
/// file, which is every restart of a binary that was not rebuilt: the
/// temporary name stays. First with `image::make`, then with a real node
/// restarted three times and then "upgraded".
#[tokio::test(flavor = "multi_thread")]
async fn g_a_restart_leaves_its_temporary_link() {
    use harness_tools::image::make;
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let bin = root.join("bin");
    std::fs::write(&bin, "image").unwrap();
    let fd = held(&bin);
    let r1 = make(fd.as_raw_fd(), &data);
    println!(
        "make, first: {:?}; the data directory: {:?}",
        matches!(r1, ImageLink::Linked(_)),
        names(&data)
    );
    let r2 = make(fd.as_raw_fd(), &data);
    println!(
        "make, again (a restart): {:?}; the data directory: {:?}",
        matches!(r2, ImageLink::Linked(_)),
        names(&data)
    );

    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    let images = |data: &Path| -> Vec<String> {
        names(data)
            .into_iter()
            .filter(|n| n.starts_with("node-image"))
            .map(|n| {
                let m = std::fs::symlink_metadata(data.join(&n)).unwrap();
                format!("{n} (inode {}, {} bytes)", m.ino(), m.len())
            })
            .collect()
    };
    for i in 1..=3 {
        let s = Server::start(
            &exe,
            &f.data,
            &f.ws,
            &f.root.join("serve.log"),
            "127.0.0.1:0",
        )
        .unwrap();
        let r = s.run(&f.ws, "run: echo RAN").await;
        println!(
            "start {i} (pid {}): command: {:?}; node-image*: {:?}",
            s.child.id(),
            cut(r).trim(),
            images(&f.data)
        );
        drop(s);
    }
    // An upgrade: another file at the path; the node started again.
    let old = ino(&exe);
    let next = f.root.join("bin/next");
    std::fs::copy(EXE, &next).unwrap();
    std::fs::rename(&next, &exe).unwrap();
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    println!(
        "after an upgrade (the old build was inode {old}, the new one is {}) and a start: node-image*: {:?}",
        ino(&exe),
        images(&f.data)
    );
    let pinned: u64 = names(&f.data)
        .iter()
        .filter(|n| n.starts_with("node-image.new-"))
        .map(|n| std::fs::symlink_metadata(f.data.join(n)).unwrap())
        .filter(|m| m.ino() == old)
        .map(|m| m.len())
        .max()
        .unwrap_or(0);
    println!(
        "the old build has no name but those left behind: {} bytes kept on disk by them",
        pinned
    );
    drop(s);
}

// ---- H. the fallback, by path, in a build tree: one concurrent command ----

/// Where no link is made the view is T.8c's: the image is bound through the
/// binary's OWN path. With an older bubblewrap (binds by path) nothing looks
/// at what was mounted, and in a build tree that path is in the session's
/// folder. Two contained commands of two sessions of one node: the first
/// waits, then renames its own file over the binary once, at a random
/// moment; the second is started at that time. Counted: for how many pairs
/// the session's file ran as `sandbox-exec` (before Landlock). Then the same
/// with the link made (T.8f), and the fallback with binds by descriptor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h_the_fallback_by_path_in_a_build_tree() {
    let rounds: usize = std::env::var("REVIEW_T8F_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let f = fixture();
    let master = f.binary("master");
    std::fs::File::open(&master).unwrap().sync_all().unwrap();
    let bw = harness_tools::contain::find_bwrap().expect("bubblewrap");
    let log = f.ws.join("ran-as");
    let mut x: u64 = 0x2545F4914F6CDD1D;
    for (binds, linked) in [
        (Binds::Path, false),
        (Binds::Path, true),
        (Binds::Fd, false),
    ] {
        let (mut ran_image, mut refused, mut session_file_ran, mut odd) = (0, 0, 0, Vec::new());
        for i in 0..rounds {
            let tag = format!("{binds:?}-{linked}-{i}");
            let dir = f.ws.join(&tag);
            std::fs::create_dir_all(&dir).unwrap();
            let exe = dir.join("rizzy-harness");
            std::fs::hard_link(&master, &exe).unwrap();
            impostor(&dir.join("imp"), &log);
            let sb = f.plain(&exe);
            if i == 0 {
                sb.probe(&f.data.join("sandbox-tmp/probe"), &f.data)
                    .expect("Landlock");
            }
            let c = Contain::new(bw.clone(), Form::Full).with_binds(binds);
            if i == 0 {
                sb.probe_containment(&c, &f.data.join("sandbox-tmp/probe-contain"))
                    .expect("containment");
            }
            let sb = sb.with_containment(c);
            if linked {
                assert!(matches!(sb.link_image(&f.data), ImageLink::Linked(_)));
            }
            let sb = Arc::new(sb);
            let (c1, c2) = (f.ctx(sb.clone(), "s1"), f.ctx(sb, "s2"));
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let spin = x % 40000;
            // Session 2's command: wait for the mark, spin, replace the binary.
            let script = format!(
                "while [ ! -e {tag}/go ]; do :; done; for ((k=0;k<{spin};k++)); do :; done; \
                 mv -f {tag}/imp {tag}/rizzy-harness; echo swapped"
            );
            let swapper = tokio::spawn(async move { run(&c2, &script).await });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let before = std::fs::read_to_string(&log).unwrap_or_default().len();
            std::fs::write(dir.join("go"), "").unwrap();
            let r = run(&c1, "echo RAN").await;
            let s = swapper.await.unwrap();
            let after = std::fs::read_to_string(&log).unwrap_or_default().len();
            if !s.contains("swapped") {
                odd.push(format!("the swapping command: {s}"));
            } else if after > before {
                session_file_ran += 1;
                if session_file_ran == 1 {
                    println!(
                        "{binds:?}, link {linked}: pair {i}: THE SESSION'S FILE RAN AS \
                         sandbox-exec; the command's output {r:?}; its record {:?}",
                        std::fs::read_to_string(&log)
                            .unwrap_or_default()
                            .lines()
                            .last()
                    );
                }
            } else if r.contains("RAN\n") {
                ran_image += 1;
            } else if r.contains("restart the node") || r.contains("bwrap:") {
                refused += 1;
            } else {
                odd.push(r);
            }
        }
        println!(
            "{binds:?}, link made: {linked}: {rounds} pairs: the command ran from the node's image \
             {ran_image}; refused {refused}; THE SESSION'S FILE RAN AS sandbox-exec \
             {session_file_ran}; other {}",
            odd.len()
        );
        for o in odd.iter().take(2) {
            println!(
                "  other: {:?}",
                o.replace(f.root.to_str().unwrap(), "<root>")
            );
        }
        if linked || binds == Binds::Fd {
            assert_eq!(session_file_ran, 0);
        }
    }
}

// ---- I. a command that runs the harness itself, on a real node ----

/// The session's temp folder is INSIDE the data directory, on the link's
/// file system. A contained command runs the harness binary it sees
/// (`doctor`, with a data directory in its temp folder), tries the node's
/// own way of linking (`ln -L` on the descriptor of a running copy) and
/// looks at what the view tells it about the link.
#[tokio::test(flavor = "multi_thread")]
async fn i_a_command_runs_the_harness_itself() {
    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    let r = s
        .run(
            &f.ws,
            "run: H=/.rizzy-harness/rizzy-harness; mkdir -p \"$TMPDIR/d\"; \
             $H --data-dir \"$TMPDIR/d\" doctor 2>&1 | grep -o -E 'no link node-image could be made[^:]*|kept by a link[^:]*' | head -2; \
             echo \"in its own data directory: $(ls -A \"$TMPDIR/d\" | tr '\\n' ' ')\"; \
             ln \"$H\" \"$TMPDIR/k1\" 2>&1 | sed 's/.*: //'; \
             ( exec 9<\"$H\"; ln -L /proc/self/fd/9 \"$TMPDIR/k2\" 2>&1 | sed 's/.*: //' ); \
             chmod 000 \"$H\" 2>&1 | sed 's/.*: //'; \
             grep -c node-image /proc/self/mountinfo | sed 's/^/mountinfo lines naming node-image: /'; \
             stat -c 'the image in the view: links=%h mode=%a' \"$H\"; \
             echo \"temp folder: $(ls -A \"$TMPDIR\" | tr '\\n' ' ')\"",
        )
        .await;
    println!("{}", cut(r));
    println!(
        "on the host: the link's mode {:o}; node-image* in the data directory: {:?}",
        mode(&f.link()),
        names(&f.data)
            .into_iter()
            .filter(|n| n.starts_with("node-image"))
            .collect::<Vec<_>>()
    );
    assert_eq!(mode(&f.link()), 0o755);
    assert_eq!(std::fs::symlink_metadata(f.link()).unwrap().nlink(), 2);
}
