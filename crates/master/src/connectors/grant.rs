//! Which turns may call connectors (X.35 step 2, PLAN.md T85): the second
//! layer under the chat's mark. The first layer is the store's: a chat in
//! which someone other than its owner ever acted (`Store::chat_foreign`)
//! uses no connector at all, whatever this says (third review of PR 157:
//! a per-turn rule cannot keep foreign text out, as the chat's history
//! carries it into every later turn). What is left for this module is to
//! keep a NODE from inventing a grant: a turn calls only when the master
//! itself started it with the owner's send, and only so many times.
//!
//! **Sends.** Every prompt and retry the master hands a node for a chat
//! that has connectors is recorded at the moment it goes on the link
//! ([`Grants::sent`], by `Master::send_command_noted`), with a digest of
//! the whole input (text, attachment names, image ids, file mentions; a
//! form reply by its message id instead of its text) and whether it was
//! the owner's; it is taken out again on every failure of that send
//! ([`Grants::unsent`]). Chats without a recorded send have no entry, so a
//! node cannot fill these tables with sessions of its own.
//!
//! **Turns**, as the mirror applies the node's log ([`Grants::on_event`]):
//! a `turn_started` with a `cause` (a callback) never grants; one with a
//! `retry` grants when the oldest retry send of that failed turn was the
//! owner's AND that failed turn was itself granted; any other waits for its
//! first `user_message`, which grants when it is not queued and its digest
//! matches the oldest matching send, the owner's. Sends older than the one
//! matched are dropped. A turn the master sent nothing for has no grant.
//!
//! **What was removed** (third review, item 4): the rule that a later or
//! queued (`queued_delivered`) message of another sender takes a granted
//! turn's grant. Every such sender now marks the chat before its message
//! is forwarded, which refuses every call in the chat for good; what those
//! rules still caught was a message the node itself made up, and a node
//! that makes up messages can call within any grant it has anyway.
//!
//! **Sub-agents** ([`Grants::check`]): a sub-agent's turn is bound, when it
//! starts, to its chat's open granted turn, unless the master sent that
//! sub-agent's chat a message of its own meanwhile (a person typed into
//! it: that turn is the message's, not a `delegate`'s). It calls while the
//! log shows it open and no longer than [`AFTER_END`] after the chat's
//! turn ended. **Counts** ([`Grants::take`], [`Grants::give_back`]):
//! [`PER_TURN`] calls per granted turn, sub-agents included, under the
//! master's own key, and [`PER_HOUR`] per chat in any hour.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use harness_core::{RawEvent, SessionId};

/// Calls one granted turn may make, its sub-agents' included.
pub const PER_TURN: u32 = 30;
/// Calls one chat may make in any hour, whatever its turns.
pub const PER_HOUR: usize = 200;
/// How long after a granted turn ended its background sub-agents may still
/// call.
pub const AFTER_END: Duration = Duration::from_secs(60 * 60);
/// Chats tracked, the least recently used forgotten first (its turns are
/// then ungranted: fails closed).
const KEPT: usize = 1024;
/// Sends kept per chat (a node queues at most 8 messages behind a running
/// turn), turns per chat (a background sub-agent's grant ends early when
/// its chat has had 8 newer turns), sub-agent turns in all.
const SENDS: usize = 16;
const TURNS: usize = 8;
const CHILDREN: usize = 2048;
// What these tables hold at worst, near enough: per chat 16 sends of about
// 64 bytes, 8 turns of about 100 and up to 200 call times of 16 (3.2 KB,
// only for a chat that made 200 calls in the hour): about 5 KB, so 1,024
// chats about 5 MB; 2,048 sub-agent turns with their parents, about 0.3
// MB. No prompt's text is kept: a digest of it.

/// What the master sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// A message, by the digest of its whole input ([`input_digest`]).
    Prompt { digest: [u8; 32] },
    /// A Retry of turn `of`.
    Retry { of: String },
}

