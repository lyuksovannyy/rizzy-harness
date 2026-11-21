//! A sub-agent that works in the chat's folder and the turn's Undo (the
//! owner, 2026-11-21; PLAN.md T32, T57).
//!
//! The lead's turn checkpoint is a snapshot of the whole folder, so what a
//! sub-agent the lead's `delegate` call started wrote in that turn is in
//! it: `files`, "Show changes" and Undo cover it, and T57 does not count
//! it as another chat. Any other turn in a sub-agent's chat (a person's
//! message to it) and what a background one writes outside the lead's
//! turns are another chat's work; the latter is undone in its own chat,
//! from its own checkpoints.

#[path = "support/rig.rs"]
mod support;

use harness_core::SessionId;
use harness_core::agent::DelegateCaps;
use harness_core::event::*;
use harness_protocol::{MasterToNode, PromptMsg};
use serde_json::json;
use support::Rig;

async fn repo_rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("keep.txt", "v1\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// The files the `after` checkpoint of `turn` in `sid` names.
fn after_files(r: &Rig, sid: SessionId, turn: &str) -> Vec<String> {
    r.seen
        .iter()
        .filter(|e| e.session_id == sid)
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c))
                if c.turn_id == turn && c.phase == CheckpointPhase::After =>
            {
                Some(c.files)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no after checkpoint for {turn}"))
}

/// Sub-agent chats made so far: (id, its first turn's id).
fn children(r: &Rig) -> Vec<SessionId> {
    r.seen
        .iter()
        .filter(|e| e.session_id != r.session_id())
        .filter_map(|e| match e.typed() {
            Ok(EventKind::SessionCreated(c)) if c.parent_session.is_some() => Some(e.session_id),
            _ => None,
        })
        .collect()
}

/// A shared sub-agent the lead waited for: its writes are in the lead's
/// turn checkpoint, "Show changes" lists them, and one Undo puts the folder
/// back exactly, without asking: it is not "another chat".
#[tokio::test]
async fn the_leads_undo_restores_what_a_shared_child_changed() {
    let mut r = repo_rig().await;
    let before = r.files();
    let (turn, outs) = r
        .lead_turn("delegate: dev=run: echo v2 > keep.txt; echo new > made.txt")
        .await;
    assert!(
        outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    let turn = turn.id.expect("a turn checkpoint");
    let sid = r.session_id();
    let mut files = after_files(&r, sid, &turn);
    files.sort();
    assert_eq!(files, ["keep.txt", "made.txt"]);
    let d = r.diff(&turn, false).await.expect("a diff");
    let names: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["keep.txt", "made.txt"], "{d:?}");
    assert!(
        d.others_wrote.is_empty() && d.others_may.is_empty(),
        "{d:?}"
    );
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

/// The same with two shared children of one step and a file the lead wrote
/// itself in the turn: one Undo, everything back.
#[tokio::test]
async fn one_undo_covers_the_lead_and_two_shared_children() {
    let mut r = repo_rig().await;
    let before = r.files();
    let (turn, outs) = r
        .lead_turn("delegate: dev=run: echo a > a.txt | dev=run: echo b > b.txt")
        .await;
    assert_eq!(outs.len(), 2, "{outs:#?}");
    let turn = turn.id.expect("a turn checkpoint");
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()), "{outs:#?}");
    assert_eq!(r.files(), before);
}

