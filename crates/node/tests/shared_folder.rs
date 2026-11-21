//! Where a sub-agent that can write works (the owner, 2026-11-21, PLAN.md
//! T32): in the chat's own folder unless the delegating agent asks for a
//! copy (`folder: "own"`) or the node says `sub_agent_folder = "own"`.

#[path = "support/rig.rs"]
mod support;

use harness_core::SessionId;
use harness_core::event::EventKind;
use support::Rig;

/// The sub-agents of the rig's chat that were made: id and folder.
fn children(r: &Rig) -> Vec<(SessionId, String)> {
    r.seen
        .iter()
        .filter(|e| e.session_id != r.session_id())
        .filter_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) => Some((e.session_id, c.cwd)),
            _ => None,
        })
        .collect()
}

/// Folders the node made for sub-agents (`<data>/sessions/<id>`).
fn session_folders(r: &Rig) -> usize {
    std::fs::read_dir(r.dir.path().join("sessions"))
        .map(|d| d.count())
        .unwrap_or(0)
}

/// Checkpoint repositories on the node: the chat's, and the sub-agents'
/// when one was made (`<data>/checkpoints/*.git`).
fn shadows(r: &Rig) -> usize {
    std::fs::read_dir(r.dir.path().join("checkpoints"))
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "git"))
                .count()
        })
        .unwrap_or(0)
}

async fn repo_rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// The default: no `folder` at all. The sub-agent writes straight into the
/// chat's folder, no copy is made, no base ref exists, and its result ends
/// with the one harness line naming the files that changed there.
#[tokio::test]
async fn a_delegation_with_no_folder_writes_straight_into_the_chats_folder() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert_eq!(outs.len(), 1, "{outs:#?}");
    let out = &outs[0];
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    // It worked in the chat's folder: same cwd, no folder of its own.
    let kids = children(&r);
    assert_eq!(kids.len(), 1);
    assert_eq!(
        std::path::Path::new(&kids[0].1).canonicalize().unwrap(),
        r.ws.canonicalize().unwrap(),
        "the sub-agent got another folder"
    );
    assert_eq!(session_folders(&r), 0);
    // No copy, no merge: no sub-agent snapshots, no merge words.
    assert_eq!(shadows(&r), 1, "a sub-agent snapshot repository was made");
    assert!(!out.contains("applied") && !out.contains("merged"), "{out}");
    // The lead is told where it worked and which files changed.
    let line = out
        .lines()
        .find(|l| l.starts_with("[it worked in this chat's folder"))
        .unwrap_or_else(|| panic!("no shared line: {out}"));
    assert!(
        line.contains("`keep.txt`") && line.contains("`made.txt`"),
        "{line}"
    );
    assert!(line.contains("read them again"), "{line}");
}

/// `shared` asked for by name is the same as leaving it out.
#[tokio::test]
async fn shared_asked_for_by_name_is_the_default() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev#shared=run: echo new > made.txt")
        .await;
    assert!(
        outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    assert_eq!(session_folders(&r), 0);
}

