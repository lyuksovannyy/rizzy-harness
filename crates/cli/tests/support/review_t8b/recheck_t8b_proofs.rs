//! T.8b re-check (`docs/security-review/2026-11-T8b-network-off.md`,
//! "Re-check"): what the fixes of T8B-1 and T8B-2 added. Not part of the
//! test suite: `run_rust_proofs.sh recheck` beside this file copies it to
//! `crates/cli/tests/`, runs it and removes the copy.
//!
//! `held_*` passes when the claim holds; `finding_*` and `note_*` PASS WHEN
//! WHAT THEY DESCRIBE REPRODUCES and print what they saw.
//!
//! Every listener is this test's own, on 127.0.0.1. The stand-ins are the
//! real bubblewrap behind a script that changes one thing. Scratch folders
//! are under `$CARGO_TARGET_DIR/review_t8b/`. The only processes that
//! outlast a tool call are the `sleep 6` of the slow stand-in, which end on
//! their own before the test returns (it waits for them).

use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_tools::contain::ContainMode;
use harness_tools::sandbox::{
    Confinement, ContainLevel, Network, Settings, Started, start_with_exe_retrying,
};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn scratch_root() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let p = target.join("review_t8b");
    std::fs::create_dir_all(&p).unwrap();
    p.canonicalize().unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    ws: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir_in(scratch_root()).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    Fixture {
        _dir: dir,
        root,
        data,
        ws,
    }
}

impl Fixture {
    fn start(&self, settings: Settings, retry: Duration) -> Started {
        start_with_exe_retrying(
            PathBuf::from(EXE),
            &settings,
            &self.data,
            std::slice::from_ref(&self.data),
            Path::new("/etc/rizzy/node.toml"),
            retry,
        )
        .unwrap()
    }

    fn off(&self, contain: ContainMode, bwrap: &Path) -> Settings {
        Settings {
            network: Network::Off,
            contain,
            bwrap: Some(bwrap.to_path_buf()),
            ..Default::default()
        }
    }

