//! The credential for an agent's `push` (ADOPTION_PLAN X.27 step 4,
//! PLAN.md T101, `docs/forge-push.md` §5): a service the master answers
//! for the node that runs the session (PLAN.md §5). The master runs no git
//! and pushes nothing; it decides where a turn may push and hands one
//! token for one push the person allowed.
//!
//! **The offer** ([`offer`]), per turn the master sends: a project chat's
//! own turn (not a sub-agent's, a temporary or a forked chat, not parallel
//! answers or a list of called agents) that its owner sent signed in as
//! themselves (not the API token, a routine run, a webhook or a callback),
//! in a chat no one else acted in; the turn's agent names `push`; the
//! project has a recorded source whose host the server still allows; the
//! owner has a connection for that host (the newest one pushes); the node
//! announced [`harness_protocol::CAP_FORGE_PUSH`]. The target goes to the
//! node in the turn's context and is kept here for the chat, replaced by
//! the next turn's (or none).
//!
//! **The Allow** ([`Master::note_push_click`]): when the chat's owner,
//! signed in, answers a card of the tool `push` with an allow, the master
//! records that approval id for the chat, for [`CLICK_TTL`].
//!
//! **The request** ([`handle`]) names the session and the approval id
//! only. It is answered with the token only when: the requesting node runs
//! the session; the session is a top-level chat with an open turn that is
//! not a callback, whose agent has the push switch; the master relayed an
//! Allow of that very card from the owner (used once here); the chat's
//! target is still the owner's connection for the project's source host,
//! still allowed; the owner's account, membership and level still let the
//! chat run; one request per chat at a time and [`PER_HOUR`] per chat in
//! any hour. The token is opened with the key proven; it travels in the
//! answer only, as a redacted secret; one audit line, never the token.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use harness_core::SessionId;
use harness_core::forge::{PUSH_TOOL, PushTarget};
use harness_protocol::MasterToNode;
use harness_store::SessionRow;

use crate::{Master, lock};

/// Credentials one chat may be handed in any hour.
pub const PER_HOUR: usize = 20;
/// How long an Allow the master relayed may be used.
pub const CLICK_TTL: Duration = Duration::from_secs(10 * 60);
/// Chats tracked in each table; past it the oldest are forgotten (fails
/// closed: a forgotten offer or click hands out nothing).
const KEPT: usize = 4096;

/// What the master keeps for pushes.
#[derive(Default)]
pub struct State {
    /// The target named in each chat's latest turn, and its owner.
    offers: Mutex<HashMap<SessionId, (PushTarget, String)>>,
    /// Allows of push cards the master relayed from the chat's owner, not
    /// yet used: (chat, approval id) → when.
    clicks: Mutex<HashMap<(SessionId, String), Instant>>,
    /// Chats with a request being answered.
    busy: Mutex<HashSet<SessionId>>,
    /// Credentials handed out per chat in the last hour.
    hour: Mutex<HashMap<SessionId, VecDeque<Instant>>>,
}

impl State {
    fn offered(&self, sid: SessionId, t: Option<(PushTarget, String)>) {
        let mut o = lock(&self.offers);
        match t {
            Some(t) => {
                if o.len() >= KEPT && !o.contains_key(&sid) {
                    o.clear();
                }
                o.insert(sid, t);
            }
            None => {
                o.remove(&sid);
            }
        }
    }

    fn click(&self, sid: SessionId, approval_id: String) {
        let mut c = lock(&self.clicks);
        c.retain(|_, at| at.elapsed() < CLICK_TTL);
        if c.len() >= KEPT {
            return;
        }
        c.insert((sid, approval_id), Instant::now());
    }

    /// Take the click once: `true` when it was there and fresh.
    fn take_click(&self, sid: SessionId, approval_id: &str) -> bool {
        lock(&self.clicks)
            .remove(&(sid, approval_id.to_string()))
            .is_some_and(|at| at.elapsed() < CLICK_TTL)
    }