/// A writing sub-agent runs in a folder that is not a git repository.
#[tokio::test]
async fn a_shared_writer_works_outside_a_git_repository() {
    let mut r = Rig::new().await;
    r.write("keep.txt", "v1\n");
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert!(
        !outs[0].contains("not started") && outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    assert_eq!(r.read("made.txt").unwrap(), "new\n");
    assert_eq!(children(&r).len(), 1);
}

/// `own` is today's behaviour: a copy of its own, merged back per file, no
/// shared line. Outside a repository it is refused, and the sentence says
/// leaving `folder` out works.
#[tokio::test]
async fn own_is_a_copy_merged_back_and_needs_a_repository() {
    let mut r = repo_rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev#own=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert!(outs[0].contains("2 changed file(s) applied"), "{outs:#?}");
    assert!(
        !outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("keep.txt").unwrap(), "v2\n");
    let kids = children(&r);
    assert!(
        std::path::Path::new(&kids[0].1).starts_with(r.dir.path().join("sessions")),
        "own: the sub-agent did not work in a folder of its own: {kids:?}"
    );
    let mut p = Rig::new().await;
    p.write("keep.txt", "v1\n");
    let (_, outs) = p
        .lead_turn("delegate: dev#own=run: echo v2 > keep.txt")
        .await;
    assert!(
        outs[0].contains("not started")
            && outs[0].contains("not a git repository")
            && outs[0].contains("leave `folder` out"),
        "{outs:#?}"
    );
    assert_eq!(p.read("keep.txt").unwrap(), "v1\n");
    assert_eq!(children(&p).len(), 0, "a child session was made");
}

/// The node's `own` wins over what a model asks: every writer gets a copy,
/// outside a repository it is refused, and when `shared` was asked for by
/// name the refusal names the setting.
#[tokio::test]
async fn the_nodes_own_setting_gives_every_writer_a_copy() {
    use harness_core::subagent::SubFolder;
    let mut r = Rig::with_policy(|p| p.limits.sub_agent_folder = SubFolder::Own).await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    for ask in ["dev", "dev#shared", "dev#own"] {
        let (_, outs) = r
            .lead_turn(&format!("delegate: {ask}=run: echo v2 > keep.txt"))
            .await;
        assert!(
            outs[0].contains("1 changed file(s) applied"),
            "{ask}: {outs:#?}"
        );
        assert!(
            !outs[0].contains("worked in this chat's folder"),
            "{ask}: {outs:#?}"
        );
        r.write("keep.txt", "v1\n");
    }
    for (_, cwd) in children(&r) {
        assert!(
            std::path::Path::new(&cwd).starts_with(r.dir.path().join("sessions")),
            "{cwd}"
        );
    }
    let mut p = Rig::with_policy(|p| p.limits.sub_agent_folder = SubFolder::Own).await;
    p.write("keep.txt", "v1\n");
    let (_, outs) = p
        .lead_turn("delegate: dev#shared=run: echo v2 > keep.txt")
        .await;
    assert!(
        outs[0].contains("not started") && outs[0].contains("sub_agent_folder = \"own\""),
        "{outs:#?}"
    );
    let (_, outs) = p.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    assert!(
        outs[0].contains("not started") && outs[0].contains("not a git repository"),
        "{outs:#?}"
    );
    assert_eq!(p.read("keep.txt").unwrap(), "v1\n");
}

/// A sub-agent that only reads is the same on every setting: it works in
/// the chat's folder and `folder` changes nothing for it.
#[tokio::test]
async fn a_reader_ignores_folder() {
    let mut r = repo_rig().await;
    for ask in ["look", "look#own", "look#shared"] {
        let (_, outs) = r
            .lead_turn(&format!("delegate: {ask}=read: keep.txt"))
            .await;
        assert!(outs[0].contains("finished: done"), "{ask}: {outs:#?}");
        assert!(
            !outs[0].contains("worked in this chat's folder"),
            "{outs:#?}"
        );
    }
    assert_eq!(session_folders(&r), 0);
}

// ---- Two writers in one folder, and the cases that go wrong -----------------

/// `(ok, output)` of each finished call of `tool` in `sid`, in order.
fn finished(r: &Rig, sid: SessionId, tool: &str) -> Vec<(bool, String)> {
    let calls: Vec<String> = r
        .seen
        .iter()
        .filter(|e| e.session_id == sid)
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallStarted(c)) if c.tool == tool => Some(c.call_id),
            _ => None,
        })
        .collect();
    r.seen
        .iter()
        .filter(|e| e.session_id == sid)
        .filter_map(|e| match e.typed() {
            Ok(EventKind::ToolCallFinished(f)) if calls.contains(&f.call_id) => {
                Some((f.ok, f.output))
            }
            _ => None,
        })
        .collect()
}

/// Nobody overwrites blindly: the lead reads `a.txt`, a shared child
/// changes it, and the lead's edit is refused as stale until it reads the
/// file again; then it lands. (`bash` is not covered by the guard.)
#[tokio::test]
async fn the_lead_cannot_edit_a_file_a_shared_child_changed_until_it_reads_it_again() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\n");
    r.lead_tools = Some(vec!["bash", "write", "read", "edit"]);
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "delegate", "input": {"agent": "dev", "task": "run: echo changed > a.txt"}},
        {"tool": "edit", "input": {"path": "a.txt", "old_string": "one", "new_string": "ONE"}},
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "edit", "input": {"path": "a.txt", "old_string": "changed", "new_string": "CHANGED"}},
    ]);
    let (_, outs) = r.lead_turn(&format!("script: {script}")).await;
    assert!(outs[0].contains("`a.txt`"), "the lead is told: {outs:#?}");
    let edits = finished(&r, r.session_id(), "edit");
    assert_eq!(edits.len(), 2, "{edits:#?}");
    assert!(
        !edits[0].0
            && edits[0]
                .1
                .contains("has changed on disk since you last read it"),
        "{edits:#?}"
    );
    assert!(edits[1].0, "{edits:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("CHANGED\n"));
}

