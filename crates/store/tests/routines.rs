//! Routines storage: routines and their triggers scoped by owner and
//! workspace, the one-claim-per-occurrence rule, runs and how they fold,
//! marks, attention, purges and what a deleted chat leaves behind.

use harness_core::{RawEvent, SessionId};
use harness_store::routines::{
    Claimed, NewRoutine, NewRun, NewTrigger, OVERLAP, RoutineCaps, RoutinePatch, RunPatch,
};
use harness_store::{NewUser, Role, Store, StoreError};
use serde_json::{Value, json};

const CAPS: RoutineCaps = RoutineCaps {
    per_user: 25,
    per_workspace: 100,
};

fn user(s: &Store, id: &str) {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        can_agentic: false,
        must_change_password: false,
    })
    .unwrap();
}

/// A store with accounts "u1" and "u2" and a team workspace "u1" owns.
fn store() -> (Store, String) {
    let s = Store::open_in_memory().unwrap();
    user(&s, "u1");
    user(&s, "u2");
    let w = s.create_workspace("Team", "u1").unwrap().workspace_id;
    (s, w)
}

fn trigger(id: &str, next: Option<i64>) -> NewTrigger {
    NewTrigger {
        trigger_id: id.into(),
        kind: "schedule".into(),
        config: r#"{"type":"interval","every_minutes":5,"anchor_ms":0}"#.into(),
        next_run_ms: next,
        secret_hash: None,
    }
}

fn routine(id: &str, owner: &str, ws: &str, triggers: Vec<NewTrigger>) -> NewRoutine {
    NewRoutine {
        routine_id: id.into(),
        workspace_id: ws.into(),
        owner_id: owner.into(),
        name: format!("Routine {id}"),
        enabled: true,
        spec: r#"{"notify":"always"}"#.into(),
        triggers,
    }
}

fn run(id: &str, routine: &str, status: &str, at: i64) -> NewRun {
    NewRun {
        run_id: id.into(),
        routine_id: routine.into(),
        trigger_id: None,
        trigger_kind: "manual".into(),
        scheduled_ms: None,
        started_ms: at,
        status: status.into(),
        reason: None,
        repeats: 1,
    }
}

fn ev(sid: SessionId, seq: u64, kind: &str, payload: Value) -> RawEvent {
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind: kind.into(),
        payload,
    }
}

/// A mirrored chat, optionally a sub-agent of `parent`.
fn chat(s: &Store, parent: Option<SessionId>) -> SessionId {
    let sid = SessionId::new();
    let mut p = json!({"title": "t", "cwd": "", "model": "m", "backend": "native",
                       "node_id": "n", "profile": "chat", "approval_mode": "ask"});
    if let Some(parent) = parent {
        p["parent_session"] = json!(parent.to_string());
    }
    s.append("n", &ev(sid, 1, "session_created", p)).unwrap();
    sid
}

/// Start run `id` and bind it to chat `sid`, then finish it as `status`.
fn finished_run(s: &Store, id: &str, routine: &str, at: i64, sid: Option<SessionId>, status: &str) {
    assert_eq!(
        s.run_start(&run(id, routine, "pending", at)).unwrap(),
        Claimed::Started
    );
    let patch = RunPatch {
        status: Some("running".into()),
        session_id: sid.map(|s| s.to_string()),
        prompt: Some("hello".into()),
        warnings: Some("[]".into()),
        ..Default::default()
    };
    s.run_update(id, &patch).unwrap();
    assert!(s.run_finish(id, status, None, at + 10).unwrap());
}

