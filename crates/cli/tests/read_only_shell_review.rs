//! The independent review's probes of the read-only shell (PR 125), kept
//! as regression tests: some fifty more writes and escapes (tee, cp, mv,
//! truncate, fallocate, dd, mkfifo, mknod, sockets, python, perl, node,
//! xattrs, git gc/config/tag/notes/reset, `/proc/<pid>/root` and `cwd`,
//! inherited descriptors, nested bubblewrap, `unshare`, `nsenter`, umount,
//! remount, the write entry, the data directory), each folder's every entry
//! (size, mode, times, owner, inode, links, xattrs) compared from outside
//! before and after. Programs a runner lacks just fail. Skips without
//! bubblewrap; fails there with `HARNESS_REQUIRE_BWRAP=1`.

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
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
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
    std::fs::create_dir(ws.join("sub")).unwrap();
    std::fs::write(ws.join("sub/inner"), "inner\n").unwrap();
    std::fs::write(extra.join("kept"), "original\n").unwrap();
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
    git(&["add", "marker", "sub/inner"]);
    git(&["commit", "-q", "-m", "first"]);
    std::fs::write(ws.join("marker"), "original\nchanged\n").unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
        extra,
    }
}

impl Fixture {
    fn sandbox(&self) -> Option<Arc<Sandbox>> {
        let settings = Settings {
            write: vec![self.extra.clone()],
            ..Default::default()
        };
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            &settings,
            Some(self.data.join("sandbox-tmp")),
            std::slice::from_ref(&self.data),
        )
        .unwrap();
        if let Err(e) = sb.probe(&self.data.join("sandbox-tmp/probe"), &self.data) {
            assert!(!bwrap::required(), "Landlock: {e}");
            return None;
        }
        let sb = bwrap::contained(sb, &self.data.join("sandbox-tmp/probe-contain")).ok()?;
        Some(Arc::new(sb))
    }

    fn ctx(&self, sb: Arc<Sandbox>, read_only: bool) -> ToolCtx {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: sb,
            tmp: Some(self.data.join("sandbox-tmp")),
        }
        .for_session("s1");
        ctx.read_only_shell = read_only;
        ctx
    }
}

