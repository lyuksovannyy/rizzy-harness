//! The MCP client against a fake server (`support/fake_mcp.py`), started
//! the way a chat starts one but without a sandbox (`Confinement::
//! Unconfined`; the contained path is `cli/tests/mcp_sandbox.rs`): its tools
//! are listed, named and bounded; calls go through; a server that hangs,
//! floods, dies or lists too much is cut with a note; it sees only the
//! allow-listed environment; it is gone after a stop.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::policy::ToolRisk;
use harness_tools::mcp::{self, ServerConfig, SessionMcp};
use harness_tools::{Tool, ToolCtx};
use serde_json::json;

fn script() -> String {
    format!("{}/tests/support/fake_mcp.py", env!("CARGO_MANIFEST_DIR"))
}

fn server(args: &[&str]) -> ServerConfig {
    let mut all = vec![script()];
    all.extend(args.iter().map(|a| a.to_string()));
    ServerConfig {
        args: all,
        ..ServerConfig::new("python3", &[])
    }
}

fn set(cfg: ServerConfig) -> Arc<SessionMcp> {
    let mut m = BTreeMap::new();
    m.insert("fake".to_string(), cfg);
    SessionMcp::new(&m).unwrap()
}

fn ctx(dir: &tempfile::TempDir) -> ToolCtx {
    ToolCtx::new(dir.path())
}

fn tool<'a>(tools: &'a [Arc<dyn Tool>], name: &str) -> &'a Arc<dyn Tool> {
    tools
        .iter()
        .find(|t| t.name() == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn alive(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        !s.rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z')
    })
}

fn pids(started: &std::path::Path) -> Vec<i32> {
    std::fs::read_to_string(started)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("started ")?.parse().ok())
        .collect()
}

#[tokio::test]
async fn tools_are_listed_under_prefixed_names_and_always_ask() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&[]));
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    let echo = tool(&l.tools, "mcp__fake__echo");
    assert!(echo.description().contains("Repeat the text."));
    assert!(echo.description().contains("MCP server `fake`"));
    assert_eq!(echo.schema()["properties"]["text"]["type"], "string");
    // The server says `readOnlyHint: true`; the harness does not believe it.
    assert_eq!(echo.risk(), ToolRisk::Mutating);
    for t in &l.tools {
        assert!(t.name().starts_with(mcp::PREFIX), "{}", t.name());
        assert_eq!(t.risk(), ToolRisk::Mutating);
    }
    let r = echo.run(&ctx(&dir), json!({"text": "hi there"})).await;
    assert!(r.ok, "{}", r.output);
    assert_eq!(r.output, "hi there");
    let f = tool(&l.tools, "mcp__fake__fail")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(!f.ok && f.output.contains("it went wrong"), "{}", f.output);
    let i = tool(&l.tools, "mcp__fake__image")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(
        i.output.contains("image from MCP server `fake` (image/png"),
        "{}",
        i.output
    );
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

#[tokio::test]
async fn a_huge_tool_list_and_a_huge_schema_are_cut_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&["--tools", "100", "--big-schema", "--page", "7"]));
    let l = s.load(&ctx(&dir), &[]).await;
    assert_eq!(l.tools.len(), mcp::MAX_TOOLS);
    let all = l.notes.join("\n");
    assert!(all.contains("are not offered in this chat"), "{all}");
    assert!(all.contains("enabled_tools"), "{all}");
    assert!(
        all.contains("their input schema is over 8000 bytes"),
        "{all}"
    );
    assert!(all.contains("big"), "{all}");
    assert!(!l.tools.iter().any(|t| t.name() == "mcp__fake__big"));
    // Shown once; the next turn's load does not repeat them.
    assert!(s.load(&ctx(&dir), &[]).await.notes.is_empty());
    // node.toml's lists choose.
    let mut only = server(&["--tools", "100"]);
    only.enabled_tools = Some(vec!["t99".into(), "echo".into()]);
    only.disabled_tools = vec!["echo".into()];
    let l = set(only).load(&ctx(&dir), &[]).await;
    let names: Vec<&str> = l.tools.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["mcp__fake__t99"]);
}