#[test]
fn crud_is_scoped_by_owner_and_workspace() {
    let (s, w) = store();
    let p1 = s.personal_workspace("u1").unwrap();
    s.routine_create(
        &routine("a", "u1", &w, vec![trigger("t1", Some(100))]),
        CAPS,
        1,
    )
    .unwrap();
    s.routine_create(&routine("b", "u1", &p1, vec![]), CAPS, 2)
        .unwrap();
    s.add_member(&w, "u2", "member").unwrap();
    s.routine_create(&routine("c", "u2", &w, vec![]), CAPS, 3)
        .unwrap();
    let ids = |owner: Option<&str>, ws: Option<&str>| -> Vec<String> {
        s.routines_for(owner, ws)
            .unwrap()
            .into_iter()
            .map(|r| r.routine_id)
            .collect()
    };
    assert_eq!(ids(Some("u1"), Some(&w)), ["a"]);
    assert_eq!(ids(Some("u1"), None), ["a", "b"]);
    assert_eq!(ids(None, Some(&w)), ["a", "c"]);
    assert_eq!(ids(Some("u2"), Some(&p1)), Vec::<String>::new());

    // The same name twice for one owner in one workspace is refused.
    let mut dup = routine("d", "u1", &w, vec![]);
    dup.name = "routine A".into();
    assert!(matches!(
        s.routine_create(&dup, CAPS, 4),
        Err(StoreError::Conflict(_))
    ));
    // Another owner may use it.
    dup.owner_id = "u2".into();
    s.routine_create(&dup, CAPS, 4).unwrap();

    let r = s.routine("a").unwrap().unwrap();
    assert_eq!((r.created_ms, r.updated_ms, r.enabled), (1, 1, true));
    assert_eq!(s.triggers("a").unwrap()[0].next_run_ms, Some(100));
    assert!(s.routine_delete("a").unwrap());
    assert!(s.routine("a").unwrap().is_none());
    assert!(s.trigger("t1").unwrap().is_none(), "triggers go with it");
    assert!(!s.routine_delete("a").unwrap());
}

#[test]
fn caps_refuse_at_save() {
    let (s, w) = store();
    let caps = RoutineCaps {
        per_user: 2,
        per_workspace: 3,
    };
    s.routine_create(&routine("a", "u1", &w, vec![]), caps, 1)
        .unwrap();
    s.routine_create(&routine("b", "u1", &w, vec![]), caps, 1)
        .unwrap();
    assert!(matches!(
        s.routine_create(&routine("c", "u1", &w, vec![]), caps, 1),
        Err(StoreError::Refused(_))
    ));
    s.add_member(&w, "u2", "member").unwrap();
    s.routine_create(&routine("c", "u2", &w, vec![]), caps, 1)
        .unwrap();
    let e = s
        .routine_create(&routine("d", "u2", &w, vec![]), caps, 1)
        .unwrap_err();
    assert!(e.to_string().contains("workspace"), "{e}");
}

#[test]
fn updates_keep_trigger_ids_and_secrets() {
    let (s, w) = store();
    let mut hook = trigger("keep", Some(10));
    hook.secret_hash = Some("abc".into());
    s.routine_create(
        &routine("a", "u1", &w, vec![hook, trigger("drop", Some(20))]),
        CAPS,
        1,
    )
    .unwrap();
    let patch = RoutinePatch {
        name: Some("Renamed".into()),
        triggers: Some(vec![trigger("keep", Some(30)), trigger("new", Some(40))]),
        ..Default::default()
    };
    let r = s.routine_update("a", &patch, 5).unwrap().unwrap();
    assert_eq!((r.name.as_str(), r.updated_ms), ("Renamed", 5));
    let t: Vec<(String, Option<i64>, Option<String>)> = s
        .triggers("a")
        .unwrap()
        .into_iter()
        .map(|t| (t.trigger_id, t.next_run_ms, t.secret_hash))
        .collect();
    assert_eq!(
        t,
        [
            ("keep".into(), Some(30), Some("abc".into())),
            ("new".into(), Some(40), None)
        ]
    );
    // Pausing, failures and resuming.
    assert!(s.routine_pause("a", "failures", 6).unwrap());
    assert!(
        !s.routine_pause("a", "blocked", 7).unwrap(),
        "already paused"
    );
    assert_eq!(s.routine_count_failure("a", true).unwrap(), 1);
    let r = s.routine("a").unwrap().unwrap();
    assert_eq!(r.paused_reason.as_deref(), Some("failures"));
    let resume = RoutinePatch {
        enabled: Some(true),
        resume: true,
        ..Default::default()
    };
    let r = s.routine_update("a", &resume, 8).unwrap().unwrap();
    assert_eq!((r.paused_reason, r.consecutive_failures), (None, 0));
    assert!(s.routine_update("nope", &resume, 8).unwrap().is_none());
}

