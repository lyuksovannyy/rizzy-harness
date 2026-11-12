//! T.8f review (2026-11), re-check after the redesign: `sandbox-exec` in a
//! contained command is started through the guard's descriptor of the
//! node's image (no link, no bind, no path). Not part of the test suite:
//! `run_rust_proofs.sh` copies this file to `crates/cli/tests/` for the run
//! and removes the copy again.
//!
//! The proofs of the first design's link (C, E, E9, F's two fallbacks, G,
//! I) are gone with the link; what they showed is in the report. Each test
//! here PRINTS what it observed and says in its comment whether it shows a
//! finding or an attack that is held. Each works on copies of the harness
//! binary and on data directories of its own under `TMPDIR`.

#[allow(dead_code)]
#[path = "support/bwrap.rs"]
mod bwrap;

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_tools::contain::{Binds, Contain, Form};
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
        // Written out: on a slow disk later renames otherwise wait seconds.
        std::fs::File::open(&exe).unwrap().sync_all().unwrap();
        exe
    }

    fn plain(&self, exe: &Path, settings: &Settings) -> Sandbox {
        Sandbox::new(
            exe.to_owned(),
            settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap()
    }

    /// Probed as the node probes it, with the real bubblewrap.
    fn contained(&self, exe: &Path, form: Form, binds: Binds) -> Sandbox {
        let sb = self.plain(exe, &Settings::default());
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
            tmp: None,
        }
        .for_session(session);
        ctx
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

fn ctx_sandbox(ctx: &ToolCtx) -> Arc<Sandbox> {
    match &ctx.sandbox {
        Confinement::Confined { sandbox, .. } => sandbox.clone(),
        _ => panic!("not confined"),
    }
}

// ---- A. what a command holds, and what of the image it can reach ----

/// What a contained command does to find and change the image: `@ID@` is
/// the image's device:inode, `@BIN@` its path on the host.
const PROBE_PY: &str = r#"
import os, sys, errno
ID = "@ID@"; BIN = "@BIN@"
def ident(p):
    st = os.stat(p)
    return f"{st.st_dev}:{st.st_ino}"
own = []
for n in sorted(int(x) for x in os.listdir('/proc/self/fd')):
    try:
        own.append(f"{n}:{'IMAGE' if ident(f'/proc/self/fd/{n}') == ID else os.readlink(f'/proc/self/fd/{n}').split('/')[-1][:12]}")
    except OSError:
        pass
print("descriptors this command holds (python's own included):", " ".join(own))
print("a path of the harness in the view:", [p for p in ('/.rizzy-harness', BIN) if os.path.lexists(p)] or "none")
pids = sorted(int(p) for p in os.listdir('/proc') if p.isdigit())
me = os.getpid()
seen = {"exe readable": 0, "exe IS the image": 0, "a descriptor IS the image": 0, "root readable": 0}
hits = []
for pid in pids:
    if pid == me:
        continue
    for what, p in (("exe", f"/proc/{pid}/exe"),):
        try:
            i = ident(p)
            seen["exe readable"] += 1
            if i == ID:
                seen["exe IS the image"] += 1
                hits.append(p)
        except OSError:
            pass
    try:
        for n in os.listdir(f"/proc/{pid}/fd"):
            try:
                if ident(f"/proc/{pid}/fd/{n}") == ID:
                    seen["a descriptor IS the image"] += 1
                    hits.append(f"/proc/{pid}/fd/{n}")
            except OSError:
                pass
    except OSError:
        pass
    try:
        if ident(f"/proc/{pid}/root{BIN}") == ID:
            seen["root readable"] += 1
            hits.append(f"/proc/{pid}/root{BIN}")
    except OSError:
        pass
print(f"other processes in /proc: {len(pids) - 1}; " + "; ".join(f"{k}: {v}" for k, v in seen.items()))
done = 0
for h in hits:
    try:
        os.chmod(h, 0)
        done += 1
        print(f"chmod 000 through {h.replace(BIN, '<bin>')}: DONE")
        break
    except OSError as e:
        print(f"chmod 000 through {h.replace(BIN, '<bin>')}: refused ({errno.errorcode.get(e.errno)})")
if not hits:
    print("no way to the image found")
"#;

/// HELD (T8F-2 is closed). The binary is outside every grant. In both forms
/// and both bind modes the command holds 0, 1, 2 only, no path of the
/// harness is in the view, and no process it can see gives it the image
/// (its own `/proc`: bubblewrap's init only; the machine's `/proc`:
/// Landlock refuses every look at a process outside the sandbox). The
/// binary's mode on the host is unchanged and the next commands run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_what_a_command_holds_and_can_reach() {
    for (form, binds) in [
        (Form::Full, Binds::Fd),
        (Form::Full, Binds::Path),
        (Form::SharedProc, Binds::Fd),
        (Form::SharedProc, Binds::Path),
    ] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        let m = std::fs::metadata(&exe).unwrap();
        std::fs::write(
            f.ws.join("probe.py"),
            PROBE_PY
                .replace("@ID@", &format!("{}:{}", m.dev(), m.ino()))
                .replace("@BIN@", exe.to_str().unwrap()),
        )
        .unwrap();
        let sb = f.contained(&exe, form, binds);
        println!("==== {form:?}, {binds:?} ====");
        let ctx = f.ctx(Arc::new(sb), "s1");
        let r = run(&ctx, "python3 probe.py").await;
        println!("{}", r.replace(f.root.to_str().unwrap(), "<root>").trim());
        println!(
            "on the host: the binary's mode before 755, after {:o}",
            mode(&exe)
        );
        assert_eq!(mode(&exe), 0o755, "{form:?} {binds:?}: the mode changed");
        assert!(!r.contains("IMAGE"), "{form:?} {binds:?}: {r}");
        let r2 = run(&ctx, "echo NEXT-COMMAND-RAN").await;
        let r3 = run(&f.ctx(ctx_sandbox(&ctx), "s2"), "echo ANOTHER-SESSION-RAN").await;
        println!(
            "the next command: {:?}; another session's: {:?}",
            r2.trim(),
            r3.trim()
        );
        assert!(r2.contains("NEXT-COMMAND-RAN\n") && r3.contains("ANOTHER-SESSION-RAN\n"));
    }
}