    fn room_this_hour(&self, sid: SessionId) -> bool {
        let mut h = lock(&self.hour);
        if h.len() >= KEPT && !h.contains_key(&sid) {
            h.retain(|_, q| {
                q.back()
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(3600))
            });
        }
        let q = h.entry(sid).or_default();
        while q
            .front()
            .is_some_and(|t| t.elapsed() >= Duration::from_secs(3600))
        {
            q.pop_front();
        }
        if q.len() >= PER_HOUR {
            return false;
        }
        q.push_back(Instant::now());
        true
    }

    /// Tests: the chat's target as kept.
    pub fn offer_of(&self, sid: SessionId) -> Option<PushTarget> {
        lock(&self.offers).get(&sid).map(|(t, _)| t.clone())
    }
}

/// Who sent a turn and how, for [`offer`].
pub struct Sender {
    /// The chat's owner, signed in as themselves.
    pub by_owner: bool,
    pub routine: bool,
    pub untrusted: bool,
    pub resume: bool,
    pub foreign: bool,
    /// Parallel answers or a list of called agents.
    pub several: bool,
}

/// The target this turn may push to (`None`: the tool is not offered), and
/// kept as the chat's (replacing an earlier turn's).
pub async fn offer(
    m: &Master,
    row: &SessionRow,
    who: Sender,
    project: Option<&harness_store::ProjectRow>,
    agent: &harness_core::agent::AgentSpec,
) -> Option<PushTarget> {
    let t = decide(m, row, who, project, agent).await;
    m.inner.pushes.offered(row.session_id, t.clone());
    t.map(|(t, _)| t)
}

async fn decide(
    m: &Master,
    row: &SessionRow,
    who: Sender,
    project: Option<&harness_store::ProjectRow>,
    agent: &harness_core::agent::AgentSpec,
) -> Option<(PushTarget, String)> {
    if !who.by_owner
        || who.routine
        || who.untrusted
        || who.resume
        || who.foreign
        || who.several
        || row.parent_id.is_some()
        || row.incognito
        || row.profile.as_deref() == Some("chat")
        || !agent.allows_tool(PUSH_TOOL)
        || !m.node_has(&row.node_id, harness_protocol::CAP_FORGE_PUSH)
    {
        return None;
    }
    let owner = row.owner_id.clone()?;
    let (host, full_name) = project?.source.clone()?;
    if !m.forge_hosts().iter().any(|h| h.host == host) {
        return None;
    }
    let sid = row.session_id;
    let (o, h) = (owner.clone(), host.clone());
    let found = m
        .inner
        .mirror
        .call(move |s| {
            if s.fork_of(sid)?.is_some() {
                return Ok::<_, harness_store::StoreError>(None);
            }
            Ok(s.connections_of(&o)?
                .into_iter()
                .filter(|c| c.host == h)
                .max_by_key(|c| (c.created_ms, c.connection_id.clone())))
        })
        .await
        .ok()
        .flatten()?;
    Some((
        PushTarget {
            host,
            full_name,
            connection_id: found.connection_id,
            login: found.login,
        },
        owner,
    ))
}

impl Master {
    /// A person answered a card with an allow (the HTTP route, after it
    /// relayed the answer): recorded when it is the chat's owner signed in
    /// and the card was a `push` card of that chat.
    pub async fn note_push_click(&self, who: Option<&str>, row: &SessionRow, approval_id: &str) {
        let (Some(who), Some(owner)) = (who, row.owner_id.as_deref()) else {
            return;
        };
        if who != owner || approval_id.is_empty() || approval_id.len() > 128 {
            return;
        }
        let (sid, a) = (row.session_id, approval_id.to_string());
        let tool = self
            .inner
            .mirror
            .call(move |s| s.approval_tool(sid, &a))
            .await
            .ok()
            .flatten();
        if tool.as_deref() == Some(PUSH_TOOL) {
            self.inner.pushes.click(sid, approval_id.to_string());
        }
    }
}

/// Answer a node's `forge.push_credential` from a task of its own; the
/// answer goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, approval_id: String) {
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let a = answer(&m, &node_id, sid, &approval_id).await;
        if let Err(e) = &a {
            tracing::info!(node = %node_id, session = %sid, why = %e, "push credential refused");
        }
        let (username, token, error) = match a {
            Ok((u, t)) => (Some(u), Some(t), None),
            Err(e) => (None, None, Some(e)),
        };
        m.send_unacked(
            &node_id,
            MasterToNode::PushCredentialResult {
                req_id,
                username,
                token,
                error,
            },
        );
    });
}

