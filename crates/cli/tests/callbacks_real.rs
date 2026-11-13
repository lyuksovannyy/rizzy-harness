//! Callback turns (ADOPTION_PLAN T.8d step 3, docs/background-commands.md
//! §4.4, §7 tests 5 and 6, §13): a background command that ends while its
//! chat is idle resumes the agent once, started by the master and decided
//! by the node, within `callback_turns` and `callback_max_steps`.
//!
//! This file: the real binary (`serve`: the master's watcher and
//! admission, the embedded node, the demo provider) for what a person sees
//! and what the bounds do, over HTTP and JSON only. The node over the
//! protocol, with a recording provider, is `callbacks.rs`. The demo provider obeys the first quoted line of program output
//! in a callback turn's opening, as a steered model would: the command's
//! output is the attacker here.
//!
//! Commands are held on files the test creates (no time margins on this
//! slow disk). Temp folders are under the target directory.

#[path = "support/bwrap.rs"]
mod bwrap;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_tools::sandbox::{Sandbox, Settings};
use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("callbacks-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

/// The contained sandbox under `data`, or `None` (and a printed reason)
/// where it cannot run here; with `HARNESS_REQUIRE_BWRAP=1` it fails.
fn contained(data: &Path) -> Option<Arc<Sandbox>> {
    std::fs::create_dir_all(data.join("sandbox-tmp")).unwrap();
    let sb = Sandbox::new(
        PathBuf::from(EXE),
        &Settings::default(),
        Some(data.join("sandbox-tmp")),
        std::slice::from_ref(&data.to_path_buf()),
    )
    .unwrap();
    if let Err(e) = sb.probe(&data.join("sandbox-tmp/probe"), data) {
        assert!(!bwrap::required(), "Landlock does not work here: {e}");
        eprintln!("SKIPPED: Landlock does not work here: {e}");
        return None;
    }
    let sb = bwrap::contained(sb, &data.join("sandbox-tmp/probe-contain")).ok()?;
    Some(Arc::new(sb))
}

/// A held command: waits for `go` in the chat's folder, removes it, then
/// prints `out` (a file in the folder) and exits 1.
fn held(out: &str) -> String {
    format!("while [ ! -e go ]; do sleep 0.05; done; rm -f go; cat {out}; exit 1")
}

/// Held on `go` without taking it: every command held so ends on one `go`.
fn held_all(out: &str) -> String {
    format!("while [ ! -e go ]; do sleep 0.05; done; cat {out}; exit 1")
}

fn bg(cmd: &str, t: u64) -> Value {
    json!({"tool": "bash", "input": {"command": cmd, "background": true, "timeout_secs": t}})
}

// ---- the real binary ----

mod real {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    pub struct Serve {
        child: Option<Child>,
        pub addr: String,
        token: String,
        log: Arc<std::sync::Mutex<Vec<String>>>,
        /// The owner's login: (cookie, key).
        pub owner: (String, String),
    }

    impl Drop for Serve {
        fn drop(&mut self) {
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    /// A data folder with node.toml (`auto` allowed, `limits` added to
    /// `[limits]`) and the demo provider; the chat's folder `ws`.
    pub fn setup(limits: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir();
        let root = dir.path().canonicalize().unwrap();
        let (data, ws) = (root.join("data"), root.join("ws"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            data.join("node.toml"),
            format!(
                "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n\n[limits]\n{limits}\n",
                ws.display().to_string()
            ),
        )
        .unwrap();
        std::fs::write(
            data.join("config.toml"),
            "[[providers]]\nid = \"demo\"\nname = \"Demo\"\npreset = \"demo\"\nkind = \"mock\"\n\
             mock_delay_ms = 1\nmock_obeys_output = true\n\n[[providers.models]]\nid = \"mock\"\n",
        )
        .unwrap();
        (dir, data, ws)
    }

    pub async fn serve(data: &Path) -> Serve {
        let mut child = Command::new(EXE)
            .args([
                "--data-dir",
                data.to_str().unwrap(),
                "serve",
                "--bind",
                "127.0.0.1:0",
            ])
            .current_dir(data.parent().unwrap())
            .env("HARNESS_LOG", "info")
            .env("NO_COLOR", "1")
            .env_remove("HARNESS_JOIN_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let log: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
        let err = child.stderr.take().unwrap();
        let l2 = log.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                l2.lock().unwrap().push(line);
            }
        });
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let addr = loop {
            let line = lines.next().expect("serve ended").unwrap();
            if let Some(a) = line.split("listening on http://").nth(1) {
                break a.trim().to_string();
            }
        };
        std::thread::spawn(move || for _ in lines {});
        let token = std::fs::read_to_string(data.join("api-token"))
            .unwrap()
            .trim()
            .to_string();
        let mut s = Serve {
            child: Some(child),
            addr,
            token,
            log,
            owner: (String::new(), String::new()),
        };
        s.owner = s.account("owner", "admin").await;
        s
    }

    impl Serve {
        pub fn log(&self) -> String {
            self.log.lock().unwrap().join("\n")
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}{path}", self.addr)
        }

        pub async fn token_post(&self, path: &str, body: Value) -> (reqwest::StatusCode, Value) {
            let r = reqwest::Client::new()
                .post(self.url(path))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await
                .unwrap();
            (r.status(), r.json().await.unwrap_or(Value::Null))
        }

        /// A logged-in account `name` with a password of its own: (cookie,
        /// login key).
        pub async fn account(&self, name: &str, role: &str) -> (String, String) {
            let c = reqwest::Client::new();
            let (st, v) = self
                .token_post(
                    "/api/users",
                    json!({"username": name, "role": role, "can_agentic": true}),
                )
                .await;
            assert!(st.is_success(), "{st} {v}");
            let pw = v["password"].as_str().unwrap().to_string();
            let r = c
                .post(self.url("/api/login"))
                .json(&json!({"username": name, "password": pw}))
                .send()
                .await
                .unwrap();
            let cookie = |r: &reqwest::Response| {
                r.headers()["set-cookie"]
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_string()
            };
            let ck = cookie(&r);
            let key = r.json::<Value>().await.unwrap()["login_key"]
                .as_str()
                .unwrap()
                .to_string();
            let r = c
                .post(self.url("/api/me/password"))
                .header("cookie", &ck)
                .header("x-harness-login", &key)
                .header("origin", format!("http://{}", self.addr))
                .json(&json!({"current": pw, "new": "a password of my own"}))
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success(), "{}", r.status());
            let ck = cookie(&r);
            let key = r.json::<Value>().await.unwrap()["login_key"]
                .as_str()
                .unwrap()
                .to_string();
            (ck, key)
        }

        /// A request as the owner's page sends it.
        pub async fn owner_req(
            &self,
            method: reqwest::Method,
            path: &str,
            body: Option<Value>,
        ) -> (reqwest::StatusCode, Value) {
            let mut rb = reqwest::Client::new()
                .request(method, self.url(path))
                .header("cookie", &self.owner.0)
                .header("x-harness-login", &self.owner.1)
                .header("origin", format!("http://{}", self.addr));
            if let Some(b) = body {
                rb = rb.json(&b);
            }
            let r = rb.send().await.unwrap();
            (r.status(), r.json().await.unwrap_or(Value::Null))
        }

        /// The owner's agentic chat in `ws`, in `mode`.
        pub async fn chat(&self, ws: &Path, mode: &str) -> String {
            let (st, v) = self
                .owner_req(
                    reqwest::Method::POST,
                    "/api/sessions",
                    Some(json!({"cwd": ws.display().to_string(), "approval_mode": mode})),
                )
                .await;
            assert!(st.is_success(), "{st} {v}");
            v["session_id"].as_str().unwrap().to_string()
        }

        /// The owner's message.
        pub async fn say(&self, sid: &str, text: &str) -> (reqwest::StatusCode, Value) {
            self.owner_req(
                reqwest::Method::POST,
                &format!("/api/sessions/{sid}/prompt"),
                Some(json!({"text": text})),
            )
            .await
        }

        pub async fn events(&self, sid: &str) -> Vec<Value> {
            let (_, v) = self
                .owner_req(
                    reqwest::Method::GET,
                    &format!("/api/sessions/{sid}/events?limit=5000"),
                    None,
                )
                .await;
            v["events"].as_array().cloned().unwrap_or_default()
        }

        pub async fn wait_for(&self, sid: &str, pred: impl Fn(&[Value]) -> bool) -> Vec<Value> {
            let by = Instant::now() + Duration::from_secs(90);
            loop {
                let evs = self.events(sid).await;
                if pred(&evs) {
                    return evs;
                }
                assert!(
                    Instant::now() < by,
                    "not found in {sid}: {:#?}\n{}",
                    evs.iter().rev().take(8).collect::<Vec<_>>(),
                    self.log()
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    pub fn count(evs: &[Value], kind: &str) -> usize {
        evs.iter().filter(|e| e["kind"] == kind).count()
    }

    /// Callback turns in the log: each one's ids.
    pub fn callbacks(evs: &[Value]) -> Vec<Value> {
        evs.iter()
            .filter(|e| e["kind"] == "turn_started" && !e["payload"]["cause"].is_null())
            .map(|e| e["payload"]["cause"]["callback"].clone())
            .collect()
    }
}

/// The bg id of the `n`-th `background_started` (1-based).
fn nth_started(evs: &[Value], n: usize) -> Option<String> {
    evs.iter()
        .filter(|e| e["kind"] == "background_started")
        .nth(n - 1)
        .and_then(|e| e["payload"]["bg_id"].as_str().map(str::to_string))
}

/// §4.4 on the real binary: an end while the chat is idle resumes the agent
/// once, with no person's message in the log, and the chat says what
/// started it; the owner's attention list holds it until they look; the
/// chat list showed the command running. Two ends at once are one turn.
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_end_resumes_the_agent_once_and_two_ends_make_one_turn() {
    let (_dir, data, ws) = real::setup("");
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let s = real::serve(&data).await;
    let sid = s.chat(&ws, "auto").await;
    std::fs::write(ws.join("a.txt"), "first result\n").unwrap();
    std::fs::write(ws.join("b.txt"), "second result\n").unwrap();
    let script = json!([bg(&held_all("a.txt"), 120), bg(&held_all("b.txt"), 120)]);
    let (st, v) = s.say(&sid, &format!("script: {script}")).await;
    assert!(st.is_success(), "{st} {v}");
    let evs = s
        .wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    assert_eq!(real::count(&evs, "background_started"), 2, "{evs:#?}");
    // The chat list says a background command runs there.
    let (_, list) = s
        .owner_req(reqwest::Method::GET, "/api/sessions", None)
        .await;
    let row = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_id"] == sid.as_str())
        .unwrap();
    assert_eq!(row["background"], 2, "{row}");
    // Both end on one `go`, while the chat is idle.
    let users_before = real::count(&evs, "user_message");
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s
        .wait_for(&sid, |e| {
            real::count(e, "background_ended") == 2
                && !real::callbacks(e).is_empty()
                && real::count(e, "turn_ended") == 1 + real::callbacks(e).len()
        })
        .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let evs2 = s.events(&sid).await;
    let cbs = real::callbacks(&evs2);
    // Exactly once each: every end is named by one callback turn, or was
    // a note at a step of the turn running when it was recorded. (That
    // two ends recorded before the turn starts are one turn, with no race
    // in the test, is the node's test and the master's.)
    let mut named: Vec<String> = cbs
        .iter()
        .flat_map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
        })
        .collect();
    named.extend(
        evs2.iter()
            .filter(|e| e["kind"] == "background_ended" && e["payload"]["delivered"] == true)
            .map(|e| e["payload"]["bg_id"].as_str().unwrap().to_string()),
    );
    named.sort();
    assert_eq!(named, vec!["bg-1", "bg-2"], "{cbs:?}");
    assert_eq!(cbs.len(), real::count(&evs2, "turn_ended") - 1, "{evs2:#?}");
    assert_eq!(
        real::count(&evs2, "user_message"),
        users_before,
        "a callback turn has no person's message"
    );
    // Its answer: the demo provider read the quoted output it was sent.
    let answer = evs
        .iter()
        .rev()
        .find(|e| e["kind"] == "assistant_message")
        .unwrap();
    assert!(
        answer["payload"]["text"]
            .as_str()
            .unwrap()
            .contains("result"),
        "{answer}"
    );
    // The owner's attention list holds it until they look.
    let (_, att) = s
        .owner_req(reqwest::Method::GET, "/api/attention", None)
        .await;
    let item = att["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["session_id"] == sid.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("no attention item: {att}"));
    assert_eq!(item["reason"], "resumed", "{item}");
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .contains("the agent was resumed without you"),
        "{item}"
    );
    let (st, _) = s
        .owner_req(
            reqwest::Method::POST,
            &format!("/api/sessions/{sid}/seen"),
            Some(json!({"seq": item["seq"]})),
        )
        .await;
    assert!(st.is_success());
    let (_, att) = s
        .owner_req(reqwest::Method::GET, "/api/attention", None)
        .await;
    assert!(
        !att["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["session_id"] == sid.as_str()),
        "{att}"
    );
    let (_, list) = s
        .owner_req(reqwest::Method::GET, "/api/sessions", None)
        .await;
    let row = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_id"] == sid.as_str())
        .unwrap();
    assert_eq!(row["background"], 0, "{row}");
}

/// §4.4 "The step limit" on the real binary: a callback turn may start a
/// background command, whose end spends the same budget; the fourth end in
/// a row is not resumed and the chat says why; only the person's message
/// refills it. The command's output steers the demo provider into each
/// new start: the worst case of a steered agent in `auto`.
#[tokio::test(flavor = "multi_thread")]
async fn a_fourth_callback_in_a_row_is_refused_and_a_persons_message_refills() {
    let (_dir, data, ws) = real::setup("");
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let s = real::serve(&data).await;
    let sid = s.chat(&ws, "auto").await;
    // Each command waits for `go`, then prints a line that makes the model
    // start the same command again.
    let again = json!([bg(&held("loop.txt"), 120)]);
    std::fs::write(ws.join("loop.txt"), format!("script: {again}\n")).unwrap();
    let (st, _) = s.say(&sid, &format!("script: {again}")).await;
    assert!(st.is_success());
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    for n in 1..=3 {
        // Released only once it runs in the background (else it ends in
        // its first wait, an ordinary result).
        s.wait_for(&sid, |e| nth_started(e, n).is_some()).await;
        std::fs::write(ws.join("go"), "").unwrap();
        s.wait_for(&sid, |e| {
            real::callbacks(e).len() == n && real::count(e, "turn_ended") == n + 1
        })
        .await;
    }
    // The third callback turn started bg-4; its end is not resumed.
    let evs = s.wait_for(&sid, |e| nth_started(e, 4).is_some()).await;
    assert_eq!(nth_started(&evs, 4).as_deref(), Some("bg-4"));
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s
        .wait_for(&sid, |e| real::count(e, "callback_refused") == 1)
        .await;
    let refused = evs
        .iter()
        .find(|e| e["kind"] == "callback_refused")
        .unwrap();
    assert_eq!(refused["payload"]["bg_ids"], json!(["bg-4"]));
    assert_eq!(
        refused["payload"]["reason"],
        "3 turns in a row without you (node limit)"
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    let evs = s.events(&sid).await;
    assert_eq!(real::callbacks(&evs).len(), 3, "no fourth turn");
    // The owner's attention list says it too.
    let (_, att) = s
        .owner_req(reqwest::Method::GET, "/api/attention", None)
        .await;
    let item = att["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["session_id"] == sid.as_str())
        .cloned()
        .unwrap();
    assert_eq!(item["reason"], "not_resumed");
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .contains("3 turns in a row without you (node limit)"),
        "{item}"
    );
    // The person's message refills it: a new end resumes the agent again.
    let (st, _) = s.say(&sid, &format!("script: {again}")).await;
    assert!(st.is_success());
    s.wait_for(&sid, |e| nth_started(e, 5).is_some()).await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 5)
        .await;
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s.wait_for(&sid, |e| real::callbacks(e).len() == 4).await;
    assert_eq!(real::callbacks(&evs)[3], json!(["bg-5"]));
}

/// A callback turn's steps stop at `callback_max_steps`, below `max_steps`,
/// and the chat says which key stopped it; `callback_turns = 0` starts no
/// turn and says nothing (off is silent).
#[tokio::test(flavor = "multi_thread")]
async fn the_step_cap_holds_and_callback_turns_0_starts_none() {
    let (_dir, data, ws) = real::setup("callback_max_steps = 2\n");
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let s = real::serve(&data).await;
    let sid = s.chat(&ws, "auto").await;
    let many = json!([
        {"tool": "bash", "input": {"command": "true"}},
        {"tool": "bash", "input": {"command": "true"}},
        {"tool": "bash", "input": {"command": "true"}},
        {"tool": "bash", "input": {"command": "true"}},
    ]);
    std::fs::write(ws.join("many.txt"), format!("script: {many}\n")).unwrap();
    let (st, _) = s
        .say(
            &sid,
            &format!("script: {}", json!([bg(&held("many.txt"), 120)])),
        )
        .await;
    assert!(st.is_success());
    s.wait_for(&sid, |e| nth_started(e, 1).is_some()).await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s
        .wait_for(&sid, |e| {
            real::callbacks(e).len() == 1 && real::count(e, "turn_ended") == 2
        })
        .await;
    let at = evs
        .iter()
        .position(|e| e["kind"] == "turn_started" && !e["payload"]["cause"].is_null())
        .unwrap();
    let turn = &evs[at..];
    assert_eq!(real::count(turn, "assistant_message"), 2, "{turn:#?}");
    let err = turn.iter().find(|e| e["kind"] == "error").unwrap();
    assert!(
        err["payload"]["message"]
            .as_str()
            .unwrap()
            .contains("callback_max_steps"),
        "{err}"
    );
    drop(s);

    let (_dir, data, ws) = real::setup("callback_turns = 0\n");
    let s = real::serve(&data).await;
    let sid = s.chat(&ws, "auto").await;
    std::fs::write(ws.join("a.txt"), "x\n").unwrap();
    let (st, _) = s
        .say(
            &sid,
            &format!("script: {}", json!([bg(&held("a.txt"), 120)])),
        )
        .await;
    assert!(st.is_success());
    s.wait_for(&sid, |e| nth_started(e, 1).is_some()).await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    std::fs::write(ws.join("go"), "").unwrap();
    s.wait_for(&sid, |e| real::count(e, "background_ended") == 1)
        .await;
    // Off is silent: the node does not announce callbacks, the server asks
    // nothing, the chat records no refusal and the owner gets no row; the
    // end waits for the next message, as before step 3.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let evs = s.events(&sid).await;
    assert!(real::callbacks(&evs).is_empty());
    assert_eq!(real::count(&evs, "callback_refused"), 0, "{evs:#?}");
    let (_, att) = s
        .owner_req(reqwest::Method::GET, "/api/attention", None)
        .await;
    assert!(
        !att["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["session_id"] == sid.as_str()),
        "{att}"
    );
}

/// §4.6 in `ask`: a callback turn's call parks on its card with no timeout,
/// and the person's new message is refused "a turn is already running"
/// until they answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_callback_turn_in_ask_mode_parks_on_its_card() {
    let (_dir, data, ws) = real::setup("");
    if contained(&data.join("probe")).is_none() {
        return;
    }
    let s = real::serve(&data).await;
    let sid = s.chat(&ws, "ask").await;
    std::fs::write(ws.join("cmd.txt"), "run: touch steered\n").unwrap();
    let (st, _) = s
        .say(
            &sid,
            &format!("script: {}", json!([bg(&held("cmd.txt"), 120)])),
        )
        .await;
    assert!(st.is_success());
    // The person's own start asks first.
    let evs = s
        .wait_for(&sid, |e| real::count(e, "approval_requested") == 1)
        .await;
    let aid = evs
        .iter()
        .find(|e| e["kind"] == "approval_requested")
        .unwrap()["payload"]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, v) = s
        .owner_req(
            reqwest::Method::POST,
            &format!("/api/sessions/{sid}/approvals/{aid}"),
            Some(json!({"decision": "allow"})),
        )
        .await;
    assert!(st.is_success(), "{st} {v}");
    s.wait_for(&sid, |e| nth_started(e, 1).is_some()).await;
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 1)
        .await;
    std::fs::write(ws.join("go"), "").unwrap();
    let evs = s
        .wait_for(&sid, |e| {
            real::callbacks(e).len() == 1 && real::count(e, "approval_requested") == 2
        })
        .await;
    assert_eq!(real::count(&evs, "turn_ended"), 1, "it waits on the card");
    assert!(!ws.join("steered").exists());
    let (st, v) = s.say(&sid, "hello").await;
    assert!(!st.is_success(), "{st} {v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap_or("")
            .contains("a turn is already running"),
        "{v}"
    );
    let aid = evs
        .iter()
        .rev()
        .find(|e| e["kind"] == "approval_requested")
        .unwrap()["payload"]["approval_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, _) = s
        .owner_req(
            reqwest::Method::POST,
            &format!("/api/sessions/{sid}/approvals/{aid}"),
            Some(json!({"decision": "deny"})),
        )
        .await;
    assert!(st.is_success());
    s.wait_for(&sid, |e| real::count(e, "turn_ended") == 2)
        .await;
    assert!(!ws.join("steered").exists());
}