/// The same between two shared children running at once: the one that read
/// the file is refused when the other changed it meanwhile, and edits
/// after a fresh read.
#[tokio::test]
async fn a_shared_child_cannot_edit_what_another_shared_child_changed_meanwhile() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\n");
    r.delegates = vec![Rig::spec("ed", &["read", "edit", "bash"], vec![])];
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "bash", "input": {"command": "sleep 2"}},
        {"tool": "edit", "input": {"path": "a.txt", "old_string": "one", "new_string": "ONE"}},
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "edit", "input": {"path": "a.txt", "old_string": "changed", "new_string": "CHANGED"}},
    ]);
    let (_, outs) = r
        .lead_turn(&format!(
            "delegate: ed=script: {script} | dev=run: sleep 1; echo changed > a.txt"
        ))
        .await;
    assert_eq!(outs.len(), 2, "{outs:#?}");
    let child = children(&r)
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| !finished(&r, *id, "edit").is_empty())
        .expect("the editing child");
    let edits = finished(&r, child, "edit");
    assert_eq!(edits.len(), 2, "{edits:#?}");
    assert!(
        !edits[0].0
            && edits[0]
                .1
                .contains("has changed on disk since you last read it"),
        "{edits:#?}"
    );
    assert!(edits[1].0, "{edits:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("CHANGED\n"));
}

