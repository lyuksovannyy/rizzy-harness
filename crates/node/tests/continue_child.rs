//! A parent gives a sub-agent it already ran a follow-up (PLAN.md T68,
//! ADOPTION_PLAN X.22): `delegate` with `session` set to the id the
//! child's result began with. The node decides from its own log whether
//! the child is the caller's own; the child keeps its conversation, not
//! its files, and gains nothing by being continued.

#[path = "support/rig.rs"]
mod support;

use harness_agent_native::provider::ChatMsg;
use harness_core::event::*;
use harness_core::subagent::{FOLLOW_UP_READS, MAX_CHILD_RUNS, follow_up_writes};
use harness_core::{RawEvent, SessionId};
use harness_protocol::MasterToNode;
use serde_json::json;
use support::Rig;

/// What `a.txt` holds at first.
const A: &str = "alpha-7f3\n";

/// A repository with `a.txt` = [`A`].
async fn rig() -> Rig {
    let r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("a.txt", A);
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    r
}

/// The scripted lead's `delegate` calls, one per step, in order.
fn delegate(calls: &[serde_json::Value]) -> String {
    format!(
        "script: {}",
        json!(
            calls
                .iter()
                .map(|c| json!({"tool": "delegate", "input": c}))
                .collect::<Vec<_>>()
        )
    )
}

/// The child a `delegate` result names.
fn child_of(out: &str) -> SessionId {
    out.strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no child in {out}"))
}

fn events_of(r: &Rig, sid: SessionId) -> Vec<EventKind> {
    r.seen
        .iter()
        .filter(|e| e.session_id == sid)
        .filter_map(|e| e.typed().ok())
        .collect()
}

fn raw_of(r: &Rig, sid: SessionId) -> Vec<RawEvent> {
    r.seen
        .iter()
        .filter(|e| e.session_id == sid)
        .cloned()
        .collect()
}

fn turns(evs: &[EventKind]) -> Vec<TurnStarted> {
    evs.iter()
        .filter_map(|k| match k {
            EventKind::TurnStarted(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// The requests sent for agent `id` (its system prompt names it).
fn requests_of(r: &Rig, id: &str) -> Vec<Vec<ChatMsg>> {
    let me = format!("You are @{id}.");
    r.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| matches!(q.messages.first(), Some(ChatMsg::System(s)) if s.contains(&me)))
        .map(|q| q.messages.clone())
        .collect()
}

fn text_of(m: &ChatMsg) -> String {
    match m {
        ChatMsg::Tool { content, .. } => content.clone(),
        ChatMsg::Assistant { text, .. } => text.clone(),
        other => other.user_text().unwrap_or_default().to_string(),
    }
}

/// The ids of the parent's `delegate` calls in `evs`, in order.
fn delegate_calls(evs: &[EventKind]) -> Vec<String> {
    evs.iter()
        .filter_map(|k| match k {
            EventKind::ToolCallStarted(c) if c.tool == "delegate" => Some(c.call_id.clone()),
            _ => None,
        })
        .collect()
}

/// A follow-up reaches the same child, which answers from its own
/// conversation (its earlier read is in the request) without reading
/// again; its log records the follow-up's turn with the parent's call.
#[tokio::test]
async fn a_follow_up_reaches_the_same_child_which_answers_from_its_conversation() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look",
            "session": child.to_string(),
            "task": "What did the file say?"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    let evs = events_of(&r, child);
    let t = turns(&evs);
    assert_eq!(t.len(), 2, "{t:#?}");
    assert_eq!(t[0].continued_by, None);
    let parent_calls = delegate_calls(&events_of(&r, r.session_id()));
    assert_eq!(t[1].continued_by.as_ref(), parent_calls.last());
    // One `read` in all: the follow-up read nothing again.
    let reads = evs
        .iter()
        .filter(|k| matches!(k, EventKind::ToolCallStarted(c) if c.tool == "read"))
        .count();
    assert_eq!(reads, 1);
    // Its request for the follow-up carried its earlier read and its
    // answer, then the harness's line and the parent's message.
    let reqs = requests_of(&r, "look");
    let last = reqs.last().unwrap();
    let all: Vec<String> = last.iter().map(text_of).collect();
    assert!(
        last.iter()
            .any(|m| matches!(m, ChatMsg::Tool { content, .. } if content.contains("alpha-7f3"))),
        "the earlier read is not in the follow-up's request: {all:#?}"
    );
    let opening = all.last().unwrap();
    assert!(opening.contains(FOLLOW_UP_READS), "{opening}");
    assert!(opening.ends_with("What did the file say?"), "{opening}");
    // Still one child: no second session was made.
    let made = r
        .seen
        .iter()
        .filter(|e| e.kind == "session_created")
        .count();
    assert_eq!(made, 2, "the chat and its one child");
}

/// Ids the chat did not start are refused by the node with one sentence:
/// an unknown id, a malformed one, the chat itself, and another chat's
/// child; nothing runs and no session is made.
#[tokio::test]
async fn unknown_malformed_own_and_foreign_ids_are_refused() {
    let mut r = rig().await;
    let other = r.new_chat().await;
    let (_, outs) = r
        .lead_turn_in(
            other,
            &delegate(&[json!({"agent": "look", "task": "read: a.txt"})]),
        )
        .await;
    let foreign = child_of(&outs[0]);
    let before = r.seen.len();
    for (id, want) in [
        (
            SessionId::new().to_string(),
            "is not a sub-agent this chat started",
        ),
        ("../../etc".to_string(), "`session` must be the id"),
        (
            r.session_id().to_string(),
            "is not a sub-agent this chat started",
        ),
        (foreign.to_string(), "is not a sub-agent this chat started"),
    ] {
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({
                "agent": "look", "session": id, "task": "read: a.txt"
            })]))
            .await;
        assert!(outs[0].contains(want), "{id}: {outs:#?}");
    }
    assert!(
        !r.seen[before..]
            .iter()
            .any(|e| e.session_id == foreign || e.kind == "session_created"),
        "a refused follow-up ran something"
    );
}

