//! Workspaces: fixed roles and their permissions, workspace-tier settings,
//! and what the caller may do in a workspace (PLAN.md §0, T10/T11).
//!
//! Code checks permissions, never role names, except for the owner-only
//! actions (deleting a workspace, granting or taking away `owner`). The
//! HTTP layer maps `WsError` to status codes.

use std::collections::BTreeMap;

use harness_store::{SessionRow, StoreError, WorkspaceRow};
use serde::{Deserialize, Serialize};

use harness_core::policy::ExecMode;

use crate::Master;
use crate::auth::Principal;
use crate::config::{AgentChain, check_chain};

/// What a role allows inside a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Perm {
    /// Rename the workspace and change its defaults.
    Settings,
    /// Add and remove members and change their roles.
    Members,
    /// Own routines here: chats started on a schedule or by a webhook, as
    /// the routine's owner.
    Routines,
}

impl Perm {
    /// What the permission lets a member do, for refusals.
    fn action(self) -> &'static str {
        match self {
            Perm::Settings => "change its settings",
            Perm::Members => "manage its members",
            Perm::Routines => "run routines",
        }
    }
}

/// The fixed roles, stored as `workspace_members.role_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Guest,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Admin, Role::Member, Role::Guest];

    /// A stored role id; `None` for one this build does not know, which
    /// then has no permissions (never a default role).
    pub fn parse(id: &str) -> Option<Role> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn id(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Guest => "guest",
        }
    }

    /// In the wire order: settings, members, routines. Who may run agent
    /// chats is no role's: it is the member's level (`levels`, N.10a).
    pub fn perms(self) -> &'static [Perm] {
        match self {
            Role::Owner | Role::Admin => &[Perm::Settings, Perm::Members, Perm::Routines],
            Role::Member => &[Perm::Routines],
            Role::Guest => &[],
        }
    }

    /// Nobody may hand out or act on a role above their own.
    pub fn rank(self) -> u8 {
        match self {
            Role::Owner => 3,
            Role::Admin => 2,
            Role::Member => 1,
            Role::Guest => 0,
        }
    }
}

/// The permissions a stored role id grants.
pub fn perms_of(role_id: &str) -> &'static [Perm] {
    Role::parse(role_id).map(Role::perms).unwrap_or(&[])
}

/// The rank of a stored role id. An unknown one ranks lowest, so a
/// workspace admin can still repair it.
pub fn rank_of(role_id: &str) -> u8 {
    Role::parse(role_id).map(Role::rank).unwrap_or(0)
}

/// Why a workspace request failed; `http.rs` turns it into a status code.
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    /// Unknown, or the caller is not a member (the same answer, so ids of
    /// other people's workspaces are not confirmed).
    #[error("unknown workspace")]
    NotFound,
    #[error("{0}")]
    Forbidden(String),
    #[error("store: {0}")]
    Store(#[from] StoreError),
}

/// Workspace-tier settings, stored as JSON on the workspace. The default
/// model is General's chain, as on the server. Every field's default is
/// "none: the server's applies", so `Default` and `{}` agree (tested).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSettings {
    /// The default agent of an ordinary (chat-mode) chat.
    #[serde(default)]
    pub default_agent: Option<String>,
    /// The default agent of an Agentic chat (none: the server's, `coder`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agentic_agent: Option<String>,
    /// Agent id -> its model and fallbacks; `general` is the workspace
    /// default model.
    #[serde(default)]
    pub agent_models: BTreeMap<String, AgentChain>,
}

/// Whether `id` may be the default agent that `field` (a workspace's or a
/// user's setting) names: it exists and may run a chat. Settings take ids,
/// not aliases. The error starts with the field.
pub fn check_default_agent(
    agents: &harness_agents::AgentSet,
    field: &str,
    id: &str,
) -> Result<(), String> {
    match agents.get(id) {
        None => Err(format!("{field}: unknown agent {id:?}")),
        Some(a) if !a.features.as_agent => Err(format!(
            "{field}: {} (@{id}) works only as a sub-agent and cannot be a default agent",
            a.name
        )),
        Some(_) => Ok(()),
    }
}

