//! Agents calling a custom connector's tools (ADOPTION_PLAN X.35 step 2,
//! PLAN.md T85, README "Custom connectors"): a service the master answers
//! (PLAN.md §5) with the chat owner's own account.
//!
//! **Which turns.** [`Master::offer_connectors`] decides, for each turn the
//! master sends, whether it may use connectors: only a message the chat's
//! owner sent signed in as themselves, a run of the owner's own routine
//! (`board::routine_turn`'s rule) or the owner's Retry; never an
//! API-token, callback or untrusted (webhook) turn. The decision goes with
//! the send into `grant`, which binds it to the ONE turn the node's log
//! shows that send started, and takes it away for good when anything else
//! is delivered into that turn; a turn the master sent nothing for, or one
//! after a restart, has none. The turn's list holds the connectors of the
//! chat's workspace the owner can use: sign-in `none`, or `token` with the
//! owner's account connected and the key file usable; each connector's
//! tools from the person's cached set ([`super::tools`]), never fetched on
//! the turn's path.
//!
//! **A call** ([`handle`]) names the session, the public tool name and its
//! arguments. It is answered from the session alone: the node must run it
//! (a sub-agent's resolves to its chat), the chat's turn must be one the
//! master granted, the open turn must not be a callback turn, its agent
//! (the sub-agent's own) must have MCP and not have the connector denied,
//! the connector must be the chat's workspace's under the name in the
//! tool, and the tool one the master listed for the owner's account and
//! the connector as it is now (its stamp). Then the owner's sealed token is
//! opened (the key proven) and ONE `tools/call` is made by the step-1
//! client rules, on a pool of its own ([`WORKERS`], [`PER_KEY`] per person
//! and per connector, refused when busy), within [`CALL_BYTES`] and the
//! call's time limit ([`CALL_TIMEOUT`]), [`PER_TURN`] per granted turn and
//! [`PER_HOUR`] per chat. The
//! result is text only (`harness_tools::mcp::render`, as for the node's
//! own MCP calls). Nothing of the token, the arguments or the result is
//! logged; no audit line per call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::SessionId;
use harness_core::agent::{AgentSpec, mcp_server_of};
use harness_core::connectors::{
    ConnectorAnswer, ConnectorToolEntry, MAX_ARGUMENT_BYTES, MAX_LISTED,
};
use harness_protocol::{CAP_CONNECTORS, MasterToNode};
use harness_store::custom_connectors::ConnectorRow;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::client::{Budget, Failure, TokenHeader};
use super::pool::{Busy, Pool};
use super::tools::REFRESH_AFTER;
use crate::sealed::Binding;
use crate::{Master, forge, lock};

/// Call slots: calls that may run at once on this server (on the one
/// connector thread, `pool`).
pub const WORKERS: usize = 4;
/// Calls at once of one person, and of one connector (everyone's).
pub const PER_KEY: usize = 2;
/// Bytes one call may read, its `initialize`, its `tools/call` and their
/// notifications together, counted on the wire before any parse (and no
/// message over `client::MAX_JSON_VALUES` values reaches the parser). The
/// node shows at most `harness_tools::MAX_OUTPUT_CHARS` of the text.
pub const CALL_BYTES: usize = 256 * 1024;
/// How long one call may take, connecting included.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Calls one granted turn may make, its sub-agents' included, and one
/// chat in any hour (`grant`).
pub use super::grant::{AFTER_END, PER_HOUR, PER_TURN};
/// Calls one node may have in progress on the master; past it, refused.
pub const PER_NODE: usize = 8;

/// Master-side state of agents' connector calls.
pub struct State {
    pool: Pool,
    /// Which turns may call, and their counts (`grant`).
    grants: Mutex<super::grant::Grants>,
    nodes: Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// Calls in progress, by (node, request id), to drop on a cancel.
    cancels: Mutex<HashMap<(String, String), CancellationToken>>,
    timeout: Mutex<Duration>,
    /// Calls sent to a connector (tests: a refusal sends none).
    sent: AtomicUsize,
}