/// Finding A8's case: the sub-agent's session log fails after it changed
/// files. Nothing un-writes them; the result says its record is incomplete,
/// that its changes are in the folder, and that the turn's Undo restores
/// them; and the Undo does.
#[tokio::test]
async fn a_shared_child_whose_record_failed_leaves_its_changes_and_the_result_says_so() {
    let mut r = repo_rig().await;
    let before = r.files();
    let db = rusqlite::Connection::open(r.dir.path().join("node.db")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER break_log BEFORE INSERT ON events
         WHEN NEW.kind = 'tool_call_finished' AND NEW.session_id <> '{}'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
        r.session_id()
    ))
    .unwrap();
    let (turn, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    let out = &outs[0];
    assert_eq!(r.read("keep.txt").as_deref(), Some("v2\n"), "{out}");
    assert_eq!(r.read("made.txt").as_deref(), Some("new\n"), "{out}");
    let line = out
        .lines()
        .find(|l| l.starts_with("[the sub-agent stopped because its session log"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.contains("its record is incomplete")
            && line.contains("is in the folder now")
            && line.contains("this turn's Undo restores them"),
        "{line}"
    );
    db.execute_batch("DROP TRIGGER break_log").unwrap();
    let undo = r.restore(&turn.id.expect("a turn checkpoint"), false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

/// A writing sub-agent of a sub-agent stays refused (one level of
/// writers), shared or not: no session for it, the folder untouched.
#[tokio::test]
async fn a_shared_childs_own_writing_sub_agent_is_refused() {
    let mut r = Rig::with_policy(|p| p.limits.max_delegation_depth = 2).await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r.delegates = vec![Rig::spec(
        "mid",
        &["bash", "write"],
        vec![Rig::spec("coder", &["bash", "write"], vec![])],
    )];
    let before = r.files();
    let (_, outs) = r
        .lead_turn("delegate: mid=delegate: coder=run: echo nested > nested.txt")
        .await;
    let out = &outs[0];
    assert!(
        out.contains("@coder was not started") && out.contains("may only read"),
        "{out}"
    );
    assert_eq!(r.files(), before);
    assert_eq!(children(&r).len(), 1, "a session was made for the coder");
}

/// In plan mode nothing writes: the writer is refused before a child
/// exists, whatever `folder` says.
#[tokio::test]
async fn in_plan_mode_a_writing_sub_agent_is_not_started_shared_or_own() {
    let mut r = repo_rig().await;
    let before = r.files();
    r.n += 1;
    r.send(harness_protocol::MasterToNode::SetApprovalMode {
        lowering: true,
        req_id: format!("m{}", r.n),
        session_id: r.session_id(),
        mode: harness_core::policy::ApprovalMode::Plan,
    });
    for ask in ["dev", "dev#shared", "dev#own"] {
        let (_, outs) = r
            .lead_turn(&format!("delegate: {ask}=run: echo v2 > keep.txt"))
            .await;
        assert!(outs[0].contains("plan mode"), "{ask}: {outs:#?}");
    }
    assert_eq!(r.files(), before);
    assert_eq!(children(&r).len(), 0);
}

/// A follow-up of a sub-agent that works in the chat's folder works where
/// it is now, on a node that shares the folder, and writes there; the
/// call may not name a folder.
#[tokio::test]
async fn shared_follow_ups_work_in_the_folder_and_name_no_folder() {
    let mut r = repo_rig().await;
    let (_, outs) = r.lead_turn("delegate: dev=run: echo one > b.txt").await;
    let child = outs[0]
        .strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .unwrap()
        .to_string();
    let (_, outs) = r
        .lead_turn(&format!("delegate: dev@{child}=run: echo two >> b.txt"))
        .await;
    assert!(
        outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    assert_eq!(r.read("b.txt").as_deref(), Some("one\ntwo\n"));
    assert_eq!(children(&r).len(), 1, "no second child");
    // A `folder` with a follow-up is refused by the tool.
    let (_, outs) = r
        .lead_turn(&format!(
            "script: {}",
            serde_json::json!([{"tool": "delegate", "input": {
                "agent": "dev", "session": child, "task": "run: echo x", "folder": "own"
            }}])
        ))
        .await;
    assert!(outs[0].contains("leave `folder` out"), "{outs:#?}");
}

// ---- The guards between two writers, each in every direction ----------------

/// A tag the way `read` prints it for `text` (the first six hex digits of
/// its SHA-256, in capitals).
fn tag_of(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(text.as_bytes());
    d.iter()
        .take(3)
        .map(|b| format!("{b:02X}"))
        .collect::<String>()
}

fn child_id(out: &str) -> String {
    out.strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .unwrap()
        .to_string()
}

const STALE: &str = "has changed on disk since you last read it";

/// `write` over a file the agent read and another writer changed since:
/// the lead after a shared child.
#[tokio::test]
async fn the_lead_cannot_write_over_a_file_a_shared_child_changed() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\n");
    r.lead_tools = Some(vec!["bash", "write", "read", "edit"]);
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "delegate", "input": {"agent": "dev", "task": "run: echo changed > a.txt"}},
        {"tool": "write", "input": {"path": "a.txt", "content": "blind\n"}},
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "write", "input": {"path": "a.txt", "content": "fresh\n"}},
    ]);
    r.lead_turn(&format!("script: {script}")).await;
    let w = finished(&r, r.session_id(), "write");
    assert_eq!(w.len(), 2, "{w:#?}");
    assert!(!w[0].0 && w[0].1.contains(STALE), "{w:#?}");
    assert!(w[1].0, "{w:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("fresh\n"));
}

/// A shared child after the lead, through a follow-up: the child read the
/// file in its first run, the lead changed it, the follow-up's write is
/// refused.
#[tokio::test]
async fn a_shared_child_cannot_write_over_what_the_lead_changed_since_its_last_read() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\n");
    r.lead_tools = Some(vec!["bash", "write", "read", "edit"]);
    r.delegates = vec![Rig::spec("wr", &["read", "write", "bash"], vec![])];
    let first = serde_json::json!([{"tool": "read", "input": {"path": "a.txt"}}]);
    let s1 = serde_json::json!([{"tool": "delegate", "input": {
        "agent": "wr", "task": format!("script: {first}")}}]);
    let (_, outs) = r.lead_turn(&format!("script: {s1}")).await;
    let child = child_id(&outs[0]);
    let again = serde_json::json!([{"tool": "write", "input": {"path": "a.txt", "content": "blind-child\n"}}]);
    let s2 = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "write", "input": {"path": "a.txt", "content": "lead\n"}},
        {"tool": "delegate", "input": {"agent": "wr", "session": child, "task": format!("script: {again}")}},
    ]);
    r.lead_turn(&format!("script: {s2}")).await;
    let id = children(&r)[0].0;
    let w = finished(&r, id, "write");
    assert_eq!(w.len(), 1, "{w:#?}");
    assert!(!w[0].0 && w[0].1.contains(STALE), "{w:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("lead\n"));
}