/// Whether `id` may be the Agentic default that `field` names: a default
/// agent (see `check_default_agent`) that can work in an Agentic chat, so
/// one with file or command tools. The error starts with the field.
pub fn check_agentic_default(
    agents: &harness_agents::AgentSet,
    field: &str,
    id: &str,
) -> Result<(), String> {
    check_default_agent(agents, field, id)?;
    match agents.get(id) {
        Some(a) if !(a.features.edit_files || a.features.commands) => Err(format!(
            "{field}: {} (@{id}) has no file or command tools, so it cannot work in an Agentic \
             chat; choose an agent that can change files or run commands",
            a.name
        )),
        _ => Ok(()),
    }
}

/// The stored document, read leniently: fields this build does not know
/// are ignored, so a later rename cannot make every workspace unreadable.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StoredSettings {
    default_agent: Option<String>,
    default_agentic_agent: Option<String>,
    agent_models: BTreeMap<String, StoredChain>,
}

#[derive(Deserialize)]
struct StoredChain {
    model: String,
    #[serde(default)]
    fallbacks: Vec<String>,
}

impl WorkspaceSettings {
    /// A workspace's stored settings. An unreadable document counts as no
    /// settings (the server's apply), with a warning.
    pub fn read(w: &WorkspaceRow) -> Self {
        match serde_json::from_str::<StoredSettings>(&w.settings) {
            Ok(s) => Self {
                default_agent: s.default_agent,
                default_agentic_agent: s.default_agentic_agent,
                agent_models: s
                    .agent_models
                    .into_iter()
                    .map(|(a, c)| {
                        (
                            a,
                            AgentChain {
                                model: c.model,
                                fallbacks: c.fallbacks,
                            },
                        )
                    })
                    .collect(),
            },
            Err(e) => {
                tracing::warn!(workspace = %w.workspace_id, error = %e, "unreadable workspace settings; using the server's");
                Self::default()
            }
        }
    }

    /// A document from a PATCH: unknown fields are an error, then every
    /// reference is checked.
    pub fn parse(v: serde_json::Value, m: &Master) -> Result<Self, String> {
        let s: Self = serde_json::from_value(v).map_err(|e| format!("bad settings: {e}"))?;
        s.validate(m)?;
        Ok(s)
    }

    /// Known agents and models only, with the same chain rules as the
    /// server's. Checks the master's model list, so it works without a
    /// provider runtime.
    pub fn validate(&self, m: &Master) -> Result<(), String> {
        let agents = m.agents();
        if let Some(a) = &self.default_agent {
            check_default_agent(&agents, "default_agent", a)?;
        }
        if let Some(a) = &self.default_agentic_agent {
            check_agentic_default(&agents, "default_agentic_agent", a)?;
        }
        let models = m.models();
        let knows = |id: &str| models.iter().any(|x| x.id == id);
        for (agent, chain) in &self.agent_models {
            check_chain(&agents, &knows, agent, chain)?;
        }
        Ok(())
    }

