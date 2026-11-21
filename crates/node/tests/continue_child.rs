//! A parent gives a sub-agent it already ran a follow-up (PLAN.md T68,
//! ADOPTION_PLAN X.22): `delegate` with `session` set to the id the
//! child's result began with. The node decides from its own log whether
//! the child is the caller's own; the child keeps its conversation, not
//! its files, and gains nothing by being continued.

#[path = "support/rig.rs"]
mod support;

use harness_agent_native::provider::ChatMsg;
use harness_core::event::*;
use harness_core::subagent::{FOLLOW_UP_READS, Landed, MAX_CHILD_RUNS, follow_up_writes};
use harness_core::{RawEvent, SessionId};
use harness_protocol::{MasterToNode, PromptMsg};
use serde_json::json;
use support::Rig;

/// What `a.txt` holds at first.
const A: &str = "alpha-7f3\n";

/// A repository with `a.txt` = [`A`].
async fn rig() -> Rig {
    init(Rig::new().await)
}

/// [`rig`] on a node that says `sub_agent_folder = "own"` (every writer
/// gets a copy of its own).
async fn rig_own() -> Rig {
    init(
        Rig::with_policy(|p| p.limits.sub_agent_folder = harness_core::subagent::SubFolder::Own)
            .await,
    )
}

fn init(r: Rig) -> Rig {
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
            json!({"agent": "dev", "folder": "own", "task": "run: echo one > b.txt"}),
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
    assert!(
        opening.contains(&follow_up_writes(Landed::All)),
        "{opening}"
    );
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
        .lead_turn("delegate: dev#own=run: echo ONE > a.txt | dev#own=run: echo TWO > a.txt")
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
    assert!(
        opening.contains(&follow_up_writes(Landed::None)),
        "{opening}"
    );
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

/// The read-first table (T45) holds for a continued child: an `edit` of
/// lines the parent changed since the child read the file is refused (the
/// child must read it again), and a file that did not change is still
/// known.
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
    r.send(MasterToNode::Prompt(PromptMsg {
        agent: Some(r.lead()),
        req_id: format!("p{}", r.n),
        ..PromptMsg::test(
            r.session_id(),
            delegate(&[json!({
                "agent": "dev", "session": child.to_string(), "task": "run: echo two > b.txt"
            })]),
        )
    }));
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

// ---- From the independent review of pull request 127 (r-child3) ----

fn last_output(r: &Rig, sid: SessionId) -> String {
    events_of(r, sid)
        .iter()
        .rev()
        .find_map(|k| match k {
            EventKind::ToolCallFinished(f) => Some(f.output.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn sessions_dir(r: &Rig) -> std::path::PathBuf {
    r.dir.path().join("sessions")
}

/// Nothing of a writer's first copy survives into its follow-up's copy: an
/// ignored file, a link out of the copy, a branch and a hook written into
/// its own `.git`, and the directory itself (a new inode). The folder is
/// gone between runs.
#[tokio::test]
async fn nothing_of_the_first_copy_survives_into_the_follow_up() {
    let mut r = rig().await;
    r.write(".gitignore", "*.log\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "ignore logs"]);
    let outside = r.dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let cmd = format!(
        "stat -c INODE=%i . && echo s > secret.log && ln -s {} link.log && \
         echo x > .git/refs/heads/evil && mkdir -p .git/hooks && \
         echo h > .git/hooks/post-checkout && echo planted",
        outside.display()
    );
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "folder": "own", "task": format!("run: {cmd}")}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let first = last_output(&r, child);
    assert!(first.contains("planted"), "{first}");
    assert!(!sessions_dir(&r).join(child.to_string()).exists());
    let probe = "stat -c INODE=%i . ; for p in secret.log link.log .git/refs/heads/evil \
                 .git/hooks/post-checkout; do if test -e $p || test -L $p; then \
                 echo SURVIVED:$p; fi; done; echo probed";
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": format!("run: {probe}")
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    let second = last_output(&r, child);
    assert!(second.contains("probed"), "{second}");
    assert!(!second.contains("SURVIVED"), "{second}");
    let ino = |s: &str| {
        s.lines()
            .find_map(|l| l.trim().strip_prefix("INODE=").map(str::to_string))
    };
    assert_ne!(ino(&first), ino(&second), "the same directory was reused");
    assert!(!sessions_dir(&r).join(child.to_string()).exists());
}

/// A copy the node could not remove (the child made a folder it cannot
/// delete) blocks the follow-up: refused as "not continued", nothing runs
/// in the old copy.
#[tokio::test]
async fn an_unremovable_copy_fails_closed() {
    let mut r = rig().await;
    r.write(".gitignore", "*.log\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "ignore logs"]);
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev",
            "task": "run: mkdir keep.log && touch keep.log/f && chmod 500 keep.log && echo made"
        })]))
        .await;
    let child = child_of(&outs[0]);
    let left = sessions_dir(&r).join(child.to_string());
    let stuck = left.exists();
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: echo ran"
        })]))
        .await;
    let _ = std::process::Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(&left)
        .status();
    if stuck {
        assert!(!outs[0].starts_with("session: "), "{outs:#?}");
        assert!(outs[0].contains("was not continued"), "{outs:#?}");
        assert_eq!(turns(&events_of(&r, child)).len(), 1, "the follow-up ran");
    }
}