/// A deleted child is unknown: refused.
#[tokio::test]
async fn a_deleted_child_is_refused() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    r.send(MasterToNode::Delete {
        req_id: "del".into(),
        session_id: child,
    });
    loop {
        if let harness_protocol::NodeToMaster::Deleted { session_id } = r.recv().await
            && session_id == child
        {
            break;
        }
    }
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look", "session": child.to_string(), "task": "again"
        })]))
        .await;
    assert!(
        outs[0].contains("is not a sub-agent this chat started"),
        "{outs:#?}"
    );
}

/// The agent named must be the child's: a reader's id named as @dev is
/// refused (it would gain `bash`).
#[tokio::test]
async fn a_child_cannot_be_continued_as_another_agent() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: echo x > b.txt"
        })]))
        .await;
    assert!(outs[0].contains("is @look, not @dev"), "{outs:#?}");
    assert!(r.read("b.txt").is_none());
    assert_eq!(turns(&events_of(&r, child)).len(), 1);
}

/// The same child twice in one step: the loop refuses the second. Spelled
/// differently (upper case), the loop lets both through and the node
/// refuses whichever comes second: the child is still working.
#[tokio::test]
async fn the_same_child_twice_in_one_step_runs_once() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let id = child.to_string();
    let (_, outs) = r
        .lead_turn(&format!(
            "delegate: look@{id}=slow: 10 | look@{id}=slow: 10"
        ))
        .await;
    assert_eq!(
        outs.iter()
            .filter(|o| o.contains("this step already sends a follow-up"))
            .count(),
        1,
        "{outs:#?}"
    );
    let (_, outs) = r
        .lead_turn(&format!(
            "delegate: look@{id}=slow: 10 | look@{}=slow: 10",
            id.to_uppercase()
        ))
        .await;
    assert_eq!(
        outs.iter()
            .filter(|o| o.contains("is still working"))
            .count(),
        1,
        "{outs:#?}"
    );
    // First run, one follow-up per step that ran.
    assert_eq!(turns(&events_of(&r, child)).len(), 3);
}

/// A follow-up counts against the turn's sub-agent cap like a new child.
#[tokio::test]
async fn a_follow_up_counts_against_the_turns_cap() {
    let mut r = rig().await;
    r.lead_caps = Some(harness_core::agent::DelegateCaps {
        max_subagents: 1,
        ..Default::default()
    });
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let (_, outs) = r
        .lead_turn(&format!("delegate: look=read: a.txt | look@{child}=again"))
        .await;
    assert!(
        outs.iter().any(|o| o.contains("cap reached: at most 1")),
        "{outs:#?}"
    );
    assert_eq!(turns(&events_of(&r, child)).len(), 1);
}

/// At most `MAX_CHILD_RUNS` runs per child; the next follow-up is refused.
#[tokio::test]
async fn a_child_has_at_most_max_child_runs() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let id = child.to_string();
    for _ in 1..MAX_CHILD_RUNS {
        let (_, outs) = r
            .lead_turn(&delegate(&[
                json!({"agent": "look", "session": id, "task": "ok?"}),
            ]))
            .await;
        assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    }
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "session": id, "task": "ok?"}),
        ]))
        .await;
    assert!(
        outs[0].contains("the most one sub-agent may have"),
        "{outs:#?}"
    );
    assert_eq!(turns(&events_of(&r, child)).len(), MAX_CHILD_RUNS);
}

