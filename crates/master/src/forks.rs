//! Forks on the master (ADOPTION_PLAN C.2, docs/fork.md).
//!
//! Two jobs. [`through`] is what the guard in `Master::apply` asks first: an event of
//! a fork that is a copy of its parent's (`harness_core::fork::inherited`)
//! is mirrored and shown, and **nothing it did when the master first applied
//! it happens again**: no memory fact is filed (the dedupe key holds the
//! session id, so a copy would file the fact twice), no artifact is
//! published, no callback turn is started for a copied `background_ended`,
//! no connector grant moves. The check is one branch at the top of `apply`,
//! so a side effect added below it later is skipped for a copy without
//! anyone remembering to skip it.
//!
//! [`fork_session`] is what a person's fork does on the master: the same
//! steps as a create (the create intent, the command, then the claim and the
//! project link), with the new chat in its parent's workspace and project.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use harness_core::{SessionId, fork};
use harness_protocol::MasterToNode;
use harness_store::SessionRow;

use crate::{CommandError, Live, Master, create_intent_key, lock};

/// `through_seq` of the chats this process has met, 0 for one that is no
/// fork. A chat's lineage is written with its first event and never changes
/// (ids are not reused), so a successful answer cannot go stale. Only
/// successful answers are kept: a failed read is no answer, and "not a fork"
/// is kept only for a chat whose first event the mirror holds (a later event
/// that arrived first says nothing yet). A deleted chat's entry is dropped.
fn known() -> &'static Mutex<HashMap<SessionId, u64>> {
    static KNOWN: OnceLock<Mutex<HashMap<SessionId, u64>>> = OnceLock::new();
    KNOWN.get_or_init(Mutex::default)
}

/// More chats than this and the table starts again (it only saves reads).
const KNOWN_MAX: usize = 100_000;

/// What the mirror says about a chat's lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lookup {
    /// A fork, its first `through` events copies.
    Fork(u64),
    /// A chat the mirror holds and that is no fork.
    NotFork,
    /// A chat the mirror does not hold (yet): no answer to keep.
    NotYet,
}

/// Read a chat's lineage from the mirror.
pub(crate) fn lookup(s: &harness_store::Store, sid: SessionId) -> harness_store::Result<Lookup> {
    if let Some(f) = s.fork_of(sid)? {
        return Ok(Lookup::Fork(f.through_seq));
    }
    Ok(if s.session(sid)?.is_some() {
        Lookup::NotFork
    } else {
        Lookup::NotYet
    })
}

/// How many of `sid`'s first events are copies, from `cache` or from
/// `find` (whose answer is kept when it is one). An error is returned and
/// kept nowhere.
fn resolve<E>(
    cache: &Mutex<HashMap<SessionId, u64>>,
    sid: SessionId,
    find: impl FnOnce() -> Result<Lookup, E>,
) -> Result<u64, E> {
    if let Some(t) = lock(cache).get(&sid).copied() {
        return Ok(t);
    }
    let through = match find()? {
        Lookup::NotYet => return Ok(0),
        Lookup::NotFork => 0,
        Lookup::Fork(t) => t,
    };
    let mut k = lock(cache);
    if k.len() >= KNOWN_MAX {
        k.clear();
    }
    k.insert(sid, through);
    Ok(through)
}

/// How many of the chat's first events are its parent's copies (0: none):
/// what `Master::apply` asks, before it takes an event of seq 2 or more, so
/// a read that fails stops the event there (the node sends it again, as for
/// any store failure) and is never taken for "not a fork". An event is
/// a copy when [`harness_core::fork::inherited`] says so.
pub(crate) async fn through(m: &Master, sid: SessionId) -> harness_store::Result<u64> {
    if let Some(t) = lock(known()).get(&sid).copied() {
        return Ok(t);
    }
    let found = m.inner.mirror.call(move |s| lookup(s, sid)).await?;
    resolve(known(), sid, || Ok::<_, harness_store::StoreError>(found))
}

/// The chat is gone (deleted, or its first event arrives again): what was
/// known of it goes.
pub(crate) fn forget(sid: SessionId) {
    lock(known()).remove(&sid);
}