/// The refusal is the same sentence (but for the id) for an unknown id and
/// for another chat's child, and it is one sentence with no runs of
/// spaces (the line continuations of every refusal in `run_continue`).
#[tokio::test]
async fn no_oracle_between_unknown_and_foreign_and_no_runs_of_spaces() {
    let mut r = rig().await;
    let other = r.new_chat().await;
    let (_, outs) = r
        .lead_turn_in(
            other,
            &delegate(&[json!({"agent": "look", "task": "read: a.txt"})]),
        )
        .await;
    let foreign = child_of(&outs[0]);
    let mine = {
        let (_, outs) = r
            .lead_turn(&delegate(&[
                json!({"agent": "look", "task": "read: a.txt"}),
            ]))
            .await;
        child_of(&outs[0])
    };
    let unknown = SessionId::new();
    let mut got = vec![];
    for id in [foreign, unknown] {
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({
                "agent": "look", "session": id.to_string(), "task": "x"
            })]))
            .await;
        got.push(outs[0].replace(&id.to_string(), "<id>"));
    }
    assert_eq!(got[0], got[1]);
    // Another refusal of `run_continue` (the wrong agent).
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": mine.to_string(), "task": "x"
        })]))
        .await;
    got.push(outs[0].clone());
    for g in &got {
        assert!(!g.contains("   "), "a run of spaces: {g:?}");
    }
}

/// The read-first table is the snapshot rule of T45: an `edit` of lines
/// that did not change since the child read the file runs without a new
/// read, though the parent changed another line of it.
#[tokio::test]
async fn an_edit_of_unchanged_lines_of_a_changed_file_runs_without_a_read() {
    let mut r = rig().await;
    r.write("c.txt", "line one\nline two\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "c"]);
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "editor",
            "task": format!("script: {}", json!([{"tool": "read", "input": {"path": "c.txt"}}]))
        })]))
        .await;
    let child = child_of(&outs[0]);
    r.write("c.txt", "LINE ONE BY PARENT\nline two\n");
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "editor", "session": child.to_string(),
            "task": format!("script: {}", json!([{"tool": "edit", "input": {
                "path": "c.txt", "old_string": "line two", "new_string": "LINE TWO BY CHILD"}}]))
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    assert_eq!(
        r.read("c.txt").as_deref(),
        Some("LINE ONE BY PARENT\nLINE TWO BY CHILD\n")
    );
}

