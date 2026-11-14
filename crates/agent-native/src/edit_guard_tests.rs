//! The file tools' staleness guard through the loop (ADOPTION_PLAN T.2):
//! what `read`, `write` and `edit` left the agent knowing is recorded with
//! each call's result, and a new backend (a node restart) rebuilds the guard
//! from the log alone. Without the text it was shown, a changed file is
//! refused with "read it again", never edited blind and never wedged.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::backend::{AgentBackend, TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use serde_json::{Value, json};

use crate::{NativeBackend, NativeConfig, mock};

#[derive(Default)]
struct Host(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
}

fn backend() -> NativeBackend {
    NativeBackend::new(
        Arc::new(mock::Mock {
            delay: Duration::from_millis(1),
        }),
        harness_tools::builtin(),
        NativeConfig::default(),
    )
}

async fn turn(
    b: &NativeBackend,
    sid: harness_core::SessionId,
    history: &[harness_core::RawEvent],
    dir: &std::path::Path,
    script: Value,
) -> Vec<EventKind> {
    let host = Host::default();
    b.run_turn(TurnCtx {
        session_id: sid,
        turn_id: "t".into(),
        history: history.to_vec(),
        user_text: format!("script: {script}"),
        cwd: dir.to_path_buf(),
        model: "mock".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host: &host,
    })
    .await;
    host.0.into_inner().unwrap()
}

/// `events` appended to `log` as the node would store them.
fn append(
    log: &mut Vec<harness_core::RawEvent>,
    sid: harness_core::SessionId,
    events: Vec<EventKind>,
) {
    for k in events {
        let (kind, payload) = k.into_parts();
        let seq = log.len() as u64 + 1;
        log.push(harness_core::RawEvent {
            session_id: sid,
            seq,
            ts_ms: 0,
            kind,
            payload,
        });
    }
}

fn results(ev: &[EventKind]) -> Vec<&ToolCallFinished> {
    ev.iter()
        .filter_map(|e| match e {
            EventKind::ToolCallFinished(f) => Some(f),
            _ => None,
        })
        .collect()
}

fn call(tool: &str, input: Value) -> Value {
    json!({ "tool": tool, "input": input })
}

#[tokio::test]
async fn the_guard_is_rebuilt_from_the_log_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("f.txt");
    std::fs::write(&f, "one\ntwo\n").unwrap();
    let sid = harness_core::SessionId::new();
    let mut log = Vec::new();

    let ev = turn(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([call("read", json!({"path": "f.txt"}))]),
    )
    .await;
    let r = results(&ev);
    assert!(r[0].ok, "{}", r[0].output);
    let seen = r[0].seen.as_ref().expect("the read is recorded");
    assert!(
        seen.path.ends_with("f.txt") && seen.sha256.len() == 64,
        "{seen:?}"
    );
    append(&mut log, sid, ev);

    // A new backend: what the agent read is known from the log.
    let ev = turn(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([call(
            "edit",
            json!({"path": "f.txt", "old_string": "one", "new_string": "1"})
        )]),
    )
    .await;
    let r = results(&ev);
    assert!(r[0].ok, "{}", r[0].output);
    assert!(r[0].seen.is_some());
    append(&mut log, sid, ev);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "1\ntwo\n");

    // Someone changes the file; after another restart there is no text to
    // compare with, so the edit is refused and the agent told to read.
    std::fs::write(&f, "1\ntwo\nthree\n").unwrap();
    let ev = turn(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([call(
            "edit",
            json!({"path": "f.txt", "old_string": "two", "new_string": "2"})
        )]),
    )
    .await;
    let r = results(&ev);
    assert!(!r[0].ok, "a changed file was edited blind: {}", r[0].output);
    assert!(r[0].output.contains("Read it again"), "{}", r[0].output);
    assert!(r[0].seen.is_none());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "1\ntwo\nthree\n");
    append(&mut log, sid, ev);

    // Reading again unblocks it.
    let ev = turn(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([
            call("read", json!({"path": "f.txt"})),
            call(
                "edit",
                json!({"path": "f.txt", "old_string": "two", "new_string": "2"})
            ),
        ]),
    )
    .await;
    let r = results(&ev);
    assert!(r[1].ok, "{}", r[1].output);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "1\n2\nthree\n");
}

/// A summary does not end what the agent read: the hash still says the
/// file is as it was read, so `write` and `edit` go on (review F1, F7).
#[tokio::test]
async fn a_summary_keeps_what_was_read() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "one\n").unwrap();
    let sid = harness_core::SessionId::new();
    let b = backend();
    let mut log = Vec::new();
    let ev = turn(
        &b,
        sid,
        &log,
        dir.path(),
        json!([call("read", json!({"path": "f.txt"}))]),
    )
    .await;
    append(&mut log, sid, ev);
    let through = log.last().unwrap().seq;
    append(
        &mut log,
        sid,
        vec![EventKind::HistorySummarized(HistorySummarized {
            turn_id: "t".into(),
            through_seq: through,
            text: "The agent read f.txt.".into(),
        })],
    );
    let ev = turn(
        &b,
        sid,
        &log,
        dir.path(),
        json!([
            call(
                "edit",
                json!({"path": "f.txt", "old_string": "one", "new_string": "1"})
            ),
            call("write", json!({"path": "f.txt", "content": "replaced\n"})),
        ]),
    )
    .await;
    let r = results(&ev);
    assert!(r[0].ok, "{}", r[0].output);
    assert!(r[1].ok, "{}", r[1].output);
}

/// Review F6: a deleted session's record is dropped.
#[tokio::test]
async fn a_deleted_session_is_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "one\n").unwrap();
    let sid = harness_core::SessionId::new();
    let b = backend();
    // In-turn records (the log is not given back): only memory holds them.
    let ev = turn(
        &b,
        sid,
        &[],
        dir.path(),
        json!([call("read", json!({"path": "f.txt"}))]),
    )
    .await;
    assert!(results(&ev)[0].ok);
    let f = dir.path().canonicalize().unwrap().join("f.txt");
    assert!(b.seen.session(sid).get(&f).is_some());
    b.forget_session(sid);
    assert!(b.seen.session(sid).get(&f).is_none());
}
