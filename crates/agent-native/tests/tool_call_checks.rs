//! A tool call's arguments are checked against the schema the model was
//! offered before anything is asked or run: a value of the wrong type is
//! repaired where it reads exactly as the right one (and the approval card
//! and the tool see the repaired value), else the model is told which
//! field is wrong; an unknown tool names the tools on offer; `bash` takes
//! a working folder inside the session's folder.

use std::sync::{Arc, Mutex};

use harness_agent_native::{ChatRequest, Completion, NativeBackend, NativeConfig, Provider};
use harness_agent_native::{ProviderError, ToolSpec};
use harness_core::SessionId;
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Records every event and every input it was asked to approve.
#[derive(Default)]
struct Host {
    events: Mutex<Vec<EventKind>>,
    asked: Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, input: &Value, _: ToolRisk) -> Decision {
        self.asked.lock().unwrap().push(input.clone());
        Decision::Allow
    }
}

impl Host {
    fn started(&self) -> Vec<Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallStarted(s) => Some(s.input.clone()),
                _ => None,
            })
            .collect()
    }
    fn finished(&self) -> Vec<ToolCallFinished> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some(f.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Calls `tool` with `input` once, then answers; keeps the tools offered.
struct Once {
    tool: String,
    input: Value,
    offered: Mutex<Vec<ToolSpec>>,
}

#[async_trait::async_trait]
impl Provider for Once {
    fn name(&self) -> &'static str {
        "once"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        *self.offered.lock().unwrap() = req.tools.clone();
        let answered = req
            .messages
            .iter()
            .any(|m| matches!(m, harness_agent_native::ChatMsg::Tool { .. }));
        if answered {
            return Ok(Completion {
                text: "done".into(),
                ..Default::default()
            });
        }
        Ok(Completion {
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: "c1".into(),
                tool: self.tool.clone(),
                input: self.input.clone(),
            }],
            usage: None,
            stop: Default::default(),
        })
    }
}

async fn run(dir: &std::path::Path, tool: &str, input: Value) -> (Host, Arc<Once>) {
    let host = Host::default();
    let p = Arc::new(Once {
        tool: tool.into(),
        input,
        offered: Mutex::default(),
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    b.run_turn(TurnCtx {
        session_id: SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "go".into(),
        cwd: dir.to_path_buf(),
        model: "x".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host: &host,
    })
    .await;
    (host, p)
}

/// `"timeout_secs": "30"` is the integer 30 on the tool card, on the
/// approval card and in the call that runs.
#[tokio::test]
async fn a_numeric_string_is_repaired_before_the_card_and_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let (host, _) = run(
        dir.path(),
        "bash",
        json!({"command": "echo ran", "timeout_secs": "30"}),
    )
    .await;
    let want = json!({"command": "echo ran", "timeout_secs": 30});
    assert_eq!(host.started(), vec![want.clone()], "the tool card");
    assert_eq!(*host.asked.lock().unwrap(), vec![want], "the approval card");
    let f = &host.finished()[0];
    assert!(f.ok && f.output.contains("ran"), "{}", f.output);
}

/// A value that is not of the right type and cannot be read as one: the
/// model is told which field and what it should be, nothing is asked and
/// nothing runs.
#[tokio::test]
async fn a_wrong_type_is_returned_to_the_model_and_nothing_runs() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    let (host, _) = run(
        dir.path(),
        "bash",
        json!({"command": format!("touch {}", marker.display()), "timeout_secs": "soon"}),
    )
    .await;
    let f = &host.finished()[0];
    assert!(!f.ok);
    assert!(
        f.output
            .contains("`timeout_secs` must be an integer, not a string"),
        "{}",
        f.output
    );
    assert!(host.asked.lock().unwrap().is_empty(), "it was asked about");
    assert!(!marker.exists(), "it ran");
}

/// A misspelt argument is no longer silently ignored.
#[tokio::test]
async fn an_unknown_argument_is_named_with_the_ones_the_tool_takes() {
    let dir = tempfile::tempdir().unwrap();
    let (host, _) = run(
        dir.path(),
        "bash",
        json!({"command": "true", "timeout": 300}),
    )
    .await;
    let f = &host.finished()[0];
    assert!(!f.ok);
    assert!(
        f.output
            .contains("`timeout` is not an argument of this tool")
            && f.output.contains("`timeout_secs`"),
        "{}",
        f.output
    );
}