impl State {
    /// Calls on `thread` (the checks' too).
    pub fn on(thread: Arc<super::pool::Thread>) -> Self {
        Self {
            pool: Pool::on(thread, WORKERS, PER_KEY),
            grants: Mutex::default(),
            nodes: Mutex::default(),
            cancels: Mutex::default(),
            timeout: Mutex::new(CALL_TIMEOUT),
            sent: AtomicUsize::new(0),
        }
    }
}

impl State {
    fn node(&self, node_id: &str) -> Arc<tokio::sync::Semaphore> {
        lock(&self.nodes)
            .entry(node_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(PER_NODE)))
            .clone()
    }

    pub fn forget(&self, sid: SessionId) {
        lock(&self.grants).forget(sid);
    }

    /// The master is about to send `what` for chat `sid` (`owner`: a turn
    /// that may use connectors); the id for [`Self::unsent`].
    pub fn sent(&self, sid: SessionId, what: super::grant::What, owner: bool) -> u64 {
        lock(&self.grants).sent(sid, what, owner)
    }

    /// That send was refused or never made.
    pub fn unsent(&self, sid: SessionId, id: u64) {
        lock(&self.grants).unsent(sid, id)
    }

    /// An event the mirror took.
    pub fn on_event(&self, e: &harness_core::RawEvent) {
        if matches!(
            e.kind.as_str(),
            "session_created" | "turn_started" | "user_message" | "turn_ended"
        ) {
            lock(&self.grants).on_event(e)
        }
    }
}

/// Whether a spec of this message may be offered `e`: its agent may use
/// MCP tools and this one (its own `deny_tools`, `deny_mcp_servers`).
fn may_use(s: &AgentSpec, e: &ConnectorToolEntry) -> bool {
    s.allows_mcp() && s.allows_tool(&e.name)
}

/// For a node without [`CAP_CONNECTORS`]: no list.
pub fn strip(msg: &mut MasterToNode) {
    let spec = match msg {
        MasterToNode::Create { agent, .. }
        | MasterToNode::Prompt { agent, .. }
        | MasterToNode::Callback { agent, .. }
        | MasterToNode::Retry { agent, .. } => agent.as_mut(),
        MasterToNode::SetAgent { agent, .. } => Some(agent),
        _ => None,
    };
    if let Some(spec) = spec {
        spec.connector_tools.clear();
        for d in &mut spec.delegates {
            d.connector_tools.clear();
        }
    }
}

/// The digest a tool set is bound to: of the sealed value, never the
/// token.
pub(crate) fn blob_hash(blob: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(blob))[..32].to_string()
}

fn binding<'a>(owner: &'a str, fields: &'a [&'a str; 3]) -> Binding<'a> {
    Binding {
        table: super::ACCOUNTS_TABLE,
        owner,
        fields,
    }
}

impl Master {
    /// Whether the turn the master is about to send for chat `row` may use
    /// connectors (`may`: the owner's own signed-in turn, a run of their
    /// routine or their Retry; not untrusted, not a callback) and, when it
    /// may, put on `spec` the tools any spec of the message may be offered.
    /// Returns that decision, which the caller records with the send
    /// (`State::sent`), and the names of connectors whose tools are not read
    /// yet (being read now, in the background), for one line the model is
    /// told. A node without [`CAP_CONNECTORS`] loses the list in
    /// `send_command` (`strip`) and is told nothing here.
    pub async fn offer_connectors(
        &self,
        row: &harness_store::SessionRow,
        may: bool,
        spec: &mut AgentSpec,
    ) -> (bool, Vec<String>) {
        spec.connector_tools.clear();
        let may = may && row.parent_id.is_none();
        let (Some(owner), Some(ws)) = (row.owner_id.as_deref(), row.workspace_id.as_deref()) else {
            return (false, Vec::new());
        };
        if !may {
            return (false, Vec::new());
        }
        let wanted = |e: &ConnectorToolEntry| {
            may_use(spec, e) || spec.delegates.iter().any(|d| may_use(d, e))
        };
        let (list, waiting) = self.connector_tools_for(owner, ws).await;
        let mut text = 0usize;
        let mut out = Vec::new();
        for e in list.into_iter().filter(|e| wanted(e)) {
            let size = e.name.len() + e.description.len() + e.schema.len();
            if out.len() >= MAX_LISTED || text + size > harness_tools::mcp::MAX_TOOL_TEXT {
                break;
            }
            text += size;
            out.push(e);
        }
        spec.connector_tools = out;
        if !self.node_has(&row.node_id, CAP_CONNECTORS) {
            return (true, Vec::new());
        }
        // Only connectors some spec of the message could use.
        let reading = waiting
            .into_iter()
            .filter(|name| {
                let probe = format!("{}{name}__x", harness_core::agent::MCP_PREFIX);
                std::iter::once(&*spec)
                    .chain(spec.delegates.iter())
                    .any(|s| s.allows_mcp() && s.allows_tool(&probe))
            })
            .collect();
        (true, reading)
    }