/// A message's input as the node records it in `user_message`: its text
/// (none for a form reply, whose text the node writes itself; its message
/// id instead), its attachments' names and types, its images' ids and
/// digests, its file mentions. The node's scrubber can change a text or an
/// attachment's text; only names are taken from attachments, and a changed
/// text matches nothing (no grant: fails closed).
pub fn input_digest(
    text: &str,
    reply_id: Option<&str>,
    attachments: &[(&str, &str)],
    images: &[(&str, &str)],
    files: &[&str],
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    let mut field = |b: &[u8]| {
        h.update((b.len() as u64).to_be_bytes());
        h.update(b);
    };
    match reply_id {
        Some(id) => {
            field(b"reply");
            field(id.as_bytes());
        }
        None => {
            field(b"text");
            field(text.as_bytes());
        }
    }
    for (n, m) in attachments {
        field(b"attachment");
        field(n.as_bytes());
        field(m.as_bytes());
    }
    for (id, sha) in images {
        field(b"image");
        field(id.as_bytes());
        field(sha.as_bytes());
    }
    for f in files {
        field(b"file");
        field(f.as_bytes());
    }
    h.finalize().into()
}

/// [`input_digest`] of a `user_message` payload.
fn event_digest(p: &serde_json::Value) -> [u8; 32] {
    fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("")
    }
    let list = |k: &str| {
        p.get(k)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let atts = list("attachments");
    let imgs = list("images");
    let files = list("files");
    let a: Vec<(&str, &str)> = atts.iter().map(|x| (s(x, "name"), s(x, "mime"))).collect();
    let i: Vec<(&str, &str)> = imgs
        .iter()
        .map(|x| (s(x, "media_id"), s(x, "sha256")))
        .collect();
    let f: Vec<&str> = files.iter().map(|x| s(x, "path")).collect();
    let reply = p
        .get("reply")
        .filter(|r| !r.is_null())
        .map(|r| s(r, "message_id"));
    input_digest(s(p, "text"), reply, &a, &i, &f)
}

#[derive(Debug, Clone)]
struct Sent {
    id: u64,
    what: What,
    owner: bool,
}

#[derive(Debug, Clone)]
struct Turn {
    id: String,
    /// Decided: `Some(true)` granted, `Some(false)` never; `None`: waits for
    /// its first message.
    granted: Option<bool>,
    ended: Option<Instant>,
    calls: u32,
}

#[derive(Default)]
struct Chat {
    sent: VecDeque<Sent>,
    turns: VecDeque<Turn>,
    hour: VecDeque<Instant>,
}

impl Chat {
    fn turn(&mut self, id: &str) -> Option<&mut Turn> {
        self.turns.iter_mut().find(|t| t.id == id)
    }

    /// Take the oldest send `fits`, and every older one with it.
    fn take(&mut self, fits: impl Fn(&What) -> bool) -> Option<Sent> {
        let i = self.sent.iter().position(|s| fits(&s.what))?;
        let s = self.sent.remove(i);
        self.sent.drain(..i);
        s
    }

    fn open(&self) -> Option<&Turn> {
        self.turns.back().filter(|t| t.ended.is_none())
    }
}

/// The key a call is counted under: the chat and its granted turn.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    pub chat: SessionId,
    pub turn: String,
}

#[derive(Default)]
pub struct Grants {
    chats: HashMap<SessionId, Chat>,
    /// Least recently used first.
    order: VecDeque<SessionId>,
    /// A sub-agent session's parent, from its `session_created`.
    parents: HashMap<SessionId, SessionId>,
    /// (sub-agent session, its turn) -> the chat's granted turn it started
    /// under.
    children: HashMap<(SessionId, String), Key>,
    children_order: VecDeque<(SessionId, String)>,
    /// Open sub-agent turns by session (its last `turn_started` not ended).
    child_open: HashMap<SessionId, String>,
    /// Messages the master sent a sub-agent's chat that no turn there has
    /// started with yet (a person typed into it).
    child_sent: HashMap<SessionId, u32>,
    next: u64,
    /// Tests: how long after its end a turn's sub-agents may call.
    pub after_end: Option<Duration>,
}

