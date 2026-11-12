//! A stop signal is a stop (ADOPTION_PLAN A.19, docs/node-update.md §7.1),
//! and a refused node is told why and stays down, on the real binary.
//!
//! SIGTERM is handled like SIGINT by `serve` and `node run`: new work is
//! refused with the reason, running turns are cancelled through the normal
//! cancel path (so each command's guard ends its whole process tree),
//! closed in the log as `stopped`, and the process exits 0. A second signal
//! skips the wait and exits 1 at once. A node the master refuses for its
//! protocol version exits non-zero with the reason instead of reconnecting.
//!
//! Through the real `bwrap` where it works; where it does not, the tests
//! skip with a printed reason, and with `HARNESS_REQUIRE_BWRAP=1` they fail.
//! Every signal goes to a pid this test started; every leftover process is
//! found by this test's own folder or `sleep <n>` and killed by its pid.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nix::sys::signal::Signal;
use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// What the stop may take here at most: the turns close in a few seconds;
/// the bound in the binary is 60 s.
const WITHIN: Duration = Duration::from_secs(30);

// ---- the test's own processes, found from outside ----

fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit(')').next()?.trim_start().chars().next()
}

fn alive(pid: i32) -> bool {
    state(pid).is_some_and(|s| !matches!(s, 'Z' | 'X' | 'x'))
}

/// Live processes whose command line holds `needle` (this test's folder:
/// the guard's, bubblewrap's and its init's command lines carry the
/// session folder) or is exactly `sleep <secs>`.
fn left(needle: &Path, secs: u32) -> Vec<i32> {
    let n = needle.as_os_str().as_encoded_bytes();
    let want = format!("sleep\0{secs}\0");
    let me = std::process::id() as i32;
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|p| *p != me)
        .filter(|p| {
            let c = std::fs::read(format!("/proc/{p}/cmdline")).unwrap_or_default();
            c == want.as_bytes() || c.windows(n.len()).any(|w| w == n)
        })
        .filter(|p| alive(*p))
        .collect()
}