    /// The tools `owner` can be offered of workspace `ws`'s connectors now,
    /// from the cache, and the names of those not read yet. Reads that are
    /// missing or old start in the background; nothing here waits for a
    /// remote server.
    async fn connector_tools_for(
        &self,
        owner: &str,
        ws: &str,
    ) -> (Vec<ConnectorToolEntry>, Vec<String>) {
        let (o, w) = (owner.to_string(), ws.to_string());
        let read = self
            .inner
            .mirror
            .call(move |s| {
                Ok::<_, harness_store::StoreError>((
                    s.connectors_of(&w)?,
                    s.connector_accounts_of(&o, &w)?,
                ))
            })
            .await;
        let (rows, accounts) = match read {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "could not read a workspace's connectors");
                return (Vec::new(), Vec::new());
            }
        };
        if rows.is_empty() {
            return (Vec::new(), Vec::new());
        }
        // The key file must open tokens now: with none, or a wrong one, no
        // token connector is offered.
        let keys_ok = match self.sealer() {
            Some(s) => match forge::stored(self).await {
                Ok(st) => s.check(&st).is_ok(),
                Err(_) => false,
            },
            None => false,
        };
        let mut list = Vec::new();
        let mut waiting = Vec::new();
        for r in rows {
            if r.auth == "token"
                && (!keys_ok || !accounts.iter().any(|a| a.connector_id == r.connector_id))
            {
                continue;
            }
            let stamp = super::stamp(&r);
            let cached = {
                let c = lock(&self.inner.connectors.tools);
                c.get(owner, &r.connector_id, &stamp)
                    .map(|(age, t)| (age, t.iter().map(|t| t.entry.clone()).collect::<Vec<_>>()))
            };
            match cached {
                Some((age, entries)) => {
                    if age >= REFRESH_AFTER {
                        self.fill_connector_tools(owner, &r);
                    }
                    list.extend(entries);
                }
                None => {
                    self.fill_connector_tools(owner, &r);
                    waiting.push(r.name.clone());
                }
            }
        }
        (list, waiting)
    }

    /// Read `owner`'s tools of connector `r` in the background (on the
    /// check workers, as a check of theirs, under their own budget of
    /// background reads), at most once per `tools::FILL_GAP`. A busy pool
    /// or a failure leaves the cache as it was; the next turn tries again.
    pub(crate) fn fill_connector_tools(&self, owner: &str, r: &ConnectorRow) {
        if !lock(&self.inner.connectors.tools).may_fill(owner, &r.connector_id) {
            return;
        }
        let (m, owner, r) = (self.clone(), owner.to_string(), r.clone());
        tokio::spawn(async move {
            // A budget of their own (review of PR 157, L9): failed
            // background reads never use up the one a person's Connect and
            // an admin's Check need.
            if m.inner
                .connectors
                .fill_attempts
                .admit(&owner, &r.connector_id)
                .is_err()
            {
                return;
            }
            let (token, bound) = match r.auth.as_str() {
                "token" => match m.owner_token(&owner, &r).await {
                    Ok((t, b)) => (Some(t), Some(b)),
                    Err(_) => return,
                },
                _ => (None, None),
            };
            let net = m.connector_net();
            // Under a key of its own, so it never holds the person's own
            // check (one at a time per person).
            let Ok(checked) = super::check(
                m.connector_pool(),
                &format!("fill:{owner}"),
                &r.name,
                &r.url,
                token,
                &net,
            )
            .await
            else {
                return;
            };
            match checked.result {
                Ok(offer) => {
                    // Only if the connector is still what it was read for,
                    // and the token the one it was read with (review of PR
                    // 157, L8: a Replace meanwhile wins).
                    let (w, c) = (r.workspace_id.clone(), r.connector_id.clone());
                    let now = m.inner.mirror.call(move |s| s.connector(&w, &c)).await;
                    let same_token = match &bound {
                        Some(b) => m.sealed_hash(&owner, &r).await.as_ref() == Some(b),
                        None => true,
                    };
                    if let Ok(Some(now)) = now
                        && super::stamp(&now) == super::stamp(&r)
                        && same_token
                    {
                        lock(&m.inner.connectors.tools).put(
                            &owner,
                            &r.connector_id,
                            &super::stamp(&r),
                            bound,
                            checked.tools,
                        );
                        m.connector_offer_put(&owner, &r.connector_id, offer);
                    }
                }
                Err(_) => {
                    m.inner.connectors.fill_attempts.failed(&r.connector_id);
                    lock(&m.inner.connectors.tools).back_off(&r.connector_id);
                }
            }
        });
    }

    /// A digest of `owner`'s sealed token for `r` as stored now: what a
    /// tool set is bound to.
    pub(crate) async fn sealed_hash(&self, owner: &str, r: &ConnectorRow) -> Option<String> {
        let (u, w, c) = (
            owner.to_string(),
            r.workspace_id.clone(),
            r.connector_id.clone(),
        );
        let blob = self
            .inner
            .mirror
            .call(move |s| s.connector_account_sealed(&u, &w, &c))
            .await
            .ok()
            .flatten()?;
        Some(blob_hash(&blob))
    }

    /// `owner`'s token for `r` as its header, opened with the key file
    /// (proven first), and the digest of the sealed value it came from;
    /// the reason in plain words when it cannot be.
    async fn owner_token(
        &self,
        owner: &str,
        r: &ConnectorRow,
    ) -> Result<(TokenHeader, String), String> {
        let (u, w, c) = (
            owner.to_string(),
            r.workspace_id.clone(),
            r.connector_id.clone(),
        );
        let blob = self
            .inner
            .mirror
            .call(move |s| s.connector_account_sealed(&u, &w, &c))
            .await
            .map_err(|_| "the account could not be read; try again".to_string())?
            .ok_or_else(|| {
                format!(
                    "the chat's owner has not connected an account to connector `{}`",
                    r.name
                )
            })?;
        let sealer = self
            .sealer()
            .ok_or("this server keeps no tokens (no sealed storage)")?;
        let st = forge::stored(self)
            .await
            .map_err(|_| "the server's key could not be read; try again".to_string())?;
        sealer.check(&st).map_err(
            |_| "this server cannot open tokens right now (its key file needs the admin)",
        )?;
        let fields = [
            r.workspace_id.as_str(),
            r.connector_id.as_str(),
            r.url.as_str(),
        ];
        let token = sealer
            .open(&binding(owner, &fields), &blob, &st)
            .map_err(|_| {
                format!(
                    "the stored token for connector `{}` does not open; its owner should connect \
                     again",
                    r.name
                )
            })?;
        let h = super::token_header(r.header.as_deref(), &token).map_err(|e| e.to_string())?;
        Ok((h, blob_hash(&blob)))
    }

    /// Tests: the time limit of one connector call.
    pub fn set_connector_call_timeout(&self, d: Duration) {
        *lock(&self.inner.connectors.calls.timeout) = d;
    }

    /// Tests: calls sent to a connector so far.
    pub fn connector_calls_sent(&self) -> usize {
        self.inner.connectors.calls.sent.load(Ordering::SeqCst)
    }

    /// Tests: calls running on the call workers now.
    pub fn connector_calls_running(&self) -> usize {
        self.inner.connectors.calls.pool.running()
    }

    /// Tests: tool sets held in memory, and their bytes.
    pub fn connector_tool_sets(&self) -> (usize, usize) {
        let c = lock(&self.inner.connectors.tools);
        (c.len(), c.bytes())
    }

    /// How many tools `user`'s agents are offered of connector `r` now.
    pub(crate) fn connector_agent_tools(&self, user: &str, r: &ConnectorRow) -> Option<usize> {
        lock(&self.inner.connectors.tools)
            .get(user, &r.connector_id, &super::stamp(r))
            .map(|(_, t)| t.len())
    }

    pub(crate) fn connector_tools_forget(&self, user: Option<&str>, id: &str) {
        lock(&self.inner.connectors.tools).forget(user, id)
    }

    /// Tests: forget every set of connector `id` (as the cache's age would).
    pub fn connector_tools_forget_for_test(&self, id: &str) {
        self.connector_tools_forget(None, id)
    }

    /// Tests: the tools `user`'s set of connector `r` holds now.
    pub fn connector_tool_names_for_test(&self, user: &str, r: &ConnectorRow) -> Vec<String> {
        lock(&self.inner.connectors.tools)
            .get(user, &r.connector_id, &super::stamp(r))
            .map(|(_, t)| t.iter().map(|t| t.raw.clone()).collect())
            .unwrap_or_default()
    }

    /// Tests: put `tools` as `user`'s set of connector `r`, as a check
    /// would (the caps are the check's; this does not apply them).
    pub fn connector_tools_put_for_test(
        &self,
        user: &str,
        r: &ConnectorRow,
        tools: Vec<super::tools::AgentTool>,
    ) {
        self.connector_tools_put(user, r, None, tools)
    }

    pub(crate) fn connector_tools_put(
        &self,
        user: &str,
        r: &ConnectorRow,
        token: Option<String>,
        tools: Vec<super::tools::AgentTool>,
    ) {
        lock(&self.inner.connectors.tools).put(
            user,
            &r.connector_id,
            &super::stamp(r),
            token,
            tools,
        )
    }
}

