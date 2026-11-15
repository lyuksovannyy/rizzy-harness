//! `recall` on the master (README "Earlier chats and memory search";
//! PLAN.md T55; ADOPTION_PLAN X.13): a node's tool asks, for the session it
//! runs, and this answers from the search index and the memory store.
//!
//! The scope comes from the session alone, never from the request: the
//! session's owner, its effective workspace, the memory its turn may read
//! (`projects::turn_memory`). Only top-level chats of that owner in that
//! workspace, never a temporary chat (`incognito`, never indexed either),
//! never a sub-agent's session, never the asking chat itself. A request is
//! answered only from the node that runs the session, while a turn of it
//! runs that `Master::prompt` granted the tool ([`Grants`]): not a callback
//! turn, not an untrusted (webhook) turn, not a temporary chat. Bounded per
//! turn and per hour.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use harness_core::SessionId;
use harness_core::recall::{
    DEFAULT_LIMIT, EXCERPT_CHARS, HitKind, MAX_LIMIT, MAX_QUERY_CHARS, RecallHit, RecallQuery,
    TITLE_CHARS, TOTAL_CHARS,
};
use harness_protocol::MasterToNode;
use harness_store::SessionRow;

use crate::{Master, lock, memory, projects, search};

/// Searches one turn may make (its sub-agents' included).
pub const PER_TURN: u32 = 8;
/// Searches one chat may make in an hour.
pub const PER_HOUR: usize = 40;
const HOUR: Duration = Duration::from_secs(3600);
/// Chats whose grants and counts are kept; past it the oldest go.
const KEPT: usize = 4096;

/// The one line a turn that may search is told, after the memory lines.
pub const CONTEXT_LINE: &str = "You can search the person's own earlier chats in this workspace \
     and their saved memory with the `recall` tool, when they refer to earlier work you were \
     not given here.";

/// Which chats' running turns may search, and how many searches each used.
#[derive(Default)]
pub struct Grants {
    turns: Mutex<HashMap<SessionId, u32>>,
    hours: Mutex<HashMap<SessionId, VecDeque<Instant>>>,
}

impl Grants {
    /// The turn about to start in `sid` may search (`true`) or may not.
    /// Returns what was recorded before, for [`Self::restore`].
    pub fn set(&self, sid: SessionId, allowed: bool) -> Option<u32> {
        let mut t = lock(&self.turns);
        if allowed {
            if t.len() >= KEPT && !t.contains_key(&sid) {
                t.clear();
            }
            t.insert(sid, 0)
        } else {
            t.remove(&sid)
        }
    }

    /// Put back what [`Self::set`] replaced (the prompt never reached the node).
    pub fn restore(&self, sid: SessionId, before: Option<u32>) {
        let mut t = lock(&self.turns);
        match before {
            Some(n) => t.insert(sid, n),
            None => t.remove(&sid),
        };
    }

    pub fn forget(&self, sid: SessionId) {
        lock(&self.turns).remove(&sid);
        lock(&self.hours).remove(&sid);
    }

    /// Take one search of `sid`'s turn, if its grant and the hour allow.
    fn take(&self, sid: SessionId) -> Result<(), String> {
        let mut t = lock(&self.turns);
        let Some(used) = t.get_mut(&sid) else {
            return Err("recall is not available in this turn".into());
        };
        if *used >= PER_TURN {
            return Err(format!(
                "this turn already searched {PER_TURN} times; work with what you found"
            ));
        }
        let mut h = lock(&self.hours);
        if h.len() >= KEPT && !h.contains_key(&sid) {
            h.clear();
        }
        let q = h.entry(sid).or_default();
        let now = Instant::now();
        while q.front().is_some_and(|t| now.duration_since(*t) > HOUR) {
            q.pop_front();
        }
        if q.len() >= PER_HOUR {
            return Err(format!(
                "this chat searched {PER_HOUR} times in the last hour; try again later"
            ));
        }
        q.push_back(now);
        *used += 1;
        Ok(())
    }
}

/// Withhold `recall` from an agent and every sub-agent it may hand work to.
pub fn deny(spec: &mut harness_core::agent::AgentSpec) {
    let deny = |list: &mut Vec<String>| {
        if !list.iter().any(|t| t == "recall") {
            list.push("recall".into());
        }
    };
    deny(&mut spec.deny_tools);
    for d in &mut spec.delegates {
        deny(&mut d.deny_tools);
    }
}

/// Answer a node's `recall.request` from a task of its own (a search never
/// holds up the node's events). The answer goes to that node only.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, q: RecallQuery) {
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let res = answer(&m, &node_id, sid, q).await;
        if let Err(e) = &res {
            tracing::info!(node = %node_id, session = %sid, why = %e, "recall refused");
        }
        let (hits, error) = match res {
            Ok(h) => (h, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        m.send_unacked(
            &node_id,
            MasterToNode::RecallResult {
                req_id,
                hits,
                error,
            },
        );
    });
}