// ---- J. between the guard's open and bubblewrap's exec ----

/// A stand-in for bubblewrap that records what the guard hands it and then
/// runs the real one. `@MODE@` is a file naming what else it does:
/// `inside`: run a lister as the program in place of `sandbox-exec`;
/// `chmod`: set the image's mode to 000 through the program's descriptor
/// first; `close`: close the program's descriptor first.
const FAKE_BWRAP: &str = r#"#!/usr/bin/python3
import os, sys, stat
LOG = "@LOG@"; MODE = "@MODE@"; REAL = "@REAL@"
a = sys.argv[1:]
if "--" not in a:
    os.execv(REAL, ["bwrap"] + a)
mode = open(MODE).read().strip() if os.path.exists(MODE) else "log"
i = a.index("--")
prog = a[i + 1]
def ident(p):
    try:
        st = os.stat(p)
        return f"{st.st_dev}:{st.st_ino}"
    except OSError as e:
        return f"({e.strerror})"
lines = [f"program word: {prog} = {ident(prog)}",
         f"the guard (bubblewrap's parent) runs: {ident(f'/proc/{os.getppid()}/exe')}"]
try:
    node = int(open(f"/proc/{os.getppid()}/stat").read().rsplit(")", 1)[1].split()[1])
    lines.append(f"the node (the guard's parent) runs: {ident(f'/proc/{node}/exe')}")
except Exception as e:
    lines.append(f"the node: {e}")
inh = []
for n in sorted(int(x) for x in os.listdir("/proc/self/fd")):
    try:
        if not os.get_inheritable(n):
            continue
        st = os.fstat(n)
    except OSError:
        continue
    kind = ("socket" if stat.S_ISSOCK(st.st_mode) else "pipe" if stat.S_ISFIFO(st.st_mode)
            else "dir" if stat.S_ISDIR(st.st_mode) else "chr" if stat.S_ISCHR(st.st_mode) else "file")
    flags = int(open(f"/proc/self/fdinfo/{n}").read().split("flags:")[1].split()[0], 8)
    inh.append(f"{n}={kind}{'(O_PATH)' if flags & 0o10000000 else ''}")
lines.append("bubblewrap inherits: " + " ".join(inh))
lines.append("@rizzy words left in the options: " + str([w for w in a[:i] if w.startswith("@rizzy")]))
with open(LOG, "a") as f:
    f.write("\n".join(lines) + "\n")
