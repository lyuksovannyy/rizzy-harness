//! `doc_create` and `sheet_create` on a node (PLAN.md T97): the capability is
//! announced only where a tool is registered and not denied; in a chat
//! without a folder a call becomes one `docs.request` for its own session,
//! whose answer is named in the call's `tool_call_finished.made` and nothing
//! is written to any folder; in a chat with one the file is written there,
//! never over a file that exists, and nobody is asked.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::SessionId;
use harness_core::docs::{DocCreated, DocRequest};
use harness_core::event::{EventKind, MadeFile, Profile};
use harness_core::policy::ApprovalMode;
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{CAP_DOCS, MasterToNode, NodeToMaster, PromptMsg};
use harness_store::{Durability, Store};

const SECRET: &str = "sk-live-9f8e7d6c5b4a39281706";

struct Node {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    rt: NodeRuntime,
    link: NodeLink,
    caps: Vec<String>,
    seen: Vec<EventKind>,
    mark: usize,
}

async fn node(profile: Profile, register: bool, denied: &[&str]) -> Node {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let ws = root.join("ws");
    std::fs::create_dir(&ws).unwrap();
    // A plain chat's runtime has only the tools it is given; a coding node
    // has the built-in set, which now holds the two.
    let mut tools: harness_tools::ToolSet = if profile == Profile::Chat {
        vec![]
    } else {
        harness_tools::builtin()
    };
    if register && profile == Profile::Chat {
        tools.push(Arc::new(harness_tools::DocCreate));
        tools.push(Arc::new(harness_tools::SheetCreate));
    }
    let rt = NodeRuntime::start(NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![root.clone()],
            max_approval_mode: ApprovalMode::Auto,
            denied_tools: denied.iter().map(|d| d.to_string()).collect(),
            ..Default::default()
        },
        store: Store::open(&root.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            tools,
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![harness_core::Secret::new(SECRET.to_string())]),
        profile,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    })
    .await
    .unwrap();
    let mut link = rt.attach().await;
    let caps = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("{other:?}"),
    };
    Node {
        _dir: dir,
        ws,
        rt,
        link,
        caps,
        seen: vec![],
        mark: 0,
    }
}

impl Node {
    async fn next(&mut self) -> NodeToMaster {
        let m = tokio::time::timeout(Duration::from_secs(30), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed");
        if let NodeToMaster::Event { event } = &m
            && let Ok(k) = event.typed()
        {
            self.seen.push(k);
        }
        m
    }

    async fn ack(&mut self) -> Result<(), String> {
        loop {
            if let NodeToMaster::Ack { ok, error, .. } = self.next().await {
                return if ok {
                    Ok(())
                } else {
                    Err(error.unwrap_or_default())
                };
            }
        }
    }

    async fn create(&mut self, profile: Profile) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: self.ws.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
                profile: Some(profile),
                agent: None,
                incognito: false,
                no_callbacks: false,
                exec_mode: None,
            })
            .unwrap();
        self.ack().await.unwrap();
        sid
    }

    async fn prompt(&mut self, sid: SessionId, text: &str) {
        self.mark = self.seen.len();
        self.link
            .to_node
            .try_send(MasterToNode::Prompt(PromptMsg::test(sid, text)))
            .unwrap();
        self.ack().await.unwrap();
    }

    /// Read until the turn ends, answering each `docs.request`; the requests
    /// seen and the finished calls (ok, output, made) of this prompt.
    async fn until_end(
        &mut self,
        sid: SessionId,
        answer: &dyn Fn(&DocRequest) -> Result<DocCreated, String>,
    ) -> (
        Vec<(SessionId, DocRequest)>,
        Vec<(bool, String, Option<MadeFile>)>,
    ) {
        let mut asked = Vec::new();
        loop {
            match self.next().await {
                NodeToMaster::Docs {
                    req_id,
                    session_id,
                    request,
                } => {
                    let (a, e) = match answer(&request) {
                        Ok(a) => (Some(a), None),
                        Err(e) => (None, Some(e)),
                    };
                    asked.push((session_id, request));
                    self.link
                        .to_node
                        .try_send(MasterToNode::DocsResult {
                            req_id,
                            answer: a,
                            error: e,
                        })
                        .unwrap();
                }
                NodeToMaster::Event { event } if event.session_id == sid => {
                    if matches!(event.typed(), Ok(EventKind::TurnEnded(_))) {
                        break;
                    }
                }
                _ => {}
            }
        }
        let results = self.seen[self.mark..]
            .iter()
            .filter_map(|k| match k {
                EventKind::ToolCallFinished(f) => Some((f.ok, f.output.clone(), f.made.clone())),
                _ => None,
            })
            .collect();
        (asked, results)
    }
}

fn made(req: &DocRequest) -> Result<DocCreated, String> {
    let (name, mime) = match req {
        DocRequest::Doc { name, .. } => (format!("{name}.docx"), harness_core::docs::DOCX_MIME),
        DocRequest::Sheet { name, .. } => (format!("{name}.xlsx"), harness_core::docs::XLSX_MIME),
    };
    Ok(DocCreated {
        file_id: "0123456789abcdef0123456789abcdef".into(),
        name,
        bytes: 4321,
        sha256: "cd".repeat(32),
        mime: mime.into(),
        notes: vec!["1 paragraphs, 0 tables, 0 numbered lists, 0 links.".into()],
    })
}

