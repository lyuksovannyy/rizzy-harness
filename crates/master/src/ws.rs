//! UI WebSocket: replay from a cursor, then live, with no gaps or duplicates.
//!
//! Correctness argument: the broadcast receiver is created when the socket
//! opens, before any backlog read. So every event committed after a backlog
//! read is also in the receiver. Per session we track the last seq sent and
//! only forward `last + 1`; anything older is dropped, anything newer (or a
//! lagged receiver) triggers a catch-up read from the mirror.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use harness_core::SessionId;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;

use crate::{Live, Master, auth};

const PAGE: usize = 1000;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Subscribe {
        session_id: SessionId,
        #[serde(default)]
        after_seq: u64,
    },
    Unsubscribe {
        session_id: SessionId,
    },
}

pub async fn upgrade(
    State(m): State<Master>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(who) = auth::authenticate(&m, &headers).await else {
        return (StatusCode::UNAUTHORIZED, "not logged in").into_response();
    };
    if who.must_change_password {
        return (StatusCode::FORBIDDEN, "change your password first").into_response();
    }
    ws.on_upgrade(move |socket| run(m, who, socket))
}

type Sink = futures::stream::SplitSink<WebSocket, Message>;

async fn send(sink: &mut Sink, v: serde_json::Value) -> bool {
    sink.send(Message::Text(v.to_string().into())).await.is_ok()
}

/// Send everything after `*last` from the mirror, advancing `*last`.
async fn catch_up(m: &Master, sink: &mut Sink, sid: SessionId, last: &mut u64) -> bool {
    loop {
        let after = *last;
        let page = match m
            .inner
            .mirror
            .call(move |s| s.events_after(sid, after, PAGE))
            .await
        {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "mirror read failed");
                return false;
            }
        };
        let n = page.len();
        for e in page {
            *last = e.seq;
            if !send(sink, json!({"type": "event", "event": e})).await {
                return false;
            }
        }
        if n < PAGE {
            return true;
        }
    }
}

async fn may_see(m: &Master, who: &auth::Principal, sid: SessionId) -> bool {
    match m.inner.mirror.call(move |s| s.session(sid)).await {
        Ok(Some(r)) => who.can_see(r.owner_id.as_deref()),
        _ => false,
    }
}

async fn run(m: Master, who: auth::Principal, socket: WebSocket) {
    let mut live = m.subscribe();
    let (mut sink, mut stream) = socket.split();
    let mut subs: HashMap<SessionId, u64> = HashMap::new();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.reset();
    loop {
        tokio::select! {
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let Ok(cmd) = serde_json::from_str::<ClientMsg>(&text) else {
                    if !send(&mut sink, json!({"type": "error", "message": "bad message"})).await {
                        break;
                    }
                    continue;
                };
                match cmd {
                    ClientMsg::Subscribe { session_id, after_seq } => {
                        if !may_see(&m, &who, session_id).await {
                            if !send(&mut sink, json!({"type": "error", "message": "unknown session", "session_id": session_id})).await {
                                break;
                            }
                            continue;
                        }
                        let last = subs.entry(session_id).or_insert(after_seq);
                        *last = after_seq;
                        if !catch_up(&m, &mut sink, session_id, last).await {
                            break;
                        }
                        let head = *last;
                        if !send(&mut sink, json!({"type": "replayed", "session_id": session_id, "head_seq": head})).await {
                            break;
                        }
                    }
                    ClientMsg::Unsubscribe { session_id } => {
                        subs.remove(&session_id);
                    }
                }
            }
            l = live.recv() => {
                let ok = match l {
                    Ok(Live::Event(e)) => match subs.get_mut(&e.session_id) {
                        Some(last) if e.seq == *last + 1 => {
                            *last = e.seq;
                            send(&mut sink, json!({"type": "event", "event": e})).await
                        }
                        Some(last) if e.seq > *last + 1 => {
                            catch_up(&m, &mut sink, e.session_id, last).await
                        }
                        _ => true,
                    },
                    Ok(Live::SessionsChanged) => send(&mut sink, json!({"type": "sessions_changed"})).await,
                    Ok(Live::NodesChanged) => send(&mut sink, json!({"type": "nodes_changed"})).await,
                    // No content: each client refetches its own memories.
                    Ok(Live::MemoryChanged) => send(&mut sink, json!({"type": "memory_changed"})).await,
                    Ok(Live::WorkspacesChanged) => send(&mut sink, json!({"type": "workspaces_changed"})).await,
                    Err(RecvError::Lagged(_)) => {
                        let mut ok = true;
                        for (sid, last) in subs.iter_mut() {
                            ok &= catch_up(&m, &mut sink, *sid, last).await;
                        }
                        // A dropped message may have been either.
                        ok && send(&mut sink, json!({"type": "sessions_changed"})).await
                            && send(&mut sink, json!({"type": "workspaces_changed"})).await
                    }
                    Err(RecvError::Closed) => false,
                };
                if !ok {
                    break;
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
