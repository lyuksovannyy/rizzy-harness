//! Messages the person sent while a turn ran (X.5) in the loop: read
//! before the next request, after the step's results, once; the person's
//! words for URL provenance; rebuilt from the log where they were read.

use std::sync::{Arc, Mutex};

use harness_core::backend::{TurnCtx, TurnHost};
use harness_core::event::*;
use harness_core::policy::{Decision, ToolRisk};
use harness_core::{RawEvent, SessionId};
use harness_tools::{ToolCtx, ToolOutcome};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::provider::*;
use crate::*;

const U: &str = "https://steer.example/only-in-the-queued-message";

fn msg(text: &str) -> UserMessage {
    UserMessage {
        turn_id: "t".into(),
        text: text.into(),
        attachments: vec![],
        reply: None,
        images: vec![],
        queued: true,
    }
}

/// Records what the loop emits. The person's message arrives while the
/// first step's tool runs (its event lands there in the log) and is
/// handed over at the next `queued_messages` after that, once.
struct Host {
    events: Mutex<Vec<EventKind>>,
    asked: Mutex<usize>,
}

#[async_trait::async_trait]
impl TurnHost for Host {
    async fn emit(&self, kind: EventKind) -> Result<(), harness_core::backend::LogError> {
        let mut ev = self.events.lock().unwrap();
        let tool_started = matches!(kind, EventKind::ToolCallStarted(_));
        ev.push(kind);
        // Sent while the first tool runs.
        if tool_started
            && ev
                .iter()
                .filter(|k| matches!(k, EventKind::ToolCallStarted(_)))
                .count()
                == 1
        {
            ev.push(EventKind::UserMessage(msg(&format!("also read {U}"))));
        }
        Ok(())
    }
    async fn approve(&self, _c: &str, _t: &str, _i: &Value, _r: ToolRisk) -> Decision {
        Decision::Allow
    }
    async fn queued_messages(&self) -> Result<Vec<UserMessage>, harness_core::backend::LogError> {
        let n = {
            let mut a = self.asked.lock().unwrap();
            *a += 1;
            *a
        };
        let mut ev = self.events.lock().unwrap();
        let waiting = ev
            .iter()
            .any(|k| matches!(k, EventKind::UserMessage(m) if m.queued));
        let read = ev
            .iter()
            .any(|k| matches!(k, EventKind::QueuedDelivered(_)));
        if n < 2 || !waiting || read {
            return Ok(vec![]);
        }
        // Its seq in the log `log()` builds: after turn_started (1) and
        // the turn's own message (2).
        let at = ev
            .iter()
            .position(|k| matches!(k, EventKind::UserMessage(m) if m.queued))
            .unwrap() as u64
            + 3;
        ev.push(EventKind::QueuedDelivered(QueuedDelivered {
            turn_id: "t".into(),
            seqs: vec![at],
        }));
        Ok(vec![msg(&format!("also read {U}"))])
    }
}

/// Answers by request number: a no-op call, then `web_fetch` of the URL
/// only the queued message holds, then a final answer.
struct Seq(Mutex<Vec<ChatRequest>>);

#[async_trait::async_trait]
impl Provider for Seq {
    fn name(&self) -> &'static str {
        "seq"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        _deltas: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        let n = {
            let mut s = self.0.lock().unwrap();
            s.push(req.clone());
            s.len() - 1
        };
        let tool_calls = match n {
            0 => vec![("noop", json!({}))],
            1 => vec![("web_fetch", json!({"url": U}))],
            _ => vec![],
        }
        .into_iter()
        .map(|(t, i)| ToolCallRef {
            call_id: format!("c{n}"),
            tool: t.into(),
            input: i,
        })
        .collect();
        Ok(Completion {
            text: if n >= 2 { "done".into() } else { String::new() },
            tool_calls,
            usage: None,
            stop: Default::default(),
        })
    }
}

/// Stands in for `web_fetch`: says whether `url` is URL provenance.
struct Probe;

#[async_trait::async_trait]
impl harness_tools::Tool for Probe {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        "probe"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let url = input["url"].as_str().unwrap_or("");
        ToolOutcome::ok(format!(
            "seen={}",
            ctx.observed.iter().any(|t| t.contains(url))
        ))
    }
}

struct Noop;

#[async_trait::async_trait]
impl harness_tools::Tool for Noop {
    fn name(&self) -> &'static str {
        "noop"
    }
    fn description(&self) -> &'static str {
        "noop"
    }
    fn schema(&self) -> Value {
        json!({"type": "object"})
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, _: &ToolCtx, _: Value) -> ToolOutcome {
        ToolOutcome::ok("ok")
    }
}

fn ctx<'a>(host: &'a dyn TurnHost, user_text: &str) -> TurnCtx<'a> {
    TurnCtx {
        session_id: SessionId::new(),
        turn_id: "t".into(),
        history: vec![],
        user_text: user_text.into(),
        cwd: "/".into(),
        model: "mock".into(),
        agent: None,
        compare: vec![],
        parent_session: None,
        extra: Default::default(),
        cancel: Default::default(),
        host,
    }
}

