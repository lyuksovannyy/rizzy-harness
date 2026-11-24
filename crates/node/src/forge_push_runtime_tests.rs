//! `push` through the node's runtime as the master drives it (X.27 step 4,
//! PLAN.md T101), with the scripted agent and the clone's fake forge on
//! 127.0.0.1: a turn in Auto mode with the master's target shows a card;
//! Allow asks the master for the credential with the card's approval id
//! and the forge gets the commit; Deny sends nothing and asks for no
//! credential; a branch that moved after the card is not pushed; a second
//! push while one runs is refused; a routine's chat, a turn without a
//! target and a node that does not push never offer it; the
//! token is in no event, log line, audit line or file of the node.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::EventKind;
use harness_core::forge::PushTarget;
use harness_core::policy::{ApprovalMode, Decision};
use harness_core::{Secret, SessionId};
use harness_protocol::{MasterToNode, NodeToMaster, PromptMsg};

use crate::forge_clone::tests::{CANARY, Logs, Mode, USER, bare, files_with_token, git_in, server};
use crate::policy::NodePolicy;

const SCRIPT: &str = r#"script: [{"tool": "push", "input": {}}]"#;

struct Rig {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    data: PathBuf,
    folder: PathBuf,
    remote: PathBuf,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    bare(&root, "o/r", |_| {});
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let folder = root.join("ws/proj");
    std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
    git_in(
        &root,
        &[
            "clone",
            "-q",
            root.join("o/r.git").to_str().unwrap(),
            folder.to_str().unwrap(),
        ],
    );
    std::fs::write(folder.join("work.txt"), "agent's work\n").unwrap();
    git_in(&folder, &["add", "-A"]);
    git_in(&folder, &["commit", "-q", "-m", "the agent's commit"]);
    Rig {
        remote: root.join("o/r.git"),
        _tmp: tmp,
        root,
        data,
        folder,
    }
}

impl Rig {
    fn policy(&self, hosts: &[String], push: bool) -> NodePolicy {
        let mut p = NodePolicy {
            workspace_roots: vec![self.root.join("ws")],
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        };
        p.skills.roots.clear();
        p.projects.dir = Some(self.root.join("ws/projects"));
        p.forge.hosts = hosts.to_vec();
        p.forge.push = push;
        p.limits.clone_timeout_s = 4;
        p.protect(&self.data);
        p.locate_projects(&self.data).unwrap();
        p
    }

    fn remote_ref(&self, branch: &str) -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.remote)
            .args([
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn head(&self) -> String {
        git_in(&self.folder, &["rev-parse", "HEAD"])
    }
}

struct Node {
    rt: crate::NodeRuntime,
    link: crate::NodeLink,
    caps: Vec<String>,
    store: harness_store::Store,
}

async fn node(r: &Rig, policy: NodePolicy) -> Node {
    let mut tools = harness_tools::builtin();
    tools.push(Arc::new(harness_tools::Push));
    tools.push(Arc::new(harness_tools::PullRequest));
    let cfg = crate::NodeConfig {
        node_id: "n".into(),
        hostname: "test".into(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(1),
            }),
            tools,
            NativeConfig::default(),
        )),
        policy,
        store: harness_store::Store::open(&r.data.join("node.db"), harness_store::Durability::Full)
            .unwrap(),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: harness_core::event::Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: None,
        git_sandbox: None,
        artifacts: None,
    };
    let store = cfg.store.clone();
    let rt = crate::NodeRuntime::start(cfg).await.unwrap();
    let mut link = rt.attach().await;
    let caps = match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { capabilities, .. } => capabilities,
        other => panic!("{other:?}"),
    };
    Node {
        rt,
        link,
        caps,
        store,
    }
}

fn spec() -> harness_core::agent::AgentSpec {
    serde_json::from_value(serde_json::json!({
        "agent_id": "coder", "name": "Coder", "system_prompt": "", "prompt_hash": "h",
        "tools": ["push", "pull_request", "read"]
    }))
    .unwrap()
}