/// Answer a node's `connector.request` from a task of its own, at most
/// [`PER_NODE`] at once per node; the answer goes to that node only. A
/// `connector.cancel` for it drops the call.
pub fn handle(
    m: &Master,
    node_id: &str,
    req_id: String,
    sid: SessionId,
    tool: String,
    arguments: Value,
) {
    let st = &m.inner.connectors.calls;
    let permit = st.node(node_id).try_acquire_owned();
    let key = (node_id.to_string(), req_id.clone());
    let cancel = CancellationToken::new();
    // A request id this node has in flight already: refused, and the first
    // call keeps its cancel (review of PR 157, L10).
    let permit = match permit {
        Ok(p) => {
            let mut c = lock(&st.cancels);
            if c.contains_key(&key) {
                Err("a connector call with this request id is already running".to_string())
            } else {
                c.insert(key.clone(), cancel.clone());
                Ok(p)
            }
        }
        Err(_) => Err("too many connector calls from this node at once; try again".to_string()),
    };
    let (m, node_id) = (m.clone(), node_id.to_string());
    tokio::spawn(async move {
        let res = match permit {
            Ok(_permit) => {
                let r = tokio::select! {
                    r = answer(&m, &node_id, sid, &tool, arguments) => Some(r),
                    _ = cancel.cancelled() => None,
                };
                lock(&m.inner.connectors.calls.cancels).remove(&key);
                r
            }
            Err(e) => Some(Err(e)),
        };
        // A cancelled call is answered to nobody: the node stopped waiting.
        let Some(res) = res else { return };
        let (answer, error) = match res {
            Ok(a) => (Some(a), None),
            Err(e) => {
                tracing::debug!(node = %node_id, session = %sid, "a connector call was refused or failed");
                (None, Some(e))
            }
        };
        m.send_unacked(
            &node_id,
            MasterToNode::ConnectorResult {
                req_id,
                answer,
                error,
            },
        );
    });
}