#[tokio::test]
async fn a_server_that_hangs_at_start_is_cut_at_its_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let mut cfg = server(&["--hang-init", "--started", started.to_str().unwrap()]);
    cfg.startup_timeout_sec = 1;
    let s = set(cfg);
    let t = Instant::now();
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    assert!(l.tools.is_empty());
    assert!(
        l.notes.join("").contains("did not answer within 1 s"),
        "{:?}",
        l.notes
    );
    // The half-started process is gone.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for p in pids(&started) {
        assert!(!alive(p), "{p} still runs");
    }
}

#[tokio::test]
async fn a_call_that_hangs_is_cut_at_its_timeout_and_the_server_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let mut cfg = server(&["--started", started.to_str().unwrap()]);
    cfg.tool_timeout_sec = 1;
    let s = set(cfg);
    let l = s.load(&ctx(&dir), &[]).await;
    let t = Instant::now();
    let r = tool(&l.tools, "mcp__fake__hang")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
    assert!(!r.ok);
    assert!(r.output.contains("no answer within 1 s"), "{}", r.output);
    let first = pids(&started)[0];
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!alive(first));
    // The next call starts it again.
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "back"}))
        .await;
    assert!(r.ok && r.output == "back", "{}", r.output);
    assert_eq!(pids(&started).len(), 2);
}

#[tokio::test]
async fn a_huge_result_is_cut_and_a_message_over_the_cap_fails_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&[]));
    let l = s.load(&ctx(&dir), &[]).await;
    let r = tool(&l.tools, "mcp__fake__huge")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(r.ok && r.truncated, "{}", r.output.len());
    assert!(r.output.contains("[... output truncated ...]"));
    assert!(r.output.chars().count() < harness_tools::MAX_OUTPUT_CHARS + 100);
    let t = Instant::now();
    let r = tool(&l.tools, "mcp__fake__flood")
        .run(&ctx(&dir), json!({}))
        .await;
    assert!(t.elapsed() < Duration::from_secs(30), "{:?}", t.elapsed());
    assert!(!r.ok);
    assert!(r.output.contains("larger than 8 MiB"), "{}", r.output);
}

#[tokio::test]
async fn a_server_that_dies_is_an_error_and_is_restarted_a_bounded_number_of_times() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let s = set(server(&["--started", started.to_str().unwrap()]));
    let l = s.load(&ctx(&dir), &[]).await;
    let die = tool(&l.tools, "mcp__fake__die");
    let mut last = String::new();
    for _ in 0..=mcp::MAX_RESTARTS {
        let r = die.run(&ctx(&dir), json!({})).await;
        assert!(!r.ok);
        last = r.output;
    }
    assert!(last.contains("told to die"), "{last}");
    assert!(last.contains("not started again"), "{last}");
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "x"}))
        .await;
    assert!(
        !r.ok && r.output.contains("not started again"),
        "{}",
        r.output
    );
    assert_eq!(pids(&started).len() as u32, mcp::MAX_RESTARTS + 1);
}

#[tokio::test]
async fn the_server_sees_only_the_allow_listed_environment() {
    // Cargo sets these for the test process; none is on the core list.
    assert!(std::env::var("CARGO_MANIFEST_DIR").is_ok());
    assert!(std::env::var("CARGO_PKG_VERSION").is_ok());
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = server(&[]);
    cfg.env_vars = vec!["CARGO_PKG_*".into()];
    cfg.env = serde_json::from_value(json!({"FAKE_TOKEN": "tok-value-1"})).unwrap();
    let s = set(cfg);
    // The node's deny-list (provider keys, `.env` names) wins over
    // `env_vars`, as over `env_pass`.
    let c = ToolCtx {
        env: harness_tools::ToolEnv::new(&[], ["CARGO_PKG_VERSION".to_string()]).unwrap(),
        ..ctx(&dir)
    };
    let l = s.load(&c, &[]).await;
    let r = tool(&l.tools, "mcp__fake__env").run(&c, json!({})).await;
    assert!(r.ok, "{}", r.output);
    let names: Vec<&str> = r
        .output
        .lines()
        .filter_map(|l| l.split('=').next())
        .collect();
    assert!(names.contains(&"PATH"), "{names:?}");
    assert!(names.contains(&"CARGO_PKG_NAME"), "{names:?}");
    assert!(r.output.contains("FAKE_TOKEN=tok-value-1"));
    assert!(!names.contains(&"CARGO_MANIFEST_DIR"), "{names:?}");
    assert!(!names.contains(&"CARGO_PKG_VERSION"), "{names:?}");
    assert!(
        !names.iter().any(|n| n.starts_with("HARNESS_")),
        "{names:?}"
    );
}

