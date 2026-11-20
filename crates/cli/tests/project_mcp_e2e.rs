//! A repository's own MCP servers end to end (ADOPTION_PLAN X.36, PLAN.md
//! T82): the real binary (`serve`, its embedded node, real bubblewrap)
//! with node.toml `[mcp] project_servers = "ask"`. A member lists a
//! project folder's `.mcp.json` over HTTP, approves one entry, and a
//! sandboxed chat in the folder starts the approved copy inside the
//! chat's containment: its own PID namespace, its `env` value on the
//! server's own process alone (no other process of the machine has it in
//! its command line or environment: not the guard, bubblewrap or
//! `sandbox-exec`). The list shows the value to the person approving (it
//! is repository content and part of what runs); the harness writes it
//! into no event and no line of the server's log. The chat's shell can
//! neither read nor write the node's approved copies; a `host` chat starts
//! an entry without `env` values as the node's user (the owner, 2026-11-20)
//! and refuses one with values.
//!
//! Needs bubblewrap and python3; with `HARNESS_REQUIRE_BWRAP=1` a node that
//! offers no sandboxed chats fails the test.

#[path = "support/bwrap.rs"]
#[allow(dead_code)]
mod bwrap;
#[path = "support/serve.rs"]
mod real;

use serde_json::{Value, json};

const FAKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tools/tests/support/fake_mcp.py"
);
const CANARY: &str = "canary-e2e-5512-qq";