/// A node's `connector.cancel`: drop that call if it still runs.
pub fn cancel(m: &Master, node_id: &str, req_id: &str) {
    if let Some(c) =
        lock(&m.inner.connectors.calls.cancels).remove(&(node_id.to_string(), req_id.to_string()))
    {
        c.cancel();
    }
}

/// The answer to one call of `tool` for session `sid` of node `node_id`.
pub async fn answer(
    m: &Master,
    node_id: &str,
    sid: SessionId,
    tool: &str,
    arguments: Value,
) -> Result<ConnectorAnswer, String> {
    let st = &m.inner.connectors.calls;
    if !m.node_has(node_id, CAP_CONNECTORS) {
        return Err("this node does not take the server's connectors".into());
    }
    let unknown = || {
        format!(
            "`{}` is not a connector tool offered here",
            harness_core::event::shown_name(tool)
        )
    };
    let connector = mcp_server_of(tool).ok_or_else(unknown)?.to_string();
    if !arguments.is_object() {
        return Err("the arguments of an MCP tool are a JSON object".into());
    }
    if serde_json::to_string(&arguments).map_or(usize::MAX, |s| s.len()) > MAX_ARGUMENT_BYTES {
        return Err(format!(
            "the arguments are over {MAX_ARGUMENT_BYTES} bytes, which a connector call does not take"
        ));
    }
    let (_row, top) = crate::recall::sessions(m, node_id, sid).await?;
    let (Some(ws), Some(owner)) = (top.workspace_id.clone(), top.owner_id.clone()) else {
        return Err("connectors are offered only in a workspace's chats".into());
    };
    let open = m
        .inner
        .mirror
        .call(move |s| s.open_turn(sid))
        .await
        .ok()
        .flatten()
        .ok_or("a connector is called only during a turn")?;
    if open.callback {
        return Err("connectors are not used in a turn started by a command's end".into());
    }
    // The master's own record: a turn it started for the owner, which has
    // taken no other message (`grant`); a sub-agent's under the chat turn
    // that started it.
    let key = lock(&st.grants).check(sid, top.session_id, &open.turn_id)?;
    // The turn's agent as this workspace has it now, under its Plugins rule
    // (the same deny lists the turn was sent, made again here).
    let r = m.resolver_for(Some(&ws)).await;
    let mut spec = open
        .agent_id
        .as_deref()
        .and_then(|id| r.agents().get(id))
        .map(|a| a.spec.clone())
        .ok_or_else(unknown)?;
    r.assignments().deny(&mut spec);
    if !spec.allows_mcp() || !spec.allows_tool(tool) {
        return Err(unknown());
    }
    // The owner, active and still a member of the workspace.
    let who = crate::board::owner(m, &top).await?;
    crate::projects::member(m, &who, &ws)
        .await
        .map_err(|_| "the chat's owner is not a member of its workspace any more".to_string())?;
    let w = ws.clone();
    let rows = m
        .inner
        .mirror
        .call(move |s| s.connectors_of(&w))
        .await
        .map_err(|_| "the workspace's connectors could not be read; try again".to_string())?;
    let c = rows
        .into_iter()
        .find(|c| c.name == connector)
        .ok_or_else(unknown)?;
    // Only a tool the master listed for this account and this connector as
    // it is now: an edited connector (another stamp) has none.
    let raw = {
        let cache = lock(&m.inner.connectors.tools);
        cache
            .get(&owner, &c.connector_id, &super::stamp(&c))
            .and_then(|(_, t)| {
                t.iter()
                    .find(|t| t.entry.name == tool)
                    .map(|t| t.raw.clone())
            })
    }
    .ok_or_else(|| {
        format!(
            "`{}` is not among the tools this server read from connector `{connector}` for this \
             chat's owner (the connector changed, or its list is old); it is offered again from \
             the next message",
            harness_core::event::shown_name(tool)
        )
    })?;
    // Counted under the granted turn and the chat's hour.
    lock(&st.grants).take(&key)?;
    let token = match c.auth.as_str() {
        "token" => Some(m.owner_token(&owner, &c).await?.0),
        _ => None,
    };
    let mut net = m.connector_net();
    net.timeout = *lock(&st.timeout);
    let sent = &st.sent;
    sent.fetch_add(1, Ordering::SeqCst);
    let out = run_on(
        &st.pool,
        &owner,
        &c,
        Remote {
            token,
            raw,
            arguments,
            net,
        },
    )
    .await;
    match out {
        Ok(Some(Ok(a))) => Ok(a),
        Ok(Some(Err(f))) => Err(format!("connector `{connector}`: {f}")),
        Ok(None) => Err(format!(
            "connector `{connector}`: the call ended unexpectedly"
        )),
        Err(b) => {
            sent.fetch_sub(1, Ordering::SeqCst);
            Err(match b {
                Busy::Person => format!(
                    "the chat's owner has {PER_KEY} connector calls running; wait for one to end"
                ),
                Busy::Other => format!(
                    "connector `{connector}` has {PER_KEY} calls running on this server; try again \
                     in a moment"
                ),
                Busy::Full => {
                    "this server is making as many connector calls as it does at once; try again in \
                     a moment"
                        .into()
                }
            })
        }
    }
}