    fn stand_in(&self, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = self.root.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

/// One bash tool call of `session`, with its own context (movable into a
/// task).
async fn bash(ws: PathBuf, data: PathBuf, c: Confinement, session: String, input: serde_json::Value) -> String {
    let mut ctx = ToolCtx::new(&ws);
    ctx.protected = Arc::new(vec![data]);
    ctx.sandbox = c.for_session(&session);
    Bash.run(&ctx, input).await.output
}

fn real_bwrap() -> PathBuf {
    harness_tools::contain::find_bwrap().unwrap()
}

fn host_ns() -> String {
    std::fs::read_link("/proc/self/ns/net")
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn udp_host() -> UdpSocket {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    s
}

fn drain(s: &UdpSocket) -> Vec<String> {
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while let Ok((n, _)) = s.recv_from(&mut buf) {
        got.push(String::from_utf8_lossy(&buf[..n]).into_owned());
    }
    got
}

/// FINDING R-1 (what is left of T8B-1). The retry exists only for the gap
/// ("bubblewrap works, but not with `--unshare-net`"). A failing moment
/// that hits bubblewrap as such (what a full process table or no memory
/// does: it does not look at the options) fails all four runs of the start,
/// and the node is on Landlock alone for its whole life: no namespace of
/// any kind, UDP out, and nothing ever probes again, although a second
/// start a moment later is contained with the namespace.
#[tokio::test]
async fn finding_r1_a_failing_moment_over_every_probe_leaves_landlock_alone_for_life() {
    let f = fixture();
    let count = f.root.join("count");
    let fake = f.stand_in(
        "bwrap-fails-four-times",
        &format!(
            "# Fails its first four runs, whatever the options, then is the real one.\n\
             n=$(cat '{count}' 2>/dev/null || echo 0)\n\
             if [ \"$n\" -lt 4 ]; then echo $((n+1)) >'{count}'\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi\nexec '{real}' \"$@\"\n",
            count = count.display(),
            real = real_bwrap().display()
        ),
    );
    let retry = Duration::from_millis(100);
    let node = f.start(f.off(ContainMode::Auto, &fake), retry);
    println!("the node's line at start ({:?}): {}", node.contain_level, node.contain_summary);
    assert!(node.contain_summary.starts_with("NOT CONTAINED"));
    assert!(node.contain_summary.contains("UDP and DNS still go out"));

    let later = f.start(f.off(ContainMode::Auto, &fake), retry);
    println!("a second start says ({:?}): {}", later.contain_level, later.contain_summary);
    assert!(later.contain_summary.contains("a network namespace of its own"));

    let host = udp_host();
    let port = host.local_addr().unwrap().port();
    for round in 0..3 {
        // Well past the retry interval each time.
        tokio::time::sleep(retry * 3).await;
        let out = bash(
            f.ws.clone(),
            f.data.clone(),
            node.confinement.clone(),
            "s1".into(),
            json!({ "command": format!(
                "echo \"pid=$$ ns=$(readlink /proc/self/ns/net)\"; \
                 printf 'alone-{round}' >/dev/udp/127.0.0.1/{port} && echo sent") }),
        )
        .await;
        let got = drain(&host);
        println!("command {round}: {out:?}; the host's listener heard {got:?}");
        assert_eq!(got, [format!("alone-{round}")]);
        assert!(out.contains(&host_ns()));
        assert!(!out.contains("pid=2 "));
    }
}

/// FINDING R-2. The gap's retry probe runs inside the tool call that
/// happens to be next, before that call's own timeout starts, on the
/// runtime thread that polls the session, with no bound of its own. A
/// bubblewrap that does not return for 6 s holds a command with
/// `timeout_secs: 1`, cancelled after 0.5 s, for those 6 s; one interval
/// later the next command is held the same way.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn finding_r2_a_probe_that_does_not_return_holds_the_command_past_its_timeout() {
    let f = fixture();
    let (refuse, slow) = (f.root.join("refuse"), f.root.join("slow"));
    std::fs::write(&refuse, "").unwrap();
    let fake = f.stand_in(
        "bwrap-slow",
        &format!(
            "for a; do if [ \"$a\" = --unshare-net ]; then\n\
             [ -e '{slow}' ] && sleep 6\n\
             if [ -e '{refuse}' ]; then\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi; fi; done\nexec '{real}' \"$@\"\n",
            slow = slow.display(),
            refuse = refuse.display(),
            real = real_bwrap().display()
        ),
    );
    let retry = Duration::from_secs(2);
    let node = f.start(f.off(ContainMode::Auto, &fake), retry);
    assert!(node.contain_summary.contains("network off WITH A GAP"));
    std::fs::write(&slow, "").unwrap();
    tokio::time::sleep(retry + Duration::from_millis(100)).await;

    let run = |session: &str, delay: Duration, cancel_after: Option<Duration>| {
        let (ws, data, c, session) = (
            f.ws.clone(),
            f.data.clone(),
            node.confinement.clone(),
            session.to_string(),
        );
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let mut ctx = ToolCtx::new(&ws);
            ctx.protected = Arc::new(vec![data]);
            ctx.sandbox = c.for_session(&session);
            if let Some(after) = cancel_after {
                let cancel = ctx.cancel.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(after).await;
                    cancel.cancel();
                });
            }
            let t = Instant::now();
            let out = Bash
                .run(&ctx, json!({ "command": "echo hi", "timeout_secs": 1 }))
                .await
                .output;
            (t.elapsed(), out)
        })
    };
    // A: the command that is next when the probe is due. B: another
    // session 0.3 s later (inside the interval). C: one interval later.
    let a = run("a", Duration::ZERO, Some(Duration::from_millis(500)));
    let b = run("b", Duration::from_millis(300), None);
    let c = run("c", retry + Duration::from_millis(300), None);
    let (a, b, c) = (a.await.unwrap(), b.await.unwrap(), c.await.unwrap());
    println!("A (timeout 1 s, cancelled at 0.5 s): returned after {:.1} s: {:?}", a.0.as_secs_f64(), a.1);
    println!("B (0.3 s later, another session): returned after {:.1} s: {:?}", b.0.as_secs_f64(), b.1);
    println!("C (one interval later): returned after {:.1} s: {:?}", c.0.as_secs_f64(), c.1);
    assert!(a.0 >= Duration::from_secs(5), "A was not held");
    assert!(b.0 < Duration::from_secs(2), "B was held too");
    assert!(c.0 >= Duration::from_secs(5), "C was not held");
}

