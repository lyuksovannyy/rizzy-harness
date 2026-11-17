//! The board tools end to end (ADOPTION_PLAN X.32, PLAN.md T78): the real
//! binary with its demo (scripted) provider, a project chat of the
//! built-in Assistant: `board_edit` adds a card through the master,
//! `board` lists it, and the card on the project's board says which agent
//! made it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EXE: &str = env!("CARGO_BIN_EXE_rizzy-harness");

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("board-e2e-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
}

mod real {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    pub struct Serve {
        child: Option<Child>,
        pub addr: String,
        token: String,
        log: Arc<std::sync::Mutex<Vec<String>>>,
        /// A member's login, when the test acts as one: (cookie, key).
        /// Chats made with the API token have no owner and never call back.
        owner: Option<(String, String)>,
    }

    impl Drop for Serve {
        fn drop(&mut self) {
            if let Some(mut c) = self.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }

    pub fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempdir();
        let root = dir.path().canonicalize().unwrap();
        let (data, ws) = (root.join("data"), root.join("ws"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(
            data.join("node.toml"),
            format!(
                "workspace_roots = [{:?}]\nmax_approval_mode = \"auto\"\n\n[skills]\nroots = []\n",
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

    pub fn serve(data: &Path) -> Serve {
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
        Serve {
            child: Some(child),
            addr,
            token,
            log,
            owner: None,
        }
    }

    impl Serve {
        /// Act as a new member `name` with a password of its own from now on.
        pub async fn as_member(&mut self, name: &str) {
            let c = reqwest::Client::new();
            let v = self
                .post(
                    "/api/users",
                    json!({"username": name, "role": "admin", "exec_ceiling": "host"}),
                )
                .await;
            let pw = v["password"].as_str().unwrap().to_string();
            let r = c
                .post(format!("http://{}/api/login", self.addr))
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
                .post(format!("http://{}/api/me/password", self.addr))
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
            self.owner = Some((ck, key));
        }

        fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
            let rb = reqwest::Client::new().request(method, format!("http://{}{path}", self.addr));
            match &self.owner {
                Some((ck, key)) => rb
                    .header("cookie", ck)
                    .header("x-harness-login", key)
                    .header("origin", format!("http://{}", self.addr)),
                None => rb.bearer_auth(&self.token),
            }
        }

        pub fn log(&self) -> String {
            self.log.lock().unwrap().join("\n")
        }

        pub async fn post(&self, path: &str, body: Value) -> Value {
            let r = self
                .req(reqwest::Method::POST, path)
                .json(&body)
                .send()
                .await
                .unwrap();
            let st = r.status();
            let v = r.json().await.unwrap_or(Value::Null);
            assert!(st.is_success(), "{path}: {st} {v}");
            v
        }

        pub async fn events(&self, sid: &str) -> Vec<Value> {
            let r = self
                .req(
                    reqwest::Method::GET,
                    &format!("/api/sessions/{sid}/events?limit=5000"),
                )
                .send()
                .await
                .unwrap();
            let v: Value = r.json().await.unwrap();
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
                    "not found in {sid}: {evs:#?}\n{}",
                    self.log()
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        pub async fn get(&self, path: &str) -> Value {
            let r = self.req(reqwest::Method::GET, path).send().await.unwrap();
            let st = r.status();
            let v = r.json().await.unwrap_or(Value::Null);
            assert!(st.is_success(), "{path}: {st} {v}");
            v
        }

        pub async fn prompt(&self, sid: &str, steps: &[Value]) {
            self.post(
                &format!("/api/sessions/{sid}/prompt"),
                json!({"text": format!("script: {}", json!(steps))}),
            )
            .await;
        }
    }
}

fn count(evs: &[Value], kind: &str) -> usize {
    evs.iter().filter(|e| e["kind"] == kind).count()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_adds_and_lists_a_card_of_its_projects_board() {
    let (_dir, data, _ws) = real::setup();
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let p = s.post("/api/projects", json!({"name": "Launch"})).await;
    let pid = p["project_id"].as_str().unwrap().to_string();
    let sid = s
        .post(
            "/api/sessions",
            json!({"profile": "chat", "project_id": pid}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(
        &sid,
        &[
            json!({"tool": "board_edit", "input": {"action": "add", "title": "Write the release notes"}}),
            json!({"tool": "board", "input": {}}),
        ],
    )
    .await;
    let evs = s.wait_for(&sid, |e| count(e, "turn_ended") == 1).await;
    let results: Vec<&Value> = evs
        .iter()
        .filter(|e| e["kind"] == "tool_call_finished")
        .map(|e| &e["payload"])
        .collect();
    assert_eq!(results.len(), 2, "{evs:#?}\n{}", s.log());
    assert_eq!(results[0]["ok"], true, "{:#?}", results[0]);
    assert!(
        results[0]["output"]
            .as_str()
            .unwrap()
            .starts_with("Added card ")
    );
    let listed = results[1]["output"].as_str().unwrap();
    assert!(
        listed.contains(
            "\"Write the release notes\", made by agent \"Assistant\", linked to this chat"
        ),
        "{listed}"
    );
    let board = s.get(&format!("/api/projects/{pid}/board")).await;
    let card = &board["columns"][0]["cards"][0];
    assert_eq!(card["title"], "Write the release notes");
    assert_eq!(card["agent"], "Assistant");
    assert_eq!(card["chat"]["session_id"], sid.as_str());
}