/// Fork `parent` at the end of `turn_id` (the caller checked the owner, the
/// level and the cut against the mirror): the new chat's id, once the node
/// has written it and the master has stamped its owner, workspace and
/// project. The new chat is on the parent's node, in the parent's workspace
/// and (when it has one) project, and follows the model defaults as its
/// parent does.
pub(crate) async fn fork_session(
    m: &Master,
    parent: &SessionRow,
    turn_id: &str,
    checkout: harness_protocol::ForkCheckout,
) -> Result<SessionId, CommandError> {
    let sid = SessionId::new();
    let node_id = parent.node_id.clone();
    let (owner, workspace) = (parent.owner_id.clone(), parent.workspace_id.clone());
    let pid = parent.session_id;
    let now = harness_core::now_ms();
    // Who asked, recorded before the node hears of it (as a create does):
    // the fork's first event that arrives after the ack, or after the ack
    // timed out, is claimed for them when it lands.
    // The parent's project and whether it follows the model defaults go
    // with it: a fork whose ack is late gets them from the late claim.
    let (project, auto) = m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>((
                s.session_project(pid)?.map(|p| p.project_id),
                s.kv_get(&crate::auto_model_key(pid))?.is_some(),
            ))
        })
        .await?;
    let intent = (owner.is_some() || workspace.is_some()).then(|| {
        serde_json::json!({"node": node_id, "owner": owner, "workspace": workspace,
            "at": now, "project": project, "auto": auto})
        .to_string()
    });
    let key = create_intent_key(sid);
    let v = intent.clone();
    m.inner
        .mirror
        .call(move |s| match v {
            Some(v) => s.kv_set(&key, &v),
            None => Ok(()),
        })
        .await?;
    // Numbered per original chat: "X (fork)", "X (fork 2)", ... (a fork of a
    // fork is the next one of the same original).
    let nth = m
        .inner
        .mirror
        .call(move |s| s.forks_of_original(pid))
        .await?
        .saturating_add(1);
    let title = fork::title_nth(&parent.title, nth);
    let sent = m
        .send_command(&node_id, |req_id| MasterToNode::Fork {
            req_id,
            session_id: sid,
            parent: pid,
            turn_id: turn_id.to_string(),
            title,
            checkout,
        })
        .await;
    match &sent {
        // The node may still make it: the intent stays for that.
        Ok(()) | Err(CommandError::Timeout) => {}
        Err(_) if intent.is_some() => {
            let key = create_intent_key(sid);
            let _ = m.inner.mirror.call(move |s| s.kv_delete(&key)).await;
        }
        Err(_) => {}
    }
    sent?;
    // The fork's events are mirrored before the ack (the same ordered
    // link), so its row exists: stamp owner and workspace, link it to the
    // parent's project, and let it follow the defaults as the parent does.
    let linked = m
        .inner
        .mirror
        .call(move |s| {
            if s.session(sid)?.is_some() {
                s.kv_delete(&create_intent_key(sid))?;
            }
            if owner.is_some() || workspace.is_some() {
                s.claim_session(sid, owner.as_deref(), workspace.as_deref())?;
            }
            if auto {
                s.kv_set(&crate::auto_model_key(sid), "1")?;
            }
            let linked = match &project {
                Some(p) => s.link_new_session(sid, p)?,
                None => false,
            };
            Ok::<_, harness_store::StoreError>(linked)
        })
        .await?;
    m.nudge(Live::SessionsChanged(crate::Audience::Chat(sid)));
    if linked {
        m.nudge(Live::ProjectsChanged(crate::Audience::Chat(sid)));
    }
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::RawEvent;

    fn cache() -> Mutex<HashMap<SessionId, u64>> {
        Mutex::default()
    }

    /// A failed read is passed on and kept nowhere: the next ask reads again
    /// and gets the real answer (never a remembered "not a fork").
    #[test]
    fn a_failed_read_is_not_cached() {
        let c = cache();
        let sid = SessionId::new();
        let failed: Result<u64, &str> = resolve(&c, sid, || Err("disk I/O error"));
        assert_eq!(failed, Err("disk I/O error"));
        assert!(lock(&c).get(&sid).is_none(), "nothing was kept");
        let then: Result<u64, &str> = resolve(&c, sid, || Ok(Lookup::Fork(7)));
        assert_eq!(then, Ok(7));
    }

    /// An event that arrives before the chat's first one finds no chat: no
    /// answer, so none is kept, and the lineage written with event 1 is read
    /// when the next event asks.
    #[test]
    fn an_event_that_arrives_before_the_first_poisons_nothing() {
        let c = cache();
        let sid = SessionId::new();
        let early: Result<u64, &str> = resolve(&c, sid, || Ok(Lookup::NotYet));
        assert_eq!(early, Ok(0));
        assert!(lock(&c).get(&sid).is_none());
        let later: Result<u64, &str> = resolve(&c, sid, || Ok(Lookup::Fork(5)));
        assert_eq!(later, Ok(5));
    }

    /// A real answer, a fork or not, is read once.
    #[test]
    fn a_real_answer_is_read_once() {
        let c = cache();
        let (a, b) = (SessionId::new(), SessionId::new());
        for (sid, found, want) in [(a, Lookup::Fork(4), 4), (b, Lookup::NotFork, 0)] {
            assert_eq!(resolve(&c, sid, || Ok::<_, &str>(found)), Ok(want));
            let again: Result<u64, &str> = resolve(&c, sid, || Err("not asked again"));
            assert_eq!(again, Ok(want));
        }
    }

    /// The guard is the protection: a fork's copied turn events move no
    /// connector grant state (a send recorded for the chat, not yet matched
    /// by a message, stays unmatched), and the fork's own events still do.
    /// (Without the guard the copied `user_message` takes the send and the
    /// copied `turn_started` makes a turn: a copy would grant.)
    #[tokio::test]
    async fn copied_turn_events_move_no_connector_grant_state() {
        use crate::auth::Auth;
        use crate::connectors::call::Note;
        use harness_core::Secret;
        use harness_protocol::PromptMsg;
        let m = Master::new(
            harness_store::Store::open_in_memory().unwrap(),
            Auth::new(Secret::new("t".into()), vec![], false),
        );
        let sid = SessionId::new();
        // The master sent the fork's chat a message of its owner's, which no
        // event has matched yet.
        let sent = MasterToNode::Prompt(PromptMsg::test(sid, "first"));
        m.inner
            .connectors
            .calls
            .note_send(&sent, Note::Chat { owner: true })
            .unwrap();
        assert_eq!(m.inner.connectors.calls.held(sid), (1, 0));
        let at = |seq: u64, kind: &str, payload: serde_json::Value| RawEvent {
            session_id: sid,
            seq,
            ts_ms: 1,
            kind: kind.into(),
            payload,
        };
        let evs = [
            at(
                1,
                "session_created",
                serde_json::json!({"title": "t (fork)", "cwd": "/w", "model": "m",
                    "forked": {"parent": SessionId::new(), "through_seq": 4}}),
            ),
            at(2, "turn_started", serde_json::json!({"turn_id": "t1"})),
            at(
                3,
                "user_message",
                serde_json::json!({"turn_id": "t1", "text": "first"}),
            ),
            at(
                4,
                "turn_ended",
                serde_json::json!({"turn_id": "t1", "reason": "completed"}),
            ),
        ];
        for e in evs {
            assert!(m.apply("n", e).await);
        }
        assert_eq!(
            m.inner.connectors.calls.held(sid),
            (1, 0),
            "the copy took the send or made a turn"
        );
        // The fork's own turn does move it.
        assert!(
            m.apply(
                "n",
                at(
                    5,
                    "session_forked",
                    serde_json::json!({
                    "parent": SessionId::new(), "through_seq": 4, "turn_id": "t1"})
                )
            )
            .await
        );
        assert!(
            m.apply(
                "n",
                at(6, "turn_started", serde_json::json!({"turn_id": "t2"}))
            )
            .await
        );
        assert_eq!(m.inner.connectors.calls.held(sid), (1, 1));
    }

    /// A deleted chat's entry is dropped.
    #[test]
    fn a_deleted_chats_entry_is_dropped() {
        let sid = SessionId::new();
        assert_eq!(
            resolve(known(), sid, || Ok::<_, &str>(Lookup::Fork(3))),
            Ok(3)
        );
        assert!(lock(known()).contains_key(&sid));
        forget(sid);
        assert!(!lock(known()).contains_key(&sid));
    }

    /// The mirror's answer: a chat with a lineage row, a chat without one,
    /// and an id the mirror does not hold.
    #[test]
    fn the_lookup_tells_no_fork_from_no_chat() {
        let s = harness_store::Store::open_in_memory().unwrap();
        let sid = SessionId::new();
        assert_eq!(lookup(&s, sid).unwrap(), Lookup::NotYet);
        let created = |sid: SessionId, forked: serde_json::Value| RawEvent {
            session_id: sid,
            seq: 1,
            ts_ms: 1,
            kind: "session_created".into(),
            payload: serde_json::json!({"title": "t", "cwd": "/w", "model": "m", "forked": forked}),
        };
        s.append("n", &created(sid, serde_json::Value::Null))
            .unwrap();
        assert_eq!(lookup(&s, sid).unwrap(), Lookup::NotFork);
        let f = SessionId::new();
        let parent = SessionId::new();
        s.append(
            "n",
            &created(f, serde_json::json!({"parent": parent, "through_seq": 6})),
        )
        .unwrap();
        assert_eq!(lookup(&s, f).unwrap(), Lookup::Fork(6));
    }
}
