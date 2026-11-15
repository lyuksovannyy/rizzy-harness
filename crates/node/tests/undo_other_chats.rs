//! An undo never silently reverts work the turn did not do (PLAN.md T57).
//!
//! Two chats work in one folder. A turn's checkpoints are snapshots of the
//! whole folder, so what the other chat changed while the turn ran used to
//! count as the turn's own, and its undo put it back without a word (found
//! in the first live test with real models, 2026-11-15). Driven by the
//! scripted agent: `script: [...]` makes one tool call per step.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_agent_native::{NativeBackend, NativeConfig, mock::Mock};
use harness_core::event::*;
use harness_core::{RawEvent, SessionId};
use harness_node::{NodeConfig, NodeLink, NodePolicy, NodeRuntime};
use harness_protocol::{MasterToNode, NodeToMaster};
use harness_store::{Durability, Store};
use serde_json::json;

struct Rig {
    dir: tempfile::TempDir,
    ws: PathBuf,
    node: Option<NodeRuntime>,
    link: NodeLink,
    seen: Vec<RawEvent>,
    n: u32,
}

fn config(dir: &Path) -> NodeConfig {
    NodeConfig {
        node_id: "local".into(),
        hostname: "test".into(),
        policy: NodePolicy {
            workspace_roots: vec![dir.into()],
            max_approval_mode: harness_core::policy::ApprovalMode::Auto,
            ..Default::default()
        },
        store: Store::open(&dir.join("node.db"), Durability::Full).unwrap(),
        backend: Arc::new(NativeBackend::new(
            Arc::new(Mock {
                delay: Duration::from_millis(5),
            }),
            harness_tools::builtin(),
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: harness_core::SecretSet::new(vec![]),
        profile: Profile::Agentic,
        incognito_ttl: None,
        checkpoints_dir: Some(dir.join("checkpoints")),
        git_sandbox: None,
        artifacts: None,
    }
}

async fn attach(node: &NodeRuntime) -> NodeLink {
    let mut link = node.attach().await;
    match link.from_node.recv().await.unwrap() {
        NodeToMaster::Hello { .. } => {}
        other => panic!("expected hello, got {other:?}"),
    }
    link
}

impl Rig {
    async fn new() -> Self {
        Self::seeded(|_, _| {}).await
    }

    /// A rig whose store `seed` fills (given the store and the folder)
    /// before the node starts.
    async fn seeded(seed: impl FnOnce(&Store, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir(&ws).unwrap();
        {
            let store = Store::open(&dir.path().join("node.db"), Durability::Normal).unwrap();
            seed(&store, &ws);
        }
        let node = NodeRuntime::start(config(dir.path())).await.unwrap();
        let link = attach(&node).await;
        Self {
            dir,
            ws,
            node: Some(node),
            link,
            seen: vec![],
            n: 0,
        }
    }

    /// Stop the node and start it again on the same data.
    async fn restart(&mut self) {
        if let Some(node) = self.node.take() {
            node.stop(Duration::from_secs(10)).await;
        }
        let node = NodeRuntime::start(config(self.dir.path())).await.unwrap();
        self.link = attach(&node).await;
        self.node = Some(node);
    }

    fn send(&self, m: MasterToNode) {
        self.link.to_node.try_send(m).unwrap();
    }

    async fn recv(&mut self) -> NodeToMaster {
        tokio::time::timeout(Duration::from_secs(60), self.link.from_node.recv())
            .await
            .expect("timed out")
            .expect("link closed")
    }

    async fn create(&mut self, title: &str) -> SessionId {
        let sid = SessionId::new();
        self.send(MasterToNode::Create {
            exec_mode: None,
            no_callbacks: false,
            profile: None,
            incognito: false,
            agent: None,
            req_id: format!("c-{sid}"),
            session_id: sid,
            title: title.into(),
            cwd: self.ws.display().to_string(),
            model: None,
            approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
        });
        self.until_in(sid, |k| matches!(k, EventKind::SessionCreated(_)))
            .await;
        sid
    }

    /// Receive until `pred` matches an event of `sid`.
    async fn until_in(&mut self, sid: SessionId, pred: impl Fn(&EventKind) -> bool) {
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => {
                    let k = event.typed().unwrap();
                    let mine = event.session_id == sid;
                    self.seen.push(event);
                    if mine && pred(&k) {
                        return;
                    }
                }
                NodeToMaster::Ack { ok, error, .. } => assert!(ok, "nack: {error:?}"),
                _ => {}
            }
        }
    }

    /// Send a prompt whose turn makes one tool call per step.
    fn script(&mut self, sid: SessionId, steps: serde_json::Value) {
        self.n += 1;
        self.send(MasterToNode::Prompt {
            savings: None,
            context: None,
            reply: None,
            agent: None,
            attachments: vec![],
            compare: vec![],
            req_id: format!("p{}", self.n),
            session_id: sid,
            text: format!("script: {steps}"),
            images: vec![],
            agent_named: false,
        });
    }

    /// A whole turn of `steps`; returns its id.
    async fn turn(&mut self, sid: SessionId, steps: serde_json::Value) -> String {
        let mark = self.seen.len();
        self.script(sid, steps);
        self.until_in(sid, |k| matches!(k, EventKind::TurnEnded(_)))
            .await;
        self.turn_since(sid, mark)
    }

    /// The turn of `sid` whose `after` checkpoint came since `mark`.
    fn turn_since(&self, sid: SessionId, mark: usize) -> String {
        self.seen[mark..]
            .iter()
            .filter(|e| e.session_id == sid)
            .find_map(|e| match e.typed() {
                Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After => {
                    Some(c.turn_id)
                }
                _ => None,
            })
            .expect("the turn has an after checkpoint")
    }

    async fn restore(
        &mut self,
        sid: SessionId,
        turn: &str,
        force: bool,
        rewind: bool,
    ) -> Result<(), String> {
        self.n += 1;
        let req = format!("r{}", self.n);
        self.send(MasterToNode::Restore {
            req_id: req.clone(),
            session_id: sid,
            turn_id: turn.into(),
            force,
            rewind,
        });
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::Ack {
                    req_id, ok, error, ..
                } if req_id == req => {
                    return if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_default())
                    };
                }
                _ => {}
            }
        }
    }

    /// What `turn` of `sid` changed, as the diff view asks for it.
    async fn diff(&mut self, sid: SessionId, turn: &str) -> harness_protocol::TurnDiff {
        self.n += 1;
        let req = format!("d{}", self.n);
        self.send(MasterToNode::Diff {
            req_id: req.clone(),
            session_id: sid,
            turn_id: turn.into(),
            rewind: false,
        });
        loop {
            match self.recv().await {
                NodeToMaster::Event { event } => self.seen.push(event),
                NodeToMaster::DiffResult {
                    req_id,
                    diff,
                    error,
                    ..
                } if req_id == req => {
                    return *diff.unwrap_or_else(|| panic!("no diff: {error:?}"));
                }
                _ => {}
            }
        }
    }

    /// The chat notes of `sid` seen since `mark`.
    fn notes(&self, sid: SessionId, mark: usize) -> Vec<String> {
        self.seen[mark..]
            .iter()
            .filter(|e| e.session_id == sid)
            .filter_map(|e| match e.typed() {
                Ok(EventKind::Error(m)) => Some(m.message),
                _ => None,
            })
            .collect()
    }

    fn saved_since(&self, mark: usize) -> usize {
        self.seen[mark..]
            .iter()
            .filter(|e| {
                matches!(e.typed(), Ok(EventKind::Checkpoint(c))
                    if c.phase == CheckpointPhase::BeforeRestore)
            })
            .count()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.ws.join(name), text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.ws.join(name)).unwrap_or_default()
    }

    fn flag(&self) -> PathBuf {
        self.dir.path().join("flag")
    }

    /// Start chat `a`'s turn: read `f.txt`, wait (in its shell) until the
    /// test raises the flag, then edit `f.txt`'s last line. Returns once
    /// the turn waits.
    async fn start_waiting_turn(&mut self, a: SessionId) {
        let wait = format!(
            "for i in $(seq 1 600); do [ -e '{}' ] && break; sleep 0.05; done",
            self.flag().display()
        );
        self.script(
            a,
            json!([
                {"tool": "read", "input": {"path": "f.txt"}},
                {"tool": "bash", "input": {"command": wait}},
                {"tool": "edit", "input": {"path": "f.txt", "old_string": "three", "new_string": "THREE-A"}},
            ]),
        );
        self.until_in(
            a,
            |k| matches!(k, EventKind::ToolCallStarted(c) if c.tool == "bash"),
        )
        .await;
    }

    /// Let the waiting turn of `a` go on, and wait for its end.
    async fn finish_waiting_turn(&mut self, a: SessionId, mark: usize) -> String {
        std::fs::write(self.flag(), "").unwrap();
        self.until_in(a, |k| matches!(k, EventKind::TurnEnded(_)))
            .await;
        self.turn_since(a, mark)
    }
}