fn target(host: &str) -> PushTarget {
    PushTarget {
        host: host.into(),
        full_name: "o/r".into(),
        connection_id: "conn1".into(),
        login: USER.into(),
        pull_requests: true,
    }
}

/// How the fake master answers.
#[derive(Clone, Copy, PartialEq)]
enum Answer {
    Allow,
    Deny,
    /// A commit in the folder between the card and the Allow.
    MoveThenAllow,
    /// The session's target taken away (as when another turn started)
    /// between the card and the Allow.
    LoseTargetThenAllow,
    /// The turn stopped while the card is up.
    Cancel,
    /// Leave the card unanswered: return once it is up.
    Hold,
}

#[derive(Default, Debug)]
struct Seen {
    cards: Vec<(String, String, bool, bool)>,
    asked_credential: Vec<String>,
    results: Vec<(String, bool, String)>,
    events: Vec<String>,
    /// Sessions whose `session_created` names this one as parent.
    children: Vec<SessionId>,
}

impl Node {
    async fn next(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(60), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn ack(&mut self) {
        loop {
            if let NodeToMaster::Ack { ok, error, .. } = self.next().await {
                assert!(ok, "{error:?}");
                return;
            }
        }
    }

    async fn create(&mut self, cwd: &Path, no_callbacks: bool, incognito: bool) -> SessionId {
        let sid = SessionId::new();
        self.link
            .to_node
            .try_send(MasterToNode::Create {
                req_id: "c".into(),
                session_id: sid,
                title: "t".into(),
                cwd: cwd.display().to_string(),
                model: None,
                approval_mode: Some(ApprovalMode::Auto),
                profile: None,
                agent: None,
                incognito,
                no_callbacks,
                exec_mode: None,
            })
            .unwrap();
        self.ack().await;
        sid
    }

    async fn prompt(&mut self, sid: SessionId, push: Option<PushTarget>) {
        let mut m = PromptMsg::test(sid, SCRIPT);
        m.agent = Some(spec());
        m.context = Some(Box::new(harness_core::backend::MasterContext {
            push,
            ..Default::default()
        }));
        self.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
        self.ack().await;
    }

    /// Read until the turn of `sid` ends, answering cards with `answer` and
    /// credential requests with the canary token.
    async fn until_end(&mut self, sid: SessionId, answer: Answer, folder: &Path) -> Seen {
        let mut seen = Seen::default();
        loop {
            match self.next().await {
                NodeToMaster::PushCredential {
                    req_id,
                    session_id,
                    approval_id,
                } => {
                    assert_eq!(session_id, sid);
                    seen.asked_credential.push(approval_id);
                    self.link
                        .to_node
                        .try_send(MasterToNode::PushCredentialResult {
                            req_id,
                            username: Some(USER.into()),
                            token: Some(Secret::new(CANARY.into())),
                            error: None,
                        })
                        .unwrap();
                }
                NodeToMaster::Event { event }
                    if event.kind == "session_created"
                        && event.payload["parent_session"] == serde_json::json!(sid) =>
                {
                    seen.children.push(event.session_id);
                }
                NodeToMaster::Event { event } if event.session_id == sid => {
                    seen.events.push(event.payload.to_string());
                    match event.typed() {
                        Ok(EventKind::ApprovalRequested(a)) => {
                            seen.cards.push((
                                a.reason.clone(),
                                a.preview.clone().unwrap_or_default(),
                                a.turn_allow,
                                a.chat_allow,
                            ));
                            if answer == Answer::MoveThenAllow {
                                std::fs::write(folder.join("work.txt"), "moved\n").unwrap();
                                git_in(folder, &["commit", "-q", "-am", "moved"]);
                            }
                            if answer == Answer::Hold {
                                return seen;
                            }
                            if answer == Answer::LoseTargetThenAllow {
                                let s = self.rt.inner.session(sid).unwrap();
                                *s.push_now.lock().unwrap() = None;
                            }
                            if answer == Answer::Cancel {
                                self.link
                                    .to_node
                                    .try_send(MasterToNode::Cancel {
                                        req_id: "stop".into(),
                                        session_id: sid,
                                    })
                                    .unwrap();
                                continue;
                            }
                            let decision = if answer == Answer::Deny {
                                Decision::Deny
                            } else {
                                // The widest answer: it counts as this one call.
                                Decision::AllowChat
                            };
                            self.link
                                .to_node
                                .try_send(MasterToNode::ApprovalRespond {
                                    req_id: format!("a-{}", a.approval_id),
                                    session_id: sid,
                                    approval_id: a.approval_id.clone(),
                                    decision,
                                    note: None,
                                })
                                .unwrap();
                        }
                        Ok(EventKind::ToolCallFinished(f)) => {
                            seen.results
                                .push((f.call_id.clone(), f.ok, f.output.clone()))
                        }
                        Ok(EventKind::TurnEnded(_)) => return seen,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn no_token_in(seen: &Seen) {
    for e in &seen.events {
        assert!(!e.contains(CANARY), "{e}");
    }
}

/// Tool call → card in Auto mode → Allow → the master is asked with the
/// card's approval id → the forge has the commit. The card shows host,
/// name, login, branch, short id and subject and offers no wider grant;
/// the token is in no event, log line at TRACE, audit line or file.
#[tokio::test]
async fn auto_mode_shows_the_card_and_allow_pushes_the_pinned_commit() {
    let logs = Logs::default();
    let sink = logs.clone();
    let sub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    assert!(
        n.caps.iter().any(|c| c == harness_protocol::CAP_FORGE_PUSH),
        "{:?}",
        n.caps
    );
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    let seen = n.until_end(sid, Answer::Allow, &r.folder).await;
    let head = r.head();
    assert_eq!(seen.cards.len(), 1, "{seen:?}");
    let (reason, preview, turn_allow, chat_allow) = &seen.cards[0];
    assert!(!turn_allow && !chat_allow);
    assert!(
        reason.contains(&forge.host()) && reason.contains("o/r"),
        "{reason}"
    );
    // The folder's part only: the destination is the master's to show.
    assert_eq!(
        preview,
        &format!("branch: main\ncommit: {} the agent's commit", &head[..12])
    );
    assert_eq!(seen.asked_credential.len(), 1);
    let (_, ok, out) = &seen.results[0];
    assert!(ok, "{out}");
    assert!(out.starts_with("[harness] pushed"), "{out}");
    assert_eq!(r.remote_ref("main").as_deref(), Some(head.as_str()));
    no_token_in(&seen);
    for _ in 0..100 {
        if n.rt.inner.audit_log.pending().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let audit: Vec<String> = n
        .store
        .audit_lines("forge.push")
        .unwrap()
        .into_iter()
        .map(|(_, d)| d)
        .collect();
    assert!(
        audit
            .iter()
            .any(|d| d.contains(&head) && d.contains("\"ok\":true")),
        "{audit:?}"
    );
    assert!(audit.iter().all(|d| !d.contains(CANARY)));
    let logged = String::from_utf8_lossy(&logs.0.lock().unwrap()).to_string();
    assert!(!logged.contains(CANARY));
    assert_eq!(files_with_token(&r.root), Vec::<PathBuf>::new());
}

/// Deny: nothing is sent to the forge and no credential is asked for.
#[tokio::test]
async fn deny_sends_nothing_and_asks_for_no_credential() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    let seen = n.until_end(sid, Answer::Deny, &r.folder).await;
    assert_eq!(seen.cards.len(), 1);
    assert!(seen.asked_credential.is_empty());
    assert!(!seen.results[0].1);
    assert!(forge.seen().is_empty(), "{:?}", forge.seen());
}

/// (c) The branch moved between the card and the Allow: nothing pushed,
/// no credential asked for, and the model is told the branch moved.
#[tokio::test]
async fn a_branch_that_moved_after_the_card_is_not_pushed() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    let seen = n.until_end(sid, Answer::MoveThenAllow, &r.folder).await;
    assert!(seen.asked_credential.is_empty());
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("moved"), "{out}");
    assert!(forge.seen().is_empty());
}

/// Where the tool is never offered: no target in the turn, a routine's
/// chat, a node whose node.toml does not push (no capability either) or
/// does not list the target's host. (A temporary chat cannot be agentic on
/// a node; the master names no target for one: `master` tests.)
#[tokio::test]
async fn push_is_offered_only_with_a_target_in_a_chats_own_turn_on_a_pushing_node() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let unknown = |seen: &Seen| {
        assert!(seen.cards.is_empty(), "{seen:?}");
        assert!(seen.asked_credential.is_empty());
        let (_, ok, out) = &seen.results[0];
        assert!(!ok && out.contains("unknown tool"), "{out}");
    };
    {
        let mut n = node(&r, r.policy(&[forge.host()], true)).await;
        let sid = n.create(&r.folder, false, false).await;
        n.prompt(sid, None).await;
        unknown(&n.until_end(sid, Answer::Allow, &r.folder).await);
        // A routine's chat.
        let sid = n.create(&r.folder, true, false).await;
        n.prompt(sid, Some(target(&forge.host()))).await;
        unknown(&n.until_end(sid, Answer::Allow, &r.folder).await);
        // A host the node does not list.
        let sid = n.create(&r.folder, false, false).await;
        n.prompt(sid, Some(target("evil.example"))).await;
        unknown(&n.until_end(sid, Answer::Allow, &r.folder).await);
    }
    let mut n = node(&r, r.policy(&[forge.host()], false)).await;
    assert!(!n.caps.iter().any(|c| c == harness_protocol::CAP_FORGE_PUSH));
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    unknown(&n.until_end(sid, Answer::Allow, &r.folder).await);
    assert!(forge.seen().is_empty());
}

/// (g) One push at a time on a node: a second chat's push while the first
/// waits on a stalled forge is refused at its Allow. The first push's limit
/// is long and the test ends it by releasing the stalled forge after the
/// second chat's assertions, so no time limit races the second turn (CI,
/// PR 242: a 4 s limit ended the first push before the second Allow).
#[tokio::test]
async fn a_second_push_while_one_runs_is_refused() {
    let r = rig();
    let stall = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let mut p = r.policy(&[stall.host()], true);
    p.limits.clone_timeout_s = 120;
    let mut n = node(&r, p).await;
    let a = n.create(&r.folder, false, false).await;
    let b = n.create(&r.folder, false, false).await;
    n.prompt(a, Some(target(&stall.host()))).await;
    // Allow a's card and hand it its credential.
    loop {
        match n.next().await {
            NodeToMaster::Event { event } if event.session_id == a => {
                if let Ok(EventKind::ApprovalRequested(x)) = event.typed() {
                    n.link
                        .to_node
                        .try_send(MasterToNode::ApprovalRespond {
                            req_id: "x".into(),
                            session_id: a,
                            approval_id: x.approval_id.clone(),
                            decision: Decision::Allow,
                            note: None,
                        })
                        .unwrap();
                }
            }
            NodeToMaster::PushCredential { req_id, .. } => {
                n.link
                    .to_node
                    .try_send(MasterToNode::PushCredentialResult {
                        req_id,
                        username: Some(USER.into()),
                        token: Some(Secret::new(CANARY.into())),
                        error: None,
                    })
                    .unwrap();
                break;
            }
            _ => {}
        }
    }
    // a's push is at the forge, which does not answer until released.
    for _ in 0..3000 {
        if !stall.seen().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!stall.seen().is_empty(), "a's push never reached the forge");
    n.prompt(b, Some(target(&stall.host()))).await;
    // Its card shows (the slot is taken only after an Allow); the Allow
    // finds the slot taken, and nothing is asked of the master.
    let seen = n.until_end(b, Answer::Allow, &r.folder).await;
    assert_eq!(seen.cards.len(), 1, "{seen:?}");
    assert!(seen.asked_credential.is_empty(), "{seen:?}");
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("another push"), "{out}");
    // Now end a's push: the forge closes the connection unanswered.
    stall.release();
    let seen = n.until_end(a, Answer::Allow, &r.folder).await;
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && !out.contains("clone_timeout_s"), "{out}");
    no_token_in(&seen);
}

/// A push to a forge that never answers ends at the node's time limit,
/// with nothing else in the test racing it.
#[tokio::test]
async fn a_stalled_push_ends_at_its_time_limit() {
    let r = rig();
    let stall = server(
        &r.root,
        Mode {
            stall: true,
            ..Default::default()
        },
    );
    let mut p = r.policy(&[stall.host()], true);
    p.limits.clone_timeout_s = 2;
    let mut n = node(&r, p).await;
    let a = n.create(&r.folder, false, false).await;
    n.prompt(a, Some(target(&stall.host()))).await;
    let seen = n.until_end(a, Answer::Allow, &r.folder).await;
    assert_eq!(seen.asked_credential.len(), 1, "{seen:?}");
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("clone_timeout_s = 2 s"), "{out}");
    no_token_in(&seen);
}

/// The turn's target gone between the card and the Allow (the check after
/// the Allow): nothing pushed, no credential asked for.
#[tokio::test]
async fn a_target_gone_after_the_card_asks_for_nothing() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    let seen = n
        .until_end(sid, Answer::LoseTargetThenAllow, &r.folder)
        .await;
    assert!(seen.asked_credential.is_empty(), "{seen:?}");
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("turn that asked has ended"), "{out}");
    assert!(forge.seen().is_empty());
}

