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
    exec: Option<harness_core::policy::ExecMode>,
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
    fn exec_mode(&self) -> Option<harness_core::policy::ExecMode> {
        self.exec
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
                thinking: None,
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
            thinking: None,
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
        exec: None,
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
            exec: None,
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

/// N.10a: the session's recorded mode reaches the shell through the loop's
/// wrapper and picks that mode's confinement; nothing in the call does. A
/// wrapper that forgets it (the trait's default, `None`) would start a
/// `host` session in the `sandbox` mode's confinement, or the other way
/// round if the default ever changed: both directions fail here.
#[tokio::test]
async fn the_sessions_mode_picks_its_confinement_through_the_loop() {
    use harness_core::policy::ExecMode;
    use harness_tools::sandbox::Confinement;
    let dir = tempfile::tempdir().unwrap();
    let ws: PathBuf = dir.path().canonicalize().unwrap().join("ws");
    std::fs::create_dir(&ws).unwrap();
    for (mode, want, not) in [
        (
            Some(ExecMode::Host),
            "HOST-CONFINEMENT",
            "SANDBOX-CONFINEMENT",
        ),
        (
            Some(ExecMode::Sandbox),
            "SANDBOX-CONFINEMENT",
            "HOST-CONFINEMENT",
        ),
        (None, "SANDBOX-CONFINEMENT", "HOST-CONFINEMENT"),
    ] {
        let host = Host {
            events: Mutex::default(),
            pin: None,
            owned: None,
            exec: mode,
        };
        let backend = NativeBackend::new(
            Arc::new(Script {
                // An input that asks for another mode (`"mode": "host"`)
                // is refused by the argument check before any confinement
                // is chosen (tests/tool_call_checks.rs); the mode comes
                // from the session alone.
                calls: vec![("bash", json!({"command": "true"}))],
                before: Mutex::default(),
            }),
            harness_tools::builtin(),
            NativeConfig {
                sandbox: Confinement::Refused("SANDBOX-CONFINEMENT".into()),
                host_shell: Confinement::Refused("HOST-CONFINEMENT".into()),
                ..Default::default()
            },
        );
        backend.run_turn(turn(&host, &ws)).await;
        let results = host.results();
        assert_eq!(results.len(), 1, "{mode:?}: {results:?}");
        let (ok, out) = &results[0];
        assert!(
            !ok && out.contains(want) && !out.contains(not),
            "{mode:?}: {out}"
        );
    }
}

/// The same for `attached_file` (PLAN.md T95): the host's link to the master
/// reaches the tool through the wrapper, and a request carries the id the
/// model gave and nothing else of the session.
#[tokio::test]
async fn the_hosts_attached_source_reaches_the_tool_through_the_loop() {
    use harness_core::attached::{AttachedAnswer, AttachedRequest, AttachedSource};

    struct Source(Mutex<Vec<AttachedRequest>>);
    #[async_trait::async_trait]
    impl AttachedSource for Source {
        async fn attached(&self, r: AttachedRequest) -> Result<AttachedAnswer, String> {
            self.0.lock().unwrap().push(r);
            Ok(AttachedAnswer::Info {
                name: "x.zip".into(),
                bytes: 5,
                sha256: "ab".repeat(32),
                label: "ZIP archive".into(),
                mime: None,
                archive: true,
            })
        }
    }
    struct Linked {
        inner: Host,
        source: Arc<Source>,
    }
    #[async_trait::async_trait]
    impl TurnHost for Linked {
        async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
            self.inner.emit(kind).await
        }
        async fn approve(&self, a: &str, b: &str, c: &Value, d: ToolRisk) -> Decision {
            self.inner.approve(a, b, c, d).await
        }
        fn attached(&self) -> Option<Arc<dyn AttachedSource>> {
            Some(self.source.clone())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let ws: PathBuf = dir.path().canonicalize().unwrap().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let source = Arc::new(Source(Mutex::default()));
    let host = Linked {
        inner: Host {
            events: Mutex::default(),
            pin: None,
            owned: None,
            exec: None,
        },
        source: source.clone(),
    };
    let backend = NativeBackend::new(
        Arc::new(Script {
            calls: vec![("attached_file", json!({"op": "info", "file_id": "f-1"}))],
            before: Mutex::default(),
        }),
        vec![Arc::new(harness_tools::AttachedFileTool)],
        NativeConfig::default(),
    );
    backend.run_turn(turn_of(&host, &ws)).await;
    let seen = source.0.lock().unwrap();
    assert_eq!(seen.len(), 1, "{:?}", host.inner.results());
    assert_eq!(seen[0].file_id, "f-1");
    let results = host.inner.results();
    assert!(
        results[0].0 && results[0].1.contains("Attached file \"x.zip\""),
        "{results:?}"
    );
}

/// The same for `doc_create` (PLAN.md T97): the host's link to the master
/// reaches the tool through the wrapper (so a plain chat's file is made by
/// the master and not written into the folder), and the file the call made
/// reaches the call's event as `made`.
#[tokio::test]
async fn the_hosts_docs_source_reaches_the_tool_and_the_file_reaches_the_log() {
    use harness_core::docs::{DocCreated, DocRequest, DocSource};
    struct Source(Mutex<Vec<DocRequest>>);
    #[async_trait::async_trait]
    impl DocSource for Source {
        async fn create(&self, r: DocRequest) -> Result<DocCreated, String> {
            self.0.lock().unwrap().push(r);
            Ok(DocCreated {
                file_id: "f-1".into(),
                name: "plan.docx".into(),
                bytes: 9,
                sha256: "ab".repeat(32),
                mime: harness_core::docs::DOCX_MIME.into(),
                notes: vec![],
                text: String::new(),
            })
        }
    }
    struct Linked {
        inner: Host,
        source: Arc<Source>,
    }
    #[async_trait::async_trait]
    impl TurnHost for Linked {
        async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
            self.inner.emit(kind).await
        }
        async fn approve(&self, a: &str, b: &str, c: &Value, d: ToolRisk) -> Decision {
            self.inner.approve(a, b, c, d).await
        }
        fn docs(&self) -> Option<Arc<dyn DocSource>> {
            Some(self.source.clone())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let ws: PathBuf = dir.path().canonicalize().unwrap().join("ws");
    std::fs::create_dir(&ws).unwrap();
    let source = Arc::new(Source(Mutex::default()));
    let host = Linked {
        inner: Host {
            events: Mutex::default(),
            pin: None,
            owned: None,
            exec: None,
        },
        source: source.clone(),
    };
    let backend = NativeBackend::new(
        Arc::new(Script {
            calls: vec![("doc_create", json!({"path": "plan", "markdown": "# P"}))],
            before: Mutex::default(),
        }),
        vec![Arc::new(harness_tools::DocCreate)],
        NativeConfig::default(),
    );
    backend.run_turn(turn_of(&host, &ws)).await;
    assert_eq!(
        source.0.lock().unwrap().len(),
        1,
        "{:?}",
        host.inner.results()
    );
    assert!(
        std::fs::read_dir(&ws).unwrap().next().is_none(),
        "nothing in the folder"
    );
    let made = host
        .inner
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallFinished(f) => f.made.clone(),
            _ => None,
        })
        .expect("the finished call names the file");
    assert_eq!(made.file_id.as_deref(), Some("f-1"));
}

/// `turn` for any host.
fn turn_of<'a>(host: &'a dyn TurnHost, cwd: &Path) -> TurnCtx<'a> {
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
