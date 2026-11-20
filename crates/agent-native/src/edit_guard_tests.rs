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
            ..Default::default()
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

/// A host where a `read` runs unasked (an "accept all" chat): refusals
/// before the card may print rows.
#[derive(Default)]
struct OpenHost(Mutex<Vec<EventKind>>);

#[async_trait::async_trait]
impl TurnHost for OpenHost {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.0.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    fn runs_unasked(&self, _: &str, _: ToolRisk) -> bool {
        true
    }
}

async fn turn_open(
    b: &NativeBackend,
    sid: harness_core::SessionId,
    history: &[harness_core::RawEvent],
    dir: &std::path::Path,
    script: Value,
) -> Vec<EventKind> {
    let host = OpenHost::default();
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

/// The line form of `edit` through the loop: a `read` in one turn gives the
/// tag, a line-form edit in the next works from the same backend's memory;
/// after a restart (the table rebuilt from the log alone, no snapshot) the
/// call is refused with the rows, and the resend with the printed tag works.
#[tokio::test]
async fn a_line_form_edit_works_across_turns_and_after_a_restart_is_refused_with_rows() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("f.txt");
    std::fs::write(&f, "one\ntwo\nthree\nfour\n").unwrap();
    let tag = |t: &str| {
        let h = harness_tools::seen::sha256(t.as_bytes());
        harness_tools::seen::tag(&h)
    };
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
    let r = results(&ev);
    assert!(
        r[0].output.starts_with('[')
            && r[0]
                .output
                .contains(&format!("#{}]", tag("one\ntwo\nthree\nfour\n")))
    );
    append(&mut log, sid, ev);

    // Next turn, same backend: the snapshot is there.
    let ev = turn(
        &b,
        sid,
        &log,
        dir.path(),
        json!([call(
            "edit",
            json!({"path": "f.txt", "tag": tag("one\ntwo\nthree\nfour\n"),
                   "lines": [{"from": 2, "to": 2, "text": "TWO"}]})
        )]),
    )
    .await;
    let r = results(&ev);
    assert!(r[0].ok, "{}", r[0].output);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "one\nTWO\nthree\nfour\n"
    );
    append(&mut log, sid, ev);

    // A new backend: the log gives the hash, not the text. The tag is
    // unknown, the rows are printed, and the resend works.
    let new_tag = tag("one\nTWO\nthree\nfour\n");
    let ev = turn_open(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([call(
            "edit",
            json!({"path": "f.txt", "tag": new_tag,
                   "lines": [{"from": 3, "to": 3, "text": "THREE"}]})
        )]),
    )
    .await;
    let r = results(&ev);
    assert!(
        !r[0].ok && r[0].output.contains("\tthree\n"),
        "{}",
        r[0].output
    );
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "one\nTWO\nthree\nfour\n"
    );
    let ev = turn_open(
        &backend(),
        sid,
        &log,
        dir.path(),
        json!([
            call(
                "edit",
                json!({"path": "f.txt", "tag": new_tag,
                       "lines": [{"from": 3, "to": 3, "text": "THREE"}]})
            ),
            call(
                "edit",
                json!({"path": "f.txt", "tag": new_tag,
                       "lines": [{"from": 3, "to": 3, "text": "THREE"}]})
            ),
        ]),
    )
    .await;
    let r = results(&ev);
    // Within one turn the refusal's rows count; the second call is the
    // resend (the first only printed them).
    assert!(!r[0].ok && r[1].ok, "{} / {}", r[0].output, r[1].output);
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "one\nTWO\nTHREE\nfour\n"
    );
}

/// A host that asks (no call runs unasked) and keeps what it was given with
/// each request: the preview of the person's card.
#[derive(Default)]
struct Asking {
    events: Mutex<Vec<EventKind>>,
    asked: Mutex<Vec<(String, Option<String>)>>,
    unasked: bool,
}

#[async_trait::async_trait]
impl TurnHost for Asking {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        self.events.lock().unwrap().push(kind);
        Ok(())
    }
    async fn approve(&self, _: &str, _: &str, _: &Value, _: ToolRisk) -> Decision {
        Decision::Allow
    }
    async fn approve_from(
        &self,
        _: &str,
        tool: &str,
        _: &Value,
        _: ToolRisk,
        _: Option<&str>,
        preview: Option<&str>,
    ) -> Decision {
        self.asked
            .lock()
            .unwrap()
            .push((tool.to_string(), preview.map(str::to_string)));
        Decision::Allow
    }
    fn runs_unasked(&self, _: &str, _: ToolRisk) -> bool {
        self.unasked
    }
}

async fn asking_turn(dir: &std::path::Path, unasked: bool, script: Value) -> Asking {
    let host = Asking {
        unasked,
        ..Default::default()
    };
    backend()
        .run_turn(TurnCtx {
            session_id: harness_core::SessionId::new(),
            turn_id: "t".into(),
            history: vec![],
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
    host
}

fn tag_of(text: &str) -> String {
    harness_tools::seen::tag(&harness_tools::seen::sha256(text.as_bytes()))
}

/// The loop asks the tool for the card's text and hands it to the host with
/// the request; it is for the person only: the model's result does not hold
/// it, and a call that runs unasked is not asked for one.
#[tokio::test]
async fn the_loop_hands_the_tools_preview_to_the_host_with_the_request() {
    let dir = tempfile::tempdir().unwrap();
    let text = "one\ntwo\nthree\n";
    std::fs::write(dir.path().join("f.txt"), text).unwrap();
    let calls = json!([
        call("read", json!({"path": "f.txt"})),
        call(
            "edit",
            json!({"path": "f.txt", "tag": tag_of(text),
                   "lines": [{"from": 2, "to": 2, "text": "TWO"}]})
        ),
        call(
            "edit",
            json!({"path": "f.txt", "old_string": "three", "new_string": "3"})
        ),
        call("bash", json!({"command": "echo hi"})),
    ]);
    let host = asking_turn(dir.path(), false, calls.clone()).await;
    let asked = host.asked.lock().unwrap().clone();
    let by_tool = |t: &str| asked.iter().filter(|(n, _)| n == t).collect::<Vec<_>>();
    let line = by_tool("edit");
    assert_eq!(line.len(), 2, "{asked:?}");
    let p = line[0].1.as_deref().expect("the line form has a preview");
    assert!(
        p.contains("row 2 is replaced:") && p.contains("\ttwo\n"),
        "{p}"
    );
    assert!(line[1].1.is_none(), "the old form has none: {asked:?}");
    assert!(by_tool("bash")[0].1.is_none());
    assert!(by_tool("read")[0].1.is_none());
    // Not in anything the model is sent: the result of the call, nor the
    // event of the call.
    for e in host.events.lock().unwrap().iter() {
        if let EventKind::ToolCallFinished(f) = e {
            assert!(!f.output.contains("is replaced:"), "{}", f.output);
        }
    }
    // A call that runs unasked is not asked for a preview at all.
    std::fs::write(dir.path().join("f.txt"), text).unwrap();
    let host = asking_turn(dir.path(), true, calls).await;
    assert!(host.asked.lock().unwrap().iter().all(|(_, p)| p.is_none()));
}