#[tokio::test]
async fn a_stop_or_idle_time_ends_the_process_and_a_closed_set_starts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let started = dir.path().join("started");
    let s = set(server(&["--started", started.to_str().unwrap()]));
    let l = s.load(&ctx(&dir), &[]).await;
    assert_eq!(s.running(), 1);
    let first = pids(&started)[0];
    assert!(alive(first));
    // Idle: stopped, started again at the next use, not counted.
    s.stop_idle(Duration::ZERO);
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
    assert!(!alive(first));
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "again"}))
        .await;
    assert!(r.ok, "{}", r.output);
    let second = pids(&started)[1];
    assert!(alive(second));
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
    assert!(!alive(second));
    assert_eq!(s.running(), 0);
    let r = tool(&l.tools, "mcp__fake__echo")
        .run(&ctx(&dir), json!({"text": "x"}))
        .await;
    assert!(!r.ok, "{}", r.output);
    assert_eq!(pids(&started).len(), 2);
    // Dropping a chat's set stops its server too.
    let started2 = dir.path().join("started2");
    let s2 = set(server(&["--started", started2.to_str().unwrap()]));
    s2.load(&ctx(&dir), &[]).await;
    let p = pids(&started2)[0];
    drop(s2);
    let by = Instant::now() + mcp::STOP_WAIT;
    while alive(p) && Instant::now() < by {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(p));
}

/// Review L1: `public_name("a_", "echo") == public_name("a", "_echo")`.
/// node.toml refuses a server name ending in `_` or `-`, and a chat never
/// carries one name twice: the later tool is left out, with a note.
#[tokio::test]
async fn two_servers_never_offer_the_same_name() {
    let mut m = BTreeMap::new();
    m.insert(
        "a_".to_string(),
        server(&["--tool-names", "echo", "--marker", "first"]),
    );
    assert!(mcp::validate(&m).is_err());
    m.insert(
        "a".to_string(),
        server(&["--tool-names", "_echo", "--marker", "second"]),
    );
    let dir = tempfile::tempdir().unwrap();
    let l = SessionMcp::new(&m).unwrap().load(&ctx(&dir), &[]).await;
    let names: Vec<&str> = l.tools.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["mcp__a___echo"], "{names:?}");
    let all = l.notes.join("\n");
    assert!(all.contains("same name"), "{all}");
    for bad in ["a-", "a_"] {
        let mut m = BTreeMap::new();
        m.insert(bad.to_string(), server(&[]));
        assert!(mcp::validate(&m).is_err(), "{bad}");
    }
}

/// Review L2: a tool name is the server's text; in a note it is one line
/// without hidden or direction characters.
#[tokio::test]
async fn server_supplied_names_in_notes_are_one_clean_line() {
    let mut names: Vec<String> = (0..mcp::MAX_TOOLS).map(|i| format!("t{i}")).collect();
    // Sorted last by public name, so these are the ones cut.
    names.push("zz-evil\nline".into());
    names.push("zz-rev\u{202e}txt".into());
    let dir = tempfile::tempdir().unwrap();
    let l = set(server(&["--tool-names", &names.join(",")]))
        .load(&ctx(&dir), &[])
        .await;
    let all = l.notes.join(" ");
    assert!(all.contains("not offered"), "{all}");
    assert!(all.contains("zz-evil line"), "{all}");
    assert!(!all.contains('\n') && !all.contains('\u{202e}'), "{all:?}");
}