INSIDE = '''
import os, sys
p = sys.argv[1]
out = []
for n in sorted(int(x) for x in os.listdir("/proc/self/fd")):
    try:
        st = os.stat(f"/proc/self/fd/{n}")
        out.append(f"{n}={st.st_dev}:{st.st_ino}")
    except OSError:
        pass
print("at the program's start the process holds:", " ".join(out))
try:
    st = os.stat(p)
    print(f"the program word {p} names: {st.st_dev}:{st.st_ino}")
except OSError as e:
    print(f"the program word {p}: {e.strerror}")
'''
if mode == "inside":
    a = a[: i + 1] + ["/usr/bin/python3", "-c", INSIDE, prog]
elif mode == "chmod":
    os.chmod(prog, 0)
elif mode == "close":
    os.close(int(prog.rsplit("/", 1)[1]))
os.execv(REAL, ["bwrap"] + a)
"#;

fn fake_bwrap(f: &Fixture) -> (PathBuf, PathBuf, PathBuf) {
    let real = harness_tools::contain::find_bwrap().expect("bubblewrap");
    let (at, log, mode) = (
        f.root.join("fake-bwrap"),
        f.root.join("fake-bwrap.log"),
        f.root.join("fake-bwrap.mode"),
    );
    std::fs::write(
        &at,
        FAKE_BWRAP
            .replace("@LOG@", log.to_str().unwrap())
            .replace("@MODE@", mode.to_str().unwrap())
            .replace("@REAL@", real.to_str().unwrap()),
    )
    .unwrap();
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755)).unwrap();
    (at, log, mode)
}

/// HELD. With two extra grants (so the guard opens descriptors before the
/// image's): in both forms and both bind modes the program word is
/// `/proc/self/fd/<n>`, `<n>` is the guard's own image, bubblewrap inherits
/// it and the grants' descriptors and NOT the node's channel (no socket),
/// and at the program's start inside the namespace `<n>` is still the
/// image and is the only descriptor above 2: bubblewrap closed the grants'.
/// With the descriptor closed before bubblewrap starts, the command does
/// not run and nothing is looked up by a path. With the image's mode set to
/// 000 between the guard's open and the exec, the command does not run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn j_between_the_guards_open_and_the_exec() {
    for (form, binds) in [
        (Form::Full, Binds::Fd),
        (Form::Full, Binds::Path),
        (Form::SharedProc, Binds::Fd),
        (Form::SharedProc, Binds::Path),
    ] {
        let f = fixture();
        let exe = f.binary("bin/rizzy-harness");
        let (fake, log, mode_file) = fake_bwrap(&f);
        let extras = [f.root.join("extra-a"), f.root.join("extra-b")];
        for e in &extras {
            std::fs::create_dir_all(e).unwrap();
        }
        let sb = f
            .plain(
                &exe,
                &Settings {
                    write: extras.to_vec(),
                    ..Default::default()
                },
            )
            .with_containment(Contain::new(fake, form).with_binds(binds));
        let ctx = f.ctx(Arc::new(sb), "s1");
        let m = std::fs::metadata(&exe).unwrap();
        let id = format!("{}:{}", m.dev(), m.ino());
        println!("==== {form:?}, {binds:?}; the image is {id} ====");
        let cut = |s: String| s.replace(f.root.to_str().unwrap(), "<root>");

        let r = run(&ctx, "echo RAN-THROUGH-THE-STAND-IN").await;
        let l = std::fs::read_to_string(&log).unwrap_or_default();
        println!("{}", cut(l.clone()).trim());
        println!("the command: {:?}", r.trim());
        assert!(r.contains("RAN-THROUGH-THE-STAND-IN\n"), "{r}");
        assert!(
            l.contains(&format!("program word: /proc/self/fd/")) && l.contains(&format!("= {id}"))
        );
        assert!(l.contains(&format!("the guard (bubblewrap's parent) runs: {id}")));
        assert!(!l.contains("socket"), "bubblewrap inherits a socket: {l}");

        std::fs::write(&mode_file, "inside").unwrap();
        let r = run(&ctx, "true").await;
        println!("{}", cut(r.clone()).trim());
        let held = r
            .lines()
            .find(|l| l.starts_with("at the program's start"))
            .unwrap_or_default()
            .to_string();
        let above_2: Vec<&str> = held
            .split_whitespace()
            .filter(|w| {
                w.split_once('=')
                    .and_then(|(n, _)| n.parse::<u32>().ok())
                    .is_some_and(|n| n > 2)
            })
            .collect();
        // The lister's own listing descriptor may be among them (a folder
        // of /proc); the image must be, and no grant's folder.
        assert!(
            above_2.iter().any(|w| w.ends_with(&format!("={id}"))),
            "{held}"
        );
        for e in &extras {
            let em = std::fs::metadata(e).unwrap();
            assert!(
                !held.contains(&format!("={}:{}", em.dev(), em.ino())),
                "a grant's descriptor reached the program: {held}"
            );
        }
        assert!(r.contains(&format!("names: {id}")), "{r}");

        std::fs::write(&mode_file, "close").unwrap();
        let r = run(&ctx, "echo RAN-WITHOUT-THE-DESCRIPTOR").await;
        println!("the descriptor closed first: {:?}", cut(r.clone()).trim());
        assert!(!r.contains("RAN-WITHOUT-THE-DESCRIPTOR\n"), "{r}");

        std::fs::write(&mode_file, "chmod").unwrap();
        let r = run(&ctx, "echo RAN-WITH-MODE-000").await;
        println!(
            "mode 000 between the guard's open and the exec: {:?}; the binary's mode now {:o}",
            cut(r.clone()).trim(),
            mode(&exe)
        );
        assert!(!r.contains("RAN-WITH-MODE-000\n"), "{r}");
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&mode_file, "log").unwrap();
        let r = run(&ctx, "echo RAN-AGAIN").await;
        println!("the mode restored: {:?}", r.trim());
        assert!(r.contains("RAN-AGAIN\n"), "{r}");
    }
}

