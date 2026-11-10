//! Phase 1 acceptance gate: kill the real process mid-session (SIGKILL, no
//! graceful shutdown), restart it, reconnect a UI client from its cursor, and
//! verify the client reaches exactly the server's state with no duplicated or
//! lost events.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_core::RawEvent;
use harness_core::event::TurnEndReason;
use harness_core::view::{Item, SessionView};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

struct Server {
    child: Option<Child>,
    addr: String,
    token: String,
}

fn start(data: &Path) -> Server {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rizzy-harness"))
        .args(["--data-dir", data.to_str().unwrap()])
        .args(["serve", "--bind", "127.0.0.1:0"])
        .env("HARNESS_LOG", "warn")
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
    // Keep draining stdout so the child never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    let token = std::fs::read_to_string(data.join("api-token")).unwrap();
    Server {
        child: Some(child),
        addr,
        token,
    }
}

impl Server {
    fn kill9(mut self) {
        let mut c = self.child.take().unwrap();
        c.kill().unwrap(); // SIGKILL on unix
        c.wait().unwrap();
    }

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

    async fn all_events(&self, sid: &str) -> Vec<RawEvent> {
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
        serde_json::from_value(v["events"].clone()).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws_connect(s: &Server, sid: &str, after_seq: u64) -> Ws {
    let mut req = format!("ws://{}/api/ws", s.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "authorization",
        format!("Bearer {}", s.token).parse().unwrap(),
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::Text(
        json!({"type": "subscribe", "session_id": sid, "after_seq": after_seq})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    ws
}

/// Read events until `stop` returns true for one of them (or the socket dies).
async fn read_until(ws: &mut Ws, out: &mut Vec<RawEvent>, mut stop: impl FnMut(&RawEvent) -> bool) {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(30), ws.next())
            .await
            .expect("timed out waiting for events");
        let Some(Ok(Message::Text(t))) = msg else {
            match msg {
                Some(Ok(_)) => continue,
                _ => return,
            }
        };
        let v: Value = serde_json::from_str(&t).unwrap();
        if v["type"] == "event" {
            let e: RawEvent = serde_json::from_value(v["event"].clone()).unwrap();
            let done = stop(&e);
            out.push(e);
            if done {
                return;
            }
        }
    }
}

fn assert_contiguous(evs: &[RawEvent]) {
    for (i, e) in evs.iter().enumerate() {
        assert_eq!(e.seq, i as u64 + 1, "gap or duplicate at index {i}");
    }
}

fn setup() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let ws = dir.path().join("ws");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!(
            "workspace_roots = [{:?}]\n",
            dir.path().display().to_string()
        ),
    )
    .unwrap();
    std::fs::write(
        data.join("config.toml"),
        "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
         mock_delay_ms = 30\n\n[[providers.models]]\nid = \"mock\"\n",
    )
    .unwrap();
    (dir, ws)
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_mid_stream_restart_and_replay_exactly() {
    let (dir, ws_dir) = setup();
    let data = dir.path().join("data");

    let s1 = start(&data);
    let sid = s1
        .post(
            "/api/sessions",
            json!({"cwd": ws_dir.display().to_string(), "title": "acceptance"}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut ws = ws_connect(&s1, &sid, 0).await;
    s1.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "slow: 400"}),
    )
    .await;

    // Let the turn stream for a while, then kill the process mid-turn.
    let mut seen = Vec::new();
    let mut deltas = 0;
    read_until(&mut ws, &mut seen, |e| {
        if e.kind == "assistant_delta" {
            deltas += 1;
        }
        deltas >= 8
    })
    .await;
    s1.kill9();
    // The client keeps whatever it had; the socket dies.
    read_until(&mut ws, &mut seen, |_| false).await;
    assert_contiguous(&seen);
    let cursor = seen.last().unwrap().seq;
    let before = SessionView::fold(&seen).unwrap();
    assert!(
        before.running_turn.is_some(),
        "killed too late: turn already ended"
    );

    // Restart and resume from the cursor.
    let s2 = start(&data);
    let mut ws = ws_connect(&s2, &sid, cursor).await;
    let mut after = Vec::new();
    read_until(&mut ws, &mut after, |e| e.kind == "turn_ended").await;

    let mut client = seen.clone();
    client.extend(after.iter().cloned());
    assert_contiguous(&client);
    assert_eq!(
        after.first().unwrap().seq,
        cursor + 1,
        "resume did not start at the cursor"
    );

    // The server's full log equals what the client assembled, and so does the fold.
    let server = s2.all_events(&sid).await;
    assert_eq!(server, client, "client and server logs differ");
    let view = SessionView::fold(&client).unwrap();
    assert_eq!(view, SessionView::fold(&server).unwrap());
    assert!(view.running_turn.is_none());
    assert!(matches!(
        view.items.last(),
        Some(Item::TurnEnd {
            reason: TurnEndReason::Interrupted,
            ..
        })
    ));

    // A fresh client from seq 0 gets the identical log.
    let mut fresh_ws = ws_connect(&s2, &sid, 0).await;
    let mut fresh = Vec::new();
    let last = server.last().unwrap().seq;
    read_until(&mut fresh_ws, &mut fresh, |e| e.seq == last).await;
    assert_eq!(fresh, server);

    // The session is usable after recovery, and live events keep flowing on
    // the resumed socket with no gap.
    s2.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "still alive"}),
    )
    .await;
    let mut more = Vec::new();
    read_until(&mut ws, &mut more, |e| e.kind == "turn_ended").await;
    client.extend(more);
    assert_contiguous(&client);
    let view = SessionView::fold(&client).unwrap();
    assert!(view.items.iter().any(
        |i| matches!(i, Item::Assistant { text, done: true, .. } if text == "You said: still alive")
    ));
    s2.kill9();
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_while_waiting_for_approval() {
    let (dir, ws_dir) = setup();
    let data = dir.path().join("data");
    let s1 = start(&data);
    let sid = s1
        .post(
            "/api/sessions",
            json!({"cwd": ws_dir.display().to_string()}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut ws = ws_connect(&s1, &sid, 0).await;
    s1.post(
        &format!("/api/sessions/{sid}/prompt"),
        json!({"text": "run: touch must-not-exist"}),
    )
    .await;
    let mut seen = Vec::new();
    read_until(&mut ws, &mut seen, |e| e.kind == "approval_requested").await;
    s1.kill9();

    let s2 = start(&data);
    let cursor = seen.last().unwrap().seq;
    let mut ws = ws_connect(&s2, &sid, cursor).await;
    read_until(&mut ws, &mut seen, |e| e.kind == "turn_ended").await;
    assert_contiguous(&seen);
    let view = SessionView::fold(&seen).unwrap();
    assert!(view.pending_approvals.is_empty());
    assert!(view.running_turn.is_none());
    assert!(!ws_dir.join("must-not-exist").exists());
    assert_eq!(s2.all_events(&sid).await, seen);
    s2.kill9();
}
