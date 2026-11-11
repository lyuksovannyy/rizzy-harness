//! Workspaces: fixed roles and their permissions, workspace-tier settings,
//! and what the caller may do in a workspace (PLAN.md §0, T10/T11).
//!
//! Code checks permissions, never role names, except for the owner-only
//! actions (deleting a workspace, granting or taking away `owner`). The
//! HTTP layer maps `WsError` to status codes.

use std::collections::BTreeMap;

use harness_store::{SessionRow, StoreError, WorkspaceRow};
use serde::{Deserialize, Serialize};

use crate::Master;
use crate::auth::Principal;
use crate::config::{AgentChain, check_chain};

/// What a role allows inside a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Perm {
    /// Start and drive agentic sessions here (on top of the account's
    /// server grant, never instead of it).
    Agentic,
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
            Perm::Agentic => "run agentic sessions",
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

    /// In the wire order: agentic, settings, members, routines.
    pub fn perms(self) -> &'static [Perm] {
        match self {
            Role::Owner | Role::Admin => {
                &[Perm::Agentic, Perm::Settings, Perm::Members, Perm::Routines]
            }
            Role::Member => &[Perm::Agentic, Perm::Routines],
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
    let (role_id, use_memory) = match (&who.user_id, member) {
        (None, _) => (Role::Owner.id().to_string(), true),
        (Some(_), Some(mr)) => (mr.role_id, mr.use_memory || personal),
        // A Personal owner whose membership row went missing is still its
        // owner (the store restores the row at the next start).
        (Some(u), None) if w.personal_owner.as_deref() == Some(u.as_str()) => {
            (Role::Owner.id().to_string(), true)
        }
        (Some(_), None) => return Err(WsError::NotFound),
    };
    Ok(Access {
        role: Role::parse(&role_id),
        role_id,
        workspace: w,
        personal,
        use_memory,
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

/// The refusal for an account without the server grant (the security
/// boundary for agentic work).
pub const NO_AGENTIC_GRANT: &str =
    "agentic sessions are not enabled for this account; ask an admin";

/// Why `who` may not run agentic sessions in `ws`, or `None` when they may:
/// both the account's server grant and the workspace permission `agentic`
/// are needed. `ws` is `None` only for chats without a workspace (API
/// token).
pub fn agentic_refusal(who: &Principal, ws: Option<&Access>) -> Option<String> {
    if !who.can_agentic {
        return Some(NO_AGENTIC_GRANT.into());
    }
    match ws {
        Some(a) if !a.can(Perm::Agentic) => Some(format!(
            "your role in {} cannot run agentic sessions",
            a.workspace.name
        )),
        _ => None,
    }
}

/// The refusal for an account that may not use the embedded node
/// (`[nodes] local_node_users`, FIX_PLAN Q7).
pub const LOCAL_NODE_ADMINS_ONLY: &str = "agentic sessions on this server's own node are for \
     server admins: it runs as the server's user, next to its keys and every account's chats. \
     Use a remote node, or ask the server admin";

/// Whether `who` may run agentic sessions on the embedded node: server
/// admins and the API token always; everyone with the agentic grant when
/// `[nodes] local_node_users = "granted"`. Without a provider runtime
/// (tests) the default applies: admins only.
pub fn may_use_local_node(m: &Master, who: &Principal) -> bool {
    who.is_admin()
        || m.provider_runtime().is_some_and(|rt| {
            rt.config().nodes.local_node_users == crate::config::LocalNodeUsers::Granted
        })
}

/// Why `who` may not run agentic work on node `node_id`, or `None` when
/// the node rule allows it (the grant and the workspace are checked by
/// [`agentic_refusal`]).
pub fn node_refusal(m: &Master, who: &Principal, node_id: &str) -> Option<String> {
    (m.is_local_node(node_id) && !may_use_local_node(m, who))
        .then(|| LOCAL_NODE_ADMINS_ONLY.to_string())
}

/// Why `who` may not make the agentic session `row` do work, or `None`
/// when they may (always `None` for chats). A row not mirrored far enough
/// to know its profile counts as agentic, so the check fails closed.
pub async fn agentic_blocked(m: &Master, who: &Principal, row: &SessionRow) -> Option<String> {
    if row.profile.as_deref() == Some("chat") {
        return None;
    }
    let ws = match &row.workspace_id {
        None => None,
        Some(id) => match access(m, who, id).await {
            Ok(a) => Some(a),
            Err(WsError::Store(e)) => {
                tracing::warn!(session = %row.session_id, error = %e, "could not check workspace access");
                return Some("could not check your access to this workspace; try again".into());
            }
            // The chat's workspace is one the caller is not in: never
            // theirs to drive.
            Err(_) => return Some("this chat is in a workspace you are not a member of".into()),
        },
    };
    agentic_refusal(who, ws.as_ref()).or_else(|| node_refusal(m, who, &row.node_id))
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
    match m.inner.mirror.call(move |s| s.running_on(&node)).await {
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

/// Cancel every running agentic session in `rows` whose owner may no
/// longer drive it.
async fn cancel_blocked(m: &Master, rows: impl Iterator<Item = SessionRow>) {
    let running =
        rows.filter(|r| r.running && r.profile.as_deref() != Some("chat") && r.owner_id.is_some());
    for row in running {
        let Ok(Some(why)) = blocked_reason(m, &row).await else {
            continue;
        };
        tracing::info!(session = %row.session_id, reason = %why, "cancelling an agentic turn that lost its access");
        cancel_retrying(m, row.session_id).await;
    }
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
            [Perm::Agentic, Perm::Settings, Perm::Members, Perm::Routines]
        );
        assert_eq!(Role::Admin.perms(), Role::Owner.perms());
        assert_eq!(Role::Member.perms(), [Perm::Agentic, Perm::Routines]);
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
