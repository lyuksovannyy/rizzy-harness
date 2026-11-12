//! One process per data directory (ADOPTION_PLAN A.20), on the real
//! binary: a second `serve`, `node run` or `node join` on a live server's
//! data directory ends at once, naming the directory and the holder, and
//! changes nothing there (the live node's session temp folders, its open
//! turns, its sub-agents' folder); a holder killed with SIGKILL leaves no
//! stale lock; `doctor`, `token` and `claim-code` still work beside it.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A `serve`, killed (by its own pid) and reaped when dropped.
struct Server {
    child: Child,
    addr: String,
    token: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(data: &Path) -> Server {
    let mut child = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn harness");
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let addr = loop {
        let Some(Ok(line)) = lines.next() else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the server ended before it listened");
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
    Server { child, addr, token }
}

/// `<root>/data` (the data directory, demo provider) and `<root>/ws`.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("data");
    let ws = root.join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
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
    (dir, data, ws)
}

/// Runs `args` on `data`: its exit status and stderr, or `None` when it was
/// still running after `within` (then killed by its pid and reaped).
fn second(
    data: &Path,
    args: &[&str],
    within: Duration,
) -> Option<(std::process::ExitStatus, String)> {
    let mut child = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(args)
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "info")
        .env_remove("HARNESS_JOIN_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn harness");
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if started.elapsed() > within {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    status.map(|s| (s, err))
}

/// Every event of every session in `<data>/node.db`, as JSON text.
fn node_log(data: &Path) -> String {
    use harness_store::{Durability, Store};
    let store = Store::open(&data.join("node.db"), Durability::Normal).unwrap();
    let mut out = String::new();
    for row in store.sessions().unwrap() {
        for e in store.events_after(row.session_id, 0, 1 << 20).unwrap() {
            out.push_str(&format!("{} {} {}\n", e.seq, e.kind, e.payload));
        }
    }
    out
}

async fn post(s: &Server, path: &str, body: Value) -> Value {
    let r = reqwest::Client::new()
        .post(format!("http://{}{path}", s.addr))
        .bearer_auth(&s.token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{path}: {}", r.status());
    r.json().await.unwrap()
}

/// A session whose turn waits on an approval: an open turn.
async fn open_turn(s: &Server, ws: &Path) -> String {
    let v = post(
        s,
        "/api/sessions",
        json!({"cwd": ws.display().to_string(), "approval_mode": "ask"}),
    )
    .await;
    let sid = v["session_id"].as_str().unwrap().to_string();
    post(
        s,
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: echo waiting"}),
    )
    .await;
    let started = Instant::now();
    loop {
        let v: Value = reqwest::Client::new()
            .get(format!(
                "http://{}/api/sessions/{sid}/events?limit=5000",
                s.addr
            ))
            .bearer_auth(&s.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let evs = v["events"].as_array().cloned().unwrap_or_default();
        if evs.iter().any(|e| e["kind"] == "approval_requested") {
            return sid;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "{evs:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A second `serve` (on the live one's own port, as typing `serve` twice
/// does), a `node run` and a `node join` on a live server's data directory
/// each end at once, non-zero, with the one-line refusal naming the
/// directory and the holder's pid; the live node's session temp folder,
/// open turn and sub-agents' folder are as they were, and nothing new is
/// made (no node key). On `main` the second start emptied the temp folder,
/// removed the sub-agents' folder and closed the open turn as interrupted
/// before it failed on the port.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_start_on_a_live_data_directory_is_refused_and_changes_nothing() {
    let (_dir, data, ws) = setup();
    let s = start(&data);
    let pid = s.child.id();
    let sid = open_turn(&s, &ws).await;
    // What a running node has there: its session's temp folder (TMPDIR) with
    // a file, and a sub-agent's folder.
    let tmp = data.join("sandbox-tmp").join(&sid);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("work"), "in progress").unwrap();
    let sub = data.join("sessions/sub-agent");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("file"), "a sub-agent's work").unwrap();
    // A joined node's file, so `node run` gets past "not joined yet".
    std::fs::write(
        data.join("remote.toml"),
        "url = \"wss://127.0.0.1:9\"\nfingerprint = \"00\"\nnode_id = \"n1\"\n",
    )
    .unwrap();
    let before = node_log(&data);

    let mut problems = Vec::new();
    let cases: [(&str, Vec<&str>); 3] = [
        ("serve", vec!["serve", "--bind", s.addr.as_str()]),
        ("node run", vec!["node", "run"]),
        (
            "node join",
            vec![
                "node",
                "join",
                "wss://127.0.0.1:9",
                "--token",
                "t",
                "--fingerprint",
                "00",
            ],
        ),
    ];
    for (name, args) in cases {
        match second(&data, &args, Duration::from_secs(15)) {
            None => problems.push(format!("{name}: still running after 15 s")),
            Some((st, err)) => {
                let line = err.lines().find(|l| l.contains("in use")).unwrap_or("");
                if st.success()
                    || !line.contains(&data.display().to_string())
                    || !line.contains(&format!("pid {pid}"))
                    || !line.contains("serve")
                {
                    problems.push(format!("{name}: exit {st}, stderr: {err}"));
                }
            }
        }
    }
    if !tmp.join("work").exists() {
        problems.push("the live session's temp folder was emptied".into());
    }
    if !sub.join("file").exists() {
        problems.push("the sub-agents' folder was removed".into());
    }
    let after = node_log(&data);
    if after != before || after.contains("interrupted") {
        problems.push(format!(
            "the live node's log was written by another process:\n{after}"
        ));
    }
    if data.join("keys/node.key").exists() {
        problems.push("a node key was made".into());
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // The live server still works.
    open_turn(&s, &ws).await;
}

/// The kernel releases the lock with its holder, however it dies: after a
/// SIGKILL the next start succeeds (no stale lock), and its lock file names
/// the new holder.
#[test]
fn a_holder_killed_with_sigkill_leaves_no_stale_lock() {
    let (_dir, data, _ws) = setup();
    let mut s = start(&data);
    s.child.kill().unwrap(); // SIGKILL
    s.child.wait().unwrap();
    let s = start(&data);
    let held = std::fs::read_to_string(data.join("lock")).unwrap();
    assert!(
        held.starts_with(&format!("{} serve", s.child.id())),
        "{held:?}"
    );
}

/// `doctor` runs beside a live server and names the holder; `token` and
/// `claim-code` (read-only beside a server) still work; `doctor` on a
/// directory nobody runs on says it is free.
#[test]
fn doctor_token_and_claim_code_work_beside_a_live_server() {
    let (_dir, data, _ws) = setup();
    let s = start(&data);
    let run = |args: &[&str]| {
        Command::new(EXE)
            .args(["--data-dir", data.to_str().unwrap()])
            .args(args)
            .env("HARNESS_LOG", "error")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let out = run(&["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text
        .lines()
        .find(|l| l.contains("data dir lock"))
        .unwrap_or("");
    assert!(
        line.contains(&format!("pid {}", s.child.id())) && line.contains("serve"),
        "{text}"
    );
    let out = run(&["token"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), s.token);
    let out = run(&["claim-code"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The live server is still the holder: none of them took the lock.
    assert!(
        second(&data, &["node", "run"], Duration::from_secs(15))
            .is_some_and(|(st, _)| !st.success())
    );
    drop(s);
    let out = run(&["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text
        .lines()
        .find(|l| l.contains("data dir lock"))
        .unwrap_or("");
    assert!(line.contains("free"), "{text}");
}