/// The turn stopped while the card is up: the card is denied, nothing is
/// sent and no credential asked for.
#[tokio::test]
async fn a_turn_stopped_while_the_card_is_up_sends_nothing() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    n.prompt(sid, Some(target(&forge.host()))).await;
    let seen = n.until_end(sid, Answer::Cancel, &r.folder).await;
    assert!(seen.asked_credential.is_empty(), "{seen:?}");
    assert!(seen.results.iter().all(|(_, ok, _)| !ok), "{seen:?}");
    assert!(forge.seen().is_empty());
}

/// (F5) A card nobody answers holds no push slot: another chat's push on
/// the same node goes through meanwhile.
#[tokio::test]
async fn an_unanswered_card_blocks_no_other_push() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let a = n.create(&r.folder, false, false).await;
    let b = n.create(&r.folder, false, false).await;
    n.prompt(a, Some(target(&forge.host()))).await;
    let held = n.until_end(a, Answer::Hold, &r.folder).await;
    assert_eq!(held.cards.len(), 1);
    n.prompt(b, Some(target(&forge.host()))).await;
    let seen = n.until_end(b, Answer::Allow, &r.folder).await;
    let (_, ok, out) = &seen.results[0];
    assert!(ok, "{out}");
    assert_eq!(r.remote_ref("main").as_deref(), Some(r.head().as_str()));
}