/// Another chat's write in the same span still refuses the lead's Undo, by
/// that file only (the shared child's own files are not "another chat's"),
/// and "Undo anyway" puts everything back.
#[tokio::test]
async fn a_different_chats_file_still_refuses_the_leads_undo() {
    let mut r = repo_rig().await;
    let before = r.files();
    let a = r.session_id();
    let b = r.new_chat().await;
    let mark = r.seen.len();
    r.n += 1;
    r.send(MasterToNode::Prompt(PromptMsg {
        agent: Some(r.lead()),
        req_id: format!("p{}", r.n),
        ..PromptMsg::test(a, "delegate: dev=run: sleep 1; echo c > c.txt")
    }));
    // Chat B writes while A's child sleeps: inside A's span.
    r.until(|k| matches!(k, EventKind::SessionCreated(c) if c.parent_session.is_some()))
        .await;
    r.n += 1;
    r.send(MasterToNode::Prompt(PromptMsg {
        agent: Some(r.lead()),
        req_id: format!("p{}", r.n),
        ..PromptMsg::test(
            b,
            format!(
                "script: {}",
                json!([{"tool": "write", "input": {"path": "b.txt", "content": "B\n"}}])
            ),
        )
    }));
    let ended = |r: &Rig, sid: SessionId| {
        r.seen[mark..]
            .iter()
            .any(|e| e.session_id == sid && matches!(e.typed(), Ok(EventKind::TurnEnded(_))))
    };
    while !(ended(&r, a) && ended(&r, b)) {
        r.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    }
    let turn = r.seen[mark..]
        .iter()
        .filter(|e| e.session_id == a)
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After => Some(c.turn_id),
            _ => None,
        })
        .expect("A's turn checkpoint");
    assert_eq!(r.read("c.txt").as_deref(), Some("c\n"));
    assert_eq!(r.read("b.txt").as_deref(), Some("B\n"));
    let d = r.diff_of(a, &turn, false).await.expect("a diff");
    assert_eq!(d.others_wrote, ["b.txt"], "{d:?}");
    assert!(!d.others_may.iter().any(|f| f == "c.txt"), "{d:?}");
    let mid = r.files();
    let undo = r.restore_of(a, &turn, false, false).await;
    let err = undo.result.expect_err("refused: another chat's file");
    assert!(
        err.contains("another chat") && err.contains("`b.txt`") && !err.contains("`c.txt`"),
        "{err}"
    );
    assert_eq!(r.files(), mid, "a refused undo changed the folder");
    let undo = r.restore_of(a, &turn, true, false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

/// A background shared sub-agent outlives the lead's turn. What it writes
/// after that is in no turn of the lead: the lead's Undo leaves it, and the
/// sub-agent's own chat undoes it from its own checkpoints.
#[tokio::test]
async fn a_background_childs_later_changes_are_undone_in_its_own_chat() {
    let mut r = repo_rig().await;
    r.lead_caps = Some(DelegateCaps {
        background: true,
        ..Default::default()
    });
    let lead = r.session_id();
    let script = json!([
        {"tool": "write", "input": {"path": "own.txt", "content": "lead\n"}},
        {"tool": "delegate", "input": {
            "agent": "dev", "background": true,
            "task": "run: sleep 1; echo late > late.txt"
        }},
    ]);
    let (turn, outs) = r.lead_turn(&format!("script: {script}")).await;
    assert!(outs[0].contains("started in the background"), "{outs:#?}");
    let turn = turn.id.expect("a turn checkpoint");
    // Its end reaches the lead (the master would start a callback turn;
    // this rig has none).
    r.until(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .await;
    assert_eq!(r.read("late.txt").as_deref(), Some("late\n"));
    // Not in the lead's turn.
    assert_eq!(after_files(&r, lead, &turn), ["own.txt"]);
    // The sub-agent's own chat has a checkpoint pair naming it.
    let child = children(&r)[0];
    let child_turn = r
        .seen
        .iter()
        .filter(|e| e.session_id == child)
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After => {
                assert_eq!(c.files, ["late.txt"]);
                Some(c.turn_id)
            }
            _ => None,
        })
        .expect("the sub-agent took checkpoints of its own");
    // The lead's Undo of its turn takes its own file and leaves the late one.
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()), "{:?}", undo.result);
    assert!(r.read("own.txt").is_none());
    assert_eq!(r.read("late.txt").as_deref(), Some("late\n"));
    // The sub-agent's own chat undoes what it wrote.
    let undo = r.restore_of(child, &child_turn, false, false).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("late.txt").is_none());
}