#[test]
fn due_triggers_skip_disabled_and_paused_routines() {
    let (s, w) = store();
    s.routine_create(
        &routine("a", "u1", &w, vec![trigger("ta", Some(100))]),
        CAPS,
        1,
    )
    .unwrap();
    let mut off = routine("b", "u1", &w, vec![trigger("tb", Some(50))]);
    off.enabled = false;
    s.routine_create(&off, CAPS, 1).unwrap();
    s.routine_create(
        &routine("c", "u1", &w, vec![trigger("tc", Some(60))]),
        CAPS,
        1,
    )
    .unwrap();
    s.routine_pause("c", "blocked", 2).unwrap();
    s.routine_create(&routine("d", "u1", &w, vec![trigger("td", None)]), CAPS, 1)
        .unwrap();
    assert!(s.routine_triggers_due(99, 10).unwrap().is_empty());
    let due = s.routine_triggers_due(100, 10).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(
        (due[0].0.trigger_id.as_str(), due[0].1.routine_id.as_str()),
        ("ta", "a")
    );
    assert_eq!(s.routine_next_due().unwrap(), Some(100));
}

#[test]
fn an_occurrence_is_claimed_once() {
    let (s, w) = store();
    s.routine_create(
        &routine("a", "u1", &w, vec![trigger("t", Some(100))]),
        CAPS,
        1,
    )
    .unwrap();
    let first = NewRun {
        trigger_id: Some("t".into()),
        trigger_kind: "schedule".into(),
        scheduled_ms: Some(100),
        ..run("r1", "a", "pending", 101)
    };
    let second = NewRun {
        run_id: "r2".into(),
        ..first.clone()
    };
    assert_eq!(
        s.claim_occurrence("t", 100, Some(400), &first).unwrap(),
        Some(Claimed::Started)
    );
    // The same occurrence again: the trigger moved on, nothing is recorded.
    assert_eq!(
        s.claim_occurrence("t", 100, Some(400), &second).unwrap(),
        None
    );
    assert!(s.run("r2").unwrap().is_none());
    let t = s.trigger("t").unwrap().unwrap();
    assert_eq!((t.next_run_ms, t.last_fired_ms), (Some(400), Some(101)));
    assert_eq!(s.active_runs().unwrap().len(), 1);

    // The next occurrence while r1 still runs: skipped, and skips fold.
    for (i, id) in ["r3", "r4"].into_iter().enumerate() {
        let next = 400 + i as i64 * 300;
        let fire = NewRun {
            run_id: id.into(),
            started_ms: next,
            ..first.clone()
        };
        assert_eq!(
            s.claim_occurrence("t", next, Some(next + 300), &fire)
                .unwrap(),
            Some(Claimed::Overlap)
        );
    }
    assert!(s.run("r4").unwrap().is_none(), "folded into r3");
    let r3 = s.run("r3").unwrap().unwrap();
    assert_eq!(
        (
            r3.status.as_str(),
            r3.reason.as_deref(),
            r3.repeats,
            r3.ended_ms
        ),
        ("skipped", Some(OVERLAP), 2, Some(700))
    );
    assert_eq!(
        s.trigger("t").unwrap().unwrap().last_fired_ms,
        Some(101),
        "a skip is no fire"
    );
}

#[test]
fn runs_finish_once_and_missed_runs_fold() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    let missed = |id: &str, at: i64, n: i64| NewRun {
        reason: Some("down".into()),
        repeats: n,
        ..run(id, "a", "missed", at)
    };
    assert_eq!(
        s.run_start(&missed("m1", 10, 3)).unwrap(),
        Claimed::Recorded
    );
    s.run_start(&missed("m2", 20, 2)).unwrap();
    let m1 = s.run("m1").unwrap().unwrap();
    assert_eq!((m1.repeats, m1.ended_ms), (5, Some(20)));
    assert!(s.run("m2").unwrap().is_none());

    s.run_start(&run("r1", "a", "pending", 30)).unwrap();
    assert!(s.run_finish("r1", "completed", None, 40).unwrap());
    // A late event never rewrites a result.
    assert!(!s.run_finish("r1", "error", Some("late"), 50).unwrap());
    let late = RunPatch {
        status: Some("running".into()),
        only_active: true,
        ..Default::default()
    };
    assert!(!s.run_update("r1", &late).unwrap());
    let r1 = s.run("r1").unwrap().unwrap();
    assert_eq!((r1.status.as_str(), r1.ended_ms), ("completed", Some(40)));

    // A skipped finish folds into an earlier skip with the same reason.
    s.run_start(&run("s1", "a", "pending", 60)).unwrap();
    s.run_finish("s1", "skipped", Some("cap"), 61).unwrap();
    s.run_start(&run("s2", "a", "pending", 70)).unwrap();
    s.run_finish("s2", "skipped", Some("cap"), 71).unwrap();
    assert!(s.run("s2").unwrap().is_none());
    assert_eq!(s.run("s1").unwrap().unwrap().repeats, 2);

    let page = s.runs_page("a", None, 2).unwrap();
    let ids: Vec<&str> = page.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(ids, ["s1", "r1"]);
    let older = s.runs_page("a", Some("r1"), 10).unwrap();
    assert_eq!(older.len(), 1);
    assert_eq!(older[0].run_id, "m1");
    assert_eq!(s.run_count("a").unwrap(), 3);
}