/// Review N4: failures far apart do not add up: a successful call starts
/// the count again.
#[tokio::test]
async fn a_successful_call_starts_the_failure_count_again() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&[]));
    let l = s.load(&ctx(&dir), &[]).await;
    let die = tool(&l.tools, "mcp__fake__die");
    let echo = tool(&l.tools, "mcp__fake__echo");
    for _ in 0..3 {
        for _ in 0..mcp::MAX_RESTARTS {
            let r = die.run(&ctx(&dir), json!({})).await;
            assert!(
                !r.ok && !r.output.contains("not started again"),
                "{}",
                r.output
            );
        }
        let r = echo.run(&ctx(&dir), json!({"text": "ok"})).await;
        assert!(r.ok, "{}", r.output);
    }
}

/// X.34 (PLAN.md T80): a server the turn's agent may not use in its
/// workspace is not started for that turn and none of its tools is
/// offered; the other servers are; a later turn whose agent may use it
/// starts it then.
#[tokio::test]
async fn a_denied_server_is_neither_started_nor_offered() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    let mut m = BTreeMap::new();
    m.insert(
        "alpha".to_string(),
        server(&["--started", a.to_str().unwrap()]),
    );
    m.insert(
        "beta".to_string(),
        server(&["--started", b.to_str().unwrap()]),
    );
    let s = SessionMcp::new(&m).unwrap();
    let l = s.load(&ctx(&dir), &["alpha".to_string()]).await;
    assert!(pids(&a).is_empty(), "the denied server was started");
    assert_eq!(pids(&b).len(), 1);
    assert!(
        l.tools
            .iter()
            .all(|t| !t.name().starts_with("mcp__alpha__"))
    );
    assert!(l.tools.iter().any(|t| t.name().starts_with("mcp__beta__")));
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    // Every server denied: nothing starts, nothing is offered.
    let none = SessionMcp::new(&m).unwrap();
    let all = ["alpha".to_string(), "beta".to_string()];
    assert!(none.load(&ctx(&dir), &all).await.tools.is_empty());
    assert_eq!(pids(&b).len(), 1);
    // Another agent's turn that may use it starts it.
    let l = s.load(&ctx(&dir), &[]).await;
    assert_eq!(pids(&a).len(), 1);
    assert!(l.tools.iter().any(|t| t.name().starts_with("mcp__alpha__")));
    s.close();
    none.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

/// X.34 step C: a note is shown once in each session that loads the table
/// (a sub-agent's model is told what its chat's was), not once per table.
#[tokio::test]
async fn notes_are_shown_once_per_session() {
    let dir = tempfile::tempdir().unwrap();
    let s = set(server(&["--tools", "45"]));
    let cut = |n: &[String]| n.iter().any(|n| n.contains("are not offered"));
    assert!(cut(&s.load_for("chat", &ctx(&dir), &[]).await.notes));
    assert!(!cut(&s.load_for("chat", &ctx(&dir), &[]).await.notes));
    assert!(cut(&s.load_for("child", &ctx(&dir), &[]).await.notes));
    s.close();
    s.wait_gone(mcp::STOP_WAIT).await;
}

