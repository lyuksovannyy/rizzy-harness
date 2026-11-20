//! Web tools in agentic chats, through the real binary (ADOPTION_PLAN T.3,
//! PLAN.md T89): `serve`'s embedded node registers `web_search` and
//! `web_fetch` from the server's `[web]`, node.toml's `denied_tools` switches
//! each off, the address rule stops a fetch of loopback, and `doctor` says
//! which tools a node's agentic chats have and from which setting.
//!
//! The only web is a fake on a loopback listener; no test reaches an outside
//! host. The rules themselves (URL provenance, redirects, secrets, modes,
//! sub-agents) are proved on a node in `crates/node/tests/web_tools.rs`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

#[path = "support/serve.rs"]
#[allow(dead_code)]
mod real;

/// A fake SearXNG on loopback: every answer is one result, a page of its
/// own. Returns its port, and the paths it was asked.
fn fake_web() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { return };
            let mut buf = [0u8; 4096];
            let n = c.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = head
                .lines()
                .next()
                .and_then(|l| l.split(' ').nth(1))
                .unwrap_or("/")
                .to_string();
            s2.lock().unwrap().push(path.clone());
            let body = if path.starts_with("/search") {
                json!({"results": [{"title": "Fake docs",
                    "url": format!("http://127.0.0.1:{port}/page"), "content": "a page"}]})
                .to_string()
            } else {
                "<html><title>x</title><body>a page</body></html>".to_string()
            };
            let _ = write!(
                c,
                "HTTP/1.1 200 OK\r\ncontent-type: {}\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{body}",
                if path.starts_with("/search") {
                    "application/json"
                } else {
                    "text/html"
                },
                body.len()
            );
        }
    });
    (port, seen)
}

/// The data directory `real::setup` makes, with `web` appended to
/// config.toml and `node_toml` to node.toml.
fn with(web: &str, node_toml: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let (dir, data, _ws) = real::setup();
    let cfg = data.join("config.toml");
    let mut c = std::fs::read_to_string(&cfg).unwrap();
    c.push_str(web);
    std::fs::write(&cfg, c).unwrap();
    let nt = data.join("node.toml");
    let mut t = std::fs::read_to_string(&nt).unwrap();
    // Top-level keys must come before the first table.
    t = format!("{node_toml}\n{t}");
    std::fs::write(&nt, t).unwrap();
    (dir, data)
}