/// A background sub-agent's writes while one of the lead's turns runs are
/// that turn's (its checkpoint holds them): the turn's Undo takes them back
/// with the lead's own, as it did for a background server's write (T47).
#[tokio::test]
async fn a_background_childs_writes_inside_the_leads_turn_are_undone_with_it() {
    let mut r = repo_rig().await;
    r.lead_caps = Some(DelegateCaps {
        background: true,
        ..Default::default()
    });
    let before = r.files();
    let lead = r.session_id();
    let script = json!([
        {"tool": "delegate", "input": {
            "agent": "dev", "background": true, "task": "run: echo fast > fast.txt"
        }},
        {"tool": "bash", "input": {"command": "sleep 1"}},
        {"tool": "write", "input": {"path": "own.txt", "content": "lead\n"}},
    ]);
    let (turn, _) = r.lead_turn(&format!("script: {script}")).await;
    let turn = turn.id.expect("a turn checkpoint");
    assert_eq!(r.read("fast.txt").as_deref(), Some("fast\n"));
    let mut files = after_files(&r, lead, &turn);
    files.sort();
    assert_eq!(files, ["fast.txt", "own.txt"]);
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

/// What a background sub-agent writes BETWEEN the lead's turns is another
/// chat's work for a rewind that spans the gap: it asks first, naming the
/// file and not the lead's own, and "Undo anyway" puts everything back.
#[tokio::test]
async fn a_rewind_across_a_background_childs_work_between_turns_asks_first() {
    let mut r = repo_rig().await;
    r.lead_caps = Some(DelegateCaps {
        background: true,
        ..Default::default()
    });
    let before = r.files();
    let script = json!([
        {"tool": "delegate", "input": {
            "agent": "dev", "background": true,
            "task": "run: sleep 1; echo late > late.txt"
        }},
    ]);
    let (first, _) = r.lead_turn(&format!("script: {script}")).await;
    let first = first.id.expect("a turn checkpoint");
    r.until(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .await;
    assert_eq!(r.read("late.txt").as_deref(), Some("late\n"));
    let own = json!([{"tool": "write", "input": {"path": "own.txt", "content": "lead\n"}}]);
    r.lead_turn(&format!("script: {own}")).await;
    let mid = r.files();
    let undo = r.restore_with(&first, false, true).await;
    let err = undo.result.expect_err("refused: another chat's file");
    assert!(
        err.contains("another chat") && err.contains("`late.txt`") && !err.contains("`own.txt`"),
        "{err}"
    );
    assert_eq!(r.files(), mid);
    let undo = r.restore_with(&first, true, true).await;
    assert_eq!(undo.result, Ok(()));
    assert_eq!(r.files(), before);
}

// ---- What a PERSON asks a sub-agent's chat to do is not the lead's work -----

/// The first line of the child's session id, as the `delegate` result gives it.
fn child_of(out: &str) -> SessionId {
    out.strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no session line: {out}"))
}

/// A person's message to `sid`'s chat now (the master accepts one for a
/// sub-agent's chat that is not running).
fn prompt(r: &mut Rig, sid: SessionId, text: String) {
    r.n += 1;
    r.send(MasterToNode::Prompt(PromptMsg {
        req_id: format!("p{}", r.n),
        agent: Some(r.lead()),
        ..PromptMsg::test(sid, text)
    }));
}

/// Wait until `count` more turns have ended.
async fn turns_end(r: &mut Rig, count: usize) {
    for _ in 0..count {
        r.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
    }
}

/// The `after` checkpoint's turn id of the last turn of `sid`.
fn last_turn(r: &Rig, sid: SessionId) -> String {
    r.seen
        .iter()
        .rev()
        .filter(|e| e.session_id == sid)
        .find_map(|e| match e.typed() {
            Ok(EventKind::Checkpoint(c)) if c.phase == CheckpointPhase::After => Some(c.turn_id),
            _ => None,
        })
        .expect("a turn checkpoint")
}

/// The lead's second turn: it writes `lead.txt` and waits a while.
fn slow_lead_turn() -> String {
    format!(
        "script: {}",
        json!([
            {"tool": "write", "input": {"path": "lead.txt", "content": "lead\n"}},
            {"tool": "bash", "input": {"command": "sleep 4"}},
        ])
    )
}

/// A finished sub-agent's chat is asked by a PERSON to make a file while
/// the lead's next turn runs. The lead's Undo of that turn must not delete
/// it silently: it is refused naming `direct.txt` (not the lead's own
/// files), and "Undo anyway" puts everything back.
#[tokio::test]
async fn a_persons_message_to_a_finished_childs_chat_during_the_leads_turn_refuses_the_undo() {
    let mut r = repo_rig().await;
    let (_, outs) = r.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    let child = child_of(&outs[0]);
    let lead = r.session_id();
    let before = r.files();
    prompt(&mut r, lead, slow_lead_turn());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    turns_end(&mut r, 2).await;
    assert_eq!(r.read("direct.txt").as_deref(), Some("direct\n"));
    let turn = last_turn(&r, lead);
    let mid = r.files();
    let undo = r.restore(&turn, false).await;
    let err = undo
        .result
        .expect_err("refused: a person's work in the child's chat");
    assert!(
        err.contains("another chat") && err.contains("`direct.txt`") && !err.contains("`lead.txt`"),
        "{err}"
    );
    assert_eq!(r.files(), mid, "a refused undo changed the folder");
    let undo = r.restore(&turn, true).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("direct.txt").is_none() && r.read("lead.txt").is_none());
    assert_eq!(r.files(), before);
}

/// The same for a background sub-agent's chat that has ended.
#[tokio::test]
async fn a_persons_message_to_a_finished_background_childs_chat_refuses_the_undo_too() {
    let mut r = repo_rig().await;
    r.lead_caps = Some(DelegateCaps {
        background: true,
        ..Default::default()
    });
    let script = json!([
        {"tool": "delegate", "input": {"agent": "dev", "background": true, "task": "run: echo fast > fast.txt"}},
        {"tool": "bash", "input": {"command": "sleep 1"}},
    ]);
    let (_, outs) = r.lead_turn(&format!("script: {script}")).await;
    let child = child_of(&outs[0]);
    let lead = r.session_id();
    prompt(&mut r, lead, slow_lead_turn());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    turns_end(&mut r, 2).await;
    let turn = last_turn(&r, lead);
    let undo = r.restore(&turn, false).await;
    let err = undo
        .result
        .expect_err("refused: a person's work in the child's chat");
    assert!(
        err.contains("`direct.txt`") && !err.contains("`lead.txt`"),
        "{err}"
    );
    let undo = r.restore(&turn, true).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("direct.txt").is_none());
}