/// A `session` that is present but not an id (a number, an object, an
/// empty string) is refused: no new child starts in place of the follow-up
/// the model asked for.
#[tokio::test]
async fn a_session_that_is_not_an_id_is_refused_and_starts_nothing() {
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let _ = child_of(&outs[0]);
    let made = |r: &Rig| {
        r.seen
            .iter()
            .filter(|e| e.kind == "session_created")
            .count()
    };
    let before = made(&r);
    for bad in [json!(12345), json!({"id": "x"}), json!(""), json!("   ")] {
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({
                "agent": "look", "session": bad, "task": "read: a.txt"
            })]))
            .await;
        assert!(
            outs[0].contains("`session` must be the id"),
            "{bad}: {outs:#?}"
        );
    }
    assert_eq!(made(&r), before, "a new child was made");
}

/// A child started in a turn the person has since undone is still this
/// chat's child (its conversation is in the log; undo restores files) and
/// is continued, in the folder as the undo left it.
#[tokio::test]
async fn a_child_of_an_undone_turn_is_continued() {
    let mut r = rig().await;
    let (turn, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: echo one > b.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    assert_eq!(r.read("b.txt").as_deref(), Some("one\n"));
    let undo = r
        .restore(turn.id.as_deref().expect("a checkpoint"), false)
        .await;
    assert_eq!(undo.result, Ok(()));
    assert!(r.read("b.txt").is_none());
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: ls b.txt || echo no-b"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child);
    assert!(last_output(&r, child).contains("no-b"));
}

/// The child's requests rebuild byte for byte from its log when its
/// history was summarised between two of its runs (T59): the request of
/// its last run is the summary in the system message plus the rebuild of
/// what the summary does not cover.
#[tokio::test]
async fn a_follow_up_rebuilds_after_a_summary_inside_the_child() {
    let mut r = Rig::summarizing(400).await;
    r.git(&["init", "-q"]);
    // Large enough that three runs' reads are over the line.
    r.write(
        "a.txt",
        &format!("{A}{}", "filler line of the synthetic file\n".repeat(60)),
    );
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    // The summary is written at the start of the fourth run; the fifth is
    // sent from it and the log after it.
    for task in ["read: a.txt", "read: a.txt", "What did it say?", "And now?"] {
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({
                "agent": "look", "session": child.to_string(), "task": task
            })]))
            .await;
        assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    }
    let log = raw_of(&r, child);
    let last_start = log.iter().rposition(|e| e.kind == "turn_started").unwrap();
    assert!(
        log[..last_start]
            .iter()
            .any(|e| e.kind == "history_summarized"),
        "no summary in an earlier run"
    );
    // The request goes by the latest summary (the last run may write one
    // of its own at its start).
    let summary = log
        .iter()
        .rev()
        .find_map(|e| match e.typed() {
            Ok(EventKind::HistorySummarized(h)) => Some(h),
            _ => None,
        })
        .unwrap_or_else(|| {
            let kinds: Vec<&str> = r.seen.iter().map(|e| e.kind.as_str()).collect();
            panic!("the child's history was not summarised before its last run: {kinds:?}")
        });
    let reqs = requests_of(&r, "look");
    let sent = reqs.last().unwrap();
    match &sent[0] {
        ChatMsg::System(s) => assert!(s.contains(&summary.text), "{s}"),
        other => panic!("{other:?}"),
    }
    let recent: Vec<RawEvent> = log
        .iter()
        .filter(|e| e.seq > summary.through_seq)
        .cloned()
        .collect();
    let rebuilt = harness_agent_native::history_to_messages(&recent);
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

// ---- Plan mode (pull request 125, X.7, T67) and follow-ups ----

/// Sets the rig's chat to `mode` and waits for the log to say so.
async fn set_mode(r: &mut Rig, mode: harness_core::policy::ApprovalMode) {
    r.n += 1;
    r.send(MasterToNode::SetApprovalMode {
        req_id: format!("m{}", r.n),
        session_id: r.session_id(),
        mode,
        lowering: false,
    });
    r.until(|k| matches!(k, EventKind::ApprovalModeChanged(m) if m.mode == mode))
        .await;
}

