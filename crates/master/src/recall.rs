//! `recall` on the master (README "Earlier chats and memory search";
//! PLAN.md T55; ADOPTION_PLAN X.13): a node's tool asks, for the session it
//! runs, and this answers from the search index and the memory store.
//!
//! The scope comes from the session alone, never from the request: the
//! session's owner, its effective workspace, the memory its turn may read
//! (`projects::turn_memory`). Only top-level chats of that owner in that
//! workspace, never a temporary chat (`incognito`, never indexed either),
//! never a sub-agent's session, never the asking chat itself.
//!
//! Whether the asking turn may search is read from the log, at the time of
//! the request, never from a flag the last prompt left: the session's open
//! turn ([`harness_store::Store::open_turn`]) must not be a callback turn,
//! must not have `recall` withheld (`AgentRef::withheld`, which records
//! what `Master::prompt` denied that turn: untrusted turns, temporary
//! chats), and its agent must have the switch; a sub-agent's parent turn
//! must pass the same. Counted per turn id and per chat per hour.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_core::SessionId;
use harness_core::recall::{
    DEFAULT_LIMIT, EXCERPT_CHARS, HitKind, MAX_LIMIT, MAX_QUERY_CHARS, RecallHit, RecallQuery,
    TITLE_CHARS, TOTAL_CHARS,
};
use harness_protocol::MasterToNode;
use harness_store::{OpenTurn, SessionRow};

use crate::{Master, lock, memory, projects, search};

/// Searches one turn may make (its sub-agents' included).
pub const PER_TURN: u32 = 8;
/// Searches one chat may make in an hour.
pub const PER_HOUR: usize = 40;
/// Searches one node may have in progress on the master; past it, refused.
pub const PER_NODE: usize = 4;
/// Words a query is searched by, at most.
const MAX_TERMS: usize = 8;
const HOUR: Duration = Duration::from_secs(3600);
/// Chats whose counts are kept; past it the oldest go.
const KEPT: usize = 4096;

/// The one line a turn that may search is told, after the memory lines.
pub const CONTEXT_LINE: &str = "You can search the person's own earlier chats in this workspace \
     and their saved memory with the `recall` tool, when they refer to earlier work you were \
     not given here.";

/// Searches used: per chat, the turn they were counted for and how many;
/// per chat, when in the last hour; and searches in progress per node.
#[derive(Default)]
pub struct Grants {
    turns: Mutex<HashMap<SessionId, (String, u32)>>,
    hours: Mutex<HashMap<SessionId, VecDeque<Instant>>>,
    nodes: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// Refusals already logged at info, per node (later ones at debug).
    told: Mutex<std::collections::HashSet<String>>,
}

impl Grants {
    pub fn forget(&self, sid: SessionId) {
        lock(&self.turns).remove(&sid);
        lock(&self.hours).remove(&sid);
    }

    /// Take one search of turn `turn` of chat `sid`, if the turn's count
    /// and the chat's hour allow.
    fn take(&self, sid: SessionId, turn: &str) -> Result<(), String> {
        let mut t = lock(&self.turns);
        if t.len() >= KEPT && !t.contains_key(&sid) {
            t.clear();
        }
        let entry = t.entry(sid).or_insert_with(|| (turn.to_string(), 0));
        if entry.0 != turn {
            *entry = (turn.to_string(), 0);
        }
        if entry.1 >= PER_TURN {
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
        entry.1 += 1;
        Ok(())
    }

    /// Give back a search whose answer never reached the node.
    fn give_back(&self, sid: SessionId, turn: &str) {
        if let Some(e) = lock(&self.turns).get_mut(&sid)
            && e.0 == turn
        {
            e.1 = e.1.saturating_sub(1);
        }
        if let Some(q) = lock(&self.hours).get_mut(&sid) {
            q.pop_back();
        }
    }

    fn node(&self, node_id: &str) -> Arc<tokio::sync::Semaphore> {
        lock(&self.nodes)
            .entry(node_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(PER_NODE)))
            .clone()
    }