/// The person asked the child's chat BEFORE the lead's turn: not this
/// turn's work at all, and its file stays.
#[tokio::test]
async fn a_persons_message_to_a_childs_chat_before_the_leads_turn_is_left_alone() {
    let mut r = repo_rig().await;
    let (_, outs) = r.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    let child = child_of(&outs[0]);
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    turns_end(&mut r, 1).await;
    let own = json!([{"tool": "write", "input": {"path": "lead.txt", "content": "lead\n"}}]);
    let (turn, _) = r.lead_turn(&format!("script: {own}")).await;
    let turn = turn.id.expect("a turn checkpoint");
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("lead.txt").is_none());
    assert_eq!(r.read("direct.txt").as_deref(), Some("direct\n"));
}

/// A follow-up the lead sent through `delegate` is the lead's own work:
/// its files are undone with the turn, without asking, and so are the
/// files of the delegation's first turn.
#[tokio::test]
async fn a_follow_up_through_delegate_is_still_the_leads_own_work() {
    let mut r = repo_rig().await;
    let (_, outs) = r.lead_turn("delegate: dev=run: echo one > b.txt").await;
    let child = child_of(&outs[0]);
    let before = r.files();
    let (turn, outs) = r
        .lead_turn(&format!(
            "delegate: dev@{child}=run: echo two >> b.txt; echo f > f.txt"
        ))
        .await;
    assert!(
        outs[0].contains("worked in this chat's folder"),
        "{outs:#?}"
    );
    let turn = turn.id.expect("a turn checkpoint");
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()), "{outs:#?}");
    assert_eq!(r.files(), before);
}