/// Whether the last request of agent `id` was sent in plan mode.
fn last_request_in_plan(r: &Rig, id: &str) -> bool {
    requests_of(r, id).last().is_some_and(|m| {
        m.iter()
            .any(|x| text_of(x).contains("[harness] Plan mode is on"))
    })
}

/// (a) With the chat in plan mode, a follow-up to a child that changes
/// files is refused by the node, and the child does not run.
#[tokio::test]
async fn in_plan_mode_a_writing_child_is_not_continued() {
    use harness_core::policy::ApprovalMode;
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: echo one > b.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    set_mode(&mut r, ApprovalMode::Plan).await;
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: echo two > b.txt"
        })]))
        .await;
    assert!(
        outs[0].contains("plan mode: @dev") && outs[0].contains("was not continued"),
        "{outs:#?}"
    );
    assert!(
        !outs[0].contains("   "),
        "a lost line continuation: {:?}",
        outs[0]
    );
    assert_eq!(turns(&events_of(&r, child)).len(), 1);
    assert_eq!(r.read("b.txt").as_deref(), Some("one\n"));
}

/// (b) A child made in plan mode stays in it for life: continued after the
/// chat left plan mode, its turn is still in plan mode (the note is in its
/// request) and its command changes nothing.
#[tokio::test]
async fn a_child_made_in_plan_mode_stays_in_it() {
    use harness_core::policy::ApprovalMode;
    let mut r = rig().await;
    // A `bash`-only `dev`: a reader by the shared rule (PLAN.md T70), so
    // plan mode starts it (the rig's own `dev` also has `write`).
    r.delegates = vec![support::Rig::spec("dev", &["bash"], vec![])];
    set_mode(&mut r, ApprovalMode::Plan).await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: cat a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    assert!(last_request_in_plan(&r, "dev"));
    set_mode(&mut r, ApprovalMode::Auto).await;
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "dev", "session": child.to_string(), "task": "run: echo two > b.txt"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    assert!(
        last_request_in_plan(&r, "dev"),
        "the follow-up of a child made in plan mode left plan mode"
    );
    assert!(r.read("b.txt").is_none(), "a child made in plan mode wrote");
}

/// (c) A reader made outside plan mode and continued while the chat is in
/// plan mode runs its turn in plan mode.
#[tokio::test]
async fn a_reader_continued_in_plan_mode_runs_in_plan_mode() {
    use harness_core::policy::ApprovalMode;
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    assert!(!last_request_in_plan(&r, "look"));
    set_mode(&mut r, ApprovalMode::Plan).await;
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "look", "session": child.to_string(), "task": "read: a.txt"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    assert!(last_request_in_plan(&r, "look"));
}

/// Entering plan mode denies every waiting card of the chat and of every
/// loaded sub-agent under it: a continued child waiting on a card in its
/// follow-up is denied, writes nothing, and its result says why.
#[tokio::test]
async fn entering_plan_mode_denies_a_continued_childs_waiting_card() {
    use harness_core::policy::ApprovalMode;
    let mut r = rig().await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "dev", "task": "run: echo one > b.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    set_mode(&mut r, ApprovalMode::Ask).await;
    r.n += 1;
    r.send(MasterToNode::Prompt(PromptMsg {
        agent: Some(r.lead()),
        req_id: format!("p{}", r.n),
        ..PromptMsg::test(
            r.session_id(),
            delegate(&[json!({
                "agent": "dev", "session": child.to_string(), "task": "run: echo two > b.txt"
            })]),
        )
    }));
    r.until(|k| matches!(k, EventKind::ApprovalRequested(a) if a.tool == "bash"))
        .await;
    assert_eq!(r.seen.last().unwrap().session_id, child);
    set_mode(&mut r, ApprovalMode::Plan).await;
    let sid = r.session_id();
    loop {
        r.until(|k| matches!(k, EventKind::TurnEnded(_))).await;
        if r.seen.last().is_some_and(|e| e.session_id == sid) {
            break;
        }
    }
    let evs = events_of(&r, child);
    assert!(
        evs.iter().any(|k| matches!(k,
            EventKind::ApprovalResolved(a) if a.decision == harness_core::policy::Decision::Deny)),
        "{evs:#?}"
    );
    assert!(
        evs.iter().any(|k| matches!(k,
            EventKind::ToolCallFinished(f) if f.output.contains("switched this chat to plan mode"))),
        "{evs:#?}"
    );
    assert_eq!(r.read("b.txt").as_deref(), Some("one\n"));
}

