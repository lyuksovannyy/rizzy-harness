//! Which turns may call connectors (X.35 step 2, PLAN.md T85; review of
//! PR 157, HIGH 1 and 2): a grant belongs to ONE turn, which the master
//! itself started with the signed-in owner's message (or a run of the
//! owner's own routine, or the owner's Retry), and into which nothing else
//! has been delivered.
//!
//! **How a turn is matched to what the master sent.** Every message the
//! master hands a node for a chat (a prompt, a retry, a callback) is put
//! first in this chat's table of sends ([`Grants::sent`], before the
//! command goes on the link, taken out again when the node refuses it or
//! the size guard does: [`Grants::unsent`]), with whether it may use
//! connectors. The node's log then says what it did with each: the master
//! reads its own mirror of the log as it is applied ([`Grants::on_event`]):
//!
//! * a `turn_started` with a `cause` (a callback) or a `retry` takes the
//!   oldest send of that kind; a callback never grants, a retry grants only
//!   when it was the owner's;
//! * any other `turn_started` waits for its first `user_message`: a
//!   message that is not queued and that matches a send (its text, or a
//!   form answer to a reply the master sent) grants the turn when that
//!   send was the owner's; a queued message never grants;
//! * every later `user_message` inside the turn takes its send too, and
//!   one that is not the owner's, or matches no send at all, takes the
//!   grant away for good;
//! * a `turn_started` or `user_message` the master sent nothing for (the
//!   node wrote it on its own) matches nothing: no grant.
//!
//! Sends older than the one matched are dropped (they never reached the
//! log), so a send left over cannot bind to a later turn. Nothing is kept
//! across a restart: every turn the log shows after one is ungranted.
//!
//! **What a grant allows** ([`Grants::check`]): calls of the chat's own
//! open turn while it is open, and of a sub-agent's turn that started while
//! a granted turn of its chat was open (the node started it for that turn's
//! `delegate`), while the sub-agent's turn is open and no longer than
//! [`AFTER_END`] after the chat's turn ended. **Counts** ([`Grants::take`]):
//! [`PER_TURN`] calls per granted turn, its sub-agents' included, under the
//! key the master granted (never a turn id a node can vary), and
//! [`PER_HOUR`] per chat in any hour, which no turn resets.

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
/// Chats tracked; past it the oldest is forgotten (its turns are then
/// ungranted: fails closed).
const KEPT: usize = 4096;
/// Sends kept per chat, turns per chat, sub-agent turns in all.
const SENDS: usize = 32;
const TURNS: usize = 16;
const CHILDREN: usize = 8192;

/// What the master sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// A message; `reply`: it answers a form, and the node records its
    /// own canonical text.
    Prompt {
        text: String,
        reply: bool,
    },
    Retry,
    Callback,
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
    order: VecDeque<SessionId>,
    /// A sub-agent session's parent, from its `session_created`.
    parents: HashMap<SessionId, SessionId>,
    /// (sub-agent session, its turn) -> the chat's granted turn it started
    /// under.
    children: HashMap<(SessionId, String), Key>,
    children_order: VecDeque<(SessionId, String)>,
    /// Open sub-agent turns by session (its last `turn_started` not ended).
    child_open: HashMap<SessionId, String>,
    next: u64,
    /// Tests: how long after its end a turn's sub-agents may call.
    pub after_end: Option<Duration>,
}

impl Grants {
    fn chat(&mut self, sid: SessionId) -> &mut Chat {
        if !self.chats.contains_key(&sid) {
            if self.chats.len() >= KEPT
                && let Some(old) = self.order.pop_front()
            {
                self.chats.remove(&old);
            }
            self.order.push_back(sid);
        }
        self.chats.entry(sid).or_default()
    }

    /// The master is about to send `what` for chat `sid`; `owner`: it may
    /// use connectors. The id names it for [`Self::unsent`].
    pub fn sent(&mut self, sid: SessionId, what: What, owner: bool) -> u64 {
        self.next += 1;
        let id = self.next;
        let c = self.chat(sid);
        if c.sent.len() >= SENDS {
            c.sent.pop_front();
        }
        c.sent.push_back(Sent { id, what, owner });
        id
    }