/// A writing child's follow-up works in a fresh copy of the folder as it
/// is now (the parent's change since is there), is told so, and its new
/// change is merged back.
#[tokio::test]
async fn a_writing_childs_follow_up_works_in_a_fresh_copy_and_merges_back() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: echo one > b.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    assert_eq!(r.read("b.txt").as_deref(), Some("one\n"), "{outs:#?}");
    // The folder moves on between its runs.
    r.write("b.txt", "parent\n");
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(),
            "task": "run: cat b.txt && echo two >> b.txt"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child);
    assert!(outs[0].contains("applied to the folder"), "{outs:#?}");
    assert_eq!(r.read("b.txt").as_deref(), Some("parent\ntwo\n"));
    let reqs = requests_of(&r, "dev");
    // The follow-up's first request: its opening says where it works now.
    let opening = reqs
        .iter()
        .rev()
        .find_map(|m| {
            m.iter()
                .rev()
                .find_map(|x| x.user_text().map(str::to_string))
                .filter(|t| t.contains("echo two"))
        })
        .expect("the follow-up's request");
    assert!(opening.contains(&follow_up_writes(true)), "{opening}");
    // What it saw in its fresh copy was the parent's version.
    let evs = events_of(&r, child);
    assert!(
        evs.iter().any(|k| matches!(k,
            EventKind::ToolCallFinished(f) if f.output.contains("parent"))),
        "{evs:#?}"
    );
}

/// A child whose changes were NOT applied (two children changed the same
/// lines) can be given a follow-up to sort it out: its copy is the folder
/// as it is now, without its unapplied change, and its opening says so.
#[tokio::test]
async fn a_child_whose_changes_were_not_applied_sees_the_folder_without_them() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn("delegate: dev=run: echo ONE > a.txt | dev=run: echo TWO > a.txt")
        .await;
    let refused = outs
        .iter()
        .find(|o| o.contains("NOT applied"))
        .expect("one not applied");
    let child = child_of(refused);
    let now = r.read("a.txt").unwrap();
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: cat a.txt"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child);
    let reqs = requests_of(&r, "dev");
    let opening = reqs
        .iter()
        .rev()
        .find_map(|m| {
            m.iter()
                .rev()
                .find_map(|x| x.user_text().map(str::to_string))
                .filter(|t| t.ends_with("run: cat a.txt"))
        })
        .expect("the follow-up's request");
    assert!(opening.contains(&follow_up_writes(false)), "{opening}");
    let evs = events_of(&r, child);
    let seen_now = evs.iter().rev().find_map(|k| match k {
        EventKind::ToolCallFinished(f) => Some(f.output.clone()),
        _ => None,
    });
    assert!(
        seen_now.as_deref().is_some_and(|o| o.contains(now.trim())),
        "{seen_now:?} vs {now}"
    );
}

/// The read-first table (T45) never lets a continued child edit a file of
/// its fresh copy on the strength of a read of a different version: the
/// table is by content, so a file the parent changed since must be read
/// again, and one that did not change is still known.
#[tokio::test]
async fn a_continued_childs_edit_needs_a_new_read_of_a_changed_file() {
    let mut r = rig().await;
    r.write("c.txt", "same\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "c"]);
    let edit = |path: &str, old: &str, new: &str| json!({"tool": "edit", "input": {"path": path, "old_string": old, "new_string": new}});
    let read = |path: &str| json!({"tool": "read", "input": {"path": path}});
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "editor",
            "task": format!("script: {}", json!([read("a.txt"), read("c.txt")]))
        })]))
        .await;
    let child = child_of(&outs[0]);
    // The parent changes a.txt; c.txt stays.
    r.write("a.txt", "changed by the parent\n");
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "editor", "session": child.to_string(),
            "task": format!("script: {}", json!([
                edit("a.txt", "changed", "CHANGED"),
                edit("c.txt", "same", "SAME"),
            ]))
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child);
    let evs = events_of(&r, child);
    let results: Vec<(bool, String)> = evs
        .iter()
        .filter_map(|k| match k {
            EventKind::ToolCallFinished(f) => Some((f.ok, f.output.clone())),
            _ => None,
        })
        .collect();
    let n = results.len();
    let (a, c) = (&results[n - 2], &results[n - 1]);
    assert!(!a.0, "an edit of a file changed since its read ran: {a:?}");
    assert!(c.0, "an unchanged file's edit was refused: {c:?}");
    assert_eq!(r.read("a.txt").as_deref(), Some("changed by the parent\n"));
    assert_eq!(r.read("c.txt").as_deref(), Some("SAME\n"));
}