    /// Without the chains that went stale since they were saved: unknown
    /// agents, chains whose main model is gone, fallbacks that are gone.
    /// The default agent stays as stored even when it went stale: a chat
    /// that needs it is refused with an error naming the setting, and the
    /// settings page shows what to fix (saving it unchanged is refused the
    /// same way).
    pub fn normalized(&self, m: &Master) -> Self {
        let agents = m.agents();
        let models = m.models();
        let knows = |id: &str| models.iter().any(|x| x.id == id);
        Self {
            default_agent: self.default_agent.clone(),
            default_agentic_agent: self.default_agentic_agent.clone(),
            agent_models: self
                .agent_models
                .iter()
                .filter(|(a, c)| agents.get(a).is_some() && knows(&c.model))
                .map(|(a, c)| {
                    let fallbacks = c.fallbacks.iter().filter(|f| knows(f)).cloned();
                    (
                        a.clone(),
                        AgentChain {
                            model: c.model.clone(),
                            fallbacks: fallbacks.collect(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// The caller's standing in one workspace.
#[derive(Debug, Clone)]
pub struct Access {
    pub workspace: WorkspaceRow,
    pub personal: bool,
    /// `None` for a stored role this build does not know: no permissions.
    pub role: Option<Role>,
    /// The stored role id, as the UI shows it.
    pub role_id: String,
    /// The member's own switch; always on in a Personal workspace (and for
    /// the API token, which is no member).
    pub use_memory: bool,
    /// Whether they answered the join question (always in Personal).
    pub memory_answered: bool,
    /// Their level here as it counts (`levels::workspace_level`): `host` in
    /// a Personal workspace, for an owner and for the API token, so there
    /// the account's ceiling decides.
    pub exec_level: harness_core::policy::ExecLevel,
}

impl Access {
    pub fn id(&self) -> &str {
        &self.workspace.workspace_id
    }

    pub fn perms(&self) -> &'static [Perm] {
        self.role.map(Role::perms).unwrap_or(&[])
    }

    pub fn can(&self, p: Perm) -> bool {
        self.perms().contains(&p)
    }

    pub fn require(&self, p: Perm) -> Result<(), WsError> {
        if self.can(p) {
            return Ok(());
        }
        Err(WsError::Forbidden(format!(
            "your role in {} cannot {}",
            self.workspace.name,
            p.action()
        )))
    }

    pub fn is_owner(&self) -> bool {
        self.role == Some(Role::Owner)
    }

    pub fn rank(&self) -> u8 {
        self.role.map(Role::rank).unwrap_or(0)
    }

    /// The workspace's own settings, stale references dropped; `None` for a
    /// Personal workspace, which has none (the server's apply).
    pub fn settings(&self, m: &Master) -> Option<WorkspaceSettings> {
        (!self.personal).then(|| WorkspaceSettings::read(&self.workspace).normalized(m))
    }

    /// The effective configuration here.
    pub fn resolver(&self, m: &Master) -> crate::Resolver {
        m.resolver(self.settings(m).as_ref())
    }
}

/// What `who` may do in workspace `ws`. Non-members get `NotFound`; the API
/// token acts as an owner of any existing workspace without being a member.
pub async fn access(m: &Master, who: &Principal, ws: &str) -> Result<Access, WsError> {
    let (id, user) = (ws.to_string(), who.user_id.clone());
    let (w, member) = m
        .inner
        .mirror
        .call(move |s| {
            let w = s.workspace(&id)?;
            let member = match (&w, &user) {
                (Some(_), Some(u)) => s.member(&id, u)?,
                _ => None,
            };
            Ok::<_, StoreError>((w, member))
        })
        .await?;
    let w = w.ok_or(WsError::NotFound)?;
    let personal = w.personal();
    let none = harness_core::policy::ExecLevel::None;
    let (role_id, use_memory, memory_answered, stored) = match (&who.user_id, member) {
        (None, _) => (Role::Owner.id().to_string(), true, true, none),
        (Some(_), Some(mr)) => (
            mr.role_id,
            mr.use_memory || personal,
            mr.memory_answered || personal,
            mr.exec_level,
        ),
        // A Personal owner whose membership row went missing is still its
        // owner (the store restores the row at the next start).
        (Some(u), None) if w.personal_owner.as_deref() == Some(u.as_str()) => {
            (Role::Owner.id().to_string(), true, true, none)
        }
        (Some(_), None) => return Err(WsError::NotFound),
    };
    let role = Role::parse(&role_id);
    Ok(Access {
        exec_level: crate::levels::workspace_level(personal, role == Some(Role::Owner), stored),
        role,
        role_id,
        workspace: w,
        personal,
        use_memory,
        memory_answered,
    })
}

fn active_key(user_id: &str) -> String {
    format!("workspace:{user_id}")
}

/// The workspace `who` works in: the one they chose last while they are
/// still a member of it, else their Personal workspace (created if an old
/// account lacks one). `None` for the API token, which has none.
pub async fn active_workspace(m: &Master, who: &Principal) -> Result<Option<String>, StoreError> {
    let Some(uid) = who.user_id.clone() else {
        return Ok(None);
    };
    m.inner
        .mirror
        .call(move |s| {
            if let Some(ws) = s.kv_get(&active_key(&uid))?
                && s.member(&ws, &uid)?.is_some()
            {
                return Ok(Some(ws));
            }
            s.personal_workspace(&uid).map(Some)
        })
        .await
}

/// Remember `ws` as `user_id`'s active workspace (membership checked by the
/// caller).
pub async fn set_active_workspace(m: &Master, user_id: &str, ws: &str) -> Result<(), StoreError> {
    let (k, v) = (active_key(user_id), ws.to_string());
    m.inner.mirror.call(move |s| s.kv_set(&k, &v)).await
}

/// The workspace a workspace-scoped read or create applies to: the one
/// asked for (members only), else the caller's active one. `None` only for
/// the API token without `?workspace`, which then sees across workspaces.
pub async fn scope(
    m: &Master,
    who: &Principal,
    requested: Option<&str>,
) -> Result<Option<Access>, WsError> {
    let ws = match requested {
        Some(ws) => ws.to_string(),
        None => match active_workspace(m, who).await? {
            Some(ws) => ws,
            None => return Ok(None),
        },
    };
    access(m, who, &ws).await.map(Some)
}

/// The refusal for an account that may not use the embedded node
/// (`[nodes] local_node_users`, FIX_PLAN Q7).
pub const LOCAL_NODE_ADMINS_ONLY: &str = "agentic sessions on this server's own node are for \
     server admins: it runs as the server's user, next to its keys and every account's chats. \
     Use a remote node, or ask the server admin";

/// Whether `who` may run agentic sessions on the embedded node: server
/// admins and the API token always; everyone else (within their level)
/// when `[nodes] local_node_users = "granted"`. Without a provider runtime
/// (tests) the default applies: admins only.
pub fn may_use_local_node(m: &Master, who: &Principal) -> bool {
    who.is_admin()
        || m.provider_runtime().is_some_and(|rt| {
            rt.config().nodes.local_node_users == crate::config::LocalNodeUsers::Granted
        })
}

/// Why `who` may not run agentic work on node `node_id`, or `None` when
/// the node rule allows it (the level is checked by `levels`).
pub fn node_refusal(m: &Master, who: &Principal, node_id: &str) -> Option<String> {
    (m.is_local_node(node_id) && !may_use_local_node(m, who))
        .then(|| LOCAL_NODE_ADMINS_ONLY.to_string())
}

/// What a create had resolved when it ended, for its host-mode audit line.
#[derive(Debug, Default)]
pub struct HostAudit {
    pub workspace: Option<String>,
    pub node: Option<String>,
}

/// One audit line for every create that asked for `host`, whichever check
/// refused it (N.10a reviews N-d, RN-4): who ("api token" for the token),
/// the workspace and node as resolved, and the outcome: "granted" with the
/// session id once the chat exists, or "refused: " and the refusal's first
/// words. No chat content.
pub async fn audit_host_create(
    m: &Master,
    who: &Principal,
    seen: &HostAudit,
    outcome: Result<harness_core::SessionId, &str>,
) {
    let user = who.user_id.clone().unwrap_or_else(|| "api token".into());
    let (outcome, session) = match outcome {
        Ok(sid) => ("granted".to_string(), Some(sid.to_string())),
        Err(why) => {
            let words: String = why.chars().take(80).collect();
            (format!("refused: {words}"), None)
        }
    };
    let detail = serde_json::json!({
        "user": user,
        "workspace": seen.workspace,
        "node": seen.node,
        "outcome": outcome,
        "session": session,
    })
    .to_string();
    if let Err(e) = m
        .inner
        .mirror
        .call(move |s| s.audit("session.create.host", &detail))
        .await
    {
        tracing::warn!(error = %e, "could not write the host-mode audit line");
    }
}

/// The refusal for an agentic row whose mode cannot be read (fails closed).
pub const MODE_UNKNOWN: &str = "this chat's execution mode is not known here (its first event \
     has not arrived, or names none), so it does no work";

/// Why `owner` may not have the agentic chat `row` do work in its recorded
/// mode, given their standing `ws` in its workspace: the mode must be at
/// most their effective level there (N.10a part 2). Fails closed on a mode
/// that cannot be read.
pub fn row_refusal(owner: &Principal, ws: Option<&Access>, row: &SessionRow) -> Option<String> {
    if row.profile.as_deref() == Some("chat") {
        return None;
    }
    match row.exec_mode.as_deref().map(ExecMode::parse) {
        Some(Some(mode)) => crate::levels::refusal(owner, ws, mode),
        _ => Some(MODE_UNKNOWN.into()),
    }
}

/// Why the agentic session `row` may not do work now, whoever asks, or
/// `None` when it may (always `None` for chats): its owner's effective level
/// in its workspace must allow its recorded mode, and the node rule must
/// allow its owner. Decided by the owner, never the caller: the API token
/// acting on a member's chat gets the member's answer. A chat without an
/// owner is the API token's (`host`). A row not mirrored far enough to know
/// its profile counts as agentic, so the check fails closed. Asked by
/// `Master::send_command` before every command that makes a node work on a
/// session, so no caller can skip it.
pub async fn work_refusal(m: &Master, row: &SessionRow) -> Option<String> {
    if row.profile.as_deref() == Some("chat") {
        return None;
    }
    let Some(owner) = row.owner_id.clone() else {
        return row_refusal(&Principal::api_token(), None, row);
    };
    let owner = match m.inner.mirror.call(move |s| s.user_by_id(&owner)).await {
        Ok(Some(u)) if !u.disabled => Principal::from_user(u),
        Ok(_) => return Some("the account is disabled or gone".into()),
        Err(e) => {
            tracing::warn!(session = %row.session_id, error = %e, "could not load a chat's owner");
            return Some("could not check access to this chat; try again".into());
        }
    };
    standing_refusal(m, &owner, row).await
}

/// `owner`'s answer for `row`: their access to its workspace, the node
/// rule, and the level.
async fn standing_refusal(m: &Master, owner: &Principal, row: &SessionRow) -> Option<String> {
    let ws = match &row.workspace_id {
        None => None,
        Some(id) => match access(m, owner, id).await {
            Ok(a) => Some(a),
            Err(WsError::Store(e)) => {
                tracing::warn!(session = %row.session_id, error = %e, "could not check workspace access");
                return Some("could not check your access to this workspace; try again".into());
            }
            // The chat's workspace is one its owner is not in: never theirs
            // to drive.
            Err(_) => return Some("this chat is in a workspace you are not a member of".into()),
        },
    };
    node_refusal(m, owner, &row.node_id).or_else(|| row_refusal(owner, ws.as_ref(), row))
}

/// The mode a new agentic chat owned by `owner` (`None`: the API token's)
/// in workspace `ws` asks its node for (`levels::create_mode`): `Err` with
/// the reason when the owner's level there does not allow it. What
/// `Master::create_session` asks, whoever calls it.
pub async fn create_mode(
    m: &Master,
    owner: Option<&str>,
    ws: Option<&str>,
    requested: Option<ExecMode>,
) -> Result<ExecMode, String> {
    let who = match owner {
        None => Principal::api_token(),
        Some(o) => {
            let o = o.to_string();
            match m.inner.mirror.call(move |s| s.user_by_id(&o)).await {
                Ok(Some(u)) if !u.disabled => Principal::from_user(u),
                Ok(_) => return Err("the account is disabled or gone".into()),
                Err(e) => {
                    tracing::warn!(error = %e, "could not load a new chat's owner");
                    return Err("could not check your level; try again".into());
                }
            }
        }
    };
    let a = match ws {
        None => None,
        Some(id) => match access(m, &who, id).await {
            Ok(a) => Some(a),
            Err(WsError::Store(e)) => {
                tracing::warn!(error = %e, "could not check workspace access");
                return Err("could not check your access to this workspace; try again".into());
            }
            Err(_) => return Err("you are not a member of this workspace".into()),
        },
    };
    crate::levels::create_mode(&who, a.as_ref(), requested)
}

/// Why `who` may not make the agentic session `row` do work, or `None`
/// when they may (always `None` for chats): the node rule for the caller,
/// then [`work_refusal`] (the owner's level). For a caller who owns the
/// chat, the owner's answer is computed from `who` as loaded for this
/// request.
pub async fn agentic_blocked(m: &Master, who: &Principal, row: &SessionRow) -> Option<String> {
    if row.profile.as_deref() == Some("chat") {
        return None;
    }
    if who.user_id.is_some() && row.owner_id == who.user_id {
        return standing_refusal(m, who, row).await;
    }
    match node_refusal(m, who, &row.node_id) {
        Some(why) => Some(why),
        None => work_refusal(m, row).await,
    }
}

/// Whether chat `row` may use its owner's personal memory (recall, and the
/// `remember` tool): always in a Personal workspace and for chats without an
/// owner, else as the owner set it for that team workspace. When that cannot
/// be read, no: memory stays out rather than reaching a team's chats.
pub async fn uses_memory(m: &Master, row: &SessionRow) -> bool {
    let (Some(ws), Some(owner)) = (row.workspace_id.clone(), row.owner_id.clone()) else {
        return true;
    };
    let res = m
        .inner
        .mirror
        .call(move |s| {
            if s.workspace(&ws)?.is_some_and(|w| w.personal()) {
                return Ok(true);
            }
            Ok::<_, StoreError>(s.member(&ws, &owner)?.is_some_and(|mr| mr.use_memory))
        })
        .await;
    res.unwrap_or_else(|e| {
        tracing::warn!(session = %row.session_id, error = %e, "could not read the memory switch; leaving memory out");
        false
    })
}

/// Run the revocation sweep for `users` in the background, so a slow node
/// never holds up the request that took the access away.
pub fn sweep_revoked(m: &Master, users: Vec<String>) {
    if users.is_empty() {
        return;
    }
    let m = m.clone();
    tokio::spawn(async move { cancel_revoked(&m, &users).await });
}

/// Stop work that lost its access (best effort): cancel every running
/// agentic session owned by one of `users` that its owner may no longer
/// drive, e.g. after they left a workspace, lost the `agentic` role or the
/// server grant, or their account was disabled or deleted. Failures are
/// logged, never returned.
pub async fn cancel_revoked(m: &Master, users: &[String]) {
    let mut rows = Vec::new();
    for user in users {
        let user = user.clone();
        match m
            .inner
            .mirror
            .call(move |s| s.sessions_of(Some(&user)))
            .await
        {
            Ok(r) => rows.extend(r),
            Err(e) => {
                tracing::warn!(error = %e, "revocation sweep: could not list sessions");
                return;
            }
        }
    }
    let running = rows
        .into_iter()
        .filter(|r| r.owner_id.as_ref().is_some_and(|o| users.contains(o)));
    cancel_blocked(m, running).await;
}

/// The sweep again for the running sessions of `node_id`, once its link is
/// up (after its hello): a turn whose owner lost access while the node was
/// away, or while its cancel could not be sent, is cancelled now (Gate 3
/// review P-3, T11).
pub async fn cancel_revoked_on(m: &Master, node_id: &str) {
    let node = node_id.to_string();
    // Every session there, not only the running ones: a background command
    // runs without a turn, and no count of them is trusted (review N-1).
    match m.inner.mirror.call(move |s| s.sessions_on(&node)).await {
        Ok(rows) => cancel_blocked(m, rows.into_iter()).await,
        Err(e) => {
            tracing::warn!(node = %node_id, error = %e, "revocation sweep: could not list sessions")
        }
    }
}

/// The sweep for one session, once a replay of it has landed (Gate 3 fix
/// review 2, R2-2): the hello's sweep read the mirror before the replay
/// arrived, so a turn only the replay shows as running was missed.
pub async fn cancel_revoked_session(m: &Master, sid: harness_core::SessionId) {
    match m.inner.mirror.call(move |s| s.session(sid)).await {
        Ok(row) => cancel_blocked(m, row.into_iter()).await,
        Err(e) => {
            tracing::warn!(session = %sid, error = %e, "revocation sweep: could not read the session")
        }
    }
}

/// Why `row`'s owner may no longer drive it, if so; `Err` when that could
/// not be decided (a store error), so nothing is cancelled on a guess.
async fn blocked_reason(m: &Master, row: &SessionRow) -> Result<Option<String>, ()> {
    let owner = row.owner_id.clone().unwrap_or_default();
    match m.inner.mirror.call(move |s| s.user_by_id(&owner)).await {
        Ok(Some(u)) if !u.disabled => Ok(agentic_blocked(m, &Principal::from_user(u), row).await),
        Ok(_) => Ok(Some("the account is disabled or gone".to_string())),
        Err(e) => {
            tracing::warn!(session = %row.session_id, error = %e, "revocation sweep: could not load the owner");
            Err(())
        }
    }
}

/// For every agentic session in `rows` whose owner may no longer drive it:
/// cancel its running turn, then stop its background commands (T.8d: a
/// turn's cancel leaves them running, and they run without a turn). The
/// stop goes to every such session on a node that has background commands
/// at all, whatever the mirror shows: a node with none answers it as a
/// no-op, and no count that can lag decides (review N-1, N-8). After the
/// stop the node refuses that chat's background starts, and stops one a
/// start hands over late, until its owner prompts it again.
async fn cancel_blocked(m: &Master, rows: impl Iterator<Item = SessionRow>) {
    let agentic = rows.filter(|r| r.profile.as_deref() != Some("chat") && r.owner_id.is_some());
    // The answer depends on the owner, the chat's workspace, its node and
    // its recorded mode (N.10a: a `host` chat has its own rule, review
    // RN-1): asked once per such tuple, not once per chat (a hello's sweep
    // reads every chat of the node).
    type Key = (Option<String>, Option<String>, String, Option<String>);
    let mut asked: std::collections::HashMap<Key, Result<Option<String>, ()>> =
        std::collections::HashMap::new();
    for row in agentic {
        let key = (
            row.owner_id.clone(),
            row.workspace_id.clone(),
            row.node_id.clone(),
            row.exec_mode.clone(),
        );
        let reason = match asked.get(&key) {
            Some(r) => r.clone(),
            None => {
                let r = blocked_reason(m, &row).await;
                asked.insert(key, r.clone());
                r
            }
        };
        let Ok(Some(why)) = reason else {
            continue;
        };
        if row.running {
            tracing::info!(session = %row.session_id, reason = %why, "cancelling an agentic turn that lost its access");
            cancel_retrying(m, row.session_id).await;
        }
        stop_background_revoked(m, row.session_id).await;
    }
}

/// The revocation's stop of a session's background commands. A node
/// that is offline gets it from the sweep at its next hello (which sends
/// it for every blocked session there); a failure is logged.
async fn stop_background_revoked(m: &Master, sid: harness_core::SessionId) {
    use harness_protocol::StopBackgroundBy;
    for attempt in 1..=CANCEL_TRIES {
        match m
            .stop_background(sid, None, StopBackgroundBy::AccessRevoked)
            .await
        {
            Err(crate::CommandError::Timeout) if attempt < CANCEL_TRIES => {
                tokio::time::sleep(CANCEL_RETRY).await;
            }
            Err(crate::CommandError::NodeOffline(_)) => return,
            Err(e) => {
                tracing::warn!(session = %sid, error = %e, "revocation sweep: background stop failed");
                return;
            }
            Ok(()) => return,
        }
    }
}

/// Stop the running turns and background commands of `sids`, in the
/// background: the chats a member had in a team workspace they were just
/// removed from (N.10a part 2). They return to the member's Personal
/// workspace, where their account's ceiling applies from their next action;
/// what was running for the team stops now, whatever that ceiling is. Also
/// a workspace's deletion (review L2-3), and a command its node acked after
/// its owner's level was lowered (`Master::recheck_after_ack`, L2-1).
pub fn stop_removed(m: &Master, sids: Vec<harness_core::SessionId>) {
    if sids.is_empty() {
        return;
    }
    let m = m.clone();
    tokio::spawn(async move {
        for sid in sids {
            // Whether or not the mirror shows a turn yet: one the node
            // started just now may not have reached it (review L2-1), and a
            // cancel with nothing running changes nothing.
            {
                for attempt in 1..=CANCEL_TRIES {
                    match m.cancel(sid).await {
                        Err(crate::CommandError::Timeout) if attempt < CANCEL_TRIES => {
                            tokio::time::sleep(CANCEL_RETRY).await;
                        }
                        Err(e) => {
                            tracing::info!(session = %sid, error = %e, "removal: cancel not delivered");
                            break;
                        }
                        Ok(()) => break,
                    }
                }
            }
            stop_background_revoked(&m, sid).await;
        }
    });
}

/// Attempts at a revocation cancel the node's queue had no room for (or
/// did not acknowledge); the first wait, doubled after each.
const CANCEL_TRIES: u32 = 5;
const CANCEL_RETRY: std::time::Duration = std::time::Duration::from_millis(500);

/// Cancel `sid`; a cancel that could not be queued or was not acknowledged
/// is tried again in the background (Gate 3 review P-3). A node that is
/// offline gets it from the sweep at its next hello.
async fn cancel_retrying(m: &Master, sid: harness_core::SessionId) {
    match m.cancel(sid).await {
        Err(crate::CommandError::Timeout) => {}
        Err(crate::CommandError::NodeOffline(_)) => {
            tracing::info!(session = %sid, "revocation sweep: node offline; cancelled when it is back");
            return;
        }
        Err(e) => {
            tracing::warn!(session = %sid, error = %e, "revocation sweep: cancel failed");
            return;
        }
        Ok(()) => return,
    }
    let m = m.clone();
    tokio::spawn(async move {
        let mut wait = CANCEL_RETRY;
        for attempt in 2..=CANCEL_TRIES {
            tokio::time::sleep(wait).await;
            wait *= 2;
            // Still blocked? A grant given back meanwhile keeps its turn
            // (R2-2).
            let row = m.inner.mirror.call(move |s| s.session(sid)).await;
            let still = match row {
                Ok(Some(row)) if row.running => {
                    matches!(blocked_reason(&m, &row).await, Ok(Some(_)))
                }
                _ => false,
            };
            if !still {
                return;
            }
            match m.cancel(sid).await {
                Err(crate::CommandError::Timeout) if attempt < CANCEL_TRIES => {}
                Err(e) => {
                    tracing::warn!(session = %sid, error = %e, attempt, "revocation sweep: cancel failed");
                    return;
                }
                Ok(()) => return,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_and_permissions() {
        assert_eq!(
            Role::Owner.perms(),
            [Perm::Settings, Perm::Members, Perm::Routines]
        );
        assert_eq!(Role::Admin.perms(), Role::Owner.perms());
        assert_eq!(Role::Member.perms(), [Perm::Routines]);
        assert!(Role::Guest.perms().is_empty());
        for r in Role::ALL {
            assert_eq!(Role::parse(r.id()), Some(r));
            assert_eq!(serde_json::to_value(r).unwrap(), r.id());
        }
        assert!(Role::Owner.rank() > Role::Admin.rank());
        assert!(Role::Admin.rank() > Role::Member.rank());
        assert!(Role::Member.rank() > Role::Guest.rank());
        // An unknown stored role has nothing, never a default.
        assert_eq!(Role::parse("superuser"), None);
        assert!(perms_of("superuser").is_empty());
        assert_eq!(rank_of("superuser"), 0);
        assert_eq!(
            serde_json::to_value(Perm::Members).unwrap(),
            serde_json::json!("members")
        );
        assert_eq!(
            serde_json::to_value(Perm::Routines).unwrap(),
            serde_json::json!("routines")
        );
    }

    fn row(settings: &str) -> WorkspaceRow {
        WorkspaceRow {
            workspace_id: "w".into(),
            name: "Team".into(),
            personal_owner: None,
            created_ms: 0,
            settings: settings.into(),
        }
    }

    #[test]
    fn settings_default_strict_and_lenient() {
        assert_eq!(
            WorkspaceSettings::read(&row("{}")),
            WorkspaceSettings::default()
        );
        assert_eq!(
            serde_json::from_str::<WorkspaceSettings>("{}").unwrap(),
            WorkspaceSettings::default()
        );
        // Stored documents ignore what this build does not know...
        let s = WorkspaceSettings::read(&row(
            r#"{"default_agent":"coder","old":1,"agent_models":{"general":{"model":"p::m","extra":true}}}"#,
        ));
        assert_eq!(s.default_agent.as_deref(), Some("coder"));
        assert_eq!(s.agent_models["general"], AgentChain::new("p::m"));
        // ...and an unreadable one counts as none.
        assert_eq!(
            WorkspaceSettings::read(&row("not json")),
            WorkspaceSettings::default()
        );
        // A PATCH is strict.
        assert!(serde_json::from_str::<WorkspaceSettings>(r#"{"memory":true}"#).is_err());
        assert!(
            serde_json::from_str::<WorkspaceSettings>(
                r#"{"agent_models":{"general":{"model":"p::m","extra":1}}}"#
            )
            .is_err()
        );
    }
}