fn gone_within(needle: &Path, secs: u32, within: Duration) -> Vec<i32> {
    let by = Instant::now() + within;
    loop {
        let l = left(needle, secs);
        if l.is_empty() || Instant::now() >= by {
            return l;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Kills, on drop, whatever [`left`] still finds for this test.
struct Cleanup(PathBuf, u32);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for _ in 0..3 {
            for p in left(&self.0, self.1) {
                let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), Signal::SIGKILL);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Whether a contained shell works here (else the test skips, or fails
/// under `HARNESS_REQUIRE_BWRAP=1`).
fn containment(scratch: &Path) -> bool {
    use harness_tools::sandbox::{Sandbox, Settings};
    std::fs::create_dir_all(scratch).unwrap();
    let sb = Sandbox::new(
        PathBuf::from(EXE),
        &Settings::default(),
        Some(scratch.join("tmp")),
        &[scratch.to_path_buf()],
    )
    .unwrap();
    if let Err(e) = sb.probe(&scratch.join("probe"), scratch) {
        assert!(!bwrap::required(), "Landlock does not work here: {e}");
        eprintln!("SKIPPED: Landlock does not work here: {e}");
        return false;
    }
    bwrap::contained(sb, &scratch.join("probe-contain")).is_ok()
}

// ---- a process of the real binary ----

/// A `serve` or `node run` this test started: its stderr is collected;
/// it is killed by its own pid and reaped when dropped.
struct Proc {
    child: Option<Child>,
    log: Arc<Mutex<Vec<String>>>,
    /// The lines it printed on stdout before it served.
    out: Vec<String>,
}

impl Proc {
    fn pid(&self) -> i32 {
        self.child.as_ref().unwrap().id() as i32
    }

    fn signal(&self, s: Signal) {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.pid()), s).unwrap();
    }

    /// Its exit status within `within`, or `None` (it is still running).
    fn exited_within(&mut self, within: Duration) -> Option<ExitStatus> {
        let by = Instant::now() + within;
        loop {
            if let Some(s) = self.child.as_mut().unwrap().try_wait().unwrap() {
                self.child = None;
                return Some(s);
            }
            if Instant::now() >= by {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn log(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Start `args` on `data`; wait for a stdout line containing `ready`.
fn spawn(data: &Path, args: &[&str], envs: &[(&str, &str)], ready: &str) -> Proc {
    let mut cmd = Command::new(EXE);
    cmd.args(["--data-dir", data.to_str().unwrap()])
        .args(args)
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("HARNESS_JOIN_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn harness");
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let err = child.stderr.take().unwrap();
    let l2 = log.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            l2.lock().unwrap().push(line);
        }
    });
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut out = Vec::new();
    loop {
        let Some(Ok(line)) = lines.next() else {
            let _ = child.kill();
            let _ = child.wait();
            std::thread::sleep(Duration::from_millis(100));
            panic!(
                "{args:?} ended before it was ready: {}",
                log.lock().unwrap().join("\n")
            );
        };
        let done = line.contains(ready);
        out.push(line);
        if done {
            break;
        }
    }
    std::thread::spawn(move || for _ in lines {});
    Proc {
        child: Some(child),
        log,
        out,
    }
}

struct Server {
    proc: Proc,
    addr: String,
    token: String,
}

fn serve(data: &Path, extra: &[&str]) -> Server {
    let mut args = vec!["serve", "--bind", "127.0.0.1:0"];
    args.extend_from_slice(extra);
    let proc = spawn(data, &args, &[], "listening on http://");
    let addr = proc
        .out
        .iter()
        .find_map(|l| l.split("listening on http://").nth(1))
        .unwrap()
        .trim()
        .to_string();
    let token = std::fs::read_to_string(data.join("api-token"))
        .unwrap()
        .trim()
        .to_string();
    Server { proc, addr, token }
}

impl Server {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = reqwest::Client::new()
            .request(method, format!("http://{}{path}", self.addr))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            r = r.json(&b);
        }
        let r = r.send().await.unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn post(&self, path: &str, body: Value) -> Value {
        let (s, v) = self.call(reqwest::Method::POST, path, Some(body)).await;
        assert!((200..300).contains(&s), "{path}: {s} {v}");
        v
    }

    async fn session(&self, ws: &Path, mode: &str, node: Option<&str>) -> String {
        let mut body = json!({"cwd": ws.display().to_string(), "approval_mode": mode});
        if let Some(n) = node {
            body["node_id"] = json!(n);
        }
        self.post("/api/sessions", body).await["session_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    async fn events(&self, sid: &str) -> Vec<Value> {
        let (s, v) = self
            .call(
                reqwest::Method::GET,
                &format!("/api/sessions/{sid}/events?limit=5000"),
                None,
            )
            .await;
        assert_eq!(s, 200, "{v}");
        v["events"].as_array().unwrap().clone()
    }

    /// Wait until the session's log holds an event of `kind`.
    async fn wait_for(&self, sid: &str, kind: &str) -> Vec<Value> {
        let by = Instant::now() + WITHIN;
        loop {
            let evs = self.events(sid).await;
            if evs.iter().any(|e| e["kind"] == kind) {
                return evs;
            }
            assert!(
                Instant::now() < by,
                "no {kind} in {sid}: {evs:?}\n{}",
                self.proc.log()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

/// The reason the last `turn_ended` of `evs` gives.
fn end_reason(evs: &[Value]) -> String {
    evs.iter()
        .rev()
        .find(|e| e["kind"] == "turn_ended")
        .map(|e| e["payload"]["reason"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// `<root>/data` (the demo provider; `auto` allowed), `<root>/ws`.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    write_config(&data, &ws);
    (dir, data, ws)
}

fn write_config(data: &Path, ws: &Path) {
    std::fs::create_dir_all(data).unwrap();
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
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
}

async fn wait_sleeping(secs: u32, s: &Server) {
    let want = format!("sleep\0{secs}\0");
    let by = Instant::now() + WITHIN;
    loop {
        let found = std::fs::read_dir("/proc").unwrap().any(|e| {
            e.ok()
                .and_then(|e| std::fs::read(e.path().join("cmdline")).ok())
                .is_some_and(|c| c == want.as_bytes())
        });
        if found {
            return;
        }
        assert!(
            Instant::now() < by,
            "the command never started:\n{}",
            s.proc.log()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// ---- serve ----

/// SIGTERM, then the same with SIGINT: a running command is ended (its
/// whole tree), the turn ends `stopped`, the server exits 0.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_signal_closes_a_running_turn_of_serve() {
    for (signal, secs) in [(Signal::SIGTERM, 974_951), (Signal::SIGINT, 974_952)] {
        let (_dir, data, ws) = setup();
        if !containment(&data.parent().unwrap().join("probe")) {
            return;
        }
        let _clean = Cleanup(ws.clone(), secs);
        let s = serve(&data, &[]);
        let sid = s.session(&ws, "auto", None).await;
        s.post(
            &format!("/api/sessions/{sid}/prompt"),
            json!({"text": format!("run: sleep {secs}")}),
        )
        .await;
        wait_sleeping(secs, &s).await;
        let mut s = s;
        let asked = Instant::now();
        s.proc.signal(signal);
        let status = s.proc.exited_within(WITHIN);
        let took = asked.elapsed();
        let log = s.proc.log();
        assert_eq!(
            status.and_then(|s| s.code()),
            Some(0),
            "{signal}: exit {status:?} after {took:?}\n{log}"
        );
        let l = gone_within(&ws, secs, Duration::from_secs(5));
        assert!(
            l.is_empty(),
            "{signal}: the command's processes are left: {l:?}"
        );
        assert!(log.contains("stopping"), "{signal}: {log}");
        // The next start finds the turn closed: it ended `stopped`, not
        // `interrupted` by this start's recovery.
        let s2 = serve(&data, &[]);
        let evs = s2.events(&sid).await;
        assert_eq!(end_reason(&evs), "stopped", "{signal}: {evs:#?}");
        assert!(
            !evs.iter().any(|e| e["kind"] == "error"
                && e["payload"]["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("restarted")),
            "{signal}: closed by the restart, not the stop: {evs:#?}"
        );
        eprintln!("{signal}: stopped in {took:?}");
    }
}

/// While the stop waits for a turn (a command that ignores SIGTERM takes
/// the bash tool's 2 s grace), a new prompt and a new session are refused
/// with the reason; a second signal exits at once (1) and leaves no
/// process behind.
#[tokio::test(flavor = "multi_thread")]
async fn new_work_is_refused_while_stopping_and_a_second_signal_forces() {
    const SECS: u32 = 974_953;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let s = serve(&data, &[]);
    let busy = s.session(&ws, "auto", None).await;
    let idle = s.session(&ws, "auto", None).await;
    s.post(
        &format!("/api/sessions/{busy}/prompt"),
        json!({"text": format!("run: trap '' TERM; sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    let mut s = s;
    s.proc.signal(Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (code, v) = s
        .call(
            reqwest::Method::POST,
            &format!("/api/sessions/{idle}/prompt"),
            Some(json!({"text": "hello"})),
        )
        .await;
    assert_eq!(code, 503, "a prompt while stopping: {code} {v}");
    assert!(v.to_string().contains("stopping"), "{v}");
    let (code, v) = s
        .call(
            reqwest::Method::POST,
            "/api/sessions",
            Some(json!({"cwd": ws.display().to_string()})),
        )
        .await;
    assert_eq!(code, 503, "a new session while stopping: {code} {v}");
    assert!(v.to_string().contains("stopping"), "{v}");
    // A stopping server still holds the data directory (A.20): a second
    // start is refused with the lock's message, not run beside it.
    let second = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(data.parent().unwrap())
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(!second.status.success(), "a second start ran: {err}");
    assert!(err.contains("is in use by"), "{err}");
    // The busy turn is still being stopped (its command ignores SIGTERM).
    assert!(!left(&ws, SECS).is_empty(), "the command already ended");
    let forced = Instant::now();
    s.proc.signal(Signal::SIGTERM);
    let status = s.proc.exited_within(Duration::from_secs(1));
    let log = s.proc.log();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "a second signal: {status:?} after {:?}\n{log}",
        forced.elapsed()
    );
    // The guard ends the command when the node is gone.
    let l = gone_within(&ws, SECS, Duration::from_secs(5));
    assert!(l.is_empty(), "left after a forced stop: {l:?}");
}

/// A turn waiting for an approval: its request is denied and the turn
/// ends `stopped`; the server exits 0.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_waiting_for_approval_is_closed_by_the_stop() {
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let s = serve(&data, &[]);
    let sid = s.session(&ws, "ask", None).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: touch must-not-exist"}),
    )
    .await;
    s.wait_for(&sid, "approval_requested").await;
    let mut s = s;
    s.proc.signal(Signal::SIGTERM);
    let status = s.proc.exited_within(WITHIN);
    assert_eq!(status.and_then(|s| s.code()), Some(0), "{}", s.proc.log());
    let s2 = serve(&data, &[]);
    let evs = s2.events(&sid).await;
    assert_eq!(end_reason(&evs), "stopped", "{evs:#?}");
    let resolved: Vec<&Value> = evs
        .iter()
        .filter(|e| e["kind"] == "approval_resolved")
        .collect();
    assert_eq!(resolved.len(), 1, "{evs:#?}");
    assert_eq!(resolved[0]["payload"]["decision"], "deny", "{evs:#?}");
    assert!(!ws.join("must-not-exist").exists());
}

// ---- node run ----

/// A remote node: `node join` against `s`, then `node run`.
async fn remote_node(s: &Server, node_data: &Path) -> Proc {
    let line = s
        .proc
        .out
        .iter()
        .find(|l| l.starts_with("nodes connect to wss://"))
        .expect("no node listener line")
        .clone();
    let port = line
        .split(':')
        .nth(2)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let fp = line
        .split("(certificate ")
        .nth(1)
        .unwrap()
        .trim_end_matches(')')
        .to_string();
    let token = s.post("/api/admin/nodes/join", json!({})).await["token"]
        .as_str()
        .unwrap()
        .to_string();
    let out = Command::new(EXE)
        .args(["--data-dir", node_data.to_str().unwrap()])
        .args(["node", "join", &format!("wss://127.0.0.1:{port}")])
        .args(["--fingerprint", &fp])
        .current_dir(node_data.parent().unwrap())
        .env("HARNESS_JOIN_TOKEN", &token)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "join: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    spawn(node_data, &["node", "run"], &[], "connecting to")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_signal_closes_a_running_turn_of_a_node() {
    for (signal, secs) in [(Signal::SIGTERM, 974_954), (Signal::SIGINT, 974_955)] {
        let (_dir, data, ws) = setup();
        if !containment(&data.parent().unwrap().join("probe")) {
            return;
        }
        let _clean = Cleanup(ws.clone(), secs);
        let node_data = data.parent().unwrap().join("node");
        write_config(&node_data, &ws);
        let s = serve(&data, &["--node-listen", "127.0.0.1:0"]);
        let mut node = remote_node(&s, &node_data).await;
        let node_id = std::fs::read_to_string(node_data.join("remote.toml"))
            .unwrap()
            .lines()
            .find_map(|l| {
                l.strip_prefix("node_id = ")
                    .map(|v| v.trim_matches('"').to_string())
            })
            .unwrap();
        let by = Instant::now() + WITHIN;
        let sid = loop {
            let mut body = json!({"cwd": ws.display().to_string(), "approval_mode": "auto"});
            body["node_id"] = json!(node_id);
            let (code, v) = s
                .call(reqwest::Method::POST, "/api/sessions", Some(body))
                .await;
            if code == 200 {
                break v["session_id"].as_str().unwrap().to_string();
            }
            assert!(Instant::now() < by, "{code} {v}\n{}", node.log());
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        s.post(
            &format!("/api/sessions/{sid}/prompt"),
            json!({"text": format!("run: sleep {secs}")}),
        )
        .await;
        wait_sleeping(secs, &s).await;
        let asked = Instant::now();
        node.signal(signal);
        let status = node.exited_within(WITHIN);
        let took = asked.elapsed();
        let log = node.log();
        assert_eq!(
            status.and_then(|s| s.code()),
            Some(0),
            "{signal}: exit {status:?} after {took:?}\n{log}"
        );
        let l = gone_within(&ws, secs, Duration::from_secs(5));
        assert!(l.is_empty(), "{signal}: left: {l:?}");
        // The master heard the end before the node went (the stop flushes
        // its link), and it names the stop.
        let evs = s.wait_for(&sid, "turn_ended").await;
        assert_eq!(end_reason(&evs), "stopped", "{signal}: {evs:#?}");
        eprintln!("{signal}: node stopped in {took:?}");
    }
}

/// A node the master refuses for its protocol version exits non-zero with
/// the reason in its log after one try, instead of reconnecting. The
/// master here is a stand-in that answers every hello so.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_refused_for_its_version_exits_with_the_reason() {
    use harness_link::wire::{ServerMsg, accept, recv_json, send_json};
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let node_data = data.parent().unwrap().join("node");
    write_config(&node_data, &ws);
    let cert = harness_link::MasterCert::load_or_create(&data).unwrap();
    let tls = cert.server_config().unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    std::fs::write(
        node_data.join("remote.toml"),
        format!(
            "url = \"wss://127.0.0.1:{port}\"\nfingerprint = \"{}\"\nnode_id = \"box\"\n",
            cert.fingerprint()
        ),
    )
    .unwrap();
    let reason = "this node speaks protocol 2 and the master protocol 1: the master is older; \
                  upgrade the master first";
    let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t2 = tries.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = l.accept().await.unwrap();
            t2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let tls = tls.clone();
            tokio::spawn(async move {
                let mut ws = accept(tcp, tls).await.unwrap();
                send_json(
                    &mut ws,
                    &ServerMsg::Challenge {
                        nonce: "00".repeat(32),
                    },
                )
                .await
                .unwrap();
                let _: Option<Value> = recv_json(&mut ws).await.unwrap();
                send_json(
                    &mut ws,
                    &ServerMsg::Rejected {
                        reason: reason.into(),
                    },
                )
                .await
                .unwrap();
            });
        }
    });
    let mut node = spawn(&node_data, &["node", "run"], &[], "connecting to");
    let status = node.exited_within(Duration::from_secs(20));
    let log = node.log();
    assert!(
        status.is_some_and(|s| s.code().is_some_and(|c| c != 0)),
        "{status:?}\n{log}"
    );
    assert!(log.contains("upgrade the master first"), "{log}");
    // A second or two more to see it does not come back.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1, "{log}");
}
