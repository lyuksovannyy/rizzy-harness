//! Where the push's token can be reached while git holds it (X.27 step 4,
//! PLAN.md T101), through the real `sandbox-exec` wrapper and the real
//! bash tool.
//!
//! The node gives the token to its one `git push` as the standard input
//! of that process, a pipe (`forge_clone::token_pipe`). A process of the
//! node's user that may open `/proc/<git>/fd/0` can read it while the push
//! runs. The stand-in here is a process of the test's user whose standard
//! input is such a pipe holding a canary, as a stalled push's git would be.
//! A sandboxed shell (Landlock alone, and inside the containment where
//! bubblewrap works) must not read it, nor that process's environment; an
//! unconfined read must (the calibration: the test would see a leak).
//!
//! Also the push's own jail (`Sandbox::helper_net`): the network is open
//! (a loopback listener gets git's connection), the data directory is not.
//!
//! On a kernel without Landlock these tests skip with a printed reason, but
//! when `CI` is set they fail.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use harness_tools::sandbox::{Confinement, Grant, Sandbox, Settings};
use harness_tools::{Bash, Tool, ToolCtx};
use serde_json::json;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");
const CANARY: &str = "ghp_CANARYpushreach0000000000000000000001";

struct Fixture {
    _dir: tempfile::TempDir,
    data: PathBuf,
    ws: PathBuf,
    contained: bool,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("home/proj");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(data.join("api-token"), "API-TOKEN-CANARY\n").unwrap();
    Fixture {
        _dir: dir,
        data,
        ws,
        contained: false,
    }
}

impl Fixture {
    fn sandbox(&self) -> Arc<Sandbox> {
        self.sandbox_with(&Settings::default())
    }

    fn sandbox_with(&self, settings: &Settings) -> Arc<Sandbox> {
        let sb = Sandbox::new(
            PathBuf::from(EXE),
            settings,
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

    async fn bash(&self, cmd: &str) -> String {
        let mut ctx = ToolCtx::new(&self.ws);
        ctx.protected = Arc::new(vec![self.data.clone()]);
        ctx.sandbox = Confinement::Confined {
            sandbox: self.sandbox(),
            tmp: None,
        }
        .for_session("s1");
        Bash.run(&ctx, json!({ "command": cmd })).await.output
    }
}

fn fixtures() -> Vec<Fixture> {
    let mut all = vec![fixture()];
    let mut f = fixture();
    let sb = Arc::try_unwrap(f.sandbox()).unwrap();
    if bwrap::contained(sb, &f.data.join("sandbox-tmp/probe-contain")).is_ok() {
        f.contained = true;
        all.push(f);
    }
    all
}

fn landlock() -> bool {
    let f = fixture();
    match f
        .sandbox()
        .probe(&f.data.join("sandbox-tmp/probe"), &f.data)
    {
        Ok(_) => true,
        Err(e) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI must run the sandbox tests on a Landlock kernel: {e}"
            );
            eprintln!("SKIPPED: Landlock does not work here: {e}");
            false
        }
    }
}

/// A process whose standard input is a pipe holding the canary, as the
/// push's git is while it waits on the forge; killed when dropped.
struct Holder(std::process::Child);

impl Holder {
    fn start() -> Self {
        let mut c = std::process::Command::new("sleep")
            .arg("60")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut w = c.stdin.take().unwrap();
        w.write_all(format!("{CANARY}\n").as_bytes()).unwrap();
        // Closed, as the node closes its end: a reader gets the line, then
        // the end, and never waits.
        drop(w);
        Self(c)
    }
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// (d) A sandboxed command cannot read the token from the push's git, by
/// its descriptor, its environment or its memory map; an unconfined
/// process of the same user can (the residual of a host chat).
#[tokio::test]
async fn a_sandboxed_shell_cannot_read_the_pushs_token() {
    if !landlock() {
        return;
    }
    for f in fixtures() {
        eprintln!("contained: {}", f.contained);
        let h = Holder::start();
        let pid = h.pid();
        let out = f
            .bash(&format!(
                "timeout 3 cat /proc/{pid}/fd/0; echo fd=$?; \
                 timeout 3 head -c 4096 /proc/{pid}/environ; echo env=$?; \
                 ls -l /proc/{pid}/fd/ 2>&1 | head -5; \
                 timeout 3 dd if=/proc/{pid}/mem bs=1 count=1 2>&1 | head -2; echo end"
            ))
            .await;
        assert!(!out.contains(CANARY), "contained={}: {out}", f.contained);
        assert!(out.contains("end"), "{out}");
        // The calibration, after it: unconfined, the same read gets it.
        let raw = std::fs::read_to_string(format!("/proc/{pid}/fd/0"));
        assert!(
            raw.as_deref().is_ok_and(|s| s.contains(CANARY)),
            "the calibration read failed: {raw:?}"
        );
    }
}

/// The push's jail: read-write on its own folder and the system folders
/// only, the network open whatever the sandbox's own network says.
#[tokio::test]
async fn the_push_jail_reaches_the_network_and_not_the_data_directory() {
    if !landlock() {
        return;
    }
    let f = fixture();
    // The agent's sandbox with its network off: the push's jail opens it.
    let sb = f.sandbox_with(&Settings {
        network: harness_tools::sandbox::Network::Off,
        ..Default::default()
    });
    let own = f.data.join("projects-staging/push-x");
    std::fs::create_dir_all(&own).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    l.set_nonblocking(true).unwrap();
    let got = std::thread::spawn(move || {
        for _ in 0..200 {
            if l.accept().is_ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    });
    let mut c = sb.helper_net(&[Grant::system(&own)], &[]);
    c.arg("sh").arg("-c").arg(format!(
        "cat {}/api-token; echo r=$?; echo hi > {}/x; echo w=$?; \
         timeout 3 git -c protocol.allow=always ls-remote http://127.0.0.1:{port}/x 2>&1 | head -1; echo end",
        f.data.display(),
        own.display()
    ));
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(&own)
        .stdin(std::process::Stdio::null());
    let out = c.output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(!text.contains("API-TOKEN-CANARY"), "{text}");
    assert!(text.contains("w=0"), "{text}");
    assert!(
        got.join().unwrap(),
        "the jail did not reach the network: {text}"
    );
}