const ORIGINAL: &str = "one\ntwo\nthree\n";
const B_TITLE: &str = "Bram's private chat about the pricing";

fn b_edits_first_line() -> serde_json::Value {
    json!([
        {"tool": "read", "input": {"path": "f.txt"}},
        {"tool": "edit", "input": {"path": "f.txt", "old_string": "one", "new_string": "ONE-B"}},
    ])
}

/// The live scenario: chat B's edit lands inside chat A's turn; A's undo is
/// refused, naming the file and nothing of B's; "Undo anyway" still works,
/// and then B's chat is told its file was put back, with nothing of A's.
#[tokio::test]
async fn another_chats_edit_during_the_turn_is_not_undone_silently() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("Anouk's chat").await;
    let b = r.create(B_TITLE).await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(b, b_edits_first_line()).await;
    let turn = r.finish_waiting_turn(a, mark).await;
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n");
    // The diff view says so before the person clicks.
    let d = r.diff(a, &turn).await;
    assert_eq!(d.others_wrote, vec!["f.txt".to_string()]);
    assert!(d.others_may.is_empty(), "{:?}", d.others_may);

    let before = r.seen.len();
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(err.contains("another chat"), "{err}");
    assert!(err.contains("`f.txt`"), "{err}");
    assert!(err.contains("Undo anyway"), "{err}");
    // Privacy: nothing of B's chat but the file's name.
    for secret in [B_TITLE, &b.to_string(), "Bram", "ONE-B"] {
        assert!(!err.contains(secret), "{secret:?} in {err}");
    }
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n", "nothing written");
    assert_eq!(r.saved_since(before), 0, "a refusal records nothing");

    // "Undo anyway".
    let before = r.seen.len();
    r.restore(a, &turn, true, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
    let told = r.notes(b, before);
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("`f.txt`"), "{told:?}");
    assert!(told[0].contains("another chat"), "{told:?}");
    for secret in ["Anouk", &a.to_string(), &turn] {
        assert!(!told[0].contains(secret), "{secret:?} in {told:?}");
    }
    let mine = r.notes(a, before);
    assert!(
        mine.iter()
            .any(|n| n.contains("`f.txt`") && n.contains("That chat was told")),
        "{mine:?}"
    );
}