/// HELD (the re-check's question 2 and 3). A node in the gap, bubblewrap
/// working again, 24 commands of 24 sessions started at once when the probe
/// is due: one probe runs; the commands that started beside it may still be
/// in the gap (the node had not decided yet); every command started after
/// they returned has its own namespace; with bubblewrap refusing again the
/// next command fails with bubblewrap's message and does not run in the
/// host's network.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn held_many_commands_at_once_one_probe_and_none_in_the_gap_afterwards() {
    let f = fixture();
    let (refuse, log) = (f.root.join("refuse"), f.root.join("runs.log"));
    std::fs::write(&refuse, "").unwrap();
    let fake = f.stand_in(
        "bwrap-logging",
        &format!(
            "net=0; probe=0; prev=\n\
             for a; do [ \"$a\" = --unshare-net ] && net=1\n\
             [ \"$prev\" = --mark-session ] && [ \"$a\" = probe ] && probe=1; prev=$a; done\n\
             echo \"net=$net probe=$probe\" >>'{log}'\n\
             if [ $net = 1 ] && [ -e '{refuse}' ]; then\n\
             echo 'bwrap: Creating new namespace failed: Resource temporarily unavailable' >&2\n\
             exit 1; fi\nexec '{real}' \"$@\"\n",
            log = log.display(),
            refuse = refuse.display(),
            real = real_bwrap().display()
        ),
    );
    let retry = Duration::from_millis(400);
    let node = f.start(f.off(ContainMode::Auto, &fake), retry);
    assert!(node.contain_summary.contains("network off WITH A GAP"));
    println!("runs of bubblewrap at start:\n{}", std::fs::read_to_string(&log).unwrap());
    std::fs::remove_file(&refuse).unwrap();
    std::fs::write(&log, "").unwrap();
    tokio::time::sleep(retry + Duration::from_millis(100)).await;

    let cmd = json!({ "command": "echo \"ns=$(readlink /proc/self/ns/net)\"" });
    let host = format!("ns={}", host_ns());
    for wave in 0..3 {
        let tasks: Vec<_> = (0..24)
            .map(|i| {
                tokio::spawn(bash(
                    f.ws.clone(),
                    f.data.clone(),
                    node.confinement.clone(),
                    format!("s{i}"),
                    cmd.clone(),
                ))
            })
            .collect();
        let mut in_gap = 0;
        for t in tasks {
            let out = t.await.unwrap();
            assert!(out.contains("ns=net:["), "{out}");
            if out.contains(&host) {
                in_gap += 1;
            }
        }
        let runs = std::fs::read_to_string(&log).unwrap();
        let probes = runs.lines().filter(|l| *l == "net=1 probe=1").count();
        println!("wave {wave}: 24 commands at once, {in_gap} in the host's namespace; probes so far: {probes}");
        assert_eq!(probes, 1, "more than one probe, or none");
        if wave > 0 {
            assert_eq!(in_gap, 0, "a command in the gap after the node left it");
        }
        tokio::time::sleep(retry + Duration::from_millis(100)).await;
    }

    // bubblewrap refuses the namespace again.
    std::fs::write(&refuse, "").unwrap();
    tokio::time::sleep(retry + Duration::from_millis(100)).await;
    let out = bash(
        f.ws.clone(),
        f.data.clone(),
        node.confinement.clone(),
        "s0".into(),
        cmd.clone(),
    )
    .await;
    println!("with bubblewrap refusing again, the tool result is: {out:?}");
    assert!(!out.contains(&host), "back in the gap: {out}");
    assert!(out.contains("Creating new namespace failed"), "{out}");
}

