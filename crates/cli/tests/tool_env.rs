//! The agent's shell gets an allow-listed environment on the real binary:
//! a variable the server was started with reaches `bash` only when node.toml
//! lists it in `env_pass`, and never when it is a `HARNESS_*` setting.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_rizzy-harness"))
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .env("HARNESS_LOG", "warn")
        .env("RIZZY_PROBE_LEAK", "leak-value-1")
        .env("RIZZY_PROBE_PASS", "pass-value-2")
        .env("HARNESS_PROBE_SECRET", "harness-value-3")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn harness");
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let addr = loop {
        let line = lines.next().expect("server exited").unwrap();
        if let Some(rest) = line.split("listening on http://").nth(1) {
            break rest.trim().to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    let token = std::fs::read_to_string(data.join("api-token")).unwrap();
    Server { child, addr, token }
}

impl Server {
    async fn post(&self, path: &str, body: Value) -> Value {
        let r = reqwest::Client::new()
            .post(format!("http://{}{path}", self.addr))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status();
        let v: Value = r.json().await.unwrap();
        assert!(status.is_success(), "{path}: {status} {v}");
        v
    }

    async fn events(&self, sid: &str) -> Vec<Value> {
        let v: Value = reqwest::Client::new()
            .get(format!(
                "http://{}/api/sessions/{sid}/events?limit=5000",
                self.addr
            ))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        v["events"].as_array().cloned().unwrap_or_default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn embedded_node_shell_sees_only_allowed_variables() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n\
             env_pass = [\"RIZZY_PROBE_P*\", \"HARNESS_*\"]\n",
            dir.path().display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 1\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
    let s = start(&data);
    let sid = s
        .post(
            "/api/sessions",
            json!({"cwd": ws.display().to_string(), "approval_mode": "auto", "agent_id": "coder"}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: env; echo end-of-env"}),
    )
    .await;
    let started = Instant::now();
    let output = loop {
        let evs = s.events(&sid).await;
        if let Some(f) = evs.iter().find(|e| e["kind"] == "tool_call_finished") {
            break f["payload"]["output"].as_str().unwrap_or("").to_string();
        }
        assert!(started.elapsed() < Duration::from_secs(30), "{evs:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(output.contains("end-of-env"), "{output}");
    assert!(output.contains("PATH="), "{output}");
    assert!(output.contains("RIZZY_PROBE_PASS=pass-value-2"), "{output}");
    assert!(!output.contains("leak-value-1"), "{output}");
    assert!(!output.contains("harness-value-3"), "{output}");
    assert!(!output.contains("HARNESS_"), "{output}");
}