/// X.36 (PLAN.md T82): a repository's approved servers join the table
/// through `set_project`, beside node.toml's: a name node.toml has stays
/// node.toml's; one taken out (its approval removed, gone from the file)
/// stops, and a tool of it a turn still holds starts nothing again; one
/// whose `env` goes only to its own process (`env_inside`) starts only
/// contained (the contained path: `cli/tests/project_mcp_e2e.rs`).
#[tokio::test]
async fn a_repositorys_servers_come_and_go_through_set_project() {
    let dir = tempfile::tempdir().unwrap();
    let mut node = BTreeMap::new();
    node.insert(
        "fake".to_string(),
        server(&["--tool-names", "who", "--marker", "node"]),
    );
    let s = SessionMcp::at_always(
        &node,
        mcp::Home {
            key: "k".into(),
            workspace: dir.path().to_path_buf(),
            pin: None,
        },
    );
    let started = dir.path().join("started");
    let proj = server(&[
        "--tool-names",
        "env,who",
        "--marker",
        "proj",
        "--started",
        started.to_str().unwrap(),
    ]);
    let mut shadow = server(&["--tool-names", "who", "--marker", "shadow"]);
    shadow.env_inside = true;
    let mut given = BTreeMap::new();
    given.insert("proj".to_string(), proj.clone());
    given.insert("fake".to_string(), shadow);
    s.set_project(&given, vec!["a note".into()]);
    assert!(s.is_project("proj") && !s.is_project("fake"));
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(l.notes.iter().any(|n| n == "a note"), "{:?}", l.notes);
    let who = tool(&l.tools, "mcp__fake__who")
        .run(&ctx(&dir), json!({}))
        .await;
    assert_eq!(who.output, "served by node tool who");
    let held = tool(&l.tools, "mcp__proj__who").clone();
    assert_eq!(
        held.run(&ctx(&dir), json!({})).await.output,
        "served by proj tool who"
    );
    let pid = pids(&started)[0];
    // The same copy again keeps its process.
    s.set_project(&given, vec![]);
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(l.notes.is_empty());
    tool(&l.tools, "mcp__proj__who")
        .run(&ctx(&dir), json!({}))
        .await;
    assert_eq!(pids(&started).len(), 1);
    // Taken out: stopped, and the held tool starts nothing.
    s.set_project(&BTreeMap::new(), vec![]);
    let by = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(Instant::now() < by, "the removed server still runs");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let r = held.run(&ctx(&dir), json!({})).await;
    assert!(
        !r.ok && r.output.contains("no longer one of this chat's servers"),
        "{}",
        r.output
    );
    assert_eq!(pids(&started).len(), 1, "a removed server started again");
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(l.tools.iter().all(|t| !t.name().starts_with("mcp__proj__")));
    // `env_inside` outside a contained sandbox: not started at all.
    let mut inside = server(&["--started", started.to_str().unwrap()]);
    inside.env = serde_json::from_value(json!({"PROJ_VALUE": "inside-only"})).unwrap();
    inside.env_inside = true;
    let mut given = BTreeMap::new();
    given.insert("inside".to_string(), inside);
    s.set_project(&given, vec![]);
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(
        l.notes
            .iter()
            .any(|n| n.contains("env values") && n.contains("a host chat never has one")),
        "{:?}",
        l.notes
    );
    assert_eq!(
        pids(&started).len(),
        1,
        "an env_inside server started uncontained"
    );
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

/// A refusal that nothing about the process can change (a repository's
/// `env` values with no bubblewrap) is raised before anything is spawned:
/// the note does not announce a restart, a second turn does not retry (the
/// count stays where it was), and no process of the server ever ran.
#[tokio::test]
async fn a_refusal_that_will_not_change_is_not_counted_or_announced_as_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let s = SessionMcp::at_always(
        &BTreeMap::new(),
        mcp::Home {
            key: "k".into(),
            workspace: dir.path().to_path_buf(),
            pin: None,
        },
    );
    let started = dir.path().join("started");
    let mut inside = server(&[
        "--marker",
        "norestart-refused",
        "--started",
        started.to_str().unwrap(),
    ]);
    inside.env = serde_json::from_value(json!({"PROJ_VALUE": "inside-only"})).unwrap();
    inside.env_inside = true;
    let mut given = BTreeMap::new();
    given.insert("inside".to_string(), inside);
    s.set_project(&given, vec![]);
    let first = s.load(&ctx(&dir), &[]).await.notes;
    let second = s.load(&ctx(&dir), &[]).await.notes;
    let n = first
        .iter()
        .find(|n| n.contains("env values"))
        .unwrap_or_else(|| panic!("no refusal note: {first:?}"));
    assert!(
        n.contains("Take the env values out of the entry")
            && n.contains("will not change until the entry or the chat does"),
        "{n}"
    );
    assert!(
        !n.contains("started again") && !n.contains("restart"),
        "a permanent refusal was announced as a restart: {n}"
    );
    // Told once: the second turn says nothing more and starts nothing.
    assert!(second.is_empty(), "{second:?}");
    assert!(pids(&started).is_empty(), "a refused server was spawned");
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

/// What a server's stop reaches without a sandbox (a `host` chat's, and a
/// node with no bubblewrap): its own process group. A process it detached
/// (`setsid`, a double fork) is in a group of its own and survives the stop;
/// the README and PLAN T47 and T82 say so. This test documents it (the
/// reviewer's case of PR 190), and removes the survivor itself.
#[tokio::test]
async fn a_process_the_server_detached_survives_its_stop() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("detached.pid");
    let started = dir.path().join("started");
    let launch = format!(
        "setsid sh -c 'echo $$ > {pid}; exec sleep 7731' & exec python3 {fake} --started {started}",
        pid = pidfile.display(),
        fake = script(),
        started = started.display(),
    );
    let cfg = ServerConfig {
        args: vec!["-c".into(), launch],
        ..ServerConfig::new("sh", &[])
    };
    let s = set(cfg);
    let l = s.load(&ctx(&dir), &[]).await;
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    let server_pid = pids(&started)[0];
    let by = Instant::now() + Duration::from_secs(10);
    let survivor = loop {
        if let Some(p) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|t| t.trim().parse::<i32>().ok())
        {
            break p;
        }
        assert!(Instant::now() < by, "the detached process never started");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
    let survived = alive(survivor);
    let server_gone = !alive(server_pid);
    let _ = std::process::Command::new("kill")
        .args(["-9", &survivor.to_string()])
        .status();
    assert!(server_gone, "the server itself outlived its stop");
    assert!(
        survived,
        "a detached descendant was reached: this note and the README say it is not"
    );
}