/// Releases a chat's request slot however the answer ends.
struct Busy<'a>(&'a State, SessionId);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        lock(&self.0.busy).remove(&self.1);
    }
}

/// The login and token for one push, or why not.
pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    approval_id: &str,
) -> Result<(String, harness_core::Secret<String>), String> {
    let st = &m.inner.pushes;
    if !m.node_has(node_id, harness_protocol::CAP_FORGE_PUSH) {
        return Err("this node does not push".into());
    }
    let refused = || "no such session on this node".to_string();
    let row = m
        .inner
        .mirror
        .call(move |s| s.session(sid))
        .await
        .ok()
        .flatten()
        .ok_or_else(refused)?;
    if row.node_id != node_id {
        tracing::warn!(node = %node_id, session = %sid, "a node asked for a push credential of another node's session");
        return Err(refused());
    }
    if row.parent_id.is_some() || row.incognito {
        return Err("push is only for a project chat's own turn".into());
    }
    // One at a time per chat.
    if !lock(&st.busy).insert(sid) {
        return Err("another push of this chat is being answered".into());
    }
    let _busy = Busy(st, sid);
    // The Allow the master relayed, once.
    if !st.take_click(sid, approval_id) {
        return Err(
            "no allow of this card reached the server from the chat's owner (or it was \
             used or is older than ten minutes)"
                .into(),
        );
    }
    let (target, owner) = lock(&st.offers)
        .get(&sid)
        .cloned()
        .ok_or("this chat's turn has no push target")?;
    if row.owner_id.as_deref() != Some(owner.as_str()) {
        return Err("the chat's owner changed".into());
    }
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("push is not available outside a turn")?;
    if open.callback || open.withheld.iter().any(|t| t == PUSH_TOOL) {
        return Err("push is not available in this turn".into());
    }
    let r = m.resolver_for(row.workspace_id.as_deref()).await;
    if !open
        .agent_id
        .as_deref()
        .and_then(|id| r.agents().get(id))
        .is_some_and(|a| a.features.push)
    {
        return Err("push is not available to this agent".into());
    }
    if m.chat_foreign(sid).await {
        return Err("someone other than the chat's owner acted in it".into());
    }
    // The owner as they are now: the account, the membership, the level.
    let o = owner.clone();
    let user = m
        .inner
        .mirror
        .call(move |s| s.user_by_id(&o))
        .await
        .ok()
        .flatten()
        .ok_or("the chat's owner no longer exists")?;
    if user.disabled {
        return Err("the chat's owner's account is disabled".into());
    }
    let who = crate::auth::Principal::from_user(user);
    if let Some(why) = crate::workspaces::agentic_blocked(m, &who, &row).await {
        return Err(why);
    }
    // The project and its source as they are now.
    let project = crate::projects::project_of(m, &row)
        .await
        .ok_or("this chat is no longer in a project")?;
    if project.source.as_ref() != Some(&(target.host.clone(), target.full_name.clone())) {
        return Err("the project's source changed".into());
    }
    if !m.forge_hosts().iter().any(|h| h.host == target.host) {
        return Err("this server no longer allows that forge host".into());
    }
    let (o, c) = (owner.clone(), target.connection_id.clone());
    let conn = m
        .inner
        .mirror
        .call(move |s| s.connection(&o, &c))
        .await
        .ok()
        .flatten()
        .ok_or("the connection was removed")?;
    if conn.host != target.host {
        return Err("the connection is for another host".into());
    }
    if !st.room_this_hour(sid) {
        return Err(format!(
            "this chat had {PER_HOUR} push credentials in the last hour; try again later"
        ));
    }
    let token = m
        .connection_token(&owner, &target.connection_id)
        .await
        .map_err(|_| "the server's sealed storage could not open the token".to_string())?
        .ok_or("the connection was removed")?;
    crate::levels::audit(
        m,
        "forge.push_credential",
        serde_json::json!({
            "user_id": owner,
            "session_id": sid,
            "connection_id": target.connection_id,
            "host": target.host,
            "full_name": target.full_name,
        }),
    )
    .await;
    Ok((conn.login, token))
}