fn results(evs: &[Value]) -> Vec<(bool, String)> {
    evs.iter()
        .filter(|e| e["kind"] == "tool_call_finished")
        .map(|e| {
            (
                e["payload"]["ok"] == true,
                e["payload"]["output"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

/// Every process of this machine (that this user can read) whose command
/// line or environment holds the canary: pid, command line, environment.
fn with_canary() -> Vec<(i32, String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
            .unwrap_or_default();
        let env = std::fs::read(format!("/proc/{pid}/environ"))
            .map(|b| String::from_utf8_lossy(&b).replace('\0', "\n"))
            .unwrap_or_default();
        if cmd.contains(CANARY) || env.contains(CANARY) {
            out.push((pid, cmd, env));
        }
    }
    out
}

/// Processes of this machine whose command line holds `marker`: their pid,
/// command line and environment.
fn processes(marker: &str) -> Vec<(i32, String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let cmd = String::from_utf8_lossy(&cmd).replace('\0', " ");
        if !cmd.contains(marker) {
            continue;
        }
        let env = std::fs::read(format!("/proc/{pid}/environ"))
            .map(|b| String::from_utf8_lossy(&b).replace('\0', "\n"))
            .unwrap_or_default();
        out.push((pid, cmd, env));
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approved_repository_server_runs_inside_the_chats_sandbox() {
    let (_dir, data, ws) = real::setup();
    let toml = std::fs::read_to_string(data.join("node.toml")).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!("{toml}\n[mcp]\nproject_servers = \"ask\"\n\n[execution]\nhost = true\n"),
    )
    .unwrap();
    std::fs::copy(FAKE, ws.join("fake_mcp.py")).unwrap();
    let marker = format!("pmcp-{}", harness_core::new_id());
    std::fs::write(
        ws.join(".mcp.json"),
        json!({"mcpServers": {"envy": {
            "command": "python3",
            "args": ["fake_mcp.py", "--marker", marker],
            "env": {"MY_TOKEN": CANARY},
        }}})
        .to_string(),
    )
    .unwrap();
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let nodes = s.get("/api/nodes").await;
    let sandboxed = nodes["nodes"].as_array().unwrap().iter().any(|n| {
        n["exec_modes"]
            .as_array()
            .is_some_and(|m| m.iter().any(|x| x == "sandbox"))
    });
    if !sandboxed {
        assert!(
            !bwrap::required(),
            "the node offers no sandboxed chats: {nodes}\n{}",
            s.log()
        );
        eprintln!("SKIPPED: no sandboxed chats here");
        return;
    }
    let p = s.post("/api/projects", json!({"name": "Repo"})).await;
    let pid = p["project_id"].as_str().unwrap().to_string();
    s.patch(
        &format!("/api/projects/{pid}"),
        json!({"settings": {"node_id": "local", "cwd": ws.display().to_string()}}),
    )
    .await;

    // The list: the entry in full, its env names, never its value.
    let (st, text) = s
        .raw(
            reqwest::Method::GET,
            &format!("/api/projects/{pid}/mcp"),
            None,
        )
        .await;
    assert!(st.is_success(), "{st} {text}\n{}", s.log());
    let l: Value = serde_json::from_str(&text).unwrap();
    let e = &l["entries"][0];
    assert_eq!(e["name"], "envy");
    assert_eq!(e["state"], "new");
    assert_eq!(e["env"], json!([{"name": "MY_TOKEN", "value": CANARY}]));
    let digest = e["digest"].as_str().unwrap().to_string();
    let (st, text) = s
        .raw(
            reqwest::Method::POST,
            &format!("/api/projects/{pid}/mcp/approve"),
            Some(json!({"name": "envy", "digest": digest})),
        )
        .await;
    assert!(st.is_success(), "{st} {text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["entries"][0]["state"],
        "approved"
    );

    let copies = data.join("project-mcp.json");
    let chat = |mode: &str| {
        json!({"project_id": pid, "profile": "agentic", "node_id": "local",
               "cwd": ws.display().to_string(), "exec_mode": mode, "approval_mode": "auto"})
    };
    let sid = s.post("/api/sessions", chat("sandbox")).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(
        &sid,
        &[
            json!({"tool": "mcp__envy__pid", "input": {}}),
            json!({"tool": "bash", "input": {"command": "true"}}),
            json!({"tool": "bash", "input": {"command": format!("cat {}", copies.display())}}),
            json!({"tool": "bash", "input": {"command": format!("echo x >> {}", copies.display())}}),
        ],
    )
    .await;
    let evs = s
        .wait_for(&sid, |e| e.iter().any(|e| e["kind"] == "turn_ended"))
        .await;
    let out = results(&evs);
    assert_eq!(out.len(), 4, "{evs:#?}\n{}", s.log());
    // Its own PID namespace: the chat's containment.
    assert!(out[0].0, "{out:?}\n{}", s.log());
    let ns = std::fs::read_link("/proc/self/ns/pid").unwrap();
    assert!(
        !out[0].1.contains(&ns.to_string_lossy().to_string()),
        "the server shares the host's PID namespace: {}",
        out[0].1
    );
    // The value reached the server's own process, and no other process
    // of this machine has it, in its command line or its environment (the
    // guard, both bubblewrap processes, `sandbox-exec` before its exec).
    let procs = processes(&marker);
    assert!(procs.len() >= 2, "no guard or no server: {procs:#?}");
    let holders = with_canary();
    assert!(
        holders.len() == 1
            && holders[0].1.starts_with("python3 ")
            && holders[0].1.contains(&marker)
            && !holders[0].1.contains(CANARY)
            && holders[0].2.contains(&format!("MY_TOKEN={CANARY}")),
        "{holders:#?}"
    );
    // The shell reaches neither the approved copies nor the store.
    assert!(!out[2].1.contains(CANARY), "{}", out[2].1);
    assert!(
        !out[2].0 || out[2].1.contains("No such file"),
        "{:?}",
        out[2]
    );
    assert!(
        !std::fs::read_to_string(&copies).unwrap().contains("\nx"),
        "the chat wrote the approved copies"
    );

    // A host chat has no bubblewrap: the entry with `env` values is not
    // started there, and the chat is told which server and why.
    let host = s.post("/api/sessions", chat("host")).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    s.prompt(&host, &[json!({"tool": "mcp__envy__pid", "input": {}})])
        .await;
    let evs = s
        .wait_for(&host, |e| e.iter().any(|e| e["kind"] == "turn_ended"))
        .await;
    let out = results(&evs);
    assert!(
        !out[0].0 && out[0].1.contains("unknown tool"),
        "{out:?}\n{}",
        s.log()
    );
    assert!(evs.iter().any(|e| {
        e["kind"] == "error"
            && e["payload"]["message"]
                .as_str()
                .is_some_and(|m| m.contains("`envy`") && m.contains("a host chat never has one"))
    }));

    // Nothing of the value in any event or in the server's log.
    for sid in [&sid, &host] {
        let all = serde_json::to_string(&s.events(sid).await).unwrap();
        assert!(!all.contains(CANARY));
    }
    assert!(!s.log().contains(CANARY), "{}", s.log());
    assert!(
        s.log().contains("project MCP server approved"),
        "{}",
        s.log()
    );
    drop(s);
    // The server went with the node.
    let by = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !processes(&marker).is_empty() {
        assert!(std::time::Instant::now() < by, "{:#?}", processes(&marker));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// The owner's decision of 2026-11-20: a `host` chat starts an approved
/// repository server, as the node's user, in the node's own PID namespace.
/// What it leaves behind is what T47 says for node.toml's servers: the
/// node's `kill -9` closes its input, so a server that reads its input
/// ends with it, and one that does not stays (until stopped here by its
/// pid); nothing of ours reaps it.
#[tokio::test(flavor = "multi_thread")]
async fn a_host_chat_runs_an_approved_repository_server_and_what_it_leaves() {
    let (_dir, data, ws) = real::setup();
    let toml = std::fs::read_to_string(data.join("node.toml")).unwrap();
    std::fs::write(
        data.join("node.toml"),
        format!("{toml}\n[mcp]\nproject_servers = \"ask\"\n\n[execution]\nhost = true\n"),
    )
    .unwrap();
    std::fs::copy(FAKE, ws.join("fake_mcp.py")).unwrap();
    let id = harness_core::new_id();
    let (ends, stays) = (format!("pmcp-ends-{id}"), format!("pmcp-stays-{id}"));
    std::fs::write(
        ws.join(".mcp.json"),
        json!({"mcpServers": {
            "ends": {"command": "python3", "args": ["fake_mcp.py", "--marker", ends]},
            "stays": {"command": "python3",
                      "args": ["fake_mcp.py", "--marker", stays, "--linger", "120"]},
        }})
        .to_string(),
    )
    .unwrap();
    let mut s = real::serve(&data);
    s.as_member("owner").await;
    let p = s.post("/api/projects", json!({"name": "Repo"})).await;
    let pid = p["project_id"].as_str().unwrap().to_string();
    s.patch(
        &format!("/api/projects/{pid}"),
        json!({"settings": {"node_id": "local", "cwd": ws.display().to_string()}}),
    )
    .await;
    let l = s.get(&format!("/api/projects/{pid}/mcp")).await;
    for e in l["entries"].as_array().unwrap() {
        let (st, text) = s
            .raw(
                reqwest::Method::POST,
                &format!("/api/projects/{pid}/mcp/approve"),
                Some(json!({"name": e["name"], "digest": e["digest"]})),
            )
            .await;
        assert!(st.is_success(), "{st} {text}\n{}", s.log());
    }
    let host = s
        .post(
            "/api/sessions",
            json!({"project_id": pid, "profile": "agentic", "node_id": "local",
                   "cwd": ws.display().to_string(), "exec_mode": "host",
                   "approval_mode": "auto"}),
        )
        .await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    // A turn that offers MCP tools starts the chat's servers.
    s.post(
        &format!("/api/sessions/{host}/prompt"),
        json!({"text": "hello"}),
    )
    .await;
    let evs = s
        .wait_for(&host, |e| e.iter().any(|e| e["kind"] == "turn_ended"))
        .await;
    assert!(
        !evs.iter().any(|e| e["kind"] == "error"),
        "{evs:#?}\n{}",
        s.log()
    );
    let by = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while processes(&ends).is_empty() || processes(&stays).is_empty() {
        assert!(
            std::time::Instant::now() < by,
            "a host chat did not start both: {:#?}\n{}",
            (processes(&ends), processes(&stays)),
            s.log()
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // As the node's user, in no sandbox: no bubblewrap or guard above it.
    for m in [&ends, &stays] {
        let procs = processes(m);
        assert_eq!(procs.len(), 1, "{procs:#?}");
        assert!(procs[0].1.starts_with("python3 "), "{procs:#?}");
    }
    let stay = processes(&stays)[0].0;
    // `kill -9` of the node (the test's own `serve` child, by its handle).
    drop(s);
    let by = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !processes(&ends).is_empty() {
        assert!(
            std::time::Instant::now() < by,
            "a server that reads its input outlived the node: {:#?}",
            processes(&ends)
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let left = processes(&stays);
    // Stopped by its pid, which this test read from /proc, before it asserts.
    if let Some(l) = left.first() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(l.0),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert_eq!(
        left.iter().map(|l| l.0).collect::<Vec<_>>(),
        vec![stay],
        "T47 says a host chat's server can outlive a kill -9 of the node: one that does \
         not read its input did not"
    );
}