/// A shell command of the other chat changes the file inside the turn: no
/// record says which file, its turn's own checkpoints do.
#[tokio::test]
async fn another_chats_shell_change_during_the_turn_is_not_undone_silently() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    r.write("g.txt", "g\n");
    let a = r.create("a").await;
    let b = r.create("b").await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(
        b,
        json!([{"tool": "bash", "input": {"command": "sed -i 's/one/ONE-B/' f.txt"}}]),
    )
    .await;
    let turn = r.finish_waiting_turn(a, mark).await;
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n");
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(err.contains("ran commands"), "{err}");
    assert!(err.contains("`f.txt`"), "{err}");
    assert!(!err.contains("g.txt"), "{err}");
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n");
    r.restore(a, &turn, true, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
}

/// No refusal when the other chat only read during the turn, changed a file
/// before the turn (the undo goes back to a state holding it), or changed
/// a file the undo does not touch after the turn.
#[tokio::test]
async fn no_refusal_when_the_other_chat_only_read_or_worked_outside_the_turn() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    r.write("g.txt", "g\n");
    let a = r.create("a").await;
    let b = r.create("b").await;
    // B edits f.txt before A's turn.
    r.turn(b, b_edits_first_line()).await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    // During it, B only reads.
    r.turn(b, json!([{"tool": "read", "input": {"path": "f.txt"}}]))
        .await;
    let turn = r.finish_waiting_turn(a, mark).await;
    // After it, B changes a file A's turn did not.
    r.turn(
        b,
        json!([
            {"tool": "read", "input": {"path": "g.txt"}},
            {"tool": "edit", "input": {"path": "g.txt", "old_string": "g", "new_string": "G-B"}},
        ]),
    )
    .await;
    let d = r.diff(a, &turn).await;
    assert!(
        d.others_wrote.is_empty() && d.others_may.is_empty(),
        "{d:?}"
    );
    let before = r.seen.len();
    r.restore(a, &turn, false, false).await.unwrap();
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nthree\n");
    assert_eq!(r.read("g.txt"), "G-B\n");
    assert!(r.notes(b, before).is_empty());
}