/// A child whose agent no longer needs what it needed when it started
/// (review of PR 131): a reader with a shell (the read-only shell, the
/// parent's folder) whose agent gained `write` is refused, and so is a
/// `publish` agent (the parent's folder) that gained a writable `bash`;
/// the other way round, a writer that lost its writing tools is refused
/// too. Nothing runs in any of them.
#[tokio::test]
async fn a_child_whose_agent_now_needs_another_folder_is_not_continued() {
    // On a node that gives every writer a copy: a copy is not something a
    // follow-up can start from a reader (`shared_follow_ups_*` in
    // shared_folder.rs: where the folder is shared, it can).
    let mut r = rig_own().await;
    for (id, first, then) in [
        (
            "scout",
            &["read", "bash"][..],
            &["read", "bash", "write"][..],
        ),
        ("poster", &["read", "publish"], &["read", "publish", "bash"]),
        ("maker", &["read", "write"], &["read", "bash"]),
    ] {
        r.delegates = vec![Rig::spec(id, first, vec![])];
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({"agent": id, "task": "read: a.txt"})]))
            .await;
        let child = child_of(&outs[0]);
        r.delegates = vec![Rig::spec(id, then, vec![])];
        let (_, outs) = r
            .lead_turn(&delegate(&[json!({
                "agent": id, "session": child.to_string(), "task": "run: echo x > b.txt"
            })]))
            .await;
        assert!(
            outs[0].contains("cannot be given a follow-up"),
            "{id}: {outs:#?}"
        );
        assert_eq!(turns(&events_of(&r, child)).len(), 1, "{id}");
        assert!(r.read("b.txt").is_none(), "{id}");
    }
}

/// A reader with a shell is continued as it ran: in the parent's folder,
/// still a reader (on this node, which has no read-only shell, its `bash`
/// is refused with the reason, never run writable), and nothing is merged
/// back.
#[tokio::test]
async fn a_reader_with_a_shell_is_continued_as_a_reader() {
    let mut r = rig().await;
    r.delegates = vec![Rig::spec(
        "scout",
        &["read", "grep", "bash", "bash_output"],
        vec![],
    )];
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "scout", "task": "read: a.txt"}),
        ]))
        .await;
    let child = child_of(&outs[0]);
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "scout", "session": child.to_string(), "task": "run: echo x > b.txt"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), child, "{outs:#?}");
    assert!(outs[0].contains("finished: done"), "{outs:#?}");
    assert!(!outs[0].contains("applied"), "a merge-back: {outs:#?}");
    assert!(r.read("b.txt").is_none());
    assert_eq!(turns(&events_of(&r, child)).len(), 2);
    let ran = events_of(&r, child).into_iter().any(|k| {
        matches!(k, EventKind::ToolCallFinished(f) if f.output.contains("read-only shell, and this node has none"))
    });
    assert!(ran, "the follow-up's bash was not judged a reader's");
    let created = raw_of(&r, child)
        .into_iter()
        .find(|e| e.kind == "session_created")
        .unwrap();
    let cwd = std::path::PathBuf::from(created.payload["cwd"].as_str().unwrap());
    assert_eq!(cwd.canonicalize().unwrap(), r.ws.canonicalize().unwrap());
}
