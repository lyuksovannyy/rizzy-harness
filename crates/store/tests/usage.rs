//! Token totals for admins: what is counted once, the period's bounds, the
//! time limit. Every fixture chat is also folded by `SessionView` (the
//! chat header's fold) and the two must agree.

use std::time::Duration;

use harness_core::event::*;
use harness_core::view::SessionView;
use harness_core::{RawEvent, SessionId};
use harness_store::usage::UsageTotals;
use harness_store::{NewUser, Role, Store};
use serde_json::{Value, json};

const DAY: i64 = 86_400_000;
const LONG: Duration = Duration::from_secs(60);

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    for (id, name) in [("u1", "alex"), ("u2", "bob")] {
        s.create_user(&NewUser {
            user_id: id.into(),
            username: name.into(),
            password_hash: "x".into(),
            role: Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::None,
            must_change_password: false,
        })
        .unwrap();
    }
    s
}

/// One chat's log, built event by event.
struct Chat {
    store: Store,
    sid: SessionId,
    seq: u64,
    log: Vec<RawEvent>,
}

impl Chat {
    fn new(store: &Store, owner: &str, parent: Option<SessionId>, ts: i64) -> Chat {
        let sid = SessionId::new();
        let mut c = Chat {
            store: store.clone(),
            sid,
            seq: 0,
            log: vec![],
        };
        let (kind, payload) = EventKind::SessionCreated(SessionCreated {
            forked: None,
            exec_mode: None,
            no_callbacks: false,
            profile: Profile::Agentic,
            agent_id: None,
            incognito: false,
            parent_session: parent,
            title: "SECRET TITLE".into(),
            cwd: "/secret/folder".into(),
            model: "m".into(),
            backend: "native".into(),
            node_id: "n1".into(),
            approval_mode: Default::default(),
        })
        .into_parts();
        c.push(ts, &kind, payload);
        store.claim_session(sid, Some(owner), None).unwrap();
        c
    }

    fn push(&mut self, ts: i64, kind: &str, payload: Value) {
        self.seq += 1;
        let e = RawEvent {
            session_id: self.sid,
            seq: self.seq,
            ts_ms: ts,
            kind: kind.into(),
            payload,
        };
        self.store.append("n1", &e).unwrap();
        self.log.push(e);
    }

    fn start(&mut self, ts: i64, turn: &str) {
        self.push(ts, "turn_started", json!({"turn_id": turn}));
    }
    fn running(&mut self, ts: i64, turn: &str, u: Value) {
        self.push(ts, "turn_usage", json!({"turn_id": turn, "usage": u}));
    }
    fn end(&mut self, ts: i64, turn: &str, reason: &str, u: Option<Value>) {
        let mut p = json!({"turn_id": turn, "reason": reason});
        if let Some(u) = u {
            p["usage"] = u;
        }
        self.push(ts, "turn_ended", p);
    }

    /// What the header's fold says this chat used.
    fn fold(&self) -> Usage {
        let mut v = SessionView::default();
        for e in &self.log {
            v.apply(e).unwrap();
        }
        v.usage
    }
}

/// A usage record: `input` and `output` in all, split over `models`
/// (name, input, output).
fn usage(input: u64, output: u64, models: &[(&str, u64, u64)]) -> Value {
    let by: serde_json::Map<String, Value> = models
        .iter()
        .map(|(m, i, o)| {
            (
                m.to_string(),
                json!({"input_tokens": i, "output_tokens": o}),
            )
        })
        .collect();
    json!({"input_tokens": input, "output_tokens": output, "context_tokens": 1, "by_model": by})
}

fn person<'a>(t: &'a UsageTotals, id: &str) -> &'a harness_store::usage::PersonUsage {
    t.people
        .iter()
        .find(|p| p.user_id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no {id} in {t:?}"))
}

fn model(p: &harness_store::usage::PersonUsage, m: &str) -> (u64, u64, u64) {
    let u = &p.bucket.models[m];
    (u.tally.input_tokens, u.tally.output_tokens, u.turns)
}

