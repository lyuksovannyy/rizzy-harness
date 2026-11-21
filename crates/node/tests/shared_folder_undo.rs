//! A sub-agent that works in the chat's folder and the turn's Undo (the
//! owner, 2026-11-21; PLAN.md T32, T57).
//!
//! The lead's turn checkpoint is a snapshot of the whole folder, so what a
//! sub-agent the lead waited for wrote in that turn is in it: `files`,
//! "Show changes" and Undo cover it, and T57 does not count the lead's own
//! sub-agent as another chat. A background sub-agent is another chat for
//! that rule (the lead did not wait for it); what it writes after the
//! lead's turn is undone in its own chat, from its own checkpoints.

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
