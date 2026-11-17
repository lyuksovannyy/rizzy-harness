//! `[chat] model_first_byte_timeout_s` (ADOPTION_PLAN X.31) on the shipped
//! binary: an Ask chat against a provider that accepts the connection and
//! never answers ends after the configured wait (once, then its one retry
//! for a silent provider), with the existing "did not answer" error, and the
//! person's Retry is offered for it. Before X.31 the wait was a fixed 600 s.
//! An out-of-range value stops the server at start with the key and range.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

/// A provider that accepts every connection and never sends a byte; it
/// counts the connections. The streams are held until the test ends.
fn silent_provider() -> (u16, Arc<Mutex<Vec<std::net::TcpStream>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let held: Arc<Mutex<Vec<std::net::TcpStream>>> = Arc::default();
    let h = held.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            h.lock().unwrap().push(s);
        }
    });
    (port, held)
}

fn config(data: &Path, port: u16, chat: &str) {
    std::fs::create_dir_all(data).unwrap();
    std::fs::write(
        data.join("config.toml"),
        format!(
            "[[providers]]\nid = \"quiet\"\nname = \"Quiet\"\npreset = \"custom_openai\"\n\
             kind = \"openai\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\n\
             models = [{{ id = \"m\" }}]\n\n[chat]\n{chat}"
        ),
    )
    .unwrap();
}

struct Served {
    child: Child,
    log: Arc<Mutex<Vec<String>>>,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Served {
    fn log(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }
}

/// `serve` on a free port: the process and its address, or the process
/// when it ended before listening.
fn serve(data: &Path) -> (Served, Option<String>) {
    let mut child = Command::new(EXE)
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .current_dir(data.parent().unwrap())
        .env("HARNESS_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("HARNESS_JOIN_TOKEN")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let l2 = log.clone();
    let err = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            l2.lock().unwrap().push(line);
        }
    });
    let mut addr = None;
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    for line in lines.by_ref().map_while(Result::ok) {
        if let Some(a) = line.split("listening on http://").nth(1) {
            addr = Some(a.trim().to_string());
            break;
        }
    }
    std::thread::spawn(move || for _ in lines {});
    (Served { child, log }, addr)
}

async fn call(
    addr: &str,
    token: &str,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut r = reqwest::Client::new()
        .request(method, format!("http://{addr}{path}"))
        .bearer_auth(token);
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let st = r.status().as_u16();
    let text = r.text().await.unwrap();
    (
        st,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ask_chat_waits_for_the_first_byte_as_configured() {
    let (port, held) = silent_provider();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().canonicalize().unwrap().join("data");
    config(&data, port, "model_first_byte_timeout_s = 10\n");
    let (s, addr) = serve(&data);
    let addr = addr.unwrap_or_else(|| panic!("serve did not start:\n{}", s.log()));
    let token = std::fs::read_to_string(data.join("api-token"))
        .unwrap()
        .trim()
        .to_string();
    let post = reqwest::Method::POST;
    let (st, v) = call(
        &addr,
        &token,
        post.clone(),
        "/api/sessions",
        Some(json!({"profile": "chat"})),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let t0 = Instant::now();
    let (st, v) = call(
        &addr,
        &token,
        post.clone(),
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": "hello"})),
    )
    .await;
    assert_eq!(st, 200, "{v}");
    // 10 s, one retry after a 2 s backoff, 10 s: about 22 s. The default
    // (600 s) would still be waiting at the bound.
    let by = t0 + Duration::from_secs(90);
    let evs = loop {
        let (_, v) = call(
            &addr,
            &token,
            reqwest::Method::GET,
            &format!("/api/sessions/{sid}/events?limit=5000"),
            None,
        )
        .await;
        let evs = v["events"].as_array().cloned().unwrap_or_default();
        if evs.iter().any(|e| e["kind"] == "turn_ended") {
            break evs;
        }
        assert!(
            Instant::now() < by,
            "no end after 90 s: {evs:?}\n{}",
            s.log()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(19) && took < Duration::from_secs(60),
        "{took:?}"
    );
    assert_eq!(held.lock().unwrap().len(), 2, "one call and its one retry");
    let errors: Vec<&str> = evs
        .iter()
        .filter(|e| e["kind"] == "error")
        .filter_map(|e| e["payload"]["message"].as_str())
        .collect();
    assert!(
        errors
            .iter()
            .any(|m| m.starts_with("no answer: the provider")
                && m.contains(" for 10 s ")
                && m.contains("[chat]")),
        "{evs:?}"
    );
    // The person's Retry is offered for the failed turn.
    let turn = evs
        .iter()
        .find(|e| e["kind"] == "turn_started")
        .and_then(|e| e["payload"]["turn_id"].as_str())
        .unwrap()
        .to_string();
    let (st, v) = call(
        &addr,
        &token,
        post,
        &format!("/api/sessions/{sid}/turns/{turn}/retry"),
        Some(json!({})),
    )
    .await;
    assert_eq!(st, 200, "{v}\n{}", s.log());
}

#[test]
fn an_out_of_range_value_stops_the_start() {
    let (port, _held) = silent_provider();
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().canonicalize().unwrap().join("data");
    config(&data, port, "model_first_byte_timeout_s = 5\n");
    let (mut s, addr) = serve(&data);
    assert!(addr.is_none(), "started with 5 s");
    let _ = s.child.wait();
    // The reader thread drains the closed pipe.
    std::thread::sleep(Duration::from_millis(300));
    let log = s.log();
    assert!(
        log.contains("[chat] model_first_byte_timeout_s = 5: must be between 10 and 3600"),
        "{log}"
    );
}