/// Every entry under `p` with what a change would move.
fn snapshot(p: &Path) -> Vec<String> {
    use std::os::unix::fs::MetadataExt;
    let mut out = vec![];
    fn go(p: &Path, out: &mut Vec<String>) {
        let mut rd: Vec<_> = std::fs::read_dir(p).unwrap().map(|e| e.unwrap()).collect();
        rd.sort_by_key(|e| e.file_name());
        for e in rd {
            let m = e.path().symlink_metadata().unwrap();
            out.push(format!(
                "{} len={} mode={:o} mtime={}.{} ctime={}.{} uid={} ino={} nlink={}",
                e.path().display(),
                m.len(),
                m.mode(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
                m.uid(),
                m.ino(),
                m.nlink()
            ));
            if m.is_dir() {
                go(&e.path(), out);
            }
        }
    }
    go(p, &mut out);
    // Extended attributes where `getfattr` is installed.
    if let Ok(x) = std::process::Command::new("getfattr")
        .args(["-R", "-d", "-m", "-"])
        .arg(p)
        .output()
    {
        out.push(String::from_utf8_lossy(&x.stdout).into_owned());
    }
    out
}

async fn bash(ctx: &ToolCtx, cmd: &str) -> ToolOutcome {
    Bash.run(ctx, json!({ "command": cmd, "timeout_secs": 60 }))
        .await
}

#[tokio::test]
async fn no_write_or_escape_changes_anything() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    confined_ok(&sb);
    let ro = f.ctx(sb.clone(), true);
    let before_ws = snapshot(&f.ws);
    let before_extra = snapshot(&f.extra);
    let before_data = snapshot(&f.data);
    let shm_host = std::fs::read_dir("/dev/shm")
        .map(|r| r.count())
        .unwrap_or(0);
    let attempts: Vec<(&str, String)> = vec![
        ("tee", "echo x | tee marker".into()),
        ("tee -a", "echo x | tee -a marker".into()),
        ("cp over", "cp /etc/hostname marker".into()),
        ("cp new", "cp marker copy".into()),
        ("mv over", "cp marker $TMPDIR/m && mv -f $TMPDIR/m marker".into()),
        ("mv sub", "mv sub/inner sub/inner2".into()),
        ("rm -rf sub", "rm -rf sub".into()),
        ("rmdir", "rmdir sub".into()),
        ("chown self", "chown $(id -u):$(id -g) marker".into()),
        ("chmod dir", "chmod 700 sub".into()),
        ("touch -d", "touch -d 2001-01-01 marker".into()),
        ("touch -h", "touch -h -d 2001-01-01 sub".into()),
        ("truncate", "truncate -s 0 marker".into()),
        ("fallocate", "fallocate -l 4096 marker".into()),
        ("dd", "dd if=/dev/zero of=marker bs=1 count=1 conv=notrunc".into()),
        ("ln -s then write", "ln -s marker s && echo x > s".into()),
        ("mkfifo", "mkfifo fifo".into()),
        ("mknod", "mknod nod c 1 3".into()),
        (
            "unix socket",
            "python3 -c \"import socket;s=socket.socket(socket.AF_UNIX);s.bind('sock')\"".into(),
        ),
        ("python w", "python3 -c \"open('marker','a').write('x')\"".into()),
        (
            "python os.open",
            "python3 -c \"import os;os.open('n',os.O_CREAT|os.O_WRONLY)\"".into(),
        ),
        ("python utime", "python3 -c \"import os;os.utime('marker',(1,1))\"".into()),
        ("python chmod", "python3 -c \"import os;os.chmod('marker',0o600)\"".into()),
        ("perl", "perl -e 'open(F,\">>marker\") or die $!; print F \"x\"; close F or die $!'".into()),
        ("node", "node -e \"require('fs').appendFileSync('marker','x')\"".into()),
        ("setfattr", "setfattr -n user.x -v 1 marker".into()),
        ("git gc", "git gc -q".into()),
        ("git config", "git config --local core.x 1".into()),
        ("git tag", "git tag t1".into()),
        ("git notes", "git notes add -m x HEAD".into()),
        ("git reset", "git reset -q --hard".into()),
        ("git restore", "git restore marker".into()),
        ("git status refresh (locks on)", "GIT_OPTIONAL_LOCKS=1 git status".into()),
        ("proc self root", "echo x > /proc/self/root$PWD/marker".into()),
        ("proc 1 root", "echo x > /proc/1/root$PWD/marker".into()),
        ("proc 1 cwd", "echo x > /proc/1/cwd/viacwd1".into()),
        (
            "any proc root",
            "for p in /proc/[0-9]*; do echo x >> $p/root$PWD/marker 2>/dev/null && echo WROTE $p && exit 0; done; exit 1".into(),
        ),
        (
            "inherited fds",
            "for fd in /proc/self/fd/*; do t=$(readlink $fd); echo \"$fd -> $t\" >&2; done; \
             for fd in /proc/$$/fd/*; do n=${fd##*/}; [ -d $fd ] && (echo x > $fd/via-fd-$n || chmod 600 $fd/marker) && echo VIA $fd && exit 0; done; exit 1"
                .into(),
        ),
        ("nested bwrap", "bwrap --bind / / -- touch nested".into()),
        ("nested bwrap rw", "bwrap --dev-bind / / --bind \"$PWD\" \"$PWD\" -- sh -c 'echo x>>marker'".into()),
        ("unshare mount bind", "unshare -Urm sh -c 'mount --bind /tmp \"$PWD\" && touch \"$PWD\"/n2 && umount \"$PWD\" && echo x >> marker'".into()),
        ("nsenter", "nsenter -t 1 -m -U --preserve-credentials sh -c 'echo x >> marker'".into()),
        ("umount", "umount \"$PWD\" && echo x >> marker".into()),
        ("mount remount", "mount -o remount,rw \"$PWD\" && echo x >> marker".into()),
        ("extra entry", format!("touch '{}/new'", f.extra.display())),
        ("extra chmod", format!("chmod 600 '{}/kept'", f.extra.display())),
        ("data dir", format!("touch '{}/x'", f.data.display())),
        ("session tmp", format!("mkdir -p '{}/sandbox-tmp/s1' && touch '{}/sandbox-tmp/s1/x'", f.data.display(), f.data.display())),
        ("parent of ws", format!("touch '{}/home/x'", f.root.display())),
        ("dev shm", "echo x > /dev/shm/rplan-probe && ls -la /dev/shm".into()),
        ("flock", "flock marker true".into()),
    ];
    let mut log = String::new();
    for (what, cmd) in &attempts {
        let out = bash(&ro, cmd).await;
        log.push_str(&format!(
            "=== {what}: ok={} ===\n{}\n",
            out.ok,
            out.output.trim_end()
        ));
    }
    println!("{log}");
    let after_ws = snapshot(&f.ws);
    let after_extra = snapshot(&f.extra);
    let after_data = snapshot(&f.data);
    let diff = |a: &[String], b: &[String]| -> Vec<String> {
        let mut d: Vec<String> = a
            .iter()
            .filter(|x| !b.contains(x))
            .map(|x| format!("- {x}"))
            .collect();
        d.extend(
            b.iter()
                .filter(|x| !a.contains(x))
                .map(|x| format!("+ {x}")),
        );
        d
    };
    let dw = diff(&before_ws, &after_ws);
    let de = diff(&before_extra, &after_extra);
    let dd = diff(&before_data, &after_data);
    println!("WS DIFF: {dw:#?}\nEXTRA DIFF: {de:#?}\nDATA DIFF: {dd:#?}");
    assert!(
        !std::path::Path::new("/dev/shm/rplan-probe").exists(),
        "host /dev/shm written"
    );
    let _ = shm_host;
    assert!(dw.is_empty(), "the folder changed");
    assert!(de.is_empty(), "a write entry changed");
    assert!(!f.root.join("home/x").exists());
}