/// The other chat's edit to a file the turn never touched, made during the
/// turn, is in the turn's snapshot: the undo names that file alone.
#[tokio::test]
async fn the_refusal_names_only_the_other_chats_files() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    r.write("g.txt", "g\n");
    let a = r.create("a").await;
    let b = r.create("b").await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(
        b,
        json!([
            {"tool": "read", "input": {"path": "g.txt"}},
            {"tool": "write", "input": {"path": "g.txt", "content": "G-B\n"}},
        ]),
    )
    .await;
    let turn = r.finish_waiting_turn(a, mark).await;
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(err.contains("`g.txt`"), "{err}");
    assert!(!err.contains("f.txt"), "{err}");
}

/// Rewind ("this turn and every later turn") goes back over the time
/// between the chat's turns: another chat's edit there was overwritten
/// without a question before (the folder matched the latest state).
#[tokio::test]
async fn a_rewind_does_not_silently_revert_another_chats_edit_between_turns() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let b = r.create("b").await;
    let first = r
        .turn(
            a,
            json!([
                {"tool": "read", "input": {"path": "f.txt"}},
                {"tool": "edit", "input": {"path": "f.txt", "old_string": "three", "new_string": "THREE-A"}},
            ]),
        )
        .await;
    r.turn(b, b_edits_first_line()).await;
    r.turn(
        a,
        json!([{"tool": "bash", "input": {"command": "echo h > h.txt"}}]),
    )
    .await;
    let err = r.restore(a, &first, false, true).await.unwrap_err();
    assert!(
        err.contains("`f.txt`") && err.contains("another chat"),
        "{err}"
    );
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n");
    r.restore(a, &first, true, true).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
    assert!(!r.ws.join("h.txt").exists());
}

/// Everything the check needs is in the logs and the checkpoint store: a
/// node restart between the turn and its undo changes nothing.
#[tokio::test]
async fn the_check_holds_across_a_node_restart() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let b = r.create("b").await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(b, b_edits_first_line()).await;
    let turn = r.finish_waiting_turn(a, mark).await;
    r.restart().await;
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(
        err.contains("`f.txt`") && err.contains("another chat"),
        "{err}"
    );
    let before = r.seen.len();
    r.restore(a, &turn, true, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
    assert_eq!(r.notes(b, before).len(), 1);
}

