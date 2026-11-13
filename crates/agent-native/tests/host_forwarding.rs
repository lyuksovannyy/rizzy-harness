//! The loop wraps its host (`Logged`, the append-first rule) before any
//! tool runs. Whatever the host says about the session's folder must reach
//! every tool call through that wrapper: the folder's pin (R.1, S-2) and a
//! sub-agent's node-owned folder (T32). These tests go through `run_turn`,
//! never through a hand-built `ToolCtx`: on 1220e1a..f83a87a and later the
//! wrapper answered with the trait's defaults (`None`) and the tools never
//! saw either.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
};
use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::{EventKind, ToolCallRef};
use harness_core::folder::{FolderPin, Identity, OwnedFolder};
use harness_core::policy::{Decision, ToolRisk};
use harness_tools::{Tool, ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// A host that pins a folder and names an owned folder, as the node's
/// session does.
struct Host {
    events: Mutex<Vec<EventKind>>,
    pin: Option<FolderPin>,
    owned: Option<OwnedFolder>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn folder_pin(&self) -> Option<FolderPin> {
        self.pin.clone()
    }
    fn owned_folder(&self) -> Option<OwnedFolder> {
        self.owned.clone()
    }
}

impl Host {
    /// `(ok, output)` of every finished tool call, in order.
    fn results(&self) -> Vec<(bool, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                EventKind::ToolCallFinished(f) => Some((f.ok, f.output.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Answers the first request with `calls` (after running `before`), and
/// every request after a tool result with a plain answer.
struct Script {
    calls: Vec<(&'static str, Value)>,
    before: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

#[async_trait::async_trait]
impl Provider for Script {
    fn name(&self) -> &'static str {
        "script"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        if matches!(req.messages.last(), Some(ChatMsg::Tool { .. })) {
            return Ok(Completion {
                text: "done".into(),
                tool_calls: vec![],
                usage: None,
                stop: Default::default(),
            });
        }
        if let Some(f) = self.before.lock().unwrap().take() {
            f();
        }
        Ok(Completion {
            text: String::new(),
            tool_calls: self
                .calls
                .iter()
                .map(|(tool, input)| ToolCallRef {
                    call_id: harness_core::new_id(),
                    tool: (*tool).into(),
                    input: input.clone(),
                })
                .collect(),
            usage: None,
            stop: Default::default(),
        })
    }
}

/// A tool that records the folder context the loop gave it.
#[derive(Default)]
struct Probe {
    seen: Mutex<Vec<(Option<FolderPin>, Option<OwnedFolder>)>>,
}

#[async_trait::async_trait]
impl Tool for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }
    fn description(&self) -> &'static str {
        "reports its context"
    }
    fn schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &ToolCtx, _: Value) -> ToolOutcome {
        self.seen
            .lock()
            .unwrap()
            .push((ctx.pin.clone(), ctx.owned.clone()));
        ToolOutcome::ok("seen")
    }
}

fn turn<'a>(host: &'a Host, cwd: &Path) -> TurnCtx<'a> {
    TurnCtx {
        session_id: harness_core::SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: "go".into(),
        cwd: cwd.to_path_buf(),
        model: "x".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

fn owned(top: &Path) -> OwnedFolder {
    std::fs::create_dir_all(top.join(".git")).unwrap();
    OwnedFolder {
        top: top.to_path_buf(),
        top_id: Identity::of(top).unwrap(),
        git: top.join(".git"),
        git_id: Identity::of(&top.join(".git")).unwrap(),
        objects: None,
    }
}

/// Point 2 of the fix: every wrapper between the host and the tools
/// forwards the folder's pin and the owned folder. A wrapper that forgets
/// one (the trait's default answers `None`) fails here.
#[tokio::test]
async fn the_hosts_pin_and_owned_folder_reach_the_tool_through_the_loop() {
    let dir = tempfile::tempdir().unwrap();
    let ws: PathBuf = dir.path().canonicalize().unwrap().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let pin = FolderPin::at(&ws).unwrap();
    let own = owned(&ws);
    let host = Host {
        events: Mutex::default(),
        pin: Some(pin.clone()),
        owned: Some(own.clone()),
    };
    let probe = Arc::new(Probe::default());
    let backend = NativeBackend::new(
        Arc::new(Script {
            calls: vec![("probe", json!({}))],
            before: Mutex::default(),
        }),
        vec![probe.clone()],
        NativeConfig::default(),
    );
    backend.run_turn(turn(&host, &ws)).await;
    let seen = probe.seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "the probe ran once: {:?}", host.results());
    assert_eq!(seen[0].0.as_ref(), Some(&pin), "the pin did not arrive");
    assert_eq!(
        seen[0].1.as_ref(),
        Some(&own),
        "the owned folder did not arrive"
    );
}

/// S-2 through the real loop: the session's folder is pinned, then
/// replaced while the turn runs (renamed away, a link to another folder or
/// a new directory put in its place). `read`, `write` and `bash` are
/// refused with the pin's reason; nothing of the other folder is read and
/// nothing is written there.
#[cfg(unix)]
#[tokio::test]
async fn a_folder_replaced_during_the_turn_refuses_read_write_and_bash() {
    for link in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let ws = root.join("ws");
        let other = root.join("other");
        std::fs::create_dir(&ws).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
        let host = Host {
            events: Mutex::default(),
            pin: Some(FolderPin::at(&ws).unwrap()),
            owned: None,
        };
        let swap = {
            let (ws, other) = (ws.clone(), other.clone());
            move || {
                std::fs::rename(&ws, ws.with_extension("moved")).unwrap();
                if link {
                    std::os::unix::fs::symlink(&other, &ws).unwrap();
                } else {
                    std::fs::create_dir(&ws).unwrap();
                    std::fs::write(ws.join("id_ed25519"), "SSH-KEY-CANARY\n").unwrap();
                }
            }
        };
        let backend = NativeBackend::new(
            Arc::new(Script {
                calls: vec![
                    ("read", json!({"path": "id_ed25519"})),
                    (
                        "write",
                        json!({"path": "authorized_keys", "content": "attacker\n"}),
                    ),
                    (
                        "bash",
                        json!({"command": "cat id_ed25519; echo x > planted"}),
                    ),
                ],
                before: Mutex::new(Some(Box::new(swap))),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        );
        backend.run_turn(turn(&host, &ws)).await;
        let results = host.results();
        assert_eq!(results.len(), 3, "link={link}: {results:?}");
        for (ok, out) in &results {
            assert!(
                !ok && out.contains(harness_core::folder::REPLACED),
                "link={link}: ran in the replaced folder: {out}"
            );
            assert!(!out.contains("SSH-KEY-CANARY"), "link={link}: {out}");
        }
        let target = if link { &other } else { &ws };
        assert!(!target.join("authorized_keys").exists(), "link={link}");
        assert!(!target.join("planted").exists(), "link={link}");
    }
}