/// The parent's mode now is the child's ceiling: a child made in Auto and
/// continued after the chat went to Ask asks before its command runs.
#[tokio::test]
async fn a_child_continued_after_the_chat_went_to_ask_asks() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: echo one > b.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    r.send(MasterToNode::SetApprovalMode {
        req_id: "m".into(),
        session_id: r.session_id(),
        mode: harness_core::policy::ApprovalMode::Ask,
        lowering: false,
    });
    r.until(|k| matches!(k, EventKind::ApprovalModeChanged(_)))
        .await;
    r.n += 1;
    r.send(MasterToNode::Prompt {
        savings: None,
        context: None,
        reply: None,
        agent: Some(r.lead()),
        attachments: vec![],
        compare: vec![],
        req_id: format!("p{}", r.n),
        session_id: r.session_id(),
        text: delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: echo two > b.txt"
        })]),
        images: vec![],
        agent_named: false,
        files: vec![],
    });
    let sid = r.session_id();
    r.until(|k| matches!(k, EventKind::ApprovalRequested(a) if a.tool == "bash"))
        .await;
    let asked = r.seen.last().unwrap().clone();
    assert_eq!(asked.session_id, child, "the child's command must ask");
    let Ok(EventKind::ApprovalRequested(a)) = asked.typed() else {
        unreachable!()
    };
    r.send(MasterToNode::ApprovalRespond {
        req_id: "a".into(),
        session_id: child,
        approval_id: a.approval_id,
        decision: harness_core::policy::Decision::Deny,
        note: None,
    });
    loop {
        r.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
        if r.seen.last().is_some_and(|e| e.session_id == sid) {
            break;
        }
    }
    assert_eq!(r.read("b.txt").as_deref(), Some("one\n"));
}

/// Depth cannot be got around: a child (at the node's depth limit, 1) has
/// no delegates, so a follow-up asking it to delegate is refused by its own
/// loop; and it cannot continue itself or its sibling.
#[tokio::test]
async fn a_follow_up_cannot_reach_past_the_depth_limit() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let (one, two) = (child_of(&outs[0]), child_of(&outs[1]));
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look", "session": one.to_string(),
            "task": delegate(&[json!({"agent": "look", "session": two.to_string(), "task": "x"})])
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), one);
    let evs = events_of(&r, one);
    // The child was offered no `delegate`: its scripted call was refused
    // by its loop, and the sibling never ran again.
    assert!(
        evs.iter().any(|k| matches!(k,
            EventKind::ToolCallFinished(f) if !f.ok)),
        "{evs:#?}"
    );
    assert_eq!(turns(&events_of(&r, two)).len(), 1);
}

/// The child's follow-up request rebuilds byte for byte from its log
/// (T59): every message before the last of what was sent, the opening with
/// the harness's line included, is what the log gives back.
#[tokio::test]
async fn a_follow_ups_request_rebuilds_from_the_childs_log() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let (_, _) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look", "session": child.to_string(), "task": "read: a.txt"
        })]))
        .await;
    let reqs = requests_of(&r, "look");
    let sent = reqs.last().unwrap();
    let rebuilt = harness_agent_native::history_to_messages(&raw_of(&r, child));
    // The log holds the last answer too: the request is a prefix.
    assert!(
        rebuilt.len() >= sent.len() - 1,
        "{rebuilt:#?}\nvs\n{sent:#?}"
    );
    assert_eq!(
        &rebuilt[..sent.len() - 1],
        &sent[1..],
        "the request does not rebuild"
    );
}

/// The parent's turn counts the follow-up's tokens once: its usage is its
/// own requests' plus the child's follow-up turn's, not the child's first.
#[tokio::test]
async fn the_parents_turn_counts_a_follow_up_once() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let mark = r.seen.len();
    let (_, _) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look", "session": child.to_string(), "task": "and?"
        })]))
        .await;
    let ended = |sid: SessionId| -> Vec<Usage> {
        r.seen[mark..]
            .iter()
            .filter(|e| e.session_id == sid)
            .filter_map(|e| match e.typed() {
                Ok(EventKind::TurnEnded(t)) => t.usage,
                _ => None,
            })
            .collect()
    };
    let child_turn = ended(child);
    assert_eq!(child_turn.len(), 1);
    let parent_turn = ended(r.session_id());
    let parent = &parent_turn[0];
    let child = &child_turn[0];
    let own: u64 = requests_of(&r, "lead")
        .iter()
        .rev()
        .take(2)
        .map(|m| harness_agent_native::provider::estimate_tokens(m))
        .sum();
    assert!(
        parent.input_tokens >= child.input_tokens,
        "{parent:?} {child:?}"
    );
    assert_eq!(
        parent.input_tokens,
        own + child.input_tokens,
        "{parent:?} {child:?}"
    );
}