/// One chat in the folder: the undo of a turn runs as before, and the
/// person's own edit during it is still the turn's (the documented limit).
#[tokio::test]
async fn a_single_chat_undoes_as_before() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    let turn = r.finish_waiting_turn(a, mark).await;
    assert_eq!(r.read("f.txt"), "one\ntwo\nTHREE-A\n");
    r.restore(a, &turn, false, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
}

/// Review R1 (F1): B edits during A's turn, then B's chat is deleted. Its
/// log goes, its change stays in the folder: A's undo is still refused,
/// through B's write tombstone, which says nothing of B.
#[tokio::test]
async fn a_deleted_chats_work_is_not_undone_silently() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let b = r.create(B_TITLE).await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(b, b_edits_first_line()).await;
    let turn = r.finish_waiting_turn(a, mark).await;
    r.n += 1;
    let req = format!("del{}", r.n);
    r.send(MasterToNode::Delete {
        req_id: req.clone(),
        session_id: b,
    });
    loop {
        match r.recv().await {
            NodeToMaster::Ack { req_id, ok, .. } if req_id == req => {
                assert!(ok);
                break;
            }
            _ => {}
        }
    }
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(
        err.contains("deleted since") && err.contains("`f.txt`"),
        "{err}"
    );
    assert!(
        !err.contains(B_TITLE) && !err.contains(&b.to_string()),
        "{err}"
    );
    let d = r.diff(a, &turn).await;
    assert_eq!(d.others_may, vec!["f.txt".to_string()]);
    assert_eq!(r.read("f.txt"), "ONE-B\ntwo\nTHREE-A\n");
    r.restore(a, &turn, true, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
}

/// F1, the incognito purge: it deletes through the same path as a person's
/// delete (`NodeRuntime::purge_incognito` calls `delete`), so a tombstone
/// would be written; but an incognito chat cannot work in a folder at all
/// (only chat sessions are incognito, and they have no folder and no file
/// tools), so a purge has no file changes to leave behind. Proven here.
#[tokio::test]
async fn an_incognito_chat_cannot_work_in_a_folder_so_its_purge_leaves_nothing() {
    let mut r = Rig::new().await;
    let sid = SessionId::new();
    r.send(MasterToNode::Create {
        exec_mode: None,
        no_callbacks: false,
        profile: None,
        incognito: true,
        agent: None,
        req_id: "inc".into(),
        session_id: sid,
        title: "t".into(),
        cwd: r.ws.display().to_string(),
        model: None,
        approval_mode: Some(harness_core::policy::ApprovalMode::Auto),
    });
    let err = loop {
        match r.recv().await {
            NodeToMaster::Ack {
                req_id, ok, error, ..
            } if req_id == "inc" => {
                assert!(!ok);
                break error.unwrap_or_default();
            }
            _ => {}
        }
    };
    assert!(err.contains("only available for chat sessions"), "{err}");
    assert_eq!(
        r.node
            .as_ref()
            .unwrap()
            .purge_incognito(Duration::ZERO)
            .await,
        0
    );
}

/// A deleted chat that only read leaves no tombstone: no refusal.
#[tokio::test]
async fn a_deleted_chat_that_only_read_refuses_nothing() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let b = r.create("b").await;
    let mark = r.seen.len();
    r.start_waiting_turn(a).await;
    r.turn(b, json!([{"tool": "read", "input": {"path": "f.txt"}}]))
        .await;
    let turn = r.finish_waiting_turn(a, mark).await;
    r.node.as_ref().unwrap().delete(b).await.unwrap();
    r.restore(a, &turn, false, false).await.unwrap();
    assert_eq!(r.read("f.txt"), ORIGINAL);
}