impl Grants {
    /// Mark `sid` used now (least recently used goes first).
    fn touch(&mut self, sid: SessionId) {
        if let Some(i) = self.order.iter().position(|s| *s == sid) {
            self.order.remove(i);
        }
        self.order.push_back(sid);
    }

    /// The chat's entry, made (the least recently used one forgotten past
    /// [`KEPT`]); only a send of the master's makes one.
    fn chat_for_send(&mut self, sid: SessionId) -> &mut Chat {
        if !self.chats.contains_key(&sid)
            && self.chats.len() >= KEPT
            && let Some(old) = self.order.pop_front()
        {
            self.chats.remove(&old);
        }
        self.touch(sid);
        self.chats.entry(sid).or_default()
    }

    /// The master hands the node `what` for chat `sid` now; `owner`: it may
    /// use connectors. The id names it for [`Self::unsent`].
    pub fn sent(&mut self, sid: SessionId, what: What, owner: bool) -> u64 {
        self.next += 1;
        let id = self.next;
        let c = self.chat_for_send(sid);
        if c.sent.len() >= SENDS {
            c.sent.pop_front();
        }
        c.sent.push_back(Sent { id, what, owner });
        id
    }

    /// The master sends a sub-agent's chat a message of its own: the turn
    /// it starts there is not bound to its chat's grant.
    pub fn sent_to_child(&mut self, sid: SessionId) {
        if self.child_sent.len() >= CHILDREN && !self.child_sent.contains_key(&sid) {
            self.child_sent.clear();
        }
        *self.child_sent.entry(sid).or_default() += 1;
    }

    /// That send failed: it is gone.
    pub fn unsent(&mut self, sid: SessionId, id: u64) {
        if let Some(c) = self.chats.get_mut(&sid) {
            c.sent.retain(|s| s.id != id);
        }
    }