/// An unknown tool's result lists the tools this turn offers.
#[tokio::test]
async fn an_unknown_tool_names_the_tools_on_offer() {
    let dir = tempfile::tempdir().unwrap();
    let (host, p) = run(dir.path(), "shell", json!({"command": "ls"})).await;
    let f = &host.finished()[0];
    assert!(!f.ok);
    assert!(f.output.starts_with("unknown tool `shell`"), "{}", f.output);
    for t in p.offered.lock().unwrap().iter() {
        assert!(
            f.output.contains(&format!("`{}`", t.name)),
            "{} not named: {}",
            t.name,
            f.output
        );
    }
}

/// `workdir` runs the command in a folder of the session's, shown on the
/// card as given.
#[tokio::test]
async fn bash_runs_in_its_workdir() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("crates/x");
    std::fs::create_dir_all(&sub).unwrap();
    let (host, _) = run(
        dir.path(),
        "bash",
        json!({"command": "pwd", "workdir": "crates/x"}),
    )
    .await;
    assert_eq!(host.asked.lock().unwrap()[0]["workdir"], "crates/x");
    let f = &host.finished()[0];
    assert!(f.ok, "{}", f.output);
    assert!(
        f.output
            .contains(&sub.canonicalize().unwrap().display().to_string()),
        "{}",
        f.output
    );
}

/// A `workdir` outside the session's folder, through `..` or through a
/// link inside it, is refused by name and nothing runs.
#[tokio::test]
async fn a_workdir_outside_the_session_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let ws = dir.path().join("ws");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, ws.join("link")).unwrap();
    for w in ["link", "..", "../outside", outside.to_str().unwrap()] {
        let (host, _) = run(&ws, "bash", json!({"command": "touch made", "workdir": w})).await;
        let f = &host.finished()[0];
        assert!(!f.ok, "{w}: {}", f.output);
        assert!(f.output.contains("workdir"), "{w}: {}", f.output);
        assert!(!outside.join("made").exists(), "{w}: it ran outside");
        // Refused before the approval card (review L3).
        assert!(host.asked.lock().unwrap().is_empty(), "{w}: it was asked");
        assert!(!ws.join("made").exists(), "{w}: it ran in the folder");
    }
    // Not a folder.
    std::fs::write(ws.join("file"), "x").unwrap();
    let (host, _) = run(&ws, "bash", json!({"command": "true", "workdir": "file"})).await;
    assert!(host.finished()[0].output.contains("not a folder"));
}

/// Calls the same tool with the same input on every request, forever.
struct Again {
    input: Value,
    calls: Mutex<usize>,
}

#[async_trait::async_trait]
impl Provider for Again {
    fn name(&self) -> &'static str {
        "again"
    }
    async fn complete(
        &self,
        _: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut c = self.calls.lock().unwrap();
            *c += 1;
            *c
        };
        Ok(Completion {
            text: String::new(),
            tool_calls: vec![ToolCallRef {
                call_id: format!("c{n}"),
                tool: "bash".into(),
                input: self.input.clone(),
            }],
            usage: None,
            stop: Default::default(),
        })
    }
}

/// Review L1: the same bad call again and again meets the repeated-call
/// guard like any other, and the turn ends with the guard's refusal.
#[tokio::test]
async fn the_same_bad_call_again_and_again_ends_at_the_guard() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let p = Arc::new(Again {
        input: json!({"command": "true", "timeout": 300}),
        calls: Mutex::new(0),
    });
    let b = NativeBackend::new(p.clone(), harness_tools::builtin(), NativeConfig::default());
    let out = b
        .run_turn(TurnCtx {
            session_id: SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
            user_text: "go".into(),
            cwd: dir.path().to_path_buf(),
            model: "x".into(),
            agent: None,
            compare: vec![],
            parent_session: None,
            extra: Default::default(),
            cancel: Default::default(),
            host: &host,
        })
        .await;
    assert_eq!(out.reason, TurnEndReason::Error);
    assert_eq!(
        out.error.as_deref(),
        Some(harness_agent_native::repeat::STOP_ERROR)
    );
    assert!(
        *p.calls.lock().unwrap() < 10,
        "{}",
        *p.calls.lock().unwrap()
    );
    let refused = host
        .finished()
        .iter()
        .filter(|f| f.output == harness_agent_native::repeat::REFUSED_OUTPUT)
        .count();
    assert!(refused >= 1);
}