/// The fixture: alex has a plain turn on model a, a turn on model b whose
/// running figures the end replaces, a compaction, a sub-agent (its own chat
/// and its cost inside the parent's turn), and an interrupted turn; bob one
/// turn on model a.
fn fixture(s: &Store) -> Vec<Chat> {
    let t0 = 10 * DAY;
    let mut alex = Chat::new(s, "u1", None, t0);
    alex.start(t0 + 1, "t1");
    alex.running(t0 + 2, "t1", usage(100, 10, &[("a", 100, 10)]));
    alex.end(
        t0 + 3,
        "t1",
        "completed",
        Some(usage(100, 10, &[("a", 100, 10)])),
    );
    // The end's figure replaces the two running ones: counted once.
    alex.start(t0 + 4, "t2");
    alex.running(t0 + 5, "t2", usage(40, 4, &[("b", 40, 4)]));
    alex.running(t0 + 6, "t2", usage(90, 9, &[("b", 90, 9)]));
    // The sub-agent: its cost is in the parent's turn (`delegate` returned).
    let mut child = Chat::new(s, "u1", Some(alex.sid), t0 + 6);
    child.start(t0 + 6, "c1");
    child.end(
        t0 + 7,
        "c1",
        "completed",
        Some(usage(30, 3, &[("b", 30, 3)])),
    );
    alex.end(
        t0 + 8,
        "t2",
        "completed",
        Some(usage(120, 12, &[("b", 120, 12)])),
    );
    // A person's /compact: tokens, not a turn.
    alex.push(
        t0 + 9,
        "history_summarized",
        json!({"turn_id": "t2", "through_seq": 5, "text": "s", "manual": true,
               "usage": usage(7, 1, &[("a", 7, 1)])}),
    );
    // An interrupted turn: no usage at the end, its last running figure stays.
    alex.start(t0 + 10, "t3");
    alex.running(t0 + 11, "t3", usage(50, 5, &[("a", 50, 5)]));
    alex.end(t0 + 12, "t3", "interrupted", None);
    let mut bob = Chat::new(s, "u2", None, t0);
    bob.start(t0 + 1, "t1");
    bob.end(
        t0 + 2,
        "t1",
        "completed",
        Some(usage(11, 2, &[("a", 11, 2)])),
    );
    vec![alex, child, bob]
}

#[test]
fn every_token_is_counted_once_and_agrees_with_the_header() {
    let s = store();
    let chats = fixture(&s);
    let t = s.usage_totals(0, 20 * DAY, LONG).unwrap();
    assert_eq!(t.covered_percent, None);
    let alex = person(&t, "u1");
    // a: t1 (100/10) + compaction (7/1) + interrupted t3 (50/5); turns: t1, t3.
    assert_eq!(model(alex, "a"), (157, 16, 2));
    // b: t2's end (120/12), which includes the sub-agent; the running
    // figures (40, 90) and the child's own chat are not added.
    assert_eq!(model(alex, "b"), (120, 12, 1));
    assert_eq!(alex.bucket.turns, 3);
    assert_eq!(alex.username.as_deref(), Some("alex"));
    assert_eq!(model(person(&t, "u2"), "a"), (11, 2, 1));
    // The header's fold over the same log says the same, chat by chat.
    let head = chats[0].fold();
    assert_eq!((head.input_tokens, head.output_tokens), (277, 28));
    assert_eq!(alex.bucket.tally.input_tokens, head.input_tokens);
    assert_eq!(alex.bucket.tally.output_tokens, head.output_tokens);
    for (m, tk) in &head.by_model {
        assert_eq!(
            alex.bucket.models[m].tally.input_tokens, tk.input_tokens,
            "{m}"
        );
        assert_eq!(
            alex.bucket.models[m].tally.output_tokens, tk.output_tokens,
            "{m}"
        );
    }
    let bobs = chats[2].fold();
    assert_eq!(
        person(&t, "u2").bucket.tally.input_tokens,
        bobs.input_tokens
    );
}

#[test]
fn a_turn_that_never_ended_counts_its_running_figure_once() {
    let s = store();
    let mut c = Chat::new(&s, "u1", None, DAY);
    c.start(DAY + 1, "t1");
    c.running(DAY + 2, "t1", usage(10, 1, &[("a", 10, 1)]));
    c.running(DAY + 3, "t1", usage(25, 2, &[("a", 25, 2)]));
    // The next turn settles it (the node died); then a running one.
    c.start(DAY + 4, "t2");
    c.running(DAY + 5, "t2", usage(5, 1, &[("a", 5, 1)]));
    let t = s.usage_totals(0, 10 * DAY, LONG).unwrap();
    assert_eq!(model(person(&t, "u1"), "a"), (30, 3, 2));
    assert_eq!(c.fold().input_tokens, 30);
}

#[test]
fn the_period_includes_its_start_and_excludes_its_end() {
    let s = store();
    let mut c = Chat::new(&s, "u1", None, 100);
    for (ts, turn) in [(1000, "a"), (2000, "b"), (3000, "c")] {
        c.start(ts - 1, turn);
        c.end(ts, turn, "completed", Some(usage(10, 1, &[("m", 10, 1)])));
    }
    let n = |from, to| {
        let t = s.usage_totals(from, to, LONG).unwrap();
        t.people.first().map_or(0, |p| p.bucket.turns)
    };
    assert_eq!(n(1000, 3000), 2, "1000 in, 3000 out");
    assert_eq!(n(1001, 3001), 2);
    assert_eq!(n(0, 1000), 0);
    assert_eq!(n(3000, 3001), 1);
    // A running figure at the end of the period is left to the period that
    // holds the end.
    let mut d = Chat::new(&s, "u2", None, 100);
    d.start(4999, "x");
    d.running(5000, "x", usage(8, 1, &[("m", 8, 1)]));
    d.end(7000, "x", "completed", Some(usage(9, 1, &[("m", 9, 1)])));
    let t = s.usage_totals(4000, 6000, LONG).unwrap();
    assert!(
        t.people.iter().all(|p| p.user_id.as_deref() != Some("u2")),
        "{t:?}"
    );
    let t = s.usage_totals(6000, 8000, LONG).unwrap();
    assert_eq!(model(person(&t, "u2"), "m"), (9, 1, 1));
}

