//! UI WebSocket: replay from a cursor, then live, with no gaps or duplicates.
//!
//! Correctness argument: the broadcast receiver is created when the socket
//! opens, before any backlog read. So every event committed after a backlog
//! read is also in the receiver. Per session we track the last seq sent and
//! only forward `last + 1`; anything older is dropped, anything newer (or a
//! lagged receiver) triggers a catch-up read from the mirror.
//!
//! A socket ends with its login (PLAN.md T2, T12; AUDIT S5). Each one is
//! registered in [`Sockets`] under its user, with the hash of its session
//! id (never the cookie). Ending a login (logout, a password change or
//! reset, disabling or deleting the account) flips that socket's own
//! `watch` flag, which cannot lag or be dropped the way a broadcast can.
//! Registration happens before the socket's first login check, and every
//! revocation happens after the session rows are gone, so a login ended
//! while a socket is still upgrading is caught by one or the other. The
//! socket also re-checks its login, without sliding its expiry, every
//! [`RECHECK`] and before every subscribe, which covers expiry and any
//! path that deletes rows without telling us.
//!
//! A browser cannot give a WebSocket a header, so a cookie login's key
//! (Gate 3 P-1, `auth`) comes as the socket's first message,
//! `{"type":"login","key":…}`, within [`KEY_WAIT`]. Before it nothing is
//! read from the mirror, subscribed or sent; without it, or with a wrong
//! one, the socket is closed as signed out. Not in the URL (no secret in
//! a URL) nor in a subprotocol, which proxies log.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use harness_core::SessionId;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;

use crate::{Live, Master, auth, lock};

const PAGE: usize = 1000;

/// The `Live::Changed` topics; after a lag each is sent once, since any of
/// them may have been dropped.
const TOPICS: [&str; 3] = ["routines", "media", "artifacts"];

/// Largest client message and frame. The client only sends subscribe and
/// unsubscribe messages (well under 1 KiB); the WebSocket layer refuses
/// anything bigger from its header, before buffering it, and drops the
/// connection.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Open UI sockets per user (the API token counts as one user). The next
/// one is refused with 429 rather than closing the oldest: closing the
/// oldest would make a ninth tab and the tab it pushed out take turns
/// reconnecting forever.
pub const MAX_SOCKETS_PER_USER: usize = 8;
/// How often an open socket re-checks its login.
pub const RECHECK: Duration = Duration::from_secs(60);
/// Close code for a socket whose login ended (application range; reads as
/// HTTP 401). The UI should stop reconnecting and show the login screen.
pub const SIGNED_OUT: u16 = 4401;
/// How long a closing socket may take to send its close frame.
const CLOSE_WAIT: Duration = Duration::from_secs(1);
/// How long a cookie socket has to send its login key.
pub const KEY_WAIT: Duration = Duration::from_secs(5);
/// Cookie sockets still waiting for their key, in all (Gate 3 fix review
/// 2, R2-1). They hold no user's place, and the cookie is not looked at
/// before the key, so they are counted apart; past this many the oldest
/// waiting one is closed (1013, try again) rather than the new one refused:
/// a page sends its key at once, so a flood would have to open sockets
/// faster than one round trip to keep the person's own from its key.
pub const MAX_KEY_WAITING: usize = 64;
/// Close code for a keyed socket over its user's [`MAX_SOCKETS_PER_USER`]
/// (application range; reads as HTTP 429).
pub const TOO_MANY: u16 = 4429;
/// Close code for a keyed socket whose account must change its password
/// first (reads as HTTP 403).
pub const CHANGE_PASSWORD: u16 = 4403;
/// Close code for a waiting socket pushed out by newer ones (RFC 6455
/// "try again later").
const TRY_AGAIN: u16 = 1013;

/// The registry key of the API token's sockets (user ids are UUIDs).
const API_TOKEN: &str = "api-token";

/// Every open UI socket, by user.
pub struct Sockets {
    open: Mutex<HashMap<String, Vec<Entry>>>,
    /// Cookie sockets waiting for their key, oldest first, each with the
    /// sender that pushes it out.
    waiting: Mutex<std::collections::VecDeque<(u64, watch::Sender<bool>)>>,
    next_id: AtomicU64,
    recheck_ms: AtomicU64,
}

struct Entry {
    id: u64,
    /// SHA-256 of the session id it was opened with; `None` for the API
    /// token.
    session: Option<String>,
    revoke: watch::Sender<bool>,
}

impl Default for Sockets {
    fn default() -> Self {
        Self {
            open: Mutex::default(),
            waiting: Mutex::default(),
            next_id: AtomicU64::new(0),
            recheck_ms: AtomicU64::new(RECHECK.as_millis() as u64),
        }
    }
}

