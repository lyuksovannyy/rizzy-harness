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
    #[serde(default)]
    pub default_agent: Option<String>,
    /// Agent id -> its model and fallbacks; `general` is the workspace
    /// default model.
    #[serde(default)]
    pub agent_models: BTreeMap<String, AgentChain>,
}

/// The stored document, read leniently: fields this build does not know
/// are ignored, so a later rename cannot make every workspace unreadable.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StoredSettings {
    default_agent: Option<String>,
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
        if let Some(a) = &self.default_agent
            && agents.get(a).is_none()
        {
            return Err(format!("unknown agent {a:?}"));
        }
        let models = m.models();
        let knows = |id: &str| models.iter().any(|x| x.id == id);
        for (agent, chain) in &self.agent_models {
            check_chain(&agents, &knows, agent, chain)?;
        }
        Ok(())
    }

    /// Without references that went stale since they were saved: unknown
    /// agents, chains whose main model is gone, fallbacks that are gone.
    /// What this returns always passes `validate`, so the UI can save what
    /// it read.
    pub fn normalized(&self, m: &Master) -> Self {
        let agents = m.agents();
        let models = m.models();
        let knows = |id: &str| models.iter().any(|x| x.id == id);
        Self {
            default_agent: self
                .default_agent
                .clone()
                .filter(|a| agents.get(a).is_some()),
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
    agentic_refusal(who, ws.as_ref())
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
    let rows = match m.inner.mirror.call(|s| s.sessions()).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "revocation sweep: could not list sessions");
            return;
        }
    };
    let running = rows.into_iter().filter(|r| {
        r.running
            && r.profile.as_deref() != Some("chat")
            && r.owner_id.as_ref().is_some_and(|o| users.contains(o))
    });
    for row in running {
        let owner = row.owner_id.clone().unwrap_or_default();
        let blocked = match m.inner.mirror.call(move |s| s.user_by_id(&owner)).await {
            Ok(Some(u)) if !u.disabled => agentic_blocked(m, &Principal::from_user(u), &row).await,
            Ok(_) => Some("the account is disabled or gone".to_string()),
            Err(e) => {
                tracing::warn!(session = %row.session_id, error = %e, "revocation sweep: could not load the owner");
                continue;
            }
        };
        let Some(why) = blocked else { continue };
        tracing::info!(session = %row.session_id, reason = %why, "cancelling an agentic turn that lost its access");
        if let Err(e) = m.cancel(row.session_id).await {
            tracing::warn!(session = %row.session_id, error = %e, "revocation sweep: cancel failed");
        }
    }
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
