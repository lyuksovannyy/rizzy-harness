//! Files the person attached that the model cannot read as text (PLAN.md
//! T94, docs/plan/X.53.md), on a node, over the link as the master drives
//! it: chunks staged, a prompt that names them saves them into the chat's
//! folder under `attachments/` before the turn's first checkpoint, the
//! message records them, the model is told where they are (rebuilt the
//! same at every step), and every limit and refusal is the node's own.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_agent_native::provider::{ChatRequest, Completion, Provider, ProviderError};
use harness_agent_native::{NativeBackend, NativeConfig};
use harness_core::SessionId;
use harness_core::event::*;
use harness_core::policy::ApprovalMode;
use harness_node::artifacts::StageConfig;
use harness_node::policy::LimitsPolicy;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MAX_CHUNK, MasterToNode, NodeToMaster, PromptMsg, UploadRef};
use harness_store::{Durability, Store};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, mpsc};

/// A model that records every request and, when told, holds the first one
/// until the test opens the gate (a turn known to be running).
struct Model {
    seen: Mutex<Vec<ChatRequest>>,
    hold_first: bool,
    entered: Notify,
    gate: Notify,
    /// Step 0 writes this file, so the turn changes something undo can undo.
    write: Option<(String, String)>,
}

#[async_trait::async_trait]
impl Provider for Model {
    fn name(&self) -> &'static str {
        "model"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.seen.lock().unwrap();
            s.push(req.clone());
            s.len() - 1
        };
        if n == 0 && self.hold_first {
            self.entered.notify_one();
            self.gate.notified().await;
        }
        let tool_calls = match (&self.write, n) {
            (Some((path, content)), 0) => vec![ToolCallRef {
                call_id: harness_core::new_id(),
                tool: "write".into(),
                input: json!({"path": path, "content": content}),
            }],
            _ => vec![],
        };
        Ok(Completion {
            text: if tool_calls.is_empty() { "done" } else { "" }.into(),
            tool_calls,
            usage: None,
            stop: Default::default(),
            thinking: None,
        })
    }
}

struct N {
    dir: tempfile::TempDir,
    node: NodeRuntime,
    link: NodeLink,
    model: Arc<Model>,
    capabilities: Vec<String>,
}

fn sha(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

async fn start_with(profile: Profile, limits: LimitsPolicy, stage: bool, model: Model) -> N {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("ws")).unwrap();
    let model = Arc::new(model);
    let node = NodeRuntime::start(NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().canonicalize().unwrap()],
            max_approval_mode: ApprovalMode::Auto,
            limits,
            ..Default::default()
        },
        store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            model.clone(),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile,
        incognito_ttl: None,
        checkpoints_dir: Some(dir.path().join("checkpoints")),
        git_sandbox: None,
        artifacts: stage.then(|| StageConfig::new(dir.path().join("data"))),
    })
    .await
    .unwrap();
    let mut link = node.attach().await;
    let capabilities = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("expected hello, got {other:?}"),
    };
    N {
        dir,
        node,
        link,
        model,
        capabilities,
    }
}

fn plain_model() -> Model {
    Model {
        seen: Mutex::new(vec![]),
        hold_first: false,
        entered: Notify::new(),
        gate: Notify::new(),
        write: None,
    }
}

async fn start() -> N {
    start_with(
        Profile::Agentic,
        LimitsPolicy::default(),
        true,
        plain_model(),
    )
    .await
}