#[test]
fn a_deleted_chat_leaves_the_totals() {
    let s = store();
    let chats = fixture(&s);
    s.delete_session(chats[2].sid, false).unwrap();
    let t = s.usage_totals(0, 20 * DAY, LONG).unwrap();
    assert!(
        t.people.iter().all(|p| p.user_id.as_deref() != Some("u2")),
        "{t:?}"
    );
    assert!(t.people.iter().any(|p| p.user_id.as_deref() == Some("u1")));
}

#[test]
fn an_orphaned_sub_agent_is_counted_as_nothing_else_counts_it() {
    let s = store();
    let mut p = Chat::new(&s, "u1", None, DAY);
    let mut c = Chat::new(&s, "u1", Some(p.sid), DAY);
    c.start(DAY + 1, "c1");
    c.end(
        DAY + 2,
        "c1",
        "completed",
        Some(usage(30, 3, &[("b", 30, 3)])),
    );
    p.start(DAY + 1, "t1");
    p.end(
        DAY + 3,
        "t1",
        "completed",
        Some(usage(50, 5, &[("b", 50, 5)])),
    );
    let t = s.usage_totals(0, 10 * DAY, LONG).unwrap();
    assert_eq!(model(person(&t, "u1"), "b"), (50, 5, 1));
    s.delete_session(p.sid, false).unwrap();
    let t = s.usage_totals(0, 10 * DAY, LONG).unwrap();
    assert_eq!(model(person(&t, "u1"), "b"), (30, 3, 1));
}

#[test]
fn a_background_sub_agent_and_a_hand_off_count_as_tokens() {
    let s = store();
    let mut c = Chat::new(&s, "u1", None, DAY);
    c.push(
        DAY + 1,
        "background_ended",
        json!({"bg_id": "b1", "reason": "exited", "confirmed": "ended", "ran_ms": 1,
               "output_bytes": 0,
               "child": {"session": SessionId::new().to_string(), "agent_id": "x",
                         "usage": usage(20, 2, &[("c", 20, 2)])}}),
    );
    c.push(
        DAY + 2,
        "handoff_written",
        json!({"through_seq": 1, "usage": usage(3, 1, &[("c", 3, 1)])}),
    );
    c.push(
        DAY + 3,
        "compact_failed",
        json!({"reason": "r", "usage": usage(4, 1, &[("c", 4, 1)])}),
    );
    let t = s.usage_totals(0, 10 * DAY, LONG).unwrap();
    assert_eq!(model(person(&t, "u1"), "c"), (27, 4, 0));
    assert_eq!(person(&t, "u1").bucket.turns, 0);
    assert_eq!(c.fold().input_tokens, 27);
}

#[test]
fn tokens_in_no_model_stay_in_the_sums_and_workspaces_are_named() {
    let s = store();
    let w = s.create_workspace("Team Red", "u1").unwrap();
    s.add_member(&w.workspace_id, "u2", "member").unwrap();
    let mut a = Chat::new(&s, "u1", None, DAY);
    s.claim_session(a.sid, Some("u1"), Some(&w.workspace_id))
        .unwrap();
    a.start(DAY + 1, "t");
    a.end(
        DAY + 2,
        "t",
        "completed",
        Some(usage(10, 1, &[("m", 6, 1)])),
    );
    let mut b = Chat::new(&s, "u2", None, DAY);
    b.start(DAY + 1, "t");
    b.end(DAY + 2, "t", "completed", Some(usage(5, 1, &[("m", 5, 1)])));
    let t = s.usage_totals(0, 10 * DAY, LONG).unwrap();
    let alex = person(&t, "u1");
    assert_eq!(alex.bucket.tally.input_tokens, 10);
    assert_eq!(
        alex.bucket.models["m"].tally.input_tokens, 6,
        "4 unattributed"
    );
    assert_eq!(t.workspaces.len(), 2);
    let red = &t.workspaces[0];
    assert_eq!(red.name.as_deref(), Some("Team Red"));
    assert_eq!(red.bucket.tally.input_tokens, 10);
    // Personal chats: one unnamed row for everyone's.
    assert_eq!(t.workspaces[1].name, None);
    assert_eq!(t.workspaces[1].workspace_id, None);
    assert_eq!(t.workspaces[1].bucket.tally.input_tokens, 5);
}

#[test]
fn a_spent_time_limit_says_how_much_was_read() {
    let s = store();
    fixture(&s);
    let t = s.usage_totals(0, 20 * DAY, Duration::ZERO).unwrap();
    assert_eq!(t.covered_percent, Some(0));
    assert!(t.people.is_empty());
}