/// What one call sends, and how.
pub struct Remote {
    pub token: Option<TokenHeader>,
    /// The server's exact name of the tool.
    pub raw: String,
    pub arguments: Value,
    /// The rules of the connection; its `timeout` is the call's limit.
    pub net: super::Net,
}

/// Make one call of connector `c` for `owner` on a worker of `pool`, under
/// the keys of the person and the connector, within [`CALL_BYTES`] and
/// `r.net.timeout`. Never logs.
pub async fn run_on(
    pool: &Pool,
    owner: &str,
    c: &ConnectorRow,
    r: Remote,
) -> Result<Option<Result<ConnectorAnswer, Failure>>, Busy> {
    let (url, name) = (c.url.clone(), c.name.clone());
    pool.run_keyed(
        &[&format!("u:{owner}"), &format!("c:{}", c.connector_id)],
        move || async move {
            let budget = Budget::new(CALL_BYTES);
            let Remote {
                token,
                raw,
                arguments,
                net,
            } = r;
            let timeout = net.timeout;
            match tokio::time::timeout(
                timeout,
                once(&url, &name, token, &raw, arguments, &net, budget),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => Err(Failure::Timeout),
            }
        },
    )
    .await
}

/// One MCP session: `initialize`, ONE `tools/call` of `raw` with
/// `arguments`, the session ended. Text only comes back.
async fn once(
    url: &str,
    connector: &str,
    token: Option<TokenHeader>,
    raw: &str,
    arguments: Value,
    net: &super::Net,
    budget: Arc<Budget>,
) -> Result<ConnectorAnswer, Failure> {
    use rmcp::model::{CallToolRequestParams, CallToolResponse};
    let Value::Object(args) = arguments else {
        return Err(Failure::Protocol);
    };
    let (service, http) = super::session(url, token, net, budget).await?;
    let mut params = CallToolRequestParams::new(raw.to_string());
    params.arguments = Some(args);
    let r = service.call_tool_once(params).await;
    let _ = service.cancel().await;
    match r {
        Ok(CallToolResponse::Complete(result)) => {
            let out = harness_tools::mcp::render(
                connector,
                serde_json::to_value(&result).unwrap_or_default(),
            );
            Ok(ConnectorAnswer {
                text: out.output,
                is_error: !out.ok,
            })
        }
        Ok(_) => Err(Failure::BadAnswer(
            "it asked for more input or started a task, which this server does not do",
        )),
        // The server's own refusal: its words are the result, as for a node
        // server's (`MCP server refused the call`), text for the model only.
        Err(rmcp::ServiceError::McpError(e)) => Ok(ConnectorAnswer {
            text: format!(
                "the connector refused the call: {}",
                crate::remote::clean_text(&e.message, 2000).unwrap_or_default()
            ),
            is_error: true,
        }),
        Err(_) => Err(http.first_failure().unwrap_or(Failure::Protocol)),
    }
}