impl Sockets {
    pub fn set_recheck(&self, every: Duration) {
        let ms = (every.as_millis() as u64).max(1);
        self.recheck_ms.store(ms, Ordering::Relaxed);
    }

    fn recheck(&self) -> Duration {
        Duration::from_millis(self.recheck_ms.load(Ordering::Relaxed))
    }

    pub fn count(&self) -> usize {
        lock(&self.open).values().map(Vec::len).sum()
    }

    /// A place among the sockets waiting for their key; at
    /// [`MAX_KEY_WAITING`] the oldest is pushed out. The receiver turns
    /// true when this one is.
    fn wait(&self) -> (u64, watch::Receiver<bool>) {
        let mut w = lock(&self.waiting);
        while w.len() >= MAX_KEY_WAITING {
            if let Some((_, out)) = w.pop_front() {
                out.send_replace(true);
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = watch::channel(false);
        w.push_back((id, tx));
        (id, rx)
    }

    fn done_waiting(&self, id: u64) {
        lock(&self.waiting).retain(|(i, _)| *i != id);
    }

    /// Take one of `owner`'s places, or `None` when all are taken.
    fn register(
        &self,
        owner: &str,
        session: Option<String>,
    ) -> Option<(u64, watch::Receiver<bool>)> {
        let mut open = lock(&self.open);
        let list = open.entry(owner.to_string()).or_default();
        if list.len() >= MAX_SOCKETS_PER_USER {
            return None;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (revoke, rx) = watch::channel(false);
        list.push(Entry {
            id,
            session,
            revoke,
        });
        Some((id, rx))
    }

    fn release(&self, owner: &str, id: u64) {
        let mut open = lock(&self.open);
        if let Some(list) = open.get_mut(owner) {
            list.retain(|e| e.id != id);
            if list.is_empty() {
                open.remove(owner);
            }
        }
    }

    /// Close every socket of `user_id` (password change or reset, account
    /// disabled or deleted). Call it after the session rows are deleted.
    pub fn revoke_user(&self, user_id: &str) {
        if let Some(list) = lock(&self.open).get(user_id) {
            for e in list {
                e.revoke.send_replace(true);
            }
        }
    }

    /// Close the sockets opened with one login (logout), by the SHA-256 of
    /// its session id. Call it after the session row is deleted.
    pub fn revoke_session(&self, session_hash: &str) {
        for e in lock(&self.open).values().flatten() {
            if e.session.as_deref() == Some(session_hash) {
                e.revoke.send_replace(true);
            }
        }
    }
}

/// A socket's place in [`Sockets`], given back when it is dropped: when
/// the socket ends in any way, its task panics or is dropped at shutdown,
/// or the upgrade never completes.
struct Place {
    m: Master,
    owner: String,
    id: u64,
}

impl Drop for Place {
    fn drop(&mut self) {
        self.m.inner.sockets.release(&self.owner, self.id);
    }
}

/// A cookie socket's place among those waiting for their key.
struct Waiting {
    m: Master,
    id: u64,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.m.inner.sockets.done_waiting(self.id);
    }
}

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

/// A cookie socket's first message.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum First {
    Login { key: String },
}

pub async fn upgrade(
    State(m): State<Master>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let ws = ws.max_message_size(MAX_MESSAGE).max_frame_size(MAX_MESSAGE);
    // The API token, or a client that sent the login key as a header: the
    // socket is authenticated now.
    if let Some(who) = auth::authenticate(&m, &headers).await {
        if who.must_change_password {
            return (StatusCode::FORBIDDEN, "change your password first").into_response();
        }
        // A cookie login is tied to its session; the API token has none (it
        // lasts as long as the process and its file).
        let session = match who.user_id {
            Some(_) => match auth::cookie_value(&headers) {
                Some(id) => Some(auth::sha256_hex(&id)),
                None => return (StatusCode::UNAUTHORIZED, "not logged in").into_response(),
            },
            None => None,
        };
        let owner = who.user_id.clone().unwrap_or_else(|| API_TOKEN.into());
        let Some((id, revoked)) = m.inner.sockets.register(&owner, session.clone()) else {
            return (StatusCode::TOO_MANY_REQUESTS, "too many open connections").into_response();
        };
        let place = Place {
            m: m.clone(),
            owner,
            id,
        };
        return ws.on_upgrade(move |socket| async move {
            let _place = place;
            let (sink, stream) = socket.split();
            run(m, who, session, revoked, sink, stream).await;
        });
    }
    // A browser's cookie alone (a page cannot give a WebSocket a header):
    // it must send the key first. Nothing about the cookie is looked up
    // before that, so the answer here is the same for a live cookie, a dead
    // one and a made-up one, and the socket takes none of the person's
    // places while it waits (Gate 3 fix review 2, R2-1).
    let Some(cookie) = auth::cookie_value(&headers) else {
        return (StatusCode::UNAUTHORIZED, "not logged in").into_response();
    };
    let hash = auth::sha256_hex(&cookie);
    let (id, pushed_out) = m.inner.sockets.wait();
    let waiting = Waiting { m: m.clone(), id };
    ws.on_upgrade(move |socket| async move {
        let (mut sink, mut stream) = socket.split();
        let first = key_first(&m, &hash, &mut stream, pushed_out).await;
        drop(waiting);
        let user = match first {
            KeyWait::User(u) => u,
            KeyWait::No => return close(&mut sink, SIGNED_OUT, "signed out").await,
            KeyWait::PushedOut => return close(&mut sink, TRY_AGAIN, "try again").await,
        };
        let who = auth::Principal::from_user(user);
        if who.must_change_password {
            return close(&mut sink, CHANGE_PASSWORD, "change your password first").await;
        }
        let owner = who.user_id.clone().unwrap_or_default();
        let Some((id, revoked)) = m.inner.sockets.register(&owner, Some(hash.clone())) else {
            return close(&mut sink, TOO_MANY, "too many open connections").await;
        };
        let _place = Place {
            m: m.clone(),
            owner,
            id,
        };
        run(m, who, Some(hash), revoked, sink, stream).await;
    })
}

/// What a cookie socket's wait for its key came to.
enum KeyWait {
    User(harness_store::UserRow),
    /// Silence, another message, a wrong key, an ended or unknown login:
    /// all the same.
    No,
    /// Newer waiting sockets pushed this one out.
    PushedOut,
}

/// Wait for a cookie socket's login key: the first message, within
/// [`KEY_WAIT`], must be the key of the login its cookie names.
async fn key_first(
    m: &Master,
    session_hash: &str,
    stream: &mut futures::stream::SplitStream<WebSocket>,
    mut pushed_out: watch::Receiver<bool>,
) -> KeyWait {
    let first = tokio::select! {
        biased;
        _ = signed_out(&mut pushed_out) => return KeyWait::PushedOut,
        r = tokio::time::timeout(KEY_WAIT, stream.next()) => r,
    };
    let Ok(Some(Ok(Message::Text(text)))) = first else {
        return KeyWait::No;
    };
    match serde_json::from_str::<First>(&text) {
        Ok(First::Login { key }) => match auth::socket_login(m, session_hash, &key).await {
            Some(u) => KeyWait::User(u),
            None => KeyWait::No,
        },
        Err(_) => KeyWait::No,
    }
}

/// Send a close frame (bounded by [`CLOSE_WAIT`]).
async fn close(sink: &mut Sink, code: u16, reason: &'static str) {
    let frame = Message::Close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }));
    let _ = tokio::time::timeout(CLOSE_WAIT, sink.send(frame)).await;
}