/// Review of PR 150 (LOW): a repository server taken out of the table
/// while it is still starting does not stay up, and the turn that started
/// it gets no tool of it.
#[tokio::test]
async fn a_server_taken_out_while_it_starts_does_not_stay_up() {
    let dir = tempfile::tempdir().unwrap();
    let s = SessionMcp::at_always(
        &BTreeMap::new(),
        mcp::Home {
            key: "k".into(),
            workspace: dir.path().to_path_buf(),
            pin: None,
        },
    );
    let started = dir.path().join("started");
    let slow = server(&[
        "--slow-init",
        "1500",
        "--started",
        started.to_str().unwrap(),
    ]);
    let mut given = BTreeMap::new();
    given.insert("slow".to_string(), slow);
    s.set_project(&given, vec![]);
    let c = ctx(&dir);
    let load = s.load(&c, &[]);
    let taker = async {
        while pids(&started).is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        s.set_project(&BTreeMap::new(), vec![]);
    };
    let (l, ()) = tokio::join!(load, taker);
    assert!(
        l.tools.iter().all(|t| !t.name().starts_with("mcp__slow__")),
        "a removed server's tools were offered"
    );
    let pid = pids(&started)[0];
    let by = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(
            Instant::now() < by,
            "the server taken out while it started still runs"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Closing works the same way.
    let t = SessionMcp::at_always(
        &BTreeMap::new(),
        mcp::Home {
            key: "k2".into(),
            workspace: dir.path().to_path_buf(),
            pin: None,
        },
    );
    let started2 = dir.path().join("started2");
    let mut given = BTreeMap::new();
    given.insert(
        "slow".to_string(),
        server(&[
            "--slow-init",
            "1500",
            "--started",
            started2.to_str().unwrap(),
        ]),
    );
    t.set_project(&given, vec![]);
    let load = t.load(&c, &[]);
    let closer = async {
        while pids(&started2).is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        t.close();
    };
    let (l, ()) = tokio::join!(load, closer);
    assert!(l.tools.is_empty());
    let pid = pids(&started2)[0];
    let by = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(
            Instant::now() < by,
            "the server of a closed table still runs"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Review of PR 155, MEDIUM 3: two tools of one server whose names clean to
/// the same public name: the first keeps it, the other is left out and
/// said so, not offered under a name another tool answers to.
#[tokio::test]
async fn two_tools_with_one_public_name_keep_the_first() {
    let hashed = mcp::public_name("fake", "a b");
    let twin = hashed.strip_prefix("mcp__fake__").unwrap().to_string();
    assert_eq!(
        mcp::public_name("fake", &twin),
        hashed,
        "the reviewer's pair"
    );
    let dir = tempfile::tempdir().unwrap();
    let l = set(server(&["--tool-names", &format!("a b,{twin},echo")]))
        .load(&ctx(&dir), &[])
        .await;
    let names: Vec<&str> = l.tools.iter().map(|t| t.name()).collect();
    assert_eq!(
        names.iter().filter(|n| **n == hashed).count(),
        1,
        "{names:?}"
    );
    assert!(names.contains(&"mcp__fake__echo"), "{names:?}");
    let all = l.notes.join(" ");
    assert!(
        all.contains("1 tool(s) of MCP server `fake` left out"),
        "{all}"
    );
    assert!(all.contains(&twin), "{all}");
}

const GONE: &str = "no longer one of this chat's servers";

fn project_table(dir: &tempfile::TempDir) -> Arc<SessionMcp> {
    SessionMcp::at_always(
        &BTreeMap::new(),
        mcp::Home {
            key: "k".into(),
            workspace: dir.path().to_path_buf(),
            pin: None,
        },
    )
}

/// A call in flight when its server is taken out of the table gets the
/// removal's answer, not "the server ended ... started again at its next
/// use": the stop was ours, nothing is counted against the restart limit
/// and no restart is announced; later calls answer the same and the server
/// is started once. A server that was NOT removed keeps all its restarts.
#[tokio::test]
async fn a_call_in_flight_when_its_server_is_removed_gets_the_removal_answer() {
    let dir = tempfile::tempdir().unwrap();
    let s = project_table(&dir);
    let started = dir.path().join("started");
    let other_started = dir.path().join("other-started");
    let proj = server(&["--started", started.to_str().unwrap()]);
    let other = server(&["--started", other_started.to_str().unwrap()]);
    let mut given = BTreeMap::new();
    given.insert("proj".to_string(), proj);
    given.insert("other".to_string(), other.clone());
    s.set_project(&given, vec![]);
    let l = s.load(&ctx(&dir), &[]).await;
    let held = tool(&l.tools, "mcp__proj__hang").clone();
    let echo = tool(&l.tools, "mcp__proj__echo").clone();
    let die = tool(&l.tools, "mcp__other__die").clone();
    let d = dir.path().to_path_buf();
    let h = tokio::spawn(async move { held.run(&ToolCtx::new(&d), json!({})).await });
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut kept = BTreeMap::new();
    kept.insert("other".to_string(), other);
    s.set_project(&kept, vec![]);
    let r = h.await.unwrap();
    assert!(!r.ok && r.output.contains(GONE), "in flight: {}", r.output);
    assert!(
        !r.output.contains("started again") && !r.output.contains("restart"),
        "an ordered stop was announced as a failure: {}",
        r.output
    );
    for _ in 0..3 {
        let r = echo.run(&ctx(&dir), json!({"text": "x"})).await;
        assert!(!r.ok && r.output.contains(GONE), "after: {}", r.output);
        assert!(!r.output.contains("restart"), "{}", r.output);
    }
    assert_eq!(pids(&started).len(), 1, "a removed server started again");
    // A crash of a server that stayed is still counted from the start.
    let r = die.run(&ctx(&dir), json!({})).await;
    assert!(
        !r.ok && r.output.contains("restart 1 of the 3"),
        "the other server's count moved: {}",
        r.output
    );
    s.close();
    assert!(s.wait_gone(mcp::STOP_WAIT).await);
}

/// A call issued once `set_project` has returned must not reach the
/// removed server, also while its process is still up for an instant (its
/// exit is published a moment after the stop order). The server's own log
/// of received calls shows none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_after_the_removal_never_reaches_the_server() {
    let dir = tempfile::tempdir().unwrap();
    for round in 0..12 {
        let s = project_table(&dir);
        let calls = dir.path().join(format!("calls-{round}"));
        let started = dir.path().join(format!("started-{round}"));
        let mut given = BTreeMap::new();
        given.insert(
            "proj".to_string(),
            server(&[
                "--started",
                started.to_str().unwrap(),
                "--calls",
                calls.to_str().unwrap(),
            ]),
        );
        s.set_project(&given, vec![]);
        let l = s.load(&ctx(&dir), &[]).await;
        let echo = tool(&l.tools, "mcp__proj__echo").clone();
        let ok = echo.run(&ctx(&dir), json!({"text": "first"})).await;
        assert!(ok.ok, "{}", ok.output);
        s.set_project(&BTreeMap::new(), vec![]);
        let r = echo.run(&ctx(&dir), json!({"text": "late"})).await;
        assert!(
            !r.ok && r.output.contains(GONE) && !r.output.contains("restart"),
            "round {round}: {}",
            r.output
        );
        let by = Instant::now() + Duration::from_secs(10);
        while alive(pids(&started)[0]) {
            assert!(Instant::now() < by, "the removed server still runs");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let log = std::fs::read_to_string(&calls).unwrap_or_default();
        assert_eq!(
            log.lines().count(),
            1,
            "round {round}: the server saw {log:?}"
        );
        s.close();
    }
}

/// A removal that lands while the server starts (slow to answer
/// `initialize`): afterwards no process of it is alive and the call that
/// started it got the removal's answer (the stop is ordered under the
/// lock `retire` takes, so a process cannot be published after it).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_removal_during_a_start_leaves_no_process_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let s = project_table(&dir);
    let started = dir.path().join("started");
    let mut given = BTreeMap::new();
    given.insert(
        "slow".to_string(),
        server(&[
            "--slow-init",
            "1200",
            "--started",
            started.to_str().unwrap(),
        ]),
    );
    s.set_project(&given, vec![]);
    let c = ctx(&dir);
    let load = s.load(&c, &[]);
    let taker = async {
        while pids(&started).is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        s.set_project(&BTreeMap::new(), vec![]);
    };
    let (l, ()) = tokio::join!(load, taker);
    assert!(l.tools.is_empty(), "{:?}", l.tools.len());
    assert!(
        l.notes.iter().any(|n| n.contains(GONE)),
        "the start's call was not told: {:?}",
        l.notes
    );
    assert!(
        !l.notes.iter().any(|n| n.contains("restart")),
        "{:?}",
        l.notes
    );
    let pid = pids(&started)[0];
    let by = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(Instant::now() < by, "the server of a removed slot runs");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(pids(&started).len(), 1);
}

/// The window between the start arm's check and its publishing the
/// process: removals racing starts on many threads leave no process
/// behind. (The window is a few instructions wide, so this is a stress, not
/// a proof; the argument is in the comment on `Slot::retire`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removals_racing_starts_leave_nothing_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut all = Vec::new();
    for round in 0..30u64 {
        let s = project_table(&dir);
        let started = dir.path().join(format!("race-{round}"));
        let mut given = BTreeMap::new();
        given.insert(
            "r".to_string(),
            server(&["--started", started.to_str().unwrap()]),
        );
        s.set_project(&given, vec![]);
        let c = ctx(&dir);
        let load = s.load(&c, &[]);
        let taker = async {
            tokio::time::sleep(Duration::from_millis(40 + (round % 8) * 12)).await;
            s.set_project(&BTreeMap::new(), vec![]);
        };
        let _ = tokio::join!(load, taker);
        all.push(started);
    }
    let by = Instant::now() + Duration::from_secs(15);
    for started in &all {
        for pid in pids(started) {
            while alive(pid) {
                assert!(
                    Instant::now() < by,
                    "a removed server's process runs: {pid}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }
}