/// The session a request names, checked against the node that sent it,
/// and the top-level chat it belongs to (a sub-agent's parent).
async fn sessions(
    m: &Master,
    node_id: &str,
    sid: SessionId,
) -> Result<(SessionRow, SessionRow), String> {
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
        tracing::warn!(node = %node_id, session = %sid, "a node asked to search for another node's session");
        return Err(refused());
    }
    let top = match row.parent_id {
        Some(_) => projects::parent_of(m, &row).await.ok_or_else(refused)?,
        None => row.clone(),
    };
    Ok((row, top))
}

pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    q: RecallQuery,
) -> Result<Vec<RecallHit>, String> {
    if !m.node_has(node_id, harness_protocol::CAP_RECALL) {
        return Err("this node does not offer recall".into());
    }
    let (row, top) = sessions(m, node_id, sid).await?;
    if row.incognito || top.incognito {
        return Err("a temporary chat does not search earlier chats".into());
    }
    let (Some(owner), Some(ws)) = (top.owner_id.clone(), top.workspace_id.clone()) else {
        return Err("this chat has no owner to search for".into());
    };
    if !row.running {
        return Err("recall is not available outside a turn".into());
    }
    // A sub-agent searches only when its own definition has the switch.
    if row.parent_id.is_some() {
        let r = m.resolver_for(row.workspace_id.as_deref()).await;
        let on = row
            .agent_id
            .as_deref()
            .and_then(|id| r.agents().get(id))
            .is_some_and(|a| a.features.recall);
        if !on {
            return Err("recall is not available to this agent".into());
        }
    }
    let query = harness_core::text::clean_line(&q.query, MAX_QUERY_CHARS);
    if query.is_empty() {
        return Err("give words to look for".into());
    }
    m.inner.recall.take(top.session_id)?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
    let chats = match q.scope.chats() {
        true => {
            chats(
                m,
                &query,
                &owner,
                &ws,
                [row.session_id, top.session_id],
                limit,
            )
            .await
        }
        false => Vec::new(),
    };
    let facts = match q.scope.memory() {
        true => facts(m, &row, &top, &query, limit).await,
        false => Vec::new(),
    };
    // Both kinds take turns, best first, so neither crowds the other out.
    let (mut c, mut f) = (chats.into_iter(), facts.into_iter());
    let mut hits = Vec::new();
    loop {
        let (a, b) = (c.next(), f.next());
        if a.is_none() && b.is_none() {
            break;
        }
        hits.extend(a);
        hits.extend(b);
    }
    Ok(capped(hits, limit))
}

/// The owner's top-level chats in the workspace that match, best first,
/// without `except`.
async fn chats(
    m: &Master,
    query: &str,
    owner: &str,
    ws: &str,
    except: [SessionId; 2],
    limit: usize,
) -> Vec<RecallHit> {
    let tokens = search::tokens(query);
    let Some(fts) = search::fts_query(&tokens, true) else {
        return Vec::new();
    };
    let (owner, ws) = (owner.to_string(), ws.to_string());
    let found = m
        .inner
        .mirror
        .call(move |s| s.recall_search(&fts, &owner, &ws, limit + except.len()))
        .await;
    match found {
        Ok(found) => found
            .into_iter()
            .filter(|h| !except.contains(&h.session_id))
            .take(limit)
            .map(|h| RecallHit {
                kind: HitKind::Chat,
                id: h.session_id.to_string(),
                title: h.title,
                when_ms: h.updated_ms,
                excerpt: h.snippet,
            })
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "recall: chat search failed");
            Vec::new()
        }
    }
}

/// Facts from the places the turn reads anyway: the owner's (where their
/// join answer and the agent's memory allow), the project's (in a project
/// chat with project memory on).
async fn facts(
    m: &Master,
    row: &SessionRow,
    top: &SessionRow,
    query: &str,
    limit: usize,
) -> Vec<RecallHit> {
    let Some(mem) = m.memory() else {
        return Vec::new();
    };
    let allowed = projects::turn_memory(m, row).await;
    let mut owners: Vec<Option<String>> = Vec::new();
    if allowed.personal {
        owners.extend(memory::places(top));
    }
    if allowed.project
        && let Some(p) = projects::project_of(m, row).await
    {
        owners.push(Some(projects::memory_owner(&p.project_id)));
    }
    let mut out = Vec::new();
    for o in owners {
        match mem.recall(o.as_deref(), query, limit).await {
            Ok(fs) => out.extend(fs.into_iter().map(|f| RecallHit {
                kind: HitKind::Memory,
                id: f.id,
                title: String::new(),
                when_ms: f.updated_ms,
                excerpt: f.text,
            })),
            Err(e) => tracing::warn!(error = %e, "recall: memory search failed"),
        }
    }
    out.truncate(limit);
    out
}

/// At most `limit` hits, each excerpt and title cut, all excerpts together
/// at most `TOTAL_CHARS`.
fn capped(hits: Vec<RecallHit>, limit: usize) -> Vec<RecallHit> {
    let cut = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let mut left = TOTAL_CHARS;
    let mut out = Vec::new();
    for mut h in hits.into_iter().take(limit) {
        if left == 0 {
            break;
        }
        h.excerpt = cut(&h.excerpt, EXCERPT_CHARS.min(left));
        h.title = cut(&h.title, TITLE_CHARS);
        left -= h.excerpt.chars().count();
        out.push(h);
    }
    out
}