/// Review R2 (F3): B runs a command turn that changes nothing (its tests)
/// while A's turn edits `g.txt` with its own `edit`. B's turn's snapshots
/// hold A's change, but A's own chain explains all of it: no refusal, and
/// B is told nothing.
#[tokio::test]
async fn another_chats_command_turn_does_not_refuse_an_undo_of_own_edits() {
    let mut r = Rig::new().await;
    r.write("g.txt", "g\n");
    let a = r.create("a").await;
    let b = r.create("b").await;
    let flag = r.dir.path().join("flag-b");
    let wait = format!(
        "for i in $(seq 1 600); do [ -e '{}' ] && break; sleep 0.05; done",
        flag.display()
    );
    r.script(b, json!([{"tool": "bash", "input": {"command": wait}}]));
    r.until_in(
        b,
        |k| matches!(k, EventKind::ToolCallStarted(c) if c.tool == "bash"),
    )
    .await;
    let turn = r
        .turn(
            a,
            json!([
                {"tool": "read", "input": {"path": "g.txt"}},
                {"tool": "edit", "input": {"path": "g.txt", "old_string": "g", "new_string": "G-A"}},
            ]),
        )
        .await;
    std::fs::write(&flag, "").unwrap();
    r.until_in(b, |k| matches!(k, EventKind::TurnEnded(_)))
        .await;
    let d = r.diff(a, &turn).await;
    assert!(
        d.others_wrote.is_empty() && d.others_may.is_empty(),
        "{d:?}"
    );
    let before = r.seen.len();
    r.restore(a, &turn, false, false).await.unwrap();
    assert_eq!(r.read("g.txt"), "g\n");
    assert!(r.notes(b, before).is_empty());
}

/// F3: a chain broken by another chat's command between two of A's own
/// edits explains nothing: still refused.
#[tokio::test]
async fn a_chain_broken_by_another_chats_command_is_still_refused() {
    let mut r = Rig::new().await;
    r.write("f.txt", ORIGINAL);
    let a = r.create("a").await;
    let b = r.create("b").await;
    let wait = format!(
        "for i in $(seq 1 600); do [ -e '{}' ] && break; sleep 0.05; done",
        r.flag().display()
    );
    let mark = r.seen.len();
    r.script(
        a,
        json!([
            {"tool": "read", "input": {"path": "f.txt"}},
            {"tool": "edit", "input": {"path": "f.txt", "old_string": "three", "new_string": "THREE-A"}},
            {"tool": "bash", "input": {"command": wait}},
            {"tool": "edit", "input": {"path": "f.txt", "old_string": "two", "new_string": "TWO-A"}},
        ]),
    );
    r.until_in(
        a,
        |k| matches!(k, EventKind::ToolCallStarted(c) if c.tool == "bash"),
    )
    .await;
    r.turn(
        b,
        json!([{"tool": "bash", "input": {"command": "sed -i 's/one/ONE-B/' f.txt"}}]),
    )
    .await;
    let turn = r.finish_waiting_turn(a, mark).await;
    assert_eq!(r.read("f.txt"), "ONE-B\nTWO-A\nTHREE-A\n");
    let err = r.restore(a, &turn, false, false).await.unwrap_err();
    assert!(
        err.contains("ran commands") && err.contains("`f.txt`"),
        "{err}"
    );
}

