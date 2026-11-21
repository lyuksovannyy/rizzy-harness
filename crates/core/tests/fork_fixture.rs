//! A forked chat's log and its fold, shared with the browser
//! (`web/src/__fixtures__/fork.json`; `web/src/fork.test.ts` folds the same
//! events with `fold.ts` and compares). Regenerate with
//! `UPDATE_FIXTURES=1 cargo test -p harness-core --test fork_fixture`.

use harness_core::SessionId;
use harness_core::event::*;
use harness_core::fork;
use harness_core::policy::ApprovalMode;
use harness_core::view::{Item, SessionView};
use serde_json::json;

struct Log {
    sid: SessionId,
    events: Vec<RawEvent>,
}

impl Log {
    fn push(&mut self, k: EventKind) -> u64 {
        let (kind, payload) = k.into_parts();
        let seq = self.events.len() as u64 + 1;
        self.events.push(RawEvent {
            session_id: self.sid,
            seq,
            ts_ms: 1_700_000_000_000 + seq as i64,
            kind,
            payload,
        });
        seq
    }
    fn turn(&mut self, id: &str, text: &str, answer: &str, input: u64, files: &[&str]) {
        self.push(EventKind::TurnStarted(TurnStarted {
            turn_id: id.into(),
            agent: None,
            cause: None,
            environment: None,
            continued_by: None,
            retry: None,
        }));
        self.push(EventKind::UserMessage(
            serde_json::from_value(json!({"turn_id": id, "text": text})).unwrap(),
        ));
        for (phase, f) in [
            (CheckpointPhase::Before, vec![]),
            (
                CheckpointPhase::After,
                files.iter().map(|s| s.to_string()).collect(),
            ),
        ] {
            self.push(EventKind::Checkpoint(Checkpoint {
                turn_id: id.into(),
                phase,
                git_ref: format!("refs/harness/s/{id}/x"),
                files: f,
                excluded: vec![],
                excluded_more: 0,
                head_moved: None,
            }));
        }
        self.push(EventKind::AssistantMessage(AssistantMessage {
            turn_id: id.into(),
            message_id: format!("m-{id}"),
            text: answer.into(),
            tool_calls: vec![],
            author: None,
            variant: None,
            blocks: vec![],
            thinking: None,
        }));
        self.push(EventKind::TurnEnded(TurnEnded {
            turn_id: id.into(),
            reason: TurnEndReason::Completed,
            usage: Some(Usage {
                input_tokens: input,
                output_tokens: input / 10,
                context_tokens: input + 5,
                ..Usage::default()
            }),
        }));
    }
}

fn events() -> Vec<RawEvent> {
    let mut p = Log {
        sid: "01a19bf2-0000-7000-8000-00000000fa01".parse().unwrap(),
        events: vec![],
    };
    p.push(EventKind::SessionCreated(SessionCreated {
        forked: None,
        title: "Parent chat".into(),
        cwd: "/w/proj".into(),
        model: "m".into(),
        backend: "b".into(),
        node_id: "n".into(),
        approval_mode: ApprovalMode::AcceptEdits,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: None,
        no_callbacks: false,
        exec_mode: None,
    }));
    p.turn("t1", "First", "One.", 100, &["a.rs"]);
    p.push(EventKind::ChatAllowAdded(ChatAllowAdded {
        grant_id: "g-parent".into(),
        approval_id: "ap1".into(),
        tool: "bash".into(),
        input: json!({"command": "ls"}),
        key: "k".into(),
    }));
    p.push(EventKind::BackgroundStarted(BackgroundStarted {
        bg_id: "bg-1".into(),
        call_id: "c1".into(),
        turn_id: "t2".into(),
        timeout_s: 60,
        ends_by_ms: 1_700_000_100_000,
        child: None,
    }));
    p.turn("t2", "Second", "Two.", 200, &["b.rs"]);
    // The parent goes on after the cut.
    p.turn("t3", "Third", "Three.", 300, &[]);
    let cut = fork::check(&p.events, "t2").unwrap();
    let new: SessionId = "01a19bf2-0000-7000-8000-00000000fa02".parse().unwrap();
    let built = fork::build(&p.events, &cut, new, "/w/proj", 1_700_000_500_000).unwrap();
    let mut f = Log {
        sid: new,
        events: built,
    };
    f.push(EventKind::ChatAllowAdded(ChatAllowAdded {
        grant_id: "g-own".into(),
        approval_id: "ap2".into(),
        tool: "bash".into(),
        input: json!({"command": "pwd"}),
        key: "k2".into(),
    }));
    f.turn("u1", "Own", "Mine.", 40, &["c.rs"]);
    let mut out = f.events;
    let n = out.len() as u64;
    out.push(RawEvent {
        session_id: new,
        seq: n + 1,
        ts_ms: 0,
        kind: "from_a_newer_node".into(),
        payload: json!({"anything": true}),
    });
    out
}

#[test]
fn fixture_is_current() {
    let evs = events();
    let expected = SessionView::fold(&evs).unwrap();
    let sent: Vec<serde_json::Value> = evs.iter().map(RawEvent::for_browser).collect();
    let doc = json!({ "events": sent, "expected": expected });
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/__fixtures__/fork.json");
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let on_disk =
        std::fs::read_to_string(&path).expect("fixture missing; run with UPDATE_FIXTURES=1");
    let on_disk: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(on_disk, doc, "fixture is stale; run with UPDATE_FIXTURES=1");
}

/// What the fixture must hold, so a regression cannot just be regenerated
/// into it.
#[test]
fn the_forks_fold_is_the_forks_own() {
    let evs = events();
    let v = SessionView::fold(&evs).unwrap();
    assert_eq!(
        v.usage.input_tokens, 40,
        "the parent's 300 are not the fork's"
    );
    assert_eq!(v.context_tokens, 45, "the meter is the last turn's");
    assert!(v.checkpoints["t1"].inherited && v.checkpoints["t2"].inherited);
    assert!(!v.checkpoints["u1"].inherited);
    assert_eq!(v.chat_allows.keys().collect::<Vec<_>>(), vec!["g-own"]);
    assert!(v.running_background().is_empty(), "bg-1 was closed");
    let divider = v
        .items
        .iter()
        .position(|i| matches!(i, Item::Forked { .. }))
        .unwrap();
    assert!(matches!(&v.items[divider - 1], Item::TurnEnd { turn_id, .. } if turn_id == "t2"));
    assert!(
        matches!(&v.items[divider + 1], Item::BackgroundEnded { bg_id, reason, .. }
            if bg_id == "bg-1" && *reason == BackgroundReason::Forked)
    );
    assert_eq!(v.unknown_events, 1);
}