type Sink = futures::stream::SplitSink<WebSocket, Message>;

/// Resolves once the socket's login was revoked (or its registry entry is
/// gone, which cannot happen while the socket holds its place).
async fn signed_out(rx: &mut watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}

/// The sending half. A send gives up as soon as the login is revoked, so a
/// client that stops reading cannot hold a revoked socket open.
struct Out {
    sink: Sink,
    revoked: watch::Receiver<bool>,
}

impl Out {
    async fn msg(&mut self, msg: Message) -> bool {
        tokio::select! {
            biased;
            _ = signed_out(&mut self.revoked) => false,
            r = self.sink.send(msg) => r.is_ok(),
        }
    }

    async fn send(&mut self, v: serde_json::Value) -> bool {
        self.msg(Message::Text(v.to_string().into())).await
    }
}

/// Is the socket's login still good? Read only: the expiry does not move.
/// The API token's sockets have no login to check. A store error counts as
/// no: the client reconnects and the upgrade authenticates afresh.
async fn login_holds(m: &Master, who: &auth::Principal, session: Option<&str>) -> bool {
    let Some(hash) = session.map(str::to_string) else {
        return true;
    };
    let now = harness_core::now_ms();
    match m
        .inner
        .mirror
        .call(move |s| s.ui_session_check(&hash, now))
        .await
    {
        Ok(Some(u)) => {
            who.user_id.as_deref() == Some(u.user_id.as_str()) && !u.must_change_password
        }
        Ok(None) => false,
        Err(e) => {
            tracing::warn!(error = %e, "socket login check failed");
            false
        }
    }
}