    /// The first refusal per node is logged at info, the rest at debug.
    fn first_refusal(&self, node_id: &str) -> bool {
        let mut t = lock(&self.told);
        if t.len() >= KEPT {
            t.clear();
        }
        t.insert(node_id.to_string())
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
/// holds up the node's events), at most [`PER_NODE`] at once per node. The
/// answer goes to that node only; a search whose answer could not be sent
/// is not counted.
pub fn handle(m: &Master, node_id: &str, req_id: String, sid: SessionId, q: RecallQuery) {
    let (m, node_id) = (m.clone(), node_id.to_string());
    let permit = m.inner.recall.node(&node_id).try_acquire_owned();
    tokio::spawn(async move {
        let res = match permit {
            Ok(_permit) => answer(&m, &node_id, sid, q).await,
            Err(_) => Err(Refusal::plain(
                "too many searches from this node at once; try again",
            )),
        };
        let (hits, error, counted) = match res {
            Ok((h, counted)) => (h, None, Some(counted)),
            Err(e) => {
                if m.inner.recall.first_refusal(&node_id) {
                    tracing::info!(node = %node_id, session = %sid, why = %e.0, "recall refused");
                } else {
                    tracing::debug!(node = %node_id, session = %sid, why = %e.0, "recall refused");
                }
                (Vec::new(), Some(e.0), None)
            }
        };
        let sent = m.send_unacked(
            &node_id,
            MasterToNode::RecallResult {
                req_id,
                hits,
                error,
            },
        );
        if !sent && let Some((chat, turn)) = counted {
            m.inner.recall.give_back(chat, &turn);
        }
    });
}

/// Why a search was refused, as the tool shows it.
#[derive(Debug)]
pub struct Refusal(pub String);

impl Refusal {
    fn plain(s: &str) -> Self {
        Refusal(s.to_string())
    }
}

impl From<String> for Refusal {
    fn from(s: String) -> Self {
        Refusal(s)
    }
}

/// The session a request names, checked against the node that sent it,
/// and the top-level chat it belongs to (a sub-agent's parent). A
/// sub-agent of a sub-agent is refused (delegation is one level deep).
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
    if top.parent_id.is_some() {
        return Err("recall is not available to a sub-agent's sub-agent".into());
    }
    Ok((row, top))
}

/// The open turn of `sid`, if it may search: not a callback turn, `recall`
/// not withheld from it, its agent with the switch (`check_agent`).
async fn turn_may_search(
    m: &Master,
    row: &SessionRow,
    check_agent: bool,
) -> Result<OpenTurn, String> {
    let sid = row.session_id;
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("recall is not available outside a turn")?;
    if open.callback || open.withheld.iter().any(|t| t == "recall") {
        return Err("recall is not available in this turn".into());
    }
    if check_agent {
        let r = m.resolver_for(row.workspace_id.as_deref()).await;
        let on = open
            .agent_id
            .as_deref()
            .and_then(|id| r.agents().get(id))
            .is_some_and(|a| a.features.recall);
        if !on {
            return Err("recall is not available to this agent".into());
        }
    }
    Ok(open)
}

/// The FTS5 terms of a query (PLAN.md T55): its words of at least two
/// characters, without the common English words when other words remain;
/// an English word loses `ing`, `ed`, `es` or `s` when four characters or
/// more are left, a word with other letters keeps its first characters
/// (all but the last two, at least four), so "decisions", "deploying" and
/// "пятница" find "decision", "deploy" and "пятницу"; every term is a
/// prefix. At most [`MAX_TERMS`]. Words are letters and digits only, so
/// the quotes cannot be broken and no query syntax passes.
pub fn terms(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut words: Vec<&str> = Vec::new();
    for w in lower.split(|c: char| !c.is_alphanumeric()) {
        if w.chars().count() >= 2 && !words.contains(&w) {
            words.push(w);
        }
    }
    let content: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !search::STOPWORDS.contains(w))
        .collect();
    let words = if content.is_empty() { words } else { content };
    let mut out: Vec<String> = Vec::new();
    for w in words {
        let stem = stem(w);
        let t = format!("\"{stem}\"*");
        if !out.contains(&t) {
            out.push(t);
        }
        if out.len() >= MAX_TERMS {
            break;
        }
    }
    out
}

fn stem(w: &str) -> String {
    if w.is_ascii() {
        for suffix in ["ing", "ed", "es", "s"] {
            if let Some(rest) = w.strip_suffix(suffix)
                && rest.len() >= 4
            {
                return rest.to_string();
            }
        }
        return w.to_string();
    }
    let n = w.chars().count();
    if n > 5 {
        w.chars().take((n - 2).max(4)).collect()
    } else {
        w.to_string()
    }
}

pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    q: RecallQuery,
) -> Result<(Vec<RecallHit>, (SessionId, String)), Refusal> {
    if !m.node_has(node_id, harness_protocol::CAP_RECALL) {
        return Err(Refusal::plain("this node does not offer recall"));
    }
    let (row, top) = sessions(m, node_id, sid).await?;
    if row.incognito || top.incognito {
        return Err(Refusal::plain(
            "a temporary chat does not search earlier chats",
        ));
    }
    let (Some(owner), Some(ws)) = (top.owner_id.clone(), top.workspace_id.clone()) else {
        return Err(Refusal::plain("this chat has no owner to search for"));
    };
    // The asking turn, from the log; a sub-agent's parent turn too (its
    // task came from there).
    let own = turn_may_search(m, &row, true).await?;
    let counted = match row.parent_id {
        Some(_) => turn_may_search(m, &top, false).await?.turn_id,
        None => own.turn_id,
    };
    let query = harness_core::text::clean_line(&q.query, MAX_QUERY_CHARS);
    let terms = terms(&query);
    // Refused before it is counted.
    if terms.is_empty() {
        return Err(Refusal::plain(
            "give a few distinctive words to look for (names, terms), at least two letters each",
        ));
    }
    m.inner.recall.take(top.session_id, &counted)?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
    let chats = match q.scope.chats() {
        true => {
            chats(
                m,
                &terms,
                &owner,
                &ws,
                [row.session_id, top.session_id],
                limit,
            )
            .await
        }
        false => Vec::new(),
    };
    // The memory store matches words its own way.
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
    Ok((scrubbed(m, capped(hits, limit)), (top.session_id, counted)))
}

/// The master's own secret set over every title and excerpt: a secret the
/// asking node does not know of must not reach its model or its log.
fn scrubbed(m: &Master, hits: Vec<RecallHit>) -> Vec<RecallHit> {
    match m.provider_runtime() {
        Some(rt) => scrub_hits(&rt.secrets, hits),
        None => hits,
    }
}

fn scrub_hits(secrets: &harness_core::SecretSet, mut hits: Vec<RecallHit>) -> Vec<RecallHit> {
    for h in &mut hits {
        h.title = secrets.scrub(&h.title);
        h.excerpt = secrets.scrub(&h.excerpt);
    }
    hits
}

/// The owner's top-level chats in the workspace that match, best first,
/// without `except`.
async fn chats(
    m: &Master,
    terms: &[String],
    owner: &str,
    ws: &str,
    except: [SessionId; 2],
    limit: usize,
) -> Vec<RecallHit> {
    let (terms, owner, ws) = (terms.to_vec(), owner.to_string(), ws.to_string());
    let found = m
        .inner
        .mirror
        .call(move |s| s.recall_search(&terms, &owner, &ws, except, limit))
        .await;
    match found {
        Ok(found) => found
            .into_iter()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Review R8: the master's secrets never leave in an answer.
    #[test]
    fn answers_are_scrubbed_with_the_masters_secrets() {
        let secrets = harness_core::SecretSet::default();
        secrets.add(harness_core::Secret::new("sk-master-only-4242".into()));
        let hit = RecallHit {
            kind: HitKind::Chat,
            id: "x".into(),
            title: "key sk-master-only-4242".into(),
            when_ms: 0,
            excerpt: "use sk-master-only-4242 here".into(),
        };
        let out = scrub_hits(&secrets, vec![hit]);
        assert!(
            !out[0].title.contains("4242") && !out[0].excerpt.contains("4242"),
            "{out:?}"
        );
    }

    /// Review R5: the terms a query is searched by.
    #[test]
    fn terms_stem_prefix_and_drop_common_words() {
        assert_eq!(
            terms("what decisions did we make about deploying?"),
            [
                "\"decision\"*",
                "\"did\"*",
                "\"make\"*",
                "\"about\"*",
                "\"deploy\"*"
            ]
        );
        assert_eq!(terms("пятница"), ["\"пятни\"*"]);
        assert_eq!(terms("what is it"), ["\"what\"*", "\"is\"*", "\"it\"*"]);
        assert!(terms("a* ? -").is_empty());
        assert_eq!(terms("\"x\" OR y*"), ["\"or\"*"]);
    }
}