/// The rewind's files part is the same restore: a person's work in the
/// child's chat during the rewound turn asks first there too.
#[tokio::test]
async fn a_rewind_asks_first_about_a_persons_work_in_a_childs_chat_as_well() {
    let mut r = repo_rig().await;
    let (_, outs) = r.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    let child = child_of(&outs[0]);
    let lead = r.session_id();
    prompt(&mut r, lead, slow_lead_turn());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    turns_end(&mut r, 2).await;
    let turn = last_turn(&r, lead);
    let undo = r.restore_with(&turn, false, true).await;
    let err = undo.result.expect_err("refused");
    assert!(
        err.contains("`direct.txt`") && !err.contains("`lead.txt`"),
        "{err}"
    );
    assert_eq!(r.read("direct.txt").as_deref(), Some("direct\n"));
}

// ---- An Undo while a sub-agent still works in the folder --------------------

/// A background shared sub-agent still running when the lead undoes its
/// turn: refused (a restore under a writer is not safe), and the sentence
/// says what is true and what to do: it names the sub-agent.
#[tokio::test]
async fn an_undo_while_a_background_sub_agent_runs_names_it_and_says_what_to_do() {
    let mut r = repo_rig().await;
    r.lead_caps = Some(DelegateCaps {
        background: true,
        ..Default::default()
    });
    let script = json!([
        {"tool": "write", "input": {"path": "own.txt", "content": "lead\n"}},
        {"tool": "delegate", "input": {
            "agent": "dev", "background": true, "task": "run: sleep 3; echo late > late.txt"
        }},
    ]);
    let (turn, _) = r.lead_turn(&format!("script: {script}")).await;
    let turn = turn.id.expect("a turn checkpoint");
    let undo = r.restore(&turn, false).await;
    let err = undo.result.expect_err("refused while the sub-agent works");
    assert!(
        err.contains(
            "its background sub-agent @dev is still working in this folder: stop it or wait \
             for it, then undo"
        ),
        "{err}"
    );
    assert_eq!(r.read("own.txt").as_deref(), Some("lead\n"));
    r.until(|k| matches!(k, EventKind::BackgroundEnded(_)))
        .await;
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
}

/// Another chat (not this chat's sub-agent) running in the folder keeps
/// today's sentence.
#[tokio::test]
async fn an_undo_while_a_different_chat_runs_keeps_the_old_sentence() {
    let mut r = repo_rig().await;
    let a = r.session_id();
    let b = r.new_chat().await;
    let own = json!([{"tool": "write", "input": {"path": "own.txt", "content": "A\n"}}]);
    let (turn, _) = r.lead_turn(&format!("script: {own}")).await;
    let turn = turn.id.expect("a turn checkpoint");
    prompt(
        &mut r,
        b,
        format!(
            "script: {}",
            json!([{"tool": "bash", "input": {"command": "sleep 3"}}])
        ),
    );
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    let undo = r.restore_of(a, &turn, false, false).await;
    let err = undo.result.expect_err("refused while another chat works");
    assert!(
        err.contains(
            "another chat is working in this folder right now; wait for it to finish or cancel it"
        ),
        "{err}"
    );
    turns_end(&mut r, 1).await;
}

// ---- A sub-agent whose task never started takes no turn from anyone else ----

/// The ack of request `req`, other events kept.
async fn ack_of(r: &mut Rig, req: &str) -> (bool, String) {
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(10), r.recv())
            .await
            .expect("an ack")
        {
            harness_protocol::NodeToMaster::Ack {
                req_id, ok, error, ..
            } if req_id == req => return (ok, error.unwrap_or_default()),
            harness_protocol::NodeToMaster::Event { event } => r.seen.push(event),
            _ => {}
        }
    }
}