impl N {
    fn ws(&self) -> PathBuf {
        self.dir.path().join("ws").canonicalize().unwrap()
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(20), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn ack(&mut self) -> Result<(), String> {
        loop {
            match self.next().await {
                NodeToMaster::Ack { ok: true, .. } => return Ok(()),
                NodeToMaster::Ack { error, .. } => return Err(error.unwrap_or_default()),
                _ => {}
            }
        }
    }

    async fn create(&mut self) -> SessionId {
        self.create_in(None).await
    }

    async fn create_in(&mut self, profile: Option<Profile>) -> SessionId {
        let sid = SessionId::new();
        let cwd = self.ws();
        self.send(MasterToNode::Create {
            req_id: format!("c-{sid}"),
            session_id: sid,
            title: "t".into(),
            cwd: cwd.display().to_string(),
            model: None,
            approval_mode: Some(ApprovalMode::Auto),
            profile,
            agent: None,
            incognito: false,
            no_callbacks: false,
            exec_mode: None,
        });
        self.ack().await.unwrap();
        sid
    }

    /// Push `bytes` as upload `id`, a chunk at a time, each acked.
    async fn push(&mut self, sid: SessionId, id: &str, bytes: &[u8]) -> Result<UploadRef, String> {
        let r = UploadRef {
            upload_id: id.into(),
            name: "x".into(),
            mime: "application/zip".into(),
            size: bytes.len() as u64,
            sha256: sha(bytes),
        };
        let mut offset = 0usize;
        for c in bytes.chunks(MAX_CHUNK as usize) {
            self.send(MasterToNode::UploadChunk {
                req_id: format!("u-{id}-{offset}"),
                session_id: sid,
                upload_id: id.into(),
                offset: offset as u64,
                size: r.size,
                sha256: r.sha256.clone(),
                data: base64::engine::general_purpose::STANDARD.encode(c),
            });
            self.ack().await?;
            offset += c.len();
        }
        Ok(r)
    }

    fn prompt(&self, sid: SessionId, text: &str, uploads: Vec<UploadRef>) -> MasterToNode {
        MasterToNode::Prompt(PromptMsg {
            req_id: format!("p-{}", harness_core::new_id()),
            uploads,
            ..PromptMsg::test(sid, text)
        })
    }

    async fn turn(
        &mut self,
        sid: SessionId,
        text: &str,
        uploads: Vec<UploadRef>,
    ) -> Result<(), String> {
        self.send(self.prompt(sid, text, uploads));
        self.ack().await?;
        loop {
            if let NodeToMaster::Event { event } = self.next().await
                && matches!(event.typed(), Ok(EventKind::TurnEnded(_)))
                && event.session_id == sid
            {
                return Ok(());
            }
        }
    }

    fn log(&self, sid: SessionId) -> Vec<harness_core::RawEvent> {
        self.node.store().events_after(sid, 0, 100_000).unwrap()
    }

    fn messages(&self, sid: SessionId) -> Vec<UserMessage> {
        self.log(sid)
            .iter()
            .filter_map(|e| match e.typed() {
                Ok(EventKind::UserMessage(m)) => Some(m),
                _ => None,
            })
            .collect()
    }

    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.ws().join("attachments"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

fn zip_bytes(n: usize) -> Vec<u8> {
    let mut v = b"PK\x03\x04".to_vec();
    v.extend((0..n).map(|i| (i % 253) as u8));
    v
}

fn id(n: u8) -> String {
    format!("{n:032x}")
}

/// The whole path: pushed in chunks, named by a prompt, saved under
/// `attachments/`, recorded on the message, shown to the model in one line.
#[tokio::test]
async fn a_pushed_file_is_saved_recorded_and_named_to_the_model() {
    let mut n = start().await;
    assert!(
        n.capabilities.contains(&"uploads".to_string()),
        "{:?}",
        n.capabilities
    );
    assert!(n.capabilities.contains(&"upload-max:10".to_string()));
    assert!(
        n.capabilities
            .contains(&"uploads-per-message:10".to_string())
    );
    let sid = n.create().await;
    // Three chunks: more than the link's chunk size.
    let bytes = zip_bytes(MAX_CHUNK as usize * 2 + 77);
    let mut r = n.push(sid, &id(1), &bytes).await.unwrap();
    r.name = "../../Q3 report.zip".into();
    n.turn(sid, "Unpack it.", vec![r]).await.unwrap();
    assert_eq!(
        std::fs::read(n.ws().join("attachments/Q3 report.zip")).unwrap(),
        bytes
    );
    let m = &n.messages(sid)[0];
    assert_eq!(m.uploads.len(), 1);
    let u = &m.uploads[0];
    assert_eq!(
        (u.path.as_str(), u.bytes, u.sha256.as_str(), u.mime.as_str()),
        (
            "attachments/Q3 report.zip",
            bytes.len() as u64,
            sha(&bytes).as_str(),
            "application/zip"
        )
    );
    // The model's request carries the one line, the same at every step
    // (the line is built from the log alone).
    let seen = n.model.seen.lock().unwrap().clone();
    let user = seen[0]
        .messages
        .iter()
        .filter_map(|m| m.user_text())
        .find(|t| t.contains("Unpack it."))
        .expect("the person's message")
        .to_string();
    assert!(
        user.contains("[harness] The person attached 1 file, saved in this chat's folder: \"attachments/Q3 report.zip\" (512.1 KB)"),
        "{user}"
    );
    assert!(user.ends_with(&m.model_text()), "{user}");
    // Staged copies are gone.
    assert!(
        std::fs::read_dir(n.dir.path().join("data/uploads-stage"))
            .unwrap()
            .flatten()
            .all(|d| std::fs::read_dir(d.path()).unwrap().count() == 0),
        "the staged file stays"
    );
}

/// A second message with the same file name does not overwrite the first.
#[tokio::test]
async fn the_same_name_in_a_second_message_gets_a_number() {
    let mut n = start().await;
    let sid = n.create().await;
    for (i, body) in [b"first".as_slice(), b"second".as_slice()]
        .into_iter()
        .enumerate()
    {
        let mut r = n.push(sid, &id(i as u8 + 1), body).await.unwrap();
        r.name = "x.zip".into();
        n.turn(sid, "go", vec![r]).await.unwrap();
    }
    assert_eq!(n.files(), ["x (2).zip", "x.zip"]);
    assert_eq!(
        std::fs::read(n.ws().join("attachments/x.zip")).unwrap(),
        b"first"
    );
    let paths: Vec<_> = n
        .messages(sid)
        .iter()
        .flat_map(|m| m.uploads.iter().map(|u| u.path.clone()))
        .collect();
    assert_eq!(paths, ["attachments/x.zip", "attachments/x (2).zip"]);
}

/// The files are saved before the turn's `before` checkpoint, so undoing
/// the turn removes what the agent wrote and leaves what the person
/// attached.
#[tokio::test]
async fn undoing_the_turn_leaves_the_attached_file() {
    let model = Model {
        write: Some(("made.txt".into(), "by the agent".into())),
        ..plain_model()
    };
    let mut n = start_with(Profile::Agentic, LimitsPolicy::default(), true, model).await;
    let sid = n.create().await;
    let r = n.push(sid, &id(1), b"person's bytes").await.unwrap();
    n.turn(sid, "go", vec![r]).await.unwrap();
    assert!(n.ws().join("made.txt").exists());
    let turn_id = n
        .log(sid)
        .iter()
        .find_map(|e| match e.typed() {
            Ok(EventKind::TurnEnded(t)) => Some(t.turn_id),
            _ => None,
        })
        .unwrap();
    n.send(MasterToNode::Restore {
        req_id: "r".into(),
        session_id: sid,
        turn_id,
        force: false,
        rewind: false,
    });
    n.ack().await.unwrap();
    assert!(
        !n.ws().join("made.txt").exists(),
        "the agent's file is undone"
    );
    assert_eq!(
        std::fs::read(n.ws().join("attachments/x")).unwrap(),
        b"person's bytes",
        "the person's file stays"
    );
}

/// A chat without tools takes no file: its chunks are refused at once with
/// the reason, and nothing is staged. So does a node with no staging
/// folder.
#[tokio::test]
async fn a_chat_without_tools_and_a_node_without_staging_refuse() {
    let mut chat = start_with(Profile::Chat, LimitsPolicy::default(), true, plain_model()).await;
    assert!(!chat.capabilities.contains(&"uploads".to_string()));
    let sid = chat.create_in(Some(Profile::Chat)).await;
    let e = chat.push(sid, &id(1), b"bytes").await.unwrap_err();
    assert!(
        e.contains("cannot open files") && e.contains("switch to Agentic"),
        "{e}"
    );

    let mut bare = start_with(
        Profile::Agentic,
        LimitsPolicy::default(),
        false,
        plain_model(),
    )
    .await;
    assert!(!bare.capabilities.contains(&"uploads".to_string()));
    let sid = bare.create().await;
    let e = bare.push(sid, &id(1), b"bytes").await.unwrap_err();
    assert!(e.contains("saves no uploaded files"), "{e}");
}

/// The node's own limits hold whatever the master sends: a file over
/// `upload_max_mb` is refused at its first chunk; more files than
/// `uploads_per_message` refuse the prompt and nothing is written.
#[tokio::test]
async fn the_nodes_limits_decide() {
    let limits = LimitsPolicy {
        upload_max_mb: 1,
        uploads_per_message: 2,
        ..Default::default()
    };
    let mut n = start_with(Profile::Agentic, limits, true, plain_model()).await;
    assert!(n.capabilities.contains(&"upload-max:1".to_string()));
    assert!(
        n.capabilities
            .contains(&"uploads-per-message:2".to_string())
    );
    let sid = n.create().await;
    let e = n
        .push(sid, &id(1), &zip_bytes(1024 * 1024 + 1))
        .await
        .unwrap_err();
    assert!(e.contains("up to 1 MiB"), "{e}");
    // Three small files, two allowed.
    let mut rs = vec![];
    for i in 0..3 {
        rs.push(n.push(sid, &id(10 + i), b"abc").await.unwrap());
    }
    n.send(n.prompt(sid, "go", rs.clone()));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("at most 2 attached files"), "{e}");
    assert_eq!(n.files(), Vec::<String>::new());
    assert!(n.messages(sid).is_empty(), "nothing was recorded");
    // Two are fine, and the refused attempt left their staged copies for it.
    n.turn(sid, "go", rs[..2].to_vec()).await.unwrap();
    assert_eq!(n.files().len(), 2);
    // A prompt that claims a bigger limit than the node's: refused too.
    let mut lie = n.push(sid, &id(20), b"abc").await.unwrap();
    lie.size = 3 << 30;
    n.send(n.prompt(sid, "go", vec![lie]));
    assert!(n.ack().await.is_err());
}

/// Nothing is saved for an id that did not arrive, another chat's id, a
/// wrong sha256 or size in the prompt, a damaged transfer, or a second
/// use of the same id.
#[tokio::test]
async fn an_unknown_foreign_wrong_or_used_upload_saves_nothing() {
    let mut n = start().await;
    let a = n.create().await;
    let b = n.create().await;
    let r = n.push(a, &id(1), b"bytes of a").await.unwrap();
    // Never pushed.
    let ghost = UploadRef {
        upload_id: id(9),
        ..r.clone()
    };
    n.send(n.prompt(a, "go", vec![ghost]));
    assert!(
        n.ack()
            .await
            .unwrap_err()
            .contains("did not reach this node")
    );
    // Another chat names it.
    n.send(n.prompt(b, "go", vec![r.clone()]));
    assert!(n.ack().await.is_err());
    // A hash or size the bytes do not have.
    let mut lie = r.clone();
    lie.sha256 = sha(b"other");
    n.send(n.prompt(a, "go", vec![lie]));
    assert!(n.ack().await.is_err());
    let mut lie = r.clone();
    lie.size += 1;
    n.send(n.prompt(a, "go", vec![lie]));
    assert!(n.ack().await.is_err());
    assert_eq!(n.files(), Vec::<String>::new());
    assert!(n.messages(a).is_empty() && n.messages(b).is_empty());
    // A chunk whose bytes are not the declared hash: refused at the last one.
    n.send(MasterToNode::UploadChunk {
        req_id: "bad".into(),
        session_id: a,
        upload_id: id(2),
        offset: 0,
        size: 5,
        sha256: sha(b"right"),
        data: base64::engine::general_purpose::STANDARD.encode(b"wrong"),
    });
    assert!(n.ack().await.unwrap_err().contains("sha256"));
    // The real one goes through, once; its id is then spent.
    n.turn(a, "go", vec![r.clone()]).await.unwrap();
    assert_eq!(n.files(), ["x"]);
    n.send(n.prompt(a, "again", vec![r]));
    assert!(n.ack().await.is_err(), "an upload id is used once");
    assert_eq!(n.files(), ["x"]);
}

/// A file saved in a message sent while a turn runs would land inside
/// that turn's changes, and undoing it would delete the file: such a
/// message is refused until the turn has ended.
#[tokio::test]
async fn a_message_with_files_waits_for_the_running_turn() {
    let model = Model {
        hold_first: true,
        ..plain_model()
    };
    let mut n = start_with(Profile::Agentic, LimitsPolicy::default(), true, model).await;
    let sid = n.create().await;
    n.send(n.prompt(sid, "first", vec![]));
    n.ack().await.unwrap();
    n.model.entered.notified().await;
    let r = n.push(sid, &id(1), b"bytes").await.unwrap();
    n.send(n.prompt(sid, "second", vec![r.clone()]));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("waits until it ends"), "{e}");
    assert_eq!(n.files(), Vec::<String>::new());
    n.model.gate.notify_one();
    // Once the turn has ended, the same upload is taken.
    loop {
        if let NodeToMaster::Event { event } = n.next().await
            && matches!(event.typed(), Ok(EventKind::TurnEnded(_)))
        {
            break;
        }
    }
    n.turn(sid, "second", vec![r]).await.unwrap();
    assert_eq!(n.files(), ["x"]);
}

/// A folder where `attachments` is planted as a link out of the folder:
/// the prompt is refused with the cause, nothing is written outside, and
/// no message is recorded.
#[tokio::test]
async fn a_planted_attachments_link_refuses_the_prompt() {
    let mut n = start().await;
    let sid = n.create().await;
    let outside = n.dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, n.ws().join("attachments")).unwrap();
    let r = n.push(sid, &id(1), b"bytes").await.unwrap();
    n.send(n.prompt(sid, "go", vec![r.clone()]));
    let e = n.ack().await.unwrap_err();
    assert!(
        e.contains("is a link") && e.contains("nothing was saved"),
        "{e}"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    assert!(n.messages(sid).is_empty());
    // The person (or the agent) removes the link: the same upload is taken.
    std::fs::remove_file(n.ws().join("attachments")).unwrap();
    n.turn(sid, "go", vec![r]).await.unwrap();
    assert_eq!(n.files(), ["x"]);
}

/// The file's name is text and is scrubbed in the event like any text;
/// the saved bytes are the person's own and are not.
#[tokio::test]
async fn the_name_is_scrubbed_in_the_event_but_the_bytes_are_not() {
    let dir_secret = "sk-test-secret-value-0123456789";
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("ws")).unwrap();
    let model = Arc::new(plain_model());
    let node = NodeRuntime::start(NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().canonicalize().unwrap()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            model.clone(),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(
            dir_secret.to_string(),
        )]),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: Some(StageConfig::new(dir.path().join("data"))),
    })
    .await
    .unwrap();
    let link = node.attach().await;
    let mut n = N {
        dir,
        node,
        link,
        model,
        capabilities: vec![],
    };
    let hello = n.next().await;
    let _ = hello;
    let sid = n.create().await;
    let bytes = format!("token={dir_secret}");
    let mut r = n.push(sid, &id(1), bytes.as_bytes()).await.unwrap();
    r.name = format!("{dir_secret}.txt");
    n.turn(sid, "go", vec![r]).await.unwrap();
    let u = &n.messages(sid)[0].uploads[0];
    assert!(
        !u.path.contains(dir_secret) && !u.name.contains(dir_secret),
        "{u:?}"
    );
    // The path the event gives is the real one, and the bytes are as sent.
    assert_eq!(
        std::fs::read(n.ws().join(&u.path)).unwrap(),
        bytes.as_bytes()
    );
    let _: Option<&Path> = None;
}