/// NOTE R-3 (older than the fix, missed in the first review). The probe's
/// script runs `sh`, `cat` and now `readlink` by name, with the node's
/// `PATH`, inside the wrapper, which grants node.toml's `[sandbox] write`
/// entries. Where such an entry holds a folder that is on the node's `PATH`
/// before the system's (not the README's example, which lists those folders
/// under `read`), one command of a node with the namespace decides every
/// later start's probe: here it leaves a `readlink` that fails, and the next
/// start is in the gap.
#[tokio::test]
async fn note_r3_a_program_on_path_in_a_write_entry_decides_the_probe() {
    let f = fixture();
    let tool = f.root.join("toolchain");
    std::fs::create_dir_all(tool.join("bin")).unwrap();
    std::fs::write(
        f.data.join("node.toml"),
        format!("[sandbox]\nnetwork = \"off\"\nwrite = [\"{}\"]\n", tool.display()),
    )
    .unwrap();
    let doctor = || {
        let path = format!(
            "{}:{}",
            tool.join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let out = std::process::Command::new(EXE)
            .args(["--data-dir", f.data.to_str().unwrap(), "doctor"])
            .env("PATH", path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.contains("shell containment: "))
            .unwrap_or_default()
            .to_string()
    };
    let before = doctor();
    println!("before: {before}");
    assert!(before.contains("a network namespace of its own"), "{before}");

    // One contained command of a node with the namespace.
    let node = f.start(
        Settings {
            network: Network::Off,
            write: vec![tool.clone()],
            ..Default::default()
        },
        Duration::from_secs(60),
    );
    assert_eq!(node.contain_level, ContainLevel::Ok);
    let out = bash(
        f.ws.clone(),
        f.data.clone(),
        node.confinement.clone(),
        "s1".into(),
        json!({ "command": format!(
            "echo \"ns=$(readlink /proc/self/ns/net)\"; \
             printf '#!/bin/sh\\nexit 1\\n' >'{0}/bin/readlink' && chmod +x '{0}/bin/readlink' && echo left",
            tool.display()) }),
    )
    .await;
    println!("the command: {out:?}");
    assert!(out.contains("left") && !out.contains(&host_ns()));

    let after = doctor();
    println!("after: {after}");
    assert!(after.contains("network off WITH A GAP"), "{after}");
}

/// HELD (the re-check's question 1). A node that itself runs in a
/// namespace that is not the machine's first: the identity check compares
/// with the node's own link, so a command's new namespace still differs.
#[test]
fn held_a_node_in_a_namespace_of_its_own_still_gets_one_per_command() {
    let f = fixture();
    std::fs::write(
        f.data.join("node.toml"),
        format!(
            "[sandbox]\nnetwork = \"off\"\nbwrap = \"{}\"\n",
            real_bwrap().display()
        ),
    )
    .unwrap();
    let (uid, gid) = (nix_uid(), nix_gid());
    let out = std::process::Command::new("unshare")
        .args([
            "--user",
            &format!("--map-user={uid}"),
            &format!("--map-group={gid}"),
            "--net",
            "sh",
            "-c",
            "echo \"the node's namespace: $(readlink /proc/self/ns/net)\"; exec \"$@\"",
            "sh",
            EXE,
            "--data-dir",
            f.data.to_str().unwrap(),
            "doctor",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    println!("the machine's namespace: {}", host_ns());
    for l in text.lines().filter(|l| l.contains("namespace")) {
        println!("{l}");
    }
    assert!(!text.contains(&format!("the node's namespace: {}", host_ns())), "{text}");
    let line = text
        .lines()
        .find(|l| l.contains("shell containment: "))
        .unwrap_or_default();
    assert!(line.contains("a network namespace of its own"), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
}

fn nix_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").unwrap().uid()
}

fn nix_gid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").unwrap().gid()
}