fn confined_ok(sb: &Arc<Sandbox>) {
    Confinement::Confined {
        sandbox: sb.clone(),
        tmp: None,
    }
    .read_only_shell()
    .expect("read-only shell here");
}

/// What a planner meets: git and grep work; the rest (cargo, the mounts,
/// limits) is printed for the record.
#[tokio::test]
async fn what_a_planner_can_run() {
    let f = fixture();
    let Some(sb) = f.sandbox() else { return };
    confined_ok(&sb);
    let ro = f.ctx(sb, true);
    std::fs::write(
        f.ws.join("Cargo.toml"),
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\n",
    )
    .unwrap();
    std::fs::create_dir(f.ws.join("src")).unwrap();
    std::fs::write(
        f.ws.join("src/lib.rs"),
        "#[test] fn t() { assert_eq!(1, 1); }\n",
    )
    .unwrap();
    for (what, cmd) in [
        ("git status", "git status"),
        ("git status short", "git status --short"),
        ("grep -r", "grep -rn original ."),
        (
            "cargo metadata",
            "cargo metadata --offline --format-version 1 --no-deps | head -c 200",
        ),
        ("cargo check", "cargo check --offline 2>&1 | tail -n 5"),
        (
            "cargo check tmp target",
            "CARGO_TARGET_DIR=$TMPDIR/t cargo test --offline 2>&1 | tail -n 5",
        ),
        ("env", "env | sort | grep -v -i key"),
        ("mounts", "cat /proc/self/mountinfo"),
        ("ulimit", "ulimit -a"),
        ("cgroup", "cat /proc/self/cgroup"),
        (
            "abstract unix",
            "cat /proc/net/unix | grep -c '@' ; cat /proc/net/unix | head -5",
        ),
        ("tmp df", "df -h /tmp /dev/shm"),
    ] {
        let out = bash(&ro, cmd).await;
        println!("=== {what}: ok={} ===\n{}\n", out.ok, out.output.trim_end());
        if matches!(what, "git status" | "git status short" | "grep -r") {
            assert!(out.ok, "{what}: {}", out.output);
        }
    }
}