/// A name whose secret is split by a zero-width character: the recorded
/// path is the file on disk, not a path the scrub rewrote afterwards.
#[tokio::test]
async fn the_recorded_path_is_the_file_on_disk_when_a_split_secret_is_scrubbed() {
    let secret = "sk-test-secret-value-0123456789";
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("ws")).unwrap();
    let model = Arc::new(plain_model());
    let node = NodeRuntime::start(NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.path().canonicalize().unwrap()],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.path().join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            model.clone(),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(secret.to_string())]),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: Some(StageConfig::new(dir.path().join("data"))),
    })
    .await
    .unwrap();
    let link = node.attach().await;
    let mut n = N {
        dir,
        node,
        link,
        model,
        capabilities: vec![],
    };
    let _hello = n.next().await;
    let sid = n.create().await;
    let mut r = n.push(sid, &id(1), b"bytes").await.unwrap();
    let (a, b) = secret.split_at(10);
    r.name = format!("{a}\u{200b}{b}.txt");
    n.turn(sid, "go", vec![r]).await.unwrap();
    let u = &n.messages(sid)[0].uploads[0];
    assert!(!u.path.contains(secret), "{u:?}");
    assert_eq!(
        std::fs::read(n.ws().join(&u.path)).unwrap(),
        b"bytes",
        "{u:?}"
    );
}

/// A prompt that names one upload twice saves nothing (an honest master
/// cannot send it).
#[tokio::test]
async fn the_same_upload_named_twice_saves_nothing() {
    let mut n = start().await;
    let sid = n.create().await;
    let r = n.push(sid, &id(1), b"bytes").await.unwrap();
    n.send(n.prompt(sid, "go", vec![r.clone(), r.clone()]));
    let e = n.ack().await.unwrap_err();
    assert!(e.contains("more than once"), "{e}");
    assert_eq!(n.files(), Vec::<String>::new());
    assert!(n.messages(sid).is_empty());
    // Once is fine.
    n.turn(sid, "go", vec![r]).await.unwrap();
    assert_eq!(n.files(), ["x"]);
}