const DOC: &str =
    r##"script: [{"tool": "doc_create", "input": {"path": "plan", "markdown": "# Plan"}}]"##;

#[tokio::test]
async fn the_capability_follows_the_tools() {
    for (register, denied, want) in [
        (true, vec![], true),
        (true, vec!["doc_create"], true),
        (true, vec!["doc_create", "sheet_create"], false),
        (false, vec![], false),
    ] {
        let n = node(Profile::Chat, register, &denied).await;
        assert_eq!(n.caps.iter().any(|c| c == CAP_DOCS), want, "{:?}", n.caps);
        n.rt.stop(Duration::from_secs(10)).await;
    }
}

/// A plain chat's call asks the master for its own session and nothing else;
/// the answer is named in the finished call's `made`; nothing is written to
/// the folder.
#[tokio::test]
async fn a_plain_chat_asks_the_master_and_names_the_file_in_its_own_log() {
    let mut n = node(Profile::Chat, true, &[]).await;
    let sid = n.create(Profile::Chat).await;
    n.prompt(sid, DOC).await;
    let (asked, results) = n.until_end(sid, &made).await;
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].0, sid);
    match &asked[0].1 {
        DocRequest::Doc { name, markdown, .. } => {
            assert_eq!((name.as_str(), markdown.as_str()), ("plan", "# Plan"));
        }
        other => panic!("{other:?}"),
    }
    let (ok, out, made) = &results[0];
    assert!(ok, "{out}");
    let made = made.as_ref().expect("the call's event names the file");
    assert_eq!(
        made.file_id.as_deref(),
        Some("0123456789abcdef0123456789abcdef")
    );
    assert_eq!((made.name.as_str(), made.size), ("plan.docx", 4321));
    assert!(made.path.is_none());
    assert!(out.contains("0123456789abcdef"), "{out}");
    assert!(
        std::fs::read_dir(&n.ws).unwrap().next().is_none(),
        "nothing is written to any folder"
    );

    // The master's refusal is a plain tool error with no file.
    n.prompt(sid, DOC).await;
    let no = |_: &DocRequest| Err("your storage is full".to_string());
    let (_, results) = n.until_end(sid, &no).await;
    assert!(
        !results[0].0 && results[0].1.contains("storage is full") && results[0].2.is_none(),
        "{results:?}"
    );
    n.rt.stop(Duration::from_secs(10)).await;
}

/// The name the model chose is scrubbed from the log, the path of a file in
/// a folder too.
#[tokio::test]
async fn a_secret_in_a_name_is_not_in_the_event() {
    let mut n = node(Profile::Chat, true, &[]).await;
    let sid = n.create(Profile::Chat).await;
    n.prompt(
        sid,
        &format!(
            r##"script: [{{"tool": "doc_create", "input": {{"path": "x", "markdown": "# P"}}}}]"##
        ),
    )
    .await;
    let leak = |r: &DocRequest| {
        let mut c = made(r).unwrap();
        c.name = format!("{SECRET}.docx");
        Ok(c)
    };
    let (_, results) = n.until_end(sid, &leak).await;
    let made = results[0].2.as_ref().unwrap();
    assert!(!made.name.contains(SECRET), "{made:?}");
    assert!(!results[0].1.contains(SECRET), "{}", results[0].1);
    n.rt.stop(Duration::from_secs(10)).await;

    let mut a = node(Profile::Agentic, true, &[]).await;
    let sid = a.create(Profile::Agentic).await;
    a.prompt(
        sid,
        &format!(r##"script: [{{"tool": "doc_create", "input": {{"path": "{SECRET}", "markdown": "# P"}}}}]"##),
    )
    .await;
    let never = |_: &DocRequest| panic!("a coding chat asks nobody");
    let (_, results) = a.until_end(sid, &never).await;
    let made = results[0].2.as_ref().unwrap();
    assert!(
        !made.name.contains(SECRET) && !made.path.as_deref().unwrap().contains(SECRET),
        "{made:?}"
    );
    a.rt.stop(Duration::from_secs(10)).await;
}

/// A chat with a folder writes there, as `write` does: a new file, a new
/// name when one exists, and the log's `seen` says it did not exist before.
#[tokio::test]
async fn a_coding_chat_writes_into_its_folder_and_asks_nobody() {
    let mut n = node(Profile::Agentic, true, &[]).await;
    let sid = n.create(Profile::Agentic).await;
    std::fs::write(n.ws.join("plan.docx"), b"mine").unwrap();
    n.prompt(sid, DOC).await;
    let never = |_: &DocRequest| panic!("a coding chat asks nobody");
    let (asked, results) = n.until_end(sid, &never).await;
    assert!(asked.is_empty());
    let (ok, out, made) = &results[0];
    assert!(ok, "{out}");
    let made = made.as_ref().unwrap();
    assert_eq!(made.name, "plan (2).docx");
    assert!(made.file_id.is_none() && made.path.as_deref().unwrap().ends_with("plan (2).docx"));
    assert_eq!(std::fs::read(n.ws.join("plan.docx")).unwrap(), b"mine");
    assert!(n.ws.join("plan (2).docx").is_file());
    let seen = n
        .seen
        .iter()
        .find_map(|k| match k {
            EventKind::ToolCallFinished(f) => f.seen.clone(),
            _ => None,
        })
        .expect("the guard's record");
    assert_eq!(seen.base.as_deref(), Some(""));
    assert_eq!(seen.sha256, made.sha256);
    n.rt.stop(Duration::from_secs(10)).await;
}