    /// An event the mirror took.
    pub fn on_event(&mut self, e: &RawEvent) {
        let p = &e.payload;
        let text = |k: &str| p.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let sid = e.session_id;
        match e.kind.as_str() {
            "session_created" => {
                if let Some(parent) = text("parent_session").and_then(|s| s.parse().ok()) {
                    if self.parents.len() >= CHILDREN {
                        self.parents.clear();
                    }
                    self.parents.insert(sid, parent);
                }
            }
            "turn_started" => {
                let Some(turn) = text("turn_id") else { return };
                if let Some(parent) = self.parents.get(&sid).copied() {
                    self.child_open.insert(sid, turn.clone());
                    // A person's message to the sub-agent's chat started
                    // it: not a `delegate`'s run.
                    if let Some(n) = self.child_sent.get_mut(&sid)
                        && *n > 0
                    {
                        *n -= 1;
                        return;
                    }
                    // A sub-agent's turn: under its chat's open turn, if
                    // that one is granted.
                    let key = self.chats.get(&parent).and_then(|c| {
                        c.open().filter(|t| t.granted == Some(true)).map(|t| Key {
                            chat: parent,
                            turn: t.id.clone(),
                        })
                    });
                    if let Some(key) = key {
                        if self.children.len() >= CHILDREN
                            && let Some(old) = self.children_order.pop_front()
                        {
                            self.children.remove(&old);
                        }
                        self.children_order.push_back((sid, turn.clone()));
                        self.children.insert((sid, turn), key);
                    }
                    return;
                }
                // Only a chat the master sent something has an entry.
                let Some(c) = self.chats.get_mut(&sid) else {
                    return;
                };
                // A turn the harness started (a callback, or the turn of an
                // agent a person's message called after the first, X.37
                // `{"mention": ...}`) is no send of the master's: never
                // granted, and it takes no recorded send (the first agent's
                // own turn is the message's, with its `user_message`).
                let callback = p.get("cause").is_some_and(|c| !c.is_null());
                let retry_of = p
                    .get("retry")
                    .and_then(|r| r.get("turn_id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let granted = if callback {
                    Some(false)
                } else if let Some(of) = retry_of {
                    // The owner's Retry of a turn that was itself granted.
                    let was = c
                        .turns
                        .iter()
                        .any(|t| t.id == of && t.granted == Some(true));
                    let sent = c.take(|w| *w == What::Retry { of: of.clone() });
                    Some(was && sent.is_some_and(|s| s.owner))
                } else {
                    None
                };
                if c.turns.len() >= TURNS {
                    c.turns.pop_front();
                }
                c.turns.push_back(Turn {
                    id: turn,
                    granted,
                    ended: None,
                    calls: 0,
                });
                self.touch(sid);
            }
            "user_message" => {
                if self.parents.contains_key(&sid) {
                    return;
                }
                let Some(turn) = text("turn_id") else { return };
                let queued = p.get("queued").and_then(|v| v.as_bool()) == Some(true);
                let heard = event_digest(p);
                let Some(c) = self.chats.get_mut(&sid) else {
                    return;
                };
                let sent = c.take(|w| *w == What::Prompt { digest: heard });
                let owner = sent.is_some_and(|s| s.owner);
                let Some(t) = c.turn(&turn) else { return };
                if t.granted.is_none() {
                    t.granted = Some(owner && !queued);
                }
            }
            "turn_ended" => {
                let Some(turn) = text("turn_id") else { return };
                if self.parents.contains_key(&sid) {
                    if self.child_open.get(&sid) == Some(&turn) {
                        self.child_open.remove(&sid);
                    }
                    return;
                }
                if let Some(t) = self.chats.get_mut(&sid).and_then(|c| c.turn(&turn)) {
                    t.ended.get_or_insert_with(Instant::now);
                    // Never granted later: a decision still waiting ends no.
                    t.granted.get_or_insert(false);
                }
            }
            _ => {}
        }
    }

    /// The granted turn a call of session `sid`, whose open turn in the log
    /// is `open`, counts under; `chat` is its chat (itself for a chat).
    pub fn check(&self, sid: SessionId, chat: SessionId, open: &str) -> Result<Key, String> {
        let refused = || {
            "connectors are used only in a turn the server started for the chat's owner (their \
             own message, their routine's run or their Retry); not in this one"
                .to_string()
        };
        let key = if sid == chat {
            Key {
                chat,
                turn: open.to_string(),
            }
        } else {
            if self.child_open.get(&sid).map(String::as_str) != Some(open) {
                return Err(refused());
            }
            self.children
                .get(&(sid, open.to_string()))
                .cloned()
                .ok_or_else(refused)?
        };
        let t = self
            .chats
            .get(&key.chat)
            .and_then(|c| c.turns.iter().find(|t| t.id == key.turn))
            .ok_or_else(refused)?;
        if t.granted != Some(true) {
            return Err(refused());
        }
        match t.ended {
            None => {}
            // The chat's own turn ended: nothing more of it.
            Some(_) if sid == chat => return Err(refused()),
            Some(at) if at.elapsed() > self.after_end.unwrap_or(AFTER_END) => {
                return Err(format!(
                    "the turn that started this sub-agent ended more than {} minutes ago; its \
                     connectors are not used any more",
                    AFTER_END.as_secs() / 60
                ));
            }
            Some(_) => {}
        }
        Ok(key)
    }

    /// Count one call under `key`: [`PER_TURN`] per granted turn and
    /// [`PER_HOUR`] per chat.
    pub fn take(&mut self, key: &Key) -> Result<(), String> {
        self.touch(key.chat);
        let c = self.chats.get_mut(&key.chat).ok_or("no such turn")?;
        while c
            .hour
            .front()
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(3600))
        {
            c.hour.pop_front();
        }
        if c.hour.len() >= PER_HOUR {
            return Err(format!(
                "this chat made {PER_HOUR} connector calls in the last hour, the most it may; \
                 try again later"
            ));
        }
        let t = c
            .turns
            .iter_mut()
            .find(|t| t.id == key.turn)
            .ok_or("no such turn")?;
        if t.calls >= PER_TURN {
            return Err(format!(
                "this turn already made {PER_TURN} connector calls, the most one turn may; tell \
                 the person what else you would do"
            ));
        }
        t.calls += 1;
        c.hour.push_back(Instant::now());
        Ok(())
    }