/// Another chat's log of `turns` turns in `ws`, ending `age_ms` ago: per
/// turn a prompt, 20 streamed pieces, a read, an edit and a command with
/// their results, and checkpoints.
fn seed_other_chat(store: &Store, ws: &Path, turns: u64, age_ms: i64) {
    let sid = SessionId::new();
    let old = harness_core::now_ms() - age_ms - 100_000;
    let mut seq = 0u64;
    let mut put = |kind: EventKind| {
        seq += 1;
        let (kind, payload) = kind.into_parts();
        store
            .append(
                "local",
                &RawEvent {
                    session_id: sid,
                    seq,
                    ts_ms: old + seq as i64,
                    kind,
                    payload,
                },
            )
            .unwrap();
    };
    put(EventKind::SessionCreated(SessionCreated {
        exec_mode: None,
        no_callbacks: false,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
        title: "other".into(),
        cwd: ws.display().to_string(),
        model: "mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: harness_core::policy::ApprovalMode::Auto,
    }));
    let file = ws.join("f.txt").display().to_string();
    for t in 0..turns {
        let turn_id = format!("turn-{t}");
        put(EventKind::TurnStarted(TurnStarted {
            turn_id: turn_id.clone(),
            agent: None,
            cause: None,
            environment: None,
        }));
        put(EventKind::UserMessage(
            serde_json::from_value(json!({"turn_id": turn_id, "text": "x".repeat(300)})).unwrap(),
        ));
        put(EventKind::Checkpoint(Checkpoint {
            turn_id: turn_id.clone(),
            phase: CheckpointPhase::Before,
            git_ref: "0".repeat(40),
            files: vec![],
            excluded: vec![],
            excluded_more: 0,
        }));
        for _ in 0..20 {
            put(EventKind::AssistantDelta(
                serde_json::from_value(
                    json!({"turn_id": turn_id, "message_id": "m", "text": "y".repeat(40)}),
                )
                .unwrap(),
            ));
        }
        for (i, tool) in ["read", "edit", "bash"].into_iter().enumerate() {
            let call_id = format!("{t}-{i}");
            put(EventKind::ToolCallStarted(ToolCallStarted {
                turn_id: turn_id.clone(),
                call_id: call_id.clone(),
                tool: tool.into(),
                input: json!({"path": "f.txt"}),
                author: None,
            }));
            put(EventKind::ToolCallFinished(ToolCallFinished {
                call_id,
                ok: true,
                output: "z".repeat(2000),
                exit_code: None,
                truncated: false,
                sources: vec![],
                note: None,
                seen: (tool != "bash").then(|| FileSeen {
                    path: file.clone(),
                    sha256: "0".repeat(64),
                    base: None,
                }),
            }));
        }
        put(EventKind::Checkpoint(Checkpoint {
            turn_id: turn_id.clone(),
            phase: CheckpointPhase::After,
            git_ref: "0".repeat(40),
            files: vec!["f.txt".into()],
            excluded: vec![],
            excluded_more: 0,
        }));
        put(EventKind::TurnEnded(
            serde_json::from_value(json!({"turn_id": turn_id, "reason": "completed"})).unwrap(),
        ));
    }
}

/// What the check costs an undo: with one chat in the folder, and with a
/// second chat of 300 turns (each with a read, an edit and a command) that
/// ended two hours before the undone turn (beyond the hour read back for
/// background commands) or half an hour before (all of it read). A
/// measurement, not a check:
/// `cargo test -p harness-node --test undo_other_chats cost -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn cost_of_the_check() {
    for (turns, age_ms) in [(0u64, 0i64), (300, 7_200_000), (300, 1_800_000)] {
        let mut r = Rig::seeded(|store, ws| {
            if turns > 0 {
                seed_other_chat(store, ws, turns, age_ms)
            }
        })
        .await;
        r.write("f.txt", ORIGINAL);
        let a = r.create("a").await;
        let mut took = Vec::new();
        for i in 0..7 {
            let turn = r
                .turn(
                    a,
                    json!([{"tool": "bash", "input": {"command": format!("echo {i} > f.txt")}}]),
                )
                .await;
            let t = std::time::Instant::now();
            r.restore(a, &turn, false, false).await.unwrap();
            took.push(t.elapsed());
        }
        took.sort();
        println!(
            "other chat with {turns} turns ending {} min before: undo median {:?}, min {:?}, \
             max {:?}",
            age_ms / 60_000,
            took[3],
            took[0],
            took[6]
        );
    }
}