#[test]
fn caps_count_the_owners_runs() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.routine_create(&routine("b", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.run_start(&run("r1", "a", "pending", 100)).unwrap();
    s.run_start(&run("r2", "b", "pending", 200)).unwrap();
    s.run_update(
        "r2",
        &RunPatch {
            status: Some("waiting".into()),
            ..Default::default()
        },
    )
    .unwrap();
    s.run_start(&NewRun {
        reason: Some("x".into()),
        ..run("m", "a", "missed", 150)
    })
    .unwrap();
    // Today: both started (the missed one never ran); running: only r1 (a
    // waiting run holds no place).
    assert_eq!(s.owner_run_counts("u1", 0).unwrap(), (2, 1));
    assert_eq!(s.owner_run_counts("u1", 150).unwrap(), (1, 1));
    assert_eq!(s.owner_run_counts("u2", 0).unwrap(), (0, 0));
}

#[test]
fn deleting_a_chat_clears_its_text_from_the_run() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    let sid = chat(&s, None);
    finished_run(&s, "r1", "a", 10, Some(sid), "completed");
    s.run_update(
        "r1",
        &RunPatch {
            usage: Some(r#"{"input_tokens":5,"output_tokens":2}"#.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s.run_by_session(sid).unwrap().unwrap().run_id, "r1");
    s.delete_session(sid, true).unwrap();
    let r = s.run("r1").unwrap().unwrap();
    assert_eq!(r.session_id, None);
    assert!(r.chat_deleted);
    assert_eq!((r.prompt, r.event, r.warnings), (None, None, None));
    assert_eq!(r.status, "completed");
    assert!(r.usage.is_some(), "usage stays");
    assert!(s.run_by_session(sid).unwrap().is_none());
}

#[test]
fn marks_promotion_and_child_sessions() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    let sid = chat(&s, None);
    let other = chat(&s, None);
    s.run_start(&run("r1", "a", "pending", 10)).unwrap();
    s.run_update(
        "r1",
        &RunPatch {
            status: Some("running".into()),
            session_id: Some(sid.to_string()),
            untrusted: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    let marks = s.routine_marks(&[sid, other]).unwrap();
    assert_eq!(marks.len(), 1);
    let m = &marks[&sid];
    assert_eq!(
        (
            m.routine_id.as_str(),
            m.name.as_str(),
            m.run_id.as_str(),
            m.status.as_str()
        ),
        ("a", "Routine a", "r1", "running")
    );
    assert!(!m.seen && !m.promoted && m.untrusted);

    // A delegate's session leads back to the run.
    let kid = chat(&s, Some(sid));
    assert_eq!(s.run_by_child_session(kid).unwrap().unwrap().run_id, "r1");
    assert!(s.run_by_child_session(other).unwrap().is_none());
    s.append(
        "n",
        &ev(kid, 2, "approval_requested", json!({"approval_id": "x"})),
    )
    .unwrap();
    assert_eq!(s.pending_approvals_with_children(sid).unwrap(), 1);
    assert_eq!(s.pending_approvals_with_children(other).unwrap(), 0);

    // Prompting in the chat marks the run continued and says untrusted.
    assert_eq!(s.run_promote(sid).unwrap(), Some(true));
    assert_eq!(s.run_promote(other).unwrap(), None);
    assert!(s.routine_marks(&[sid]).unwrap()[&sid].promoted);
    assert!(s.first_event(sid, "session_created").unwrap().is_some());
    assert!(s.first_event(sid, "turn_ended").unwrap().is_none());
}

#[test]
fn seen_only_marks_finished_runs() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.run_start(&run("r1", "a", "pending", 10)).unwrap();
    assert!(!s.mark_run_seen("a", "r1").unwrap(), "still going");
    s.run_finish("r1", "completed", None, 20).unwrap();
    assert!(
        !s.mark_run_seen("b", "r1").unwrap(),
        "another routine's run"
    );
    assert!(s.mark_run_seen("a", "r1").unwrap());
    assert!(!s.mark_run_seen("a", "r1").unwrap(), "once");
}

#[test]
fn attention_rows_per_routine_and_pause() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.routine_create(&routine("b", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.routine_create(&routine("c", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    s.add_member(&w, "u2", "member").unwrap();
    s.routine_create(&routine("z", "u2", &w, vec![]), CAPS, 1)
        .unwrap();
    finished_run(&s, "a1", "a", 10, None, "completed");
    finished_run(&s, "a2", "a", 20, None, "error");
    finished_run(&s, "b1", "b", 30, None, "completed");
    s.mark_run_seen("b", "b1").unwrap();
    s.run_start(&NewRun {
        reason: Some("down".into()),
        ..run("c1", "c", "missed", 40)
    })
    .unwrap();
    finished_run(&s, "z1", "z", 50, None, "completed");
    s.routine_pause("b", "failures", 60).unwrap();

    let rows = s.routine_attention("u1").unwrap();
    assert_eq!(rows.len(), 2, "{rows:#?}");
    let a = rows.iter().find(|r| r.routine_id == "a").unwrap();
    assert_eq!(
        (a.run_id.as_deref(), a.status.as_deref(), a.unseen, a.ts_ms),
        (Some("a2"), Some("error"), 2, 30)
    );
    assert_eq!(a.paused_reason, None);
    let b = rows.iter().find(|r| r.routine_id == "b").unwrap();
    assert_eq!(
        (b.paused_reason.as_deref(), b.run_id.as_deref(), b.ts_ms),
        (Some("failures"), Some("b1"), 60)
    );
    // Missed runs never appear; another owner's never either.
    assert!(
        rows.iter()
            .all(|r| r.routine_id != "c" && r.routine_id != "z")
    );
    let unseen = s.routine_unseen(Some("u1")).unwrap();
    assert_eq!(unseen.get("a"), Some(&2));
    assert_eq!(unseen.get("b"), None);
}

#[test]
fn purge_keeps_the_newest_and_every_run_with_a_chat() {
    let (s, w) = store();
    s.routine_create(&routine("a", "u1", &w, vec![]), CAPS, 1)
        .unwrap();
    let sid = chat(&s, None);
    finished_run(&s, "old-chat", "a", 1, Some(sid), "completed");
    for i in 0..5 {
        finished_run(&s, &format!("r{i}"), "a", 100 + i, None, "completed");
    }
    s.run_start(&run("live", "a", "pending", 2)).unwrap();
    // Keep 2 per routine: r0..r2 go; the run with a chat and the active
    // one stay.
    assert_eq!(s.runs_purge(2, 0).unwrap(), 3);
    let left: Vec<String> = s
        .runs_page("a", None, 10)
        .unwrap()
        .into_iter()
        .map(|r| r.run_id)
        .collect();
    assert_eq!(left, ["r4", "r3", "live", "old-chat"]);
    // By age: everything finished and chat-less before 104 goes.
    assert_eq!(s.runs_purge(100, 104).unwrap(), 1);
    assert!(s.run("r3").unwrap().is_none());
}

#[test]
fn cascades_with_the_workspace_and_the_account() {
    let (s, w) = store();
    s.add_member(&w, "u2", "member").unwrap();
    s.routine_create(
        &routine("a", "u2", &w, vec![trigger("t", Some(1))]),
        CAPS,
        1,
    )
    .unwrap();
    s.run_start(&run("r", "a", "pending", 1)).unwrap();
    let p2 = s.personal_workspace("u2").unwrap();
    s.routine_create(&routine("b", "u2", &p2, vec![]), CAPS, 1)
        .unwrap();
    s.delete_workspace(&w).unwrap();
    assert!(s.routine("a").unwrap().is_none());
    assert!(s.run("r").unwrap().is_none());
    assert!(s.trigger("t").unwrap().is_none());
    s.delete_user("u2").unwrap();
    assert!(s.routine("b").unwrap().is_none());
}