/// A lead turn whose delegation's first `turn_started` fails to be written
/// (the failure hook of the A8 case): the child's chat exists with a
/// `session_created` and no turn. Returns its id.
async fn never_started_child(r: &mut Rig) -> SessionId {
    let db = rusqlite::Connection::open(r.dir.path().join("node.db")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER break_start BEFORE INSERT ON events
         WHEN NEW.kind = 'turn_started' AND NEW.session_id <> '{}'
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
        r.session_id()
    ))
    .unwrap();
    r.lead_turn("delegate: dev=run: echo v2 > keep.txt").await;
    db.execute_batch("DROP TRIGGER break_start").unwrap();
    let child = children(r)[0];
    assert!(
        !r.seen
            .iter()
            .any(|e| e.session_id == child && matches!(e.typed(), Ok(EventKind::TurnStarted(_)))),
        "the child's first start was meant to fail"
    );
    child
}

fn events_of(r: &Rig, sid: SessionId) -> usize {
    r.seen.iter().filter(|e| e.session_id == sid).count()
}

/// (a) A person's message to such a chat is refused with a plain sentence
/// and nothing is appended to its log: it would otherwise be the chat's
/// "first" turn, which the lead's Undo reads as the lead's own work.
#[tokio::test]
async fn a_persons_message_to_a_sub_agent_that_never_started_is_refused() {
    let mut r = repo_rig().await;
    let child = never_started_child(&mut r).await;
    let before = events_of(&r, child);
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    let (ok, err) = {
        let req = format!("p{}", r.n);
        ack_of(&mut r, &req).await
    };
    assert!(!ok, "accepted");
    assert!(err.contains("never started its task"), "{err}");
    assert_eq!(events_of(&r, child), before, "something was appended");
    assert!(r.read("direct.txt").is_none());
    // Nor does the log of the store hold a turn for it.
    let id = child;
    let n = r
        .seen
        .iter()
        .filter(|e| e.session_id == id && matches!(e.typed(), Ok(EventKind::TurnStarted(_))))
        .count();
    assert_eq!(n, 0);
}

/// (b) The other paths that start a turn go through the same write: a
/// retry and a callback for such a chat are refused too, nothing appended.
/// (Both are refused earlier for their own reasons as well; the refusal at
/// the write is what no new path can forget.)
#[tokio::test]
async fn a_retry_or_a_callback_for_a_sub_agent_that_never_started_starts_nothing() {
    let mut r = repo_rig().await;
    let child = never_started_child(&mut r).await;
    let before = events_of(&r, child);
    r.n += 1;
    let req = format!("x{}", r.n);
    r.send(MasterToNode::Retry(harness_protocol::RetryMsg {
        req_id: req.clone(),
        agent: Some(r.lead()),
        ..harness_protocol::RetryMsg::test(child, "t-none")
    }));
    let (ok, _) = ack_of(&mut r, &req).await;
    assert!(!ok);
    r.n += 1;
    let req = format!("y{}", r.n);
    r.send(MasterToNode::Callback(harness_protocol::CallbackMsg {
        req_id: req.clone(),
        agent: Some(r.lead()),
        ..harness_protocol::CallbackMsg::test(child, vec!["bg1".into()])
    }));
    let (ok, _) = ack_of(&mut r, &req).await;
    assert!(!ok);
    assert_eq!(events_of(&r, child), before);
}

/// (c) The consequence: a person's message to such a chat while the lead's
/// next turn runs would write a file the lead's Undo deletes silently.
/// The message never runs, so there is nothing to revert.
#[tokio::test]
async fn a_never_started_sub_agents_chat_cannot_be_used_to_write_during_a_lead_turn() {
    let mut r = repo_rig().await;
    let child = never_started_child(&mut r).await;
    let lead = r.session_id();
    prompt(&mut r, lead, slow_lead_turn());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    prompt(&mut r, child, "run: echo direct > direct.txt".into());
    let (ok, _) = {
        let req = format!("p{}", r.n);
        ack_of(&mut r, &req).await
    };
    assert!(!ok, "the message was accepted");
    turns_end(&mut r, 1).await;
    assert!(r.read("direct.txt").is_none(), "the file was written");
    let turn = last_turn(&r, lead);
    let undo = r.restore(&turn, false).await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("lead.txt").is_none());
}