/// One agentic chat on the embedded node, the scripted calls, and the
/// `(tool, ok, output)` of each result.
async fn run(s: &real::Serve, ws: &std::path::Path, steps: &[Value]) -> Vec<(bool, String)> {
    let sid = s
        .post(
            "/api/sessions",
            json!({"cwd": ws.display().to_string(), "approval_mode": "auto"}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(&sid, steps).await;
    let evs = s
        .wait_for(&sid, |e| e.iter().any(|e| e["kind"] == "turn_ended"))
        .await;
    evs.iter()
        .filter(|e| e["kind"] == "tool_call_finished")
        .map(|e| {
            (
                e["payload"]["ok"] == true,
                e["payload"]["output"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn call(tool: &str, input: Value) -> Value {
    json!({"tool": tool, "input": input})
}

/// `[web]` in config.toml reaches the embedded node's agentic chats: a
/// search runs and finds the fake's page; a fetch of a loopback address
/// (the person's own URL, so only the address rule can stop it) is refused
/// and never reaches the listener.
#[tokio::test(flavor = "multi_thread")]
async fn the_embedded_node_has_the_servers_web_tools() {
    let (port, seen) = fake_web();
    let (_dir, data) = with(
        &format!(
            "\n[web]\nsearch = \"searxng\"\nsearxng_url = \"http://127.0.0.1:{port}\"\nfetch = true\n"
        ),
        "",
    );
    let ws = data.parent().unwrap().join("ws");
    let s = real::serve(&data);
    let page = format!("http://127.0.0.1:{port}/page");
    let r = run(
        &s,
        &ws,
        &[
            call("web_search", json!({"query": "docs"})),
            call("web_fetch", json!({"url": page})),
            call(
                "web_fetch",
                json!({"url": format!("http://localhost.:{port}/page")}),
            ),
        ],
    )
    .await;
    assert_eq!(r.len(), 3, "{r:#?}\n{}", s.log());
    assert!(r[0].0 && r[0].1.starts_with("[S1] Fake docs"), "{:?}", r[0]);
    assert!(
        !r[1].0 && r[1].1.contains("private and local addresses"),
        "{:?}",
        r[1]
    );
    assert!(!r[2].0 && !r[2].1.contains("unknown tool"), "{:?}", r[2]);
    let paths = seen.lock().unwrap().clone();
    assert_eq!(paths.len(), 1, "only the search was asked: {paths:?}");
    assert!(paths[0].starts_with("/search"), "{paths:?}");
    // The start-up log says which tools and from where; no key.
    let log = s.log();
    assert!(
        log.contains("web tools") && log.contains("web_search") && log.contains("config.toml"),
        "{log}"
    );
}

/// With no `[web]` the server's chats have `web_fetch` (its default) and so
/// does the embedded node; `web_search` is not there. `fetch = false`
/// takes the fetch away too.
#[tokio::test(flavor = "multi_thread")]
async fn the_defaults_and_the_off_switch() {
    let (_dir, data) = with("", "");
    let ws = data.parent().unwrap().join("ws");
    let s = real::serve(&data);
    let r = run(
        &s,
        &ws,
        &[
            call("web_search", json!({"query": "x"})),
            call("web_fetch", json!({"url": "http://127.0.0.1:9/x"})),
        ],
    )
    .await;
    assert!(r[0].1.contains("unknown tool `web_search`"), "{r:#?}");
    assert!(!r[1].1.contains("unknown tool"), "{r:#?}");
    drop(s);

    let (_dir, data) = with("\n[web]\nfetch = false\n", "");
    let ws = data.parent().unwrap().join("ws");
    let s = real::serve(&data);
    let r = run(
        &s,
        &ws,
        &[call("web_fetch", json!({"url": "http://127.0.0.1:9/x"}))],
    )
    .await;
    assert!(r[0].1.contains("unknown tool `web_fetch`"), "{r:#?}");
}

/// node.toml's `denied_tools` refuses each tool with the policy's words.
#[tokio::test(flavor = "multi_thread")]
async fn denied_tools_refuses_them_on_the_embedded_node() {
    let (port, seen) = fake_web();
    let (_dir, data) = with(
        &format!("\n[web]\nsearch = \"searxng\"\nsearxng_url = \"http://127.0.0.1:{port}\"\n"),
        "denied_tools = [\"web_search\", \"web_fetch\"]",
    );
    let ws = data.parent().unwrap().join("ws");
    let s = real::serve(&data);
    let r = run(
        &s,
        &ws,
        &[
            call("web_search", json!({"query": "x"})),
            call("web_fetch", json!({"url": "http://127.0.0.1:9/x"})),
        ],
    )
    .await;
    for (ok, out) in &r {
        assert!(!ok && out.contains("denied by this node's policy"), "{out}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

/// `doctor` names the tools and the setting they come from; a key is never
/// printed. A directory that has joined a master (remote.toml) is a remote
/// node and reads node.toml's `[web]`, off unless it says otherwise.
#[test]
fn doctor_says_which_web_tools_a_node_has() {
    let exe = real::EXE;
    let doctor = |data: &std::path::Path, envs: &[(&str, &str)]| -> String {
        let out = std::process::Command::new(exe)
            .args(["--data-dir", data.to_str().unwrap(), "doctor"])
            .env("HARNESS_BRAVE_TEST_KEY", "brave-key-never-printed-1234")
            .envs(envs.iter().copied())
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let line = |out: &str| {
        out.lines()
            .find(|l| l.contains("web tools"))
            .unwrap_or_else(|| panic!("no web tools line:\n{out}"))
            .to_string()
    };
    // The embedded node: the server's `[web]`.
    let (_d, data) = with(
        "\n[web]\nsearch = \"brave\"\nbrave_key_env = \"HARNESS_BRAVE_TEST_KEY\"\nfetch = true\n",
        "",
    );
    let out = doctor(&data, &[]);
    let l = line(&out);
    assert!(
        l.starts_with("ok") && l.contains("web_search (brave)") && l.contains("web_fetch"),
        "{l}"
    );
    assert!(l.contains("config.toml"), "{l}");
    assert!(!out.contains("brave-key-never-printed"), "{out}");
    // A remote node: node.toml's `[web]`, off by default.
    let (_d, data) = with("\n[web]\nfetch = true\n", "");
    std::fs::write(
        data.join("remote.toml"),
        "url = \"https://master.invalid:7000\"\nfingerprint = \"x\"\nnode_id = \"n\"\n",
    )
    .unwrap();
    let l = line(&doctor(&data, &[]));
    assert!(l.contains("none") && l.contains("node.toml"), "{l}");
    let nt = data.join("node.toml");
    let mut t = std::fs::read_to_string(&nt).unwrap();
    t.push_str("\n[web]\nfetch = true\n");
    std::fs::write(&nt, t).unwrap();
    let l = line(&doctor(&data, &[]));
    assert!(
        l.contains("web_fetch") && !l.contains("web_search") && l.contains("node.toml"),
        "{l}"
    );
    // A key that is not there is a failure that names the variable only.
    std::fs::write(
        &nt,
        std::fs::read_to_string(&nt).unwrap().replace(
            "[web]\nfetch = true",
            "[web]\nsearch = \"brave\"\nbrave_key_env = \"HARNESS_NO_SUCH_KEY\"",
        ),
    )
    .unwrap();
    let l = line(&doctor(&data, &[]));
    assert!(
        l.starts_with("FAIL") && l.contains("HARNESS_NO_SUCH_KEY"),
        "{l}"
    );
}