fn log(sid: SessionId, own: &str, kinds: Vec<EventKind>) -> Vec<RawEvent> {
    let mut all = vec![
        EventKind::TurnStarted(TurnStarted {
            turn_id: "t".into(),
            agent: None,
            cause: None,
        }),
        EventKind::UserMessage(UserMessage {
            queued: false,
            ..msg(own)
        }),
    ];
    all.extend(kinds);
    all.into_iter()
        .enumerate()
        .map(|(i, k)| {
            let (kind, payload) = k.into_parts();
            RawEvent {
                session_id: sid,
                seq: i as u64 + 1,
                ts_ms: 0,
                kind,
                payload,
            }
        })
        .collect()
}

/// The message reaches the request after the first step's result, as the
/// person's words under the harness's line, exactly once in every later
/// request; its URL is the person's (URL provenance); and the log rebuilds
/// the same messages, the message where it was read, not where it arrived.
#[tokio::test]
async fn a_queued_message_joins_the_next_request_once() {
    let host = Host {
        events: Mutex::new(vec![]),
        asked: Mutex::new(0),
    };
    let seq = Arc::new(Seq(Mutex::new(vec![])));
    let b = NativeBackend::new(
        seq.clone(),
        vec![Arc::new(Noop), Arc::new(Probe)],
        NativeConfig::default(),
    );
    let out = b.run_turn(ctx(&host, "start")).await;
    assert_eq!(out.reason, TurnEndReason::Completed, "{out:?}");
    let reqs = seq.0.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3);
    let count = |r: &ChatRequest| {
        r.messages
            .iter()
            .filter(|m| m.user_text().is_some_and(|t| t.contains(U)))
            .count()
    };
    assert_eq!(count(&reqs[0]), 0);
    assert_eq!(count(&reqs[1]), 1);
    assert_eq!(count(&reqs[2]), 1);
    let n = reqs[1].messages.len();
    assert!(matches!(reqs[1].messages[n - 2], ChatMsg::Tool { .. }));
    assert_eq!(
        reqs[1].messages[n - 1],
        ChatMsg::User(queued_text(&format!("also read {U}")))
    );
    // The person's words: the fetch of a URL only they wrote is allowed.
    let probed = host
        .events
        .lock()
        .unwrap()
        .iter()
        .find_map(|e| match e {
            EventKind::ToolCallFinished(f) if f.call_id == "c1" => Some(f.output.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(probed, "seen=true");
    // Rebuilt from the log: the same messages as the last request.
    let sid = SessionId::new();
    let events = host.events.lock().unwrap().clone();
    let rebuilt = place_notes(history_to_messages(&log(sid, "start", events)));
    let sent: Vec<ChatMsg> = reqs[2].messages[1..].to_vec();
    let mut rebuilt = rebuilt;
    // The final answer is in the log, not in the request it answered.
    assert!(matches!(rebuilt.pop(), Some(ChatMsg::Assistant { .. })));
    assert_eq!(rebuilt, sent);
}

/// A queued message the model never read is not in the history it is
/// sent; once read, it stands where it was read.
#[test]
fn an_unread_message_is_not_history() {
    let sid = SessionId::new();
    let unread = log(sid, "start", vec![EventKind::UserMessage(msg("later"))]);
    let m = history_to_messages(&unread);
    assert_eq!(m, vec![ChatMsg::User("start".into())]);
    let mut kinds = vec![EventKind::UserMessage(msg("later"))];
    kinds.push(EventKind::QueuedDelivered(QueuedDelivered {
        turn_id: "t".into(),
        seqs: vec![3],
    }));
    let m = history_to_messages(&log(sid, "start", kinds));
    assert_eq!(
        m,
        vec![
            ChatMsg::User("start".into()),
            ChatMsg::User(queued_text("later"))
        ]
    );
    // The person's own words of it are theirs, without the harness's line.
    assert_eq!(own_words(&queued_text("later")), "later");
}

/// Messages an earlier turn never read go before this turn's own message.
#[tokio::test]
async fn messages_an_earlier_turn_left_go_first() {
    let host = Host {
        events: Mutex::new(vec![]),
        asked: Mutex::new(usize::MAX / 2),
    };
    let seq = Arc::new(Seq(Mutex::new(vec![])));
    let b = NativeBackend::new(seq.clone(), vec![Arc::new(Noop)], NativeConfig::default());
    let mut c = ctx(&host, "go on");
    c.extra.queued = vec![msg("left over")];
    b.run_turn(c).await;
    let first = seq.0.lock().unwrap()[0].clone();
    let users: Vec<&str> = first
        .messages
        .iter()
        .filter_map(|m| m.user_text())
        .collect();
    assert_eq!(users, vec![queued_text("left over").as_str(), "go on"]);
}