/// A shared child after another shared child running at once.
#[tokio::test]
async fn a_shared_child_cannot_write_over_what_another_shared_child_changed_meanwhile() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\n");
    r.delegates = vec![Rig::spec("wr", &["read", "write", "bash"], vec![])];
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "bash", "input": {"command": "sleep 2"}},
        {"tool": "write", "input": {"path": "a.txt", "content": "blind\n"}},
    ]);
    r.lead_turn(&format!(
        "delegate: wr=script: {script} | dev=run: sleep 1; echo changed > a.txt"
    ))
    .await;
    let id = children(&r)
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| !finished(&r, *id, "write").is_empty())
        .expect("the writing child");
    let w = finished(&r, id, "write");
    assert!(!w[0].0 && w[0].1.contains(STALE), "{w:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("changed\n"));
}

/// An edit by tag and row on rows the other writer changed is refused.
#[tokio::test]
async fn an_edit_by_tag_on_rows_a_shared_child_changed_is_refused() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\ntwo\nthree\n");
    r.lead_tools = Some(vec!["bash", "write", "read", "edit"]);
    let tag = tag_of("one\ntwo\nthree\n");
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "delegate", "input": {"agent": "dev", "task": "run: printf 'one\\ntwo\\nTHREE\\n' > a.txt"}},
        {"tool": "edit", "input": {"path": "a.txt", "tag": tag, "lines": [{"from": 3, "to": 3, "text": "mine3"}]}},
    ]);
    r.lead_turn(&format!("script: {script}")).await;
    let e = finished(&r, r.session_id(), "edit");
    assert_eq!(e.len(), 1, "{e:#?}");
    assert!(!e[0].0, "{e:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("one\ntwo\nTHREE\n"));
}

/// ... and on rows it did not change it lands, at the rows' numbers now.
#[tokio::test]
async fn an_edit_by_tag_on_rows_a_shared_child_did_not_change_lands_at_the_current_rows() {
    let mut r = Rig::new().await;
    r.write("a.txt", "one\ntwo\nthree\n");
    r.lead_tools = Some(vec!["bash", "write", "read", "edit"]);
    let tag = tag_of("one\ntwo\nthree\n");
    let script = serde_json::json!([
        {"tool": "read", "input": {"path": "a.txt"}},
        {"tool": "delegate", "input": {"agent": "dev", "task": "run: printf 'zero\\none\\ntwo\\nthree\\n' > a.txt"}},
        {"tool": "edit", "input": {"path": "a.txt", "tag": tag, "lines": [{"from": 3, "to": 3, "text": "mine3"}]}},
    ]);
    r.lead_turn(&format!("script: {script}")).await;
    let e = finished(&r, r.session_id(), "edit");
    assert_eq!(e.len(), 1, "{e:#?}");
    assert!(e[0].0, "{e:#?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("zero\none\ntwo\nmine3\n"));
}

// ---- Follow-ups that are refused, and what they say -------------------------

/// A sub-agent's own sub-agent never writes in the chat's folder, on a
/// follow-up either: the follow-up of a grandchild whose agent can change
/// files now is refused, and nothing is written.
#[tokio::test]
async fn a_sub_agents_writing_sub_agent_stays_refused_on_a_follow_up_too() {
    let mut r = Rig::with_policy(|p| p.limits.max_delegation_depth = 2).await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r.delegates = vec![Rig::spec(
        "mid",
        &["bash", "write"],
        vec![Rig::spec("coder", &["read"], vec![])],
    )];
    let (_, outs) = r
        .lead_turn("delegate: mid=delegate: coder=read: keep.txt")
        .await;
    let mid = child_id(&outs[0]);
    let grandchild = children(&r)
        .into_iter()
        .map(|(id, _)| id)
        .find(|id| {
            r.seen.iter().any(|e| {
                e.session_id == *id
                    && matches!(e.typed(), Ok(EventKind::SessionCreated(c))
                        if c.agent_id.as_deref() == Some("coder"))
            })
        })
        .expect("the coder");
    r.delegates = vec![Rig::spec(
        "mid",
        &["bash", "write"],
        vec![Rig::spec("coder", &["bash", "write"], vec![])],
    )];
    let before = r.files();
    let (_, outs) = r
        .lead_turn(&format!(
            "delegate: mid@{mid}=delegate: coder@{grandchild}=run: echo nested > nested.txt"
        ))
        .await;
    let out = &outs[0];
    assert!(out.contains("cannot be given a follow-up"), "{out}");
    assert_eq!(r.files(), before);
}