    /// A call counted under `key` was not made for a reason of this
    /// server's (busy, a token that does not open): its count comes back.
    pub fn give_back(&mut self, key: &Key) {
        if let Some(c) = self.chats.get_mut(&key.chat) {
            c.hour.pop_back();
            if let Some(t) = c.turns.iter_mut().find(|t| t.id == key.turn) {
                t.calls = t.calls.saturating_sub(1);
            }
        }
    }

    pub fn forget(&mut self, sid: SessionId) {
        self.chats.remove(&sid);
        self.order.retain(|s| *s != sid);
    }

    /// Tests: what the tables hold for chat `sid`, as (sends not matched
    /// yet, turns).
    #[cfg(test)]
    pub(crate) fn held(&self, sid: SessionId) -> (usize, usize) {
        self.chats
            .get(&sid)
            .map_or((0, 0), |c| (c.sent.len(), c.turns.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(sid: SessionId, kind: &str, payload: serde_json::Value) -> RawEvent {
        RawEvent {
            session_id: sid,
            seq: 0,
            ts_ms: 0,
            kind: kind.into(),
            payload,
        }
    }

    fn prompt(text: &str) -> What {
        What::Prompt {
            digest: input_digest(text, None, &[], &[], &[]),
        }
    }

    struct Log<'a>(&'a mut Grants, SessionId);

    impl Log<'_> {
        fn started(&mut self, t: &str) {
            self.0
                .on_event(&ev(self.1, "turn_started", json!({"turn_id": t})));
        }
        fn said(&mut self, t: &str, text: &str, queued: bool) {
            self.0.on_event(&ev(
                self.1,
                "user_message",
                json!({"turn_id": t, "text": text, "queued": queued}),
            ));
        }
        fn ended(&mut self, t: &str) {
            self.0
                .on_event(&ev(self.1, "turn_ended", json!({"turn_id": t})));
        }
        fn ok(&self, t: &str) -> bool {
            self.0.check(self.1, self.1, t).is_ok()
        }
    }

    /// A grant needs the master's own send for the owner: a node-written
    /// turn, a refused send, a left-over send, a queued first message, a
    /// callback and a Retry of an ungranted turn get none; nothing survives
    /// a restart.
    #[test]
    fn a_grant_needs_the_masters_send_for_the_owner() {
        let sid = SessionId::new();
        let mut g = Grants::default();
        g.sent(sid, prompt("one"), true);
        let mut l = Log(&mut g, sid);
        l.started("t1");
        l.said("t1", "one", false);
        assert!(l.ok("t1"));
        l.ended("t1");
        assert!(!l.ok("t1"), "an ended turn calls no more");
        // A node-written turn and message: nothing was sent.
        l.started("t2");
        l.said("t2", "invented", false);
        assert!(!l.ok("t2"));
        l.ended("t2");
        // A refused send.
        let id = l.0.sent(sid, prompt("refused"), true);
        l.0.unsent(sid, id);
        l.started("t3");
        l.said("t3", "refused", false);
        assert!(!l.ok("t3"));
        l.ended("t3");
        // A left-over send binds to no later turn.
        l.0.sent(sid, prompt("lost"), true);
        l.0.sent(sid, prompt("hook"), false);
        l.started("t4");
        l.said("t4", "hook", false);
        assert!(!l.ok("t4"));
        l.ended("t4");
        l.started("t5");
        l.said("t5", "lost", false);
        assert!(!l.ok("t5"), "dropped with the match");
        l.ended("t5");
        // A queued first message.
        l.0.sent(sid, prompt("q"), true);
        l.started("t6");
        l.said("t6", "q", true);
        assert!(!l.ok("t6"));
        l.ended("t6");
        // The same text, attachments differ: no match.
        l.0.sent(
            sid,
            What::Prompt {
                digest: input_digest("x", None, &[("a.pdf", "application/pdf")], &[], &[]),
            },
            true,
        );
        l.started("t7");
        l.said("t7", "x", false);
        assert!(!l.ok("t7"), "an attachment is part of the input");
        l.ended("t7");
        // A form reply matches by its message id, not by any reply.
        l.0.sent(
            sid,
            What::Prompt {
                digest: input_digest("", Some("m1"), &[], &[], &[]),
            },
            true,
        );
        l.started("t8");
        l.0.on_event(&ev(
            sid,
            "user_message",
            json!({"turn_id": "t8", "text": "canonical", "reply": {"message_id": "m2"}}),
        ));
        assert!(!Log(l.0, sid).ok("t8"), "another reply's id");
        let mut l = Log(l.0, sid);
        l.ended("t8");
        // A callback; a Retry of a granted turn by the owner; of one that
        // was not granted.
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "c1", "cause": {"callback": ["b"]}}),
        ));
        assert!(!Log(l.0, sid).ok("c1"));
        let mut l = Log(l.0, sid);
        l.ended("c1");
        l.0.sent(sid, prompt("g"), true);
        l.started("g1");
        l.said("g1", "g", false);
        l.ended("g1");
        l.0.sent(sid, What::Retry { of: "g1".into() }, true);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "r1", "retry": {"turn_id": "g1"}}),
        ));
        assert!(
            Log(l.0, sid).ok("r1"),
            "the owner's retry of a granted turn"
        );
        let mut l = Log(l.0, sid);
        l.ended("r1");
        l.0.sent(sid, What::Retry { of: "t2".into() }, true);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "r2", "retry": {"turn_id": "t2"}}),
        ));
        assert!(!Log(l.0, sid).ok("r2"), "t2 was not granted");
        assert!(Grants::default().check(sid, sid, "g1").is_err());
        // A node's own session gets no entry at all.
        let other = SessionId::new();
        g.on_event(&ev(other, "turn_started", json!({"turn_id": "x"})));
        assert!(!g.chats.contains_key(&other));
    }

    /// X.37: the turn of an agent a message called after the first is the
    /// harness's, not the master's send: it is never granted, it takes no
    /// recorded send (the one waiting is still there for the message that
    /// carries it), and the message's own turn keeps its grant.
    #[test]
    fn a_mention_turn_is_not_granted_and_takes_no_send() {
        let sid = SessionId::new();
        let mut g = Grants::default();
        g.sent(sid, prompt("@a @b hi"), true);
        let mut l = Log(&mut g, sid);
        l.started("t1");
        l.said("t1", "@a @b hi", false);
        assert!(l.ok("t1"), "the message's own turn keeps today's grant");
        l.ended("t1");
        // Another send is waiting when the mention turn starts.
        l.0.sent(sid, prompt("next"), true);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "m1", "cause": {"mention": {"message_seq": 3, "by": "person"}}}),
        ));
        assert_eq!(
            l.0.chats.get(&sid).unwrap().sent.len(),
            1,
            "starting it took no recorded send"
        );
        let mut l = Log(l.0, sid);
        assert!(!l.ok("m1"), "no connector in a mention turn");
        // A hostile node writes the waiting send's message into it: it is
        // still no grant (the turn was decided when it started).
        l.said("m1", "next", false);
        assert!(!l.ok("m1"));
        l.ended("m1");
    }

    /// Counts are per granted turn and per chat per hour, a call not made
    /// gives its count back; sub-agents count against the turn that
    /// started them, not when a person typed into their chat, and not past
    /// the end.
    #[test]
    fn counts_and_sub_agents() {
        let sid = SessionId::new();
        let kid = SessionId::new();
        let mut g = Grants::default();
        g.on_event(&ev(
            kid,
            "session_created",
            json!({"parent_session": sid.to_string()}),
        ));
        g.sent(sid, prompt("go"), true);
        g.on_event(&ev(sid, "turn_started", json!({"turn_id": "t1"})));
        g.on_event(&ev(
            sid,
            "user_message",
            json!({"turn_id": "t1", "text": "go"}),
        ));
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k1"})));
        g.on_event(&ev(sid, "turn_ended", json!({"turn_id": "t1"})));
        let key = g.check(kid, sid, "k1").unwrap();
        assert_eq!(key.turn, "t1");
        for _ in 0..PER_TURN {
            g.take(&key).unwrap();
        }
        assert!(g.take(&key).unwrap_err().contains("already made"));
        g.give_back(&key);
        g.take(&key).unwrap();
        // A person's message typed into the sub-agent's chat: its turn is
        // not bound to the chat's grant.
        g.on_event(&ev(kid, "turn_ended", json!({"turn_id": "k1"})));
        g.sent(sid, prompt("again"), true);
        g.on_event(&ev(sid, "turn_started", json!({"turn_id": "t2"})));
        g.on_event(&ev(
            sid,
            "user_message",
            json!({"turn_id": "t2", "text": "again"}),
        ));
        g.sent_to_child(kid);
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k2"})));
        assert!(g.check(kid, sid, "k2").is_err());
        g.on_event(&ev(kid, "turn_ended", json!({"turn_id": "k2"})));
        // A sub-agent turn under a chat turn that is not granted: none.
        g.on_event(&ev(sid, "turn_ended", json!({"turn_id": "t2"})));
        g.sent(sid, prompt("api"), false);
        g.on_event(&ev(sid, "turn_started", json!({"turn_id": "t3"})));
        g.on_event(&ev(
            sid,
            "user_message",
            json!({"turn_id": "t3", "text": "api"}),
        ));
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k3"})));
        assert!(g.check(kid, sid, "k3").is_err());
        g.on_event(&ev(kid, "turn_ended", json!({"turn_id": "k3"})));
        g.on_event(&ev(sid, "turn_ended", json!({"turn_id": "t3"})));
        // The hour: 200 whatever the turns.
        let mut used = PER_TURN as usize;
        for i in 0..10 {
            let t = format!("h{i}");
            g.sent(sid, prompt(&t), true);
            g.on_event(&ev(sid, "turn_started", json!({"turn_id": t})));
            g.on_event(&ev(sid, "user_message", json!({"turn_id": t, "text": t})));
            let key = g.check(sid, sid, &t).unwrap();
            while g.take(&key).is_ok() {
                used += 1;
            }
            g.on_event(&ev(sid, "turn_ended", json!({"turn_id": t})));
        }
        assert_eq!(used, PER_HOUR);
        // The end of a turn's sub-agents' grant.
        g.after_end = Some(Duration::ZERO);
        g.sent(sid, prompt("z"), true);
        g.on_event(&ev(sid, "turn_started", json!({"turn_id": "z1"})));
        g.on_event(&ev(
            sid,
            "user_message",
            json!({"turn_id": "z1", "text": "z"}),
        ));
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k4"})));
        g.on_event(&ev(sid, "turn_ended", json!({"turn_id": "z1"})));
        std::thread::sleep(Duration::from_millis(5));
        assert!(
            g.check(kid, sid, "k4")
                .unwrap_err()
                .contains("ended more than")
        );
    }

    /// The least recently used chat goes first: an active chat survives a
    /// flood of new ones.
    #[test]
    fn the_least_recently_used_chat_is_forgotten_first() {
        let mut g = Grants::default();
        let busy = SessionId::new();
        g.sent(busy, prompt("go"), true);
        g.on_event(&ev(busy, "turn_started", json!({"turn_id": "t"})));
        g.on_event(&ev(
            busy,
            "user_message",
            json!({"turn_id": "t", "text": "go"}),
        ));
        let key = g.check(busy, busy, "t").unwrap();
        for i in 0..(KEPT * 2) {
            g.sent(SessionId::new(), prompt(&format!("{i}")), true);
            if i % 100 == 0 {
                g.take(&key).unwrap();
            }
        }
        assert!(g.check(busy, busy, "t").is_ok(), "kept while in use");
        assert!(g.chats.len() <= KEPT);
    }
}
