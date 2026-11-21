//! Forks on the master (ADOPTION_PLAN C.2, docs/fork.md).
//!
//! Two jobs. [`copied`] is the guard `Master::apply` runs first: an event of
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

use harness_core::{RawEvent, SessionId, fork};
use harness_protocol::MasterToNode;
use harness_store::SessionRow;

use crate::{CommandError, Live, Master, create_intent_key, lock};

/// `through_seq` of the chats this process has met, 0 for one that is no
/// fork: read from the mirror once per chat. A chat's lineage is written
/// with its first event and never changes, so the answer cannot go stale
/// (ids are not reused).
fn known() -> &'static Mutex<HashMap<SessionId, u64>> {
    static KNOWN: OnceLock<Mutex<HashMap<SessionId, u64>>> = OnceLock::new();
    KNOWN.get_or_init(Mutex::default)
}

/// More chats than this and the table starts again (it only saves reads).
const KNOWN_MAX: usize = 100_000;

/// Whether `e`, just taken into the mirror, is a copy of its fork's
/// parent's event. The fork's own first event is never one.
pub(crate) async fn copied(m: &Master, e: &RawEvent) -> bool {
    if e.seq < 2 {
        return false;
    }
    let sid = e.session_id;
    let cached = lock(known()).get(&sid).copied();
    let through = match cached {
        Some(t) => t,
        None => {
            let t = m
                .inner
                .mirror
                .call(move |s| s.fork_of(sid))
                .await
                .ok()
                .flatten()
                .map_or(0, |f| f.through_seq);
            let mut k = lock(known());
            if k.len() >= KNOWN_MAX {
                k.clear();
            }
            k.insert(sid, t);
            t
        }
    };
    fork::inherited(through, e.seq)
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
) -> Result<SessionId, CommandError> {
    let sid = SessionId::new();
    let node_id = parent.node_id.clone();
    let (owner, workspace) = (parent.owner_id.clone(), parent.workspace_id.clone());
    let pid = parent.session_id;
    let now = harness_core::now_ms();
    // Who asked, recorded before the node hears of it (as a create does):
    // the fork's first event that arrives after the ack, or after the ack
    // timed out, is claimed for them when it lands.
    let intent = (owner.is_some() || workspace.is_some()).then(|| {
        serde_json::json!({"node": node_id, "owner": owner, "workspace": workspace, "at": now})
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
    let title = fork::title(&parent.title);
    let sent = m
        .send_command(&node_id, |req_id| MasterToNode::Fork {
            req_id,
            session_id: sid,
            parent: pid,
            turn_id: turn_id.to_string(),
            title,
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
            if s.kv_get(&crate::auto_model_key(pid))?.is_some() {
                s.kv_set(&crate::auto_model_key(sid), "1")?;
            }
            let linked = match s.session_project(pid)? {
                Some(p) => s.link_new_session(sid, &p.project_id)?,
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