    /// The node refused send `id` (or it was never sent): it is gone.
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
                    // A sub-agent's turn: under its chat's open turn, if
                    // that one is granted.
                    self.child_open.insert(sid, turn.clone());
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
                let callback = p.get("cause").is_some_and(|c| !c.is_null());
                let retry = p.get("retry").is_some_and(|c| !c.is_null());
                let c = self.chat(sid);
                let granted = if callback {
                    c.take(|w| *w == What::Callback);
                    Some(false)
                } else if retry {
                    Some(c.take(|w| *w == What::Retry).is_some_and(|s| s.owner))
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
            }
            "user_message" => {
                if self.parents.contains_key(&sid) {
                    return;
                }
                let Some(turn) = text("turn_id") else { return };
                let said = text("text").unwrap_or_default();
                let queued = p.get("queued").and_then(|v| v.as_bool()) == Some(true);
                let reply = p.get("reply").is_some_and(|v| !v.is_null());
                let c = self.chat(sid);
                let sent = c.take(|w| match w {
                    What::Prompt { text, reply: r } => *text == said || (*r && reply),
                    _ => false,
                });
                let owner = sent.is_some_and(|s| s.owner);
                let Some(t) = c.turn(&turn) else { return };
                t.granted = match t.granted {
                    None => Some(owner && !queued),
                    Some(g) => Some(g && owner),
                };
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
            "connectors are used only in a turn the chat's owner started (their own message, \
             their routine's run or their Retry) and that has taken no other message; not in \
             this one"
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

    pub fn forget(&mut self, sid: SessionId) {
        self.chats.remove(&sid);
        self.order.retain(|s| *s != sid);
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
            text: text.into(),
            reply: false,
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

    /// The review's table of turn kinds (PR 157, HIGH 1).
    #[test]
    fn a_grant_belongs_to_one_turn_the_master_started_for_the_owner() {
        let sid = SessionId::new();
        let mut g = Grants::default();
        // The owner's own message: granted.
        g.sent(sid, prompt("one"), true);
        let mut l = Log(&mut g, sid);
        l.started("t1");
        l.said("t1", "one", false);
        assert!(l.ok("t1"));
        // An API-token prompt queued into it: never again, whatever comes.
        l.0.sent(sid, prompt("api"), false);
        l.said("t1", "api", true);
        assert!(!l.ok("t1"));
        l.0.sent(sid, prompt("owner again"), true);
        l.said("t1", "owner again", true);
        assert!(!l.ok("t1"), "an owner message never grants a running turn");
        l.ended("t1");
        // An API-token turn, the owner's message queued into it: no.
        l.0.sent(sid, prompt("api2"), false);
        l.started("t2");
        l.said("t2", "api2", false);
        l.0.sent(sid, prompt("mine"), true);
        l.said("t2", "mine", true);
        assert!(!l.ok("t2"));
        l.ended("t2");
        // The turn that only reads queued messages: no.
        l.started("t3");
        l.said("t3", "x", true);
        assert!(!l.ok("t3"));
        l.ended("t3");
        // A node-written turn and message: nothing was sent, no grant.
        l.started("t4");
        l.said("t4", "invented", false);
        assert!(!l.ok("t4"));
        l.ended("t4");
        // A send the node refused leaves nothing to match.
        let id = l.0.sent(sid, prompt("refused"), true);
        l.0.unsent(sid, id);
        l.started("t5");
        l.said("t5", "refused", false);
        assert!(!l.ok("t5"));
        l.ended("t5");
        // An owner message left over (never in the log) binds to no later
        // turn: the later turn's own send is matched and older ones go.
        l.0.sent(sid, prompt("lost"), true);
        l.0.sent(sid, prompt("hook"), false);
        l.started("t6");
        l.said("t6", "hook", false);
        assert!(!l.ok("t6"));
        l.ended("t6");
        l.started("t7");
        l.said("t7", "lost", false);
        assert!(!l.ok("t7"), "the left-over send was dropped with the match");
        l.ended("t7");
        // A callback turn: no; a retry: only the owner's.
        l.0.sent(sid, What::Callback, true);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "c1", "cause": {"callback": ["b"]}}),
        ));
        assert!(!Log(l.0, sid).ok("c1"));
        let mut l = Log(l.0, sid);
        l.ended("c1");
        l.0.sent(sid, What::Retry, true);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "r1", "retry": {"turn_id": "t7", "reason": "error"}}),
        ));
        assert!(Log(l.0, sid).ok("r1"));
        let mut l = Log(l.0, sid);
        l.ended("r1");
        assert!(!l.ok("r1"), "an ended turn calls no more");
        l.0.sent(sid, What::Retry, false);
        l.0.on_event(&ev(
            sid,
            "turn_started",
            json!({"turn_id": "r2", "retry": {"turn_id": "t7", "reason": "error"}}),
        ));
        assert!(!Log(l.0, sid).ok("r2"));
        // Nothing survives a restart.
        let fresh = Grants::default();
        assert!(fresh.check(sid, sid, "t1").is_err());
    }

    /// Counts are per granted turn and per chat per hour; sub-agents count
    /// against the turn that started them, also after it ended.
    #[test]
    fn counts_are_bounds() {
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
        // Its own invented turn: not under any grant.
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k2"})));
        assert!(g.check(kid, sid, "k2").is_err());
        g.on_event(&ev(kid, "turn_ended", json!({"turn_id": "k2"})));
        assert!(g.check(kid, sid, "k1").is_err(), "not open");
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
        g.on_event(&ev(kid, "turn_started", json!({"turn_id": "k3"})));
        g.on_event(&ev(sid, "turn_ended", json!({"turn_id": "z1"})));
        std::thread::sleep(Duration::from_millis(5));
        assert!(
            g.check(kid, sid, "k3")
                .unwrap_err()
                .contains("ended more than")
        );
    }
}