// ---- H. a session against the binary's path while a command starts ----

/// HELD for a replacement and a deletion (T8F-3b is closed): by path and by
/// descriptor, in a build tree, two contained commands of two sessions; the
/// first waits, spins a random time and then replaces the binary with its
/// own file, or deletes it; the second is `echo RAN`. The session's file
/// never runs as `sandbox-exec` and the command always runs, also the next
/// one. OPEN BY DECISION for the mode (T8F-5): `chmod 000` at that moment,
/// or before, stops the command; the messages are printed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h_a_session_against_the_binary_while_a_command_starts() {
    let rounds: usize = std::env::var("REVIEW_T8F_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let f = fixture();
    let master = f.binary("master");
    let bw = harness_tools::contain::find_bwrap().expect("bubblewrap");
    let log = f.ws.join("ran-as");
    let mut x: u64 = 0x2545F4914F6CDD1D;
    for (binds, act, n) in [
        (Binds::Path, "replace", rounds),
        (Binds::Fd, "replace", rounds / 3),
        (Binds::Path, "delete", rounds / 3),
        (Binds::Path, "chmod", rounds / 3),
        (Binds::Fd, "chmod", rounds / 6),
    ] {
        let (mut ran, mut next_ran, mut session_file_ran) = (0, 0, 0);
        let mut failed = std::collections::BTreeMap::<String, usize>::new();
        for i in 0..n {
            let tag = format!("{binds:?}-{act}-{i}");
            let dir = f.ws.join(&tag);
            std::fs::create_dir_all(&dir).unwrap();
            let exe = dir.join("rizzy-harness");
            std::fs::hard_link(&master, &exe).unwrap();
            impostor(&dir.join("imp"), &log);
            let sb = f.plain(&exe, &Settings::default());
            if i == 0 {
                sb.probe(&f.data.join("sandbox-tmp/probe"), &f.data)
                    .expect("Landlock");
            }
            let c = Contain::new(bw.clone(), Form::Full).with_binds(binds);
            if i == 0 {
                sb.probe_containment(&c, &f.data.join("sandbox-tmp/probe-contain"))
                    .expect("containment");
            }
            let sb = Arc::new(sb.with_containment(c));
            let (c1, c2) = (f.ctx(sb.clone(), "s1"), f.ctx(sb, "s2"));
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let spin = x % 40000;
            let what = match act {
                "replace" => format!("mv -f {tag}/imp {tag}/rizzy-harness"),
                "delete" => format!("rm -f {tag}/rizzy-harness"),
                _ => format!("chmod 000 {tag}/rizzy-harness"),
            };
            let script = format!(
                "while [ ! -e {tag}/go ]; do :; done; for ((k=0;k<{spin};k++)); do :; done; \
                 {what}; echo acted"
            );
            let other = tokio::spawn(async move { run(&c2, &script).await });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            let before = std::fs::read_to_string(&log).unwrap_or_default().len();
            std::fs::write(dir.join("go"), "").unwrap();
            let r = run(&c1, "echo RAN").await;
            let s = other.await.unwrap();
            assert!(s.contains("acted"), "{s}");
            // The command after it, when everything is over.
            let r2 = run(&c1, "echo RAN-NEXT").await;
            let after = std::fs::read_to_string(&log).unwrap_or_default().len();
            if act == "chmod" {
                // One inode for every pair: the owner restores the mode.
                std::fs::set_permissions(&master, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            if after > before {
                session_file_ran += 1;
            }
            if r.contains("RAN\n") {
                ran += 1;
            } else {
                *failed
                    .entry(format!(
                        "the command: {}",
                        r.replace(f.root.to_str().unwrap(), "<root>")
                            .trim()
                            .replace('\n', " | ")
                    ))
                    .or_default() += 1;
            }
            if r2.contains("RAN-NEXT\n") {
                next_ran += 1;
            } else {
                *failed
                    .entry(format!(
                        "the next command: {}",
                        r2.replace(f.root.to_str().unwrap(), "<root>")
                            .trim()
                            .replace('\n', " | ")
                    ))
                    .or_default() += 1;
            }
        }
        println!(
            "{binds:?}, {act} at a random moment: {n} pairs: the command ran {ran}; the next \
             command ran {next_ran}; THE SESSION'S FILE RAN AS sandbox-exec {session_file_ran}"
        );
        for (k, v) in &failed {
            // The descriptor's number differs between runs.
            println!("  {v:4} x {k}");
        }
        assert_eq!(session_file_ran, 0);
        if act != "chmod" {
            assert_eq!((ran, next_ran), (n, n), "{binds:?} {act}");
        }
    }
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

fn node_files(data: &Path, ws: &Path, extra: &str) {
    std::fs::write(
        data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n{extra}",
            ws.display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
}

impl Server {
    fn start(exe: &Path, data: &Path, ws: &Path, log: &Path, bind: &str) -> Result<Server, String> {
        use std::io::BufRead;
        if !data.join("node.toml").exists() {
            node_files(data, ws, "");
        }
        let mut child = std::process::Command::new(exe)
            .args(["--data-dir", data.to_str().unwrap()])
            .args(["serve", "--bind", bind])
            .env("HARNESS_LOG", "info")
            // The node's own working directory: the fixture, never the
            // source tree (see `l_a_binary_the_node_cannot_read`).
            .current_dir(data.parent().unwrap())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::fs::File::create(log).unwrap())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
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

    fn log_lines(&self, word: &str) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap()
            .lines()
            .filter(|l| l.contains(word))
            .map(str::to_string)
            .collect()
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

// ---- B. a node run from its build tree ----

/// HELD for the content, a rebuild and a deletion; OPEN BY DECISION for the
/// mode (T8F-5). A real `serve` from `ws/target/debug`: a session cannot
/// change the image's content through any name while the node runs; after
/// a rebuild and after the binary is deleted outright commands still run;
/// `chmod 000` on a name the session kept stops every command, and the
/// node cannot be started again until the mode is restored. The messages
/// the agent and the owner get are printed.
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
    for l in s.log_lines("WARNING: a session can replace") {
        let at = l.find("and a session that can write there").unwrap_or(0);
        println!(
            "start-up warning, its last part: …{}",
            cut(l[at..].to_string())
        );
    }
    let r = s
        .run(
            &f.ws,
            "run: ln target/debug/rizzy-harness .keep && echo third-name-made; \
             echo x >> target/debug/rizzy-harness; echo append=$?; \
             : > .keep; echo truncate=$?; \
             cp /usr/bin/true target/debug/rizzy-harness; echo cp-over=$?",
        )
        .await;
    println!("1 (content, while the node runs): {}", cut(r));
    assert_eq!(
        std::fs::metadata(f.ws.join(".keep")).unwrap().len(),
        std::fs::metadata(EXE).unwrap().len()
    );
    let r = s
        .run(
            &f.ws,
            "run: rm target/debug/rizzy-harness && echo next-build > target/debug/rizzy-harness \
             && echo rebuilt",
        )
        .await;
    println!("2 (a rebuild): {}", cut(r));
    let r = s.run(&f.ws, "run: echo AFTER-THE-REBUILD").await;
    println!("3 (after the rebuild): {}", cut(r.clone()));
    assert!(r.contains("AFTER-THE-REBUILD\n"));
    let r = s
        .run(
            &f.ws,
            "run: rm -f target/debug/rizzy-harness .keep.none; echo deleted",
        )
        .await;
    println!("4 (the binary deleted, nothing at its path): {}", cut(r));
    let r = s.run(&f.ws, "run: echo AFTER-THE-DELETION").await;
    println!("5 (after the deletion): {}", cut(r.clone()));
    assert!(r.contains("AFTER-THE-DELETION\n"));
    // The node's helpers start sandbox-exec by the path (unchanged, the
    // documented residual): what the node's log says while commands run on.
    let failed = s.log_lines("checkpoint failed");
    println!(
        "  while commands ran on, the node logged {} failed checkpoints, the first: {}; the last: {}",
        failed.len(),
        failed
            .first()
            .map(|l| l.rsplit("error=").next().unwrap_or(""))
            .unwrap_or("none"),
        failed
            .last()
            .map(|l| l.rsplit("error=").next().unwrap_or(""))
            .unwrap_or("none"),
    );
    println!(
        "  the data directory holds nothing named node-image*: {}",
        !names(&f.data).iter().any(|n| n.starts_with("node-image"))
    );
    let r = s.run(&f.ws, "run: chmod 000 .keep; echo chmod=$?").await;
    println!(
        "6 (chmod 000 on the session's own name of the image): {}",
        cut(r)
    );
    let r = s.run(&f.ws, "run: echo AFTER-CHMOD").await;
    println!("7 (the next command, any session): {}", cut(r.clone()));
    assert!(!r.contains("AFTER-CHMOD\n"));
    let tail: Vec<String> = s
        .log_lines("ermission denied")
        .into_iter()
        .rev()
        .take(3)
        .collect();
    for l in tail.iter().rev() {
        println!("  node log: {}", cut(l.clone()));
    }
    // The owner restarts the node: the image's file (now only .keep).
    std::fs::rename(f.ws.join(".keep"), &exe).unwrap();
    drop(s);
    let again = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve2.log"),
        "127.0.0.1:0",
    );
    println!(
        "8 (the node stopped and started again, the image still mode {:o}): {}",
        mode(&exe),
        match &again {
            Ok(_) => "it runs".to_string(),
            Err(e) => cut(e.clone()),
        }
    );
    drop(again);
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve3.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let r = s
        .run(&f.ws, "run: echo AFTER-THE-OWNER-RESTORED-THE-MODE")
        .await;
    println!("9 (mode restored, node started): {}", cut(r));
}

// ---- D. a second start, doctor ----

/// HELD (T8F-4 is closed; T8F-1, T8F-6 no longer apply). Two builds on one
/// data directory: a second start that fails on the port, `doctor` of
/// either build, and a second node that does start leave the first node's
/// commands running; nothing named `node-image*` is made.
#[tokio::test(flavor = "multi_thread")]
async fn d_a_second_node_and_doctor() {
    let f = fixture();
    let a = f.binary("bin-a/rizzy-harness");
    let b = f.binary("bin-b/rizzy-harness");
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    let sa = Server::start(&a, &f.data, &f.ws, &f.root.join("a.log"), "127.0.0.1:0").unwrap();
    for l in sa.log_lines("file view: ") {
        let at = l.find("Its sandbox-exec").unwrap_or(0);
        println!("A's file view line, its last part: {}", &l[at..]);
    }
    println!(
        "A, first command: {:?}",
        sa.run(&f.ws, "run: echo A-RAN").await.trim()
    );
    for exe in [&a, &b] {
        let out = std::process::Command::new(exe)
            .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout).into_owned();
        println!(
            "doctor ({}): {} lines, a file view line: {}",
            if exe == &a {
                "A's build"
            } else {
                "another build"
            },
            out.lines().count(),
            out.lines().any(|l| l.contains(" file view: "))
        );
    }
    let r = Server::start(&b, &f.data, &f.ws, &f.root.join("b-fail.log"), &sa.addr);
    println!(
        "B (another build) started on A's port: {}",
        match &r {
            Ok(_) => "it runs".to_string(),
            Err(e) => cut(e
                .lines()
                .filter(|l| l.contains("rror") || l.contains("in use"))
                .take(2)
                .collect::<Vec<_>>()
                .join(" / ")),
        }
    );
    drop(r);
    let r = sa.run(&f.ws, "run: echo A-RAN-AFTER-B-FAILED").await;
    println!("A, after B's failed start: {:?}", r.trim());
    assert!(r.contains("A-RAN-AFTER-B-FAILED\n"));
    match Server::start(&b, &f.data, &f.ws, &f.root.join("b.log"), "127.0.0.1:0") {
        Ok(sb) => {
            let ra = sa.run(&f.ws, "run: echo A-RAN-WITH-B").await;
            let rb = sb.run(&f.ws, "run: echo B-RAN").await;
            println!("with B running: A: {:?}; B: {:?}", ra.trim(), rb.trim());
            assert!(ra.contains("A-RAN-WITH-B\n") && rb.contains("B-RAN\n"));
        }
        Err(e) => println!("B did not start: {}", cut(e)),
    }
    println!(
        "node-image* in the data directory: {:?}",
        names(&f.data)
            .into_iter()
            .filter(|n| n.starts_with("node-image"))
            .collect::<Vec<_>>()
    );
}

// ---- K. the node, its guard and sandbox-exec are one inode ----

/// HELD. A real `serve` whose node.toml names the stand-in bubblewrap of
/// `J` (it records and runs the real one): the node, the guard and the
/// program bubblewrap is given are one device and inode, before and after
/// the binary at the node's path is replaced by another build, and that
/// inode is then not the file at the path.
#[tokio::test(flavor = "multi_thread")]
async fn k_the_node_its_guard_and_the_wrapper_are_one_inode() {
    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    let (fake, log, _) = fake_bwrap(&f);
    node_files(
        &f.data,
        &f.ws,
        &format!("\n[sandbox]\nbwrap = {:?}\n", fake.display().to_string()),
    );
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let m = std::fs::metadata(&exe).unwrap();
    let id = format!("{}:{}", m.dev(), m.ino());
    let last = |log: &Path| -> String {
        let l = std::fs::read_to_string(log).unwrap_or_default();
        let v: Vec<&str> = l.lines().collect();
        v.iter()
            .rev()
            .take(5)
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let r = s.run(&f.ws, "run: echo FIRST").await;
    println!(
        "the image the node was started from: {id}\n{}\nthe command: {:?}",
        last(&log),
        r.trim()
    );
    let l = last(&log);
    assert_eq!(l.matches(&id).count(), 3, "{l}");
    // An upgrade: another build renamed over the path.
    let next = f.root.join("bin/next");
    std::fs::copy(EXE, &next).unwrap();
    std::fs::rename(&next, &exe).unwrap();
    let m2 = std::fs::metadata(&exe).unwrap();
    let r = s.run(&f.ws, "run: echo AFTER-THE-UPGRADE").await;
    println!(
        "the file now at the node's path: {}:{}\n{}\nthe command: {:?}",
        m2.dev(),
        m2.ino(),
        last(&log),
        r.trim()
    );
    let l = last(&log);
    assert_eq!(l.matches(&id).count(), 3, "{l}");
    assert!(r.contains("AFTER-THE-UPGRADE\n"));
}

// ---- F. a binary this user does not own (run by foreign_owner.sh) ----

/// HELD (the first design's `EPERM` fallback no longer exists).
/// `REVIEW_T8F_FOREIGN_EXE`: a copy of the binary owned by another user,
/// mode 755, as an installed root-owned binary is; inside bubblewrap's user
/// namespace its owner is not mapped. A real `serve` from it: a command
/// runs, the binary is replaced by a rename as an upgrade does, and the
/// next command runs too.
#[tokio::test(flavor = "multi_thread")]
async fn f_a_binary_this_user_does_not_own() {
    let Some(p) = std::env::var_os("REVIEW_T8F_FOREIGN_EXE") else {
        println!("SKIPPED: run through foreign_owner.sh");
        return;
    };
    let f = fixture();
    let exe = PathBuf::from(p);
    let m = std::fs::metadata(&exe).unwrap();
    println!(
        "the binary: owner uid {} (this user is {}), mode {:o}",
        m.uid(),
        nix::unistd::geteuid(),
        m.mode() & 0o7777
    );
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let r = s.run(&f.ws, "run: echo FIRST-RAN; echo pid=$$").await;
    println!("first command: {:?}", r.trim());
    assert!(r.contains("FIRST-RAN\n") && r.contains("pid=2\n"), "{r}");
    let next = exe.with_extension("next");
    std::fs::copy(EXE, &next).unwrap();
    std::fs::rename(&next, &exe).unwrap();
    let r = s.run(&f.ws, "run: echo RAN-AFTER-THE-UPGRADE").await;
    println!(
        "after the upgrade (the old file has no name left): {:?}",
        r.trim()
    );
    assert!(r.contains("RAN-AFTER-THE-UPGRADE\n"), "{r}");
    println!(
        "the image's inode was {}; the path now holds {}",
        m.ino(),
        ino(&exe)
    );
}

// ---- L. a binary the node can execute but not read ----

/// FINDING R-1. The node opens its own image for READING (`open_image`,
/// `File::open("/proc/self/exe")`); where that fails (a binary with mode
/// 711 or 311: executable, not readable) it holds no descriptor, says
/// nothing, and starts every guard from the binary's PATH. The guard's own
/// image, and so `sandbox-exec`, is then whatever file is at that path when
/// the command starts. Shown with the stand-in bubblewrap of `J` on a real
/// `serve`: before an upgrade all three are one inode; after it the node is
/// the old build and its guard and `sandbox-exec` are the new file; and a
/// script put at the path runs as the GUARD (outside bubblewrap and
/// Landlock, with the node's channel on its stdin).
#[tokio::test(flavor = "multi_thread")]
async fn l_a_binary_the_node_cannot_read() {
    let f = fixture();
    let exe = f.binary("bin/rizzy-harness");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o311)).unwrap();
    if std::fs::File::open(&exe).is_ok() {
        println!("SKIPPED: this user reads a mode 311 file (root)");
        return;
    }
    let (fake, log, _) = fake_bwrap(&f);
    node_files(
        &f.data,
        &f.ws,
        &format!("\n[sandbox]\nbwrap = {:?}\n", fake.display().to_string()),
    );
    let s = Server::start(
        &exe,
        &f.data,
        &f.ws,
        &f.root.join("serve.log"),
        "127.0.0.1:0",
    )
    .unwrap();
    let cut = |r: String| r.replace(f.root.to_str().unwrap(), "<root>");
    let m = std::fs::metadata(&exe).unwrap();
    let last = |log: &Path| -> String {
        let l = std::fs::read_to_string(log).unwrap_or_default();
        let v: Vec<&str> = l.lines().collect();
        v.iter()
            .rev()
            .take(5)
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let r = s.run(&f.ws, "run: echo FIRST").await;
    println!(
        "the node's image: {}:{}, mode {:o}\n{}\nthe command: {:?}",
        m.dev(),
        m.ino(),
        m.mode() & 0o7777,
        last(&log),
        r.trim()
    );
    println!(
        "lines of the node's log that name the image or a descriptor of it: {:?}",
        s.log_lines("own image").len() + s.log_lines("descriptor of").len()
    );
    // An upgrade: another build renamed over the path.
    let next = f.root.join("bin/next");
    std::fs::copy(EXE, &next).unwrap();
    std::fs::rename(&next, &exe).unwrap();
    let m2 = std::fs::metadata(&exe).unwrap();
    let r = s.run(&f.ws, "run: echo AFTER-THE-UPGRADE").await;
    println!(
        "the file now at the node's path: {}:{}\n{}\nthe command: {:?}",
        m2.dev(),
        m2.ino(),
        last(&log),
        r.trim()
    );
    // A script at the path.
    let rec = f.root.join("ran-as");
    let imp = f.root.join("bin/imp");
    std::fs::write(
        &imp,
        format!(
            "#!/bin/sh\necho \"ran as $1, pid $$, $(ls /proc | grep -c '^[0-9]') processes in \
             /proc, stdin $(readlink /proc/$$/fd/0)\" >> '{}'\n",
            rec.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&imp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&imp, &exe).unwrap();
    let r = s.run(&f.ws, "run: echo AFTER-A-SCRIPT-AT-THE-PATH").await;
    let record = std::fs::read_to_string(&rec).unwrap_or_default();
    println!(
        "a script at the node's path: the command: {:?}\nthe script's record as the GUARD: {:?}\n(and {} \
         records as sandbox-exec: the node's helpers, which start it by path, the documented residual)",
        cut(r).trim(),
        record
            .lines()
            .find(|l| l.contains("ran as sandbox-guard"))
            .unwrap_or("none"),
        record
            .lines()
            .filter(|l| l.contains("ran as sandbox-exec"))
            .count()
    );
    // An aside, part of that residual: the script prints nothing, so the
    // node's `git rev-parse --git-common-dir` "answers" with an empty path
    // and the checkpoint order file is written into the node's own working
    // directory (`order_file` in crates/node/src/git.rs joins onto it).
    println!(
        "in the node's working directory afterwards: harness-checkpoints exists: {}",
        f.data
            .parent()
            .unwrap()
            .join("harness-checkpoints")
            .exists()
    );
}