/// A sub-agent's session never keeps a target, even one the master (or a
/// node bug) sends it in a message of its own.
#[tokio::test]
async fn a_sub_agents_session_is_never_offered_push() {
    let r = rig();
    let forge = server(
        &r.root,
        Mode {
            ask: true,
            ..Default::default()
        },
    );
    let mut n = node(&r, r.policy(&[forge.host()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    let mut lead = spec();
    lead.delegates = vec![spec_as("scout")];
    let mut m = PromptMsg::test(sid, "delegate: scout=hello");
    m.agent = Some(lead);
    n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
    n.ack().await;
    let seen = n.until_end(sid, Answer::Allow, &r.folder).await;
    let child = *seen.children.first().expect("a sub-agent's session");
    let mut m = PromptMsg::test(child, SCRIPT);
    m.agent = Some(spec_as("scout"));
    m.context = Some(Box::new(harness_core::backend::MasterContext {
        push: Some(target(&forge.host())),
        ..Default::default()
    }));
    n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
    n.ack().await;
    let seen = n.until_end(child, Answer::Allow, &r.folder).await;
    assert!(
        seen.cards.is_empty() && seen.asked_credential.is_empty(),
        "{seen:?}"
    );
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("unknown tool"), "{out}");
    assert!(forge.seen().is_empty());
}

fn spec_as(id: &str) -> harness_core::agent::AgentSpec {
    serde_json::from_value(serde_json::json!({
        "agent_id": id, "name": id, "system_prompt": "", "prompt_hash": "h",
        "tools": ["push", "read"]
    }))
    .unwrap()
}

// ------------------------------------------------ pull requests (T102)

const PR_SCRIPT: &str = r#"script: [{"tool": "pull_request", "input": {"head": "feat", "title": "Add x", "body": "What changed", "draft": true}}]"#;

/// The tool shows a card with the call's own input in Auto mode, offers no
/// wider grant, and after the Allow asks the master with the card's
/// approval id and nothing else; the master's answer is the result.
/// Denied: nothing is asked.
#[tokio::test]
async fn a_pull_request_shows_its_card_and_asks_the_master_by_approval_only() {
    let r = rig();
    let mut n = node(&r, r.policy(&["github.com".to_string()], true)).await;
    assert!(
        n.caps.iter().any(|c| c == harness_protocol::CAP_FORGE_PR),
        "{:?}",
        n.caps
    );
    for allow in [true, false] {
        let sid = n.create(&r.folder, false, false).await;
        let mut m = PromptMsg::test(sid, PR_SCRIPT);
        m.agent = Some(spec());
        m.context = Some(Box::new(harness_core::backend::MasterContext {
            push: Some(target("github.com")),
            ..Default::default()
        }));
        n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
        n.ack().await;
        let (mut cards, mut asked, mut results) = (Vec::new(), Vec::new(), Vec::new());
        loop {
            match n.next().await {
                NodeToMaster::PullRequest {
                    req_id,
                    session_id,
                    approval_id,
                } => {
                    assert_eq!(session_id, sid);
                    asked.push(approval_id);
                    n.link
                        .to_node
                        .try_send(MasterToNode::PullRequestResult {
                            req_id,
                            number: Some(7),
                            url: Some("https://github.com/o/r/pull/7".into()),
                            existed: false,
                            error: None,
                        })
                        .unwrap();
                }
                NodeToMaster::Event { event } if event.session_id == sid => match event.typed() {
                    Ok(EventKind::ApprovalRequested(a)) => {
                        assert_eq!(a.tool, "pull_request");
                        assert!(!a.turn_allow && !a.chat_allow);
                        assert_eq!(a.input["title"], "Add x");
                        cards.push(a.approval_id.clone());
                        n.link
                            .to_node
                            .try_send(MasterToNode::ApprovalRespond {
                                req_id: "x".into(),
                                session_id: sid,
                                approval_id: a.approval_id.clone(),
                                decision: if allow {
                                    Decision::Allow
                                } else {
                                    Decision::Deny
                                },
                                note: None,
                            })
                            .unwrap();
                    }
                    Ok(EventKind::ToolCallFinished(f)) => results.push((f.ok, f.output)),
                    Ok(EventKind::TurnEnded(_)) => break,
                    _ => {}
                },
                _ => {}
            }
        }
        assert_eq!(cards.len(), 1);
        if allow {
            assert_eq!(asked, cards, "the request names the card's approval");
            let (ok, out) = &results[0];
            assert!(
                *ok && out.contains("#7") && out.contains("| https://github.com/o/r/pull/7"),
                "{out}"
            );
        } else {
            assert!(asked.is_empty());
            assert!(!results[0].0);
        }
    }
}

/// A node whose node.toml denies the tool announces nothing for it and
/// never offers it; a turn without a target never offers it either.
#[tokio::test]
async fn a_pull_request_is_not_offered_where_denied_or_without_a_target() {
    let r = rig();
    let mut p = r.policy(&["github.com".to_string()], true);
    p.denied_tools = vec!["pull_request".into()];
    let mut n = node(&r, p).await;
    assert!(!n.caps.iter().any(|c| c == harness_protocol::CAP_FORGE_PR));
    for target_given in [true, false] {
        let sid = n.create(&r.folder, false, false).await;
        let mut m = PromptMsg::test(sid, PR_SCRIPT);
        m.agent = Some(spec());
        m.context = Some(Box::new(harness_core::backend::MasterContext {
            push: target_given.then(|| target("github.com")),
            ..Default::default()
        }));
        n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
        n.ack().await;
        let seen = n.until_end(sid, Answer::Allow, &r.folder).await;
        assert!(seen.cards.is_empty(), "{seen:?}");
        let (_, ok, out) = &seen.results[0];
        assert!(!ok && out.contains("unknown tool"), "{out}");
    }
}

/// `[forge] push = false` withholds pull requests too (the node sends
/// nothing to a forge on an agent's behalf): no capability, not offered.
#[tokio::test]
async fn forge_push_off_withholds_pull_requests_too() {
    let r = rig();
    let mut n = node(&r, r.policy(&["github.com".to_string()], false)).await;
    assert!(
        !n.caps.iter().any(|c| c == harness_protocol::CAP_FORGE_PR),
        "{:?}",
        n.caps
    );
    let sid = n.create(&r.folder, false, false).await;
    let mut m = PromptMsg::test(sid, PR_SCRIPT);
    m.agent = Some(spec());
    m.context = Some(Box::new(harness_core::backend::MasterContext {
        push: Some(target("github.com")),
        ..Default::default()
    }));
    n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
    n.ack().await;
    let seen = n.until_end(sid, Answer::Allow, &r.folder).await;
    assert!(seen.cards.is_empty(), "{seen:?}");
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("unknown tool"), "{out}");
}

/// (Review R9) A target that does not say the master opens pull requests
/// (an older master's, which has no such service) offers no
/// `pull_request`: the node never sends a request that master would not
/// know. Push in that turn is unchanged.
#[tokio::test]
async fn a_target_without_pull_requests_offers_none() {
    let r = rig();
    let mut n = node(&r, r.policy(&["github.com".to_string()], true)).await;
    let sid = n.create(&r.folder, false, false).await;
    let mut m = PromptMsg::test(sid, PR_SCRIPT);
    m.agent = Some(spec());
    let old = PushTarget {
        pull_requests: false,
        ..target("github.com")
    };
    // As an older master writes it: no `pull_requests` key.
    let wire = serde_json::to_value(&old).unwrap();
    assert!(wire.get("pull_requests").is_none(), "{wire}");
    m.context = Some(Box::new(harness_core::backend::MasterContext {
        push: Some(serde_json::from_value(wire).unwrap()),
        ..Default::default()
    }));
    n.link.to_node.try_send(MasterToNode::Prompt(m)).unwrap();
    n.ack().await;
    let seen = n.until_end(sid, Answer::Allow, &r.folder).await;
    assert!(seen.cards.is_empty(), "{seen:?}");
    let (_, ok, out) = &seen.results[0];
    assert!(!ok && out.contains("unknown tool"), "{out}");
}