/// Send everything after `*last` from the mirror, advancing `*last`.
async fn catch_up(m: &Master, out: &mut Out, sid: SessionId, last: &mut u64) -> bool {
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
            if !out.send(json!({"type": "event", "event": e})).await {
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

/// An authenticated socket, its place taken. Nothing was subscribed, read
/// or sent before the key (P-1).
async fn run(
    m: Master,
    who: auth::Principal,
    session: Option<String>,
    revoked: watch::Receiver<bool>,
    sink: Sink,
    mut stream: futures::stream::SplitStream<WebSocket>,
) {
    let mut out = Out {
        sink,
        revoked: revoked.clone(),
    };
    let mut revoked = revoked;
    let mut live = m.subscribe();
    // Registered before this first check: a login that ended before it is
    // found here, one that ends after it flips `revoked`.
    let mut ended = !login_holds(&m, &who, session.as_deref()).await;
    let mut subs: HashMap<SessionId, u64> = HashMap::new();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.reset();
    let mut recheck = tokio::time::interval(m.inner.sockets.recheck());
    recheck.reset();
    while !ended {
        tokio::select! {
            biased;
            _ = signed_out(&mut revoked) => {
                ended = true;
            }
            msg = stream.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    // Includes a message or frame over `MAX_MESSAGE`.
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let Ok(cmd) = serde_json::from_str::<ClientMsg>(&text) else {
                    if !out.send(json!({"type": "error", "message": "bad message"})).await {
                        break;
                    }
                    continue;
                };
                match cmd {
                    ClientMsg::Subscribe { session_id, after_seq } => {
                        if !login_holds(&m, &who, session.as_deref()).await {
                            ended = true;
                            continue;
                        }
                        if !may_see(&m, &who, session_id).await {
                            if !out.send(json!({"type": "error", "message": "unknown session", "session_id": session_id})).await {
                                break;
                            }
                            continue;
                        }
                        let last = subs.entry(session_id).or_insert(after_seq);
                        *last = after_seq;
                        if !catch_up(&m, &mut out, session_id, last).await {
                            break;
                        }
                        let head = *last;
                        if !out.send(json!({"type": "replayed", "session_id": session_id, "head_seq": head})).await {
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
                            out.send(json!({"type": "event", "event": e})).await
                        }
                        Some(last) if e.seq > *last + 1 => {
                            catch_up(&m, &mut out, e.session_id, last).await
                        }
                        _ => true,
                    },
                    Ok(Live::SessionsChanged) => out.send(json!({"type": "sessions_changed"})).await,
                    Ok(Live::NodesChanged) => out.send(json!({"type": "nodes_changed"})).await,
                    // No content: each client refetches its own memories.
                    Ok(Live::MemoryChanged) => out.send(json!({"type": "memory_changed"})).await,
                    Ok(Live::WorkspacesChanged) => out.send(json!({"type": "workspaces_changed"})).await,
                    Ok(Live::ProjectsChanged) => out.send(json!({"type": "projects_changed"})).await,
                    // Content-free and only for the owner (or everyone, for a
                    // change to something shared).
                    Ok(Live::Changed { topic, owner }) => {
                        if owner.is_none() || who.can_see(owner.as_deref()) {
                            out.send(json!({"type": format!("{topic}_changed")})).await
                        } else {
                            true
                        }
                    }
                    // Revocation does not travel on this channel, so a lag
                    // cannot skip it.
                    Err(RecvError::Lagged(_)) => {
                        let mut ok = true;
                        for (sid, last) in subs.iter_mut() {
                            ok &= catch_up(&m, &mut out, *sid, last).await;
                        }
                        // A dropped message may have been any of them.
                        ok = ok && out.send(json!({"type": "sessions_changed"})).await
                            && out.send(json!({"type": "workspaces_changed"})).await
                            && out.send(json!({"type": "projects_changed"})).await;
                        for topic in TOPICS {
                            ok = ok && out.send(json!({"type": format!("{topic}_changed")})).await;
                        }
                        ok
                    }
                    Err(RecvError::Closed) => false,
                };
                if !ok {
                    break;
                }
            }
            _ = ping.tick() => {
                if !out.msg(Message::Ping(Vec::new().into())).await {
                    break;
                }
            }
            _ = recheck.tick() => {
                ended = !login_holds(&m, &who, session.as_deref()).await;
            }
        }
    }
    // A send cut short by a revocation also ends here, with the flag set.
    if ended || *revoked.borrow() {
        let close = Message::Close(Some(CloseFrame {
            code: SIGNED_OUT,
            reason: "signed out".into(),
        }));
        let _ = tokio::time::timeout(CLOSE_WAIT, out.sink.send(close)).await;
    }
}
