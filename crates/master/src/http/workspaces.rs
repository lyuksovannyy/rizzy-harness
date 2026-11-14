//! Workspace endpoints. Members act within their role (the guards below);
//! server admins see every workspace, recover owners and delete team
//! workspaces, but never see chats (PLAN.md T10).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_store::{MemberRow, StoreError, WorkspaceRow};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, require, require_admin, runtime};
use crate::auth::Principal;
use crate::workspaces::{self, Access, Perm, Role, WorkspaceSettings, access};
use crate::{Live, Master, levels};
use harness_core::policy::ExecLevel;

const ONLY_YOU: &str = "a Personal workspace has only you";
const MAX_NAME: usize = 64;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn forbidden(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}

fn conflict(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg.into())
}

fn no_member() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "no such member".into())
}

/// Tell every UI to refetch its workspaces, and its chats too when their
/// rows changed: chats moved workspace (they fall back to their owners'
/// Personal ones), or a role changed, which changes `blocked` on them.
pub(super) fn changed(m: &Master, chats_changed: bool) {
    let _ = m.inner.live.send(Live::WorkspacesChanged);
    if chats_changed {
        let _ = m.inner.live.send(Live::SessionsChanged);
    }
}

/// The server's policy, or its default without a provider runtime (tests).
fn members_can_create(m: &Master) -> bool {
    m.provider_runtime()
        .is_none_or(|rt| rt.config().workspaces.members_can_create)
}

fn may_create(m: &Master, who: &Principal) -> bool {
    who.user_id.is_some() && (who.is_admin() || members_can_create(m))
}

/// A workspace name: trimmed, 1-64 characters, no control characters.
fn clean_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 || n > MAX_NAME || name.chars().any(char::is_control) {
        return Err(bad(format!(
            "a workspace name needs 1-{MAX_NAME} characters and no control characters"
        )));
    }
    Ok(name.to_string())
}

fn parse_role(id: &str) -> ApiResult<Role> {
    Role::parse(id).ok_or_else(|| bad("role must be owner, admin, member or guest"))
}

/// May `a` hand out `role`? Only owners make owners, and nobody grants a
/// role above their own. (Making someone an owner also makes their level
/// here `host`; that is not bounded by the granter's own level: see
/// docs/execution-modes.md, "Open".)
fn guard_grant(a: &Access, role: Role) -> ApiResult<()> {
    if role == Role::Owner && !a.is_owner() {
        return Err(forbidden("only an owner can make someone an owner"));
    }
    if role.rank() > a.rank() {
        return Err(forbidden("you cannot give a role above your own"));
    }
    Ok(())
}

/// May `a` change or remove a member who holds `target_role`? Owners only
/// by owners, and nobody acts on a role above their own.
pub(super) fn guard_target(a: &Access, target_role: &str) -> ApiResult<()> {
    if target_role == Role::Owner.id() && !a.is_owner() {
        return Err(forbidden("only an owner can change or remove an owner"));
    }
    if workspaces::rank_of(target_role) > a.rank() {
        return Err(forbidden(
            "you cannot change a member whose role is above yours",
        ));
    }
    Ok(())
}

/// A `WorkspaceSummary` as the web types it. `exec_level` is the caller's
/// effective level there (N.10a part 2): what the new-chat choice offers.
fn summary_json(w: &WorkspaceRow, role_id: &str, members: u64, exec_level: ExecLevel) -> Value {
    json!({
        "workspace_id": w.workspace_id,
        "name": w.name,
        "personal": w.personal(),
        "role": role_id,
        "permissions": workspaces::perms_of(role_id),
        "members": members,
        "exec_level": exec_level,
    })
}

/// `who`'s effective level in the workspace of membership `ms`.
fn level_in(who: &Principal, ms: &harness_store::Membership) -> ExecLevel {
    let owner = ms.role_id == Role::Owner.id();
    let here = levels::workspace_level(ms.workspace.personal(), owner, ms.exec_level);
    who.exec_ceiling.min(here)
}

/// A `WorkspaceDetail`: the summary, the settings with stale references
/// dropped (so saving what was read always validates), and the caller's
/// own memory switch.
async fn detail(m: &Master, who: &Principal, a: &Access) -> ApiResult<Json<Value>> {
    let id = a.id().to_string();
    let n = m.inner.mirror.call(move |s| s.member_count(&id)).await?;
    let mut v = summary_json(&a.workspace, &a.role_id, n, levels::effective(who, Some(a)));
    v["settings"] = json!(WorkspaceSettings::read(&a.workspace).normalized(m));
    v["use_memory"] = json!(a.use_memory);
    v["created_ms"] = json!(a.workspace.created_ms);
    Ok(Json(v))
}

/// A member as the list shows them: their level here as it counts (an
/// owner's is `host`), never their account's ceiling, which is theirs.
pub(super) async fn member_json(m: &Master, r: &MemberRow) -> Value {
    let prefs = crate::prefs::Prefs::load(m, Some(&r.user_id)).await;
    let owner = r.role_id == Role::Owner.id();
    json!({
        "user_id": r.user_id,
        "username": r.username,
        "display_name": prefs.display_name,
        "role": r.role_id,
        "joined_ms": r.joined_ms,
        "exec_level": levels::workspace_level(false, owner, r.exec_level),
    })
}

pub(super) async fn load_member(m: &Master, ws: &str, user_id: &str) -> ApiResult<MemberRow> {
    let (ws, uid) = (ws.to_string(), user_id.to_string());
    m.inner
        .mirror
        .call(move |s| s.member(&ws, &uid))
        .await?
        .ok_or_else(no_member)
}

/// The caller's active workspace as a summary, for `GET /api/me` (`null`
/// for the API token).
pub(super) async fn active_summary(m: &Master, who: &Principal) -> ApiResult<Value> {
    let Some(uid) = who.user_id.clone() else {
        return Ok(Value::Null);
    };
    let Some(active) = workspaces::active_workspace(m, who).await? else {
        return Ok(Value::Null);
    };
    let list = m.inner.mirror.call(move |s| s.memberships(&uid)).await?;
    Ok(list
        .iter()
        .find(|ms| ms.workspace.workspace_id == active)
        .map(|ms| summary_json(&ms.workspace, &ms.role_id, ms.members, level_in(who, ms)))
        .unwrap_or(Value::Null))
}

pub(super) async fn list(State(m): State<Master>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let Some(uid) = who.user_id.clone() else {
        return Ok(Json(
            json!({ "workspaces": [], "active": null, "can_create": false }),
        ));
    };
    // Resolving the active workspace also creates a missing Personal one.
    let active = workspaces::active_workspace(&m, &who).await?;
    let list = m.inner.mirror.call(move |s| s.memberships(&uid)).await?;
    Ok(Json(json!({
        "workspaces": list
            .iter()
            .map(|ms| summary_json(&ms.workspace, &ms.role_id, ms.members, level_in(&who, ms)))
            .collect::<Vec<_>>(),
        "active": active,
        "can_create": may_create(&m, &who),
    })))
}

#[derive(Deserialize)]
pub(super) struct NameBody {
    name: String,
}

pub(super) async fn create(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NameBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let Some(uid) = who.user_id.clone() else {
        return Err(bad("the API token cannot own a workspace"));
    };
    if !may_create(&m, &who) {
        return Err(forbidden(
            "only server admins may create workspaces on this server",
        ));
    }
    let name = clean_name(&b.name)?;
    let w = m
        .inner
        .mirror
        .call(move |s| s.create_workspace(&name, &uid))
        .await?;
    changed(&m, false);
    // Its owner's level there is `host`, so their ceiling decides.
    Ok(Json(summary_json(
        &w,
        Role::Owner.id(),
        1,
        who.exec_ceiling,
    )))
}

#[derive(Deserialize)]
pub(super) struct ActiveBody {
    workspace_id: String,
}

pub(super) async fn set_active(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<ActiveBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let Some(uid) = who.user_id.clone() else {
        return Err(bad("the API token has no active workspace"));
    };
    let a = access(&m, &who, &b.workspace_id).await?;
    workspaces::set_active_workspace(&m, &uid, a.id()).await?;
    Ok(Json(json!({ "active": a.id() })))
}

pub(super) async fn get_one(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    detail(&m, &who, &access(&m, &who, &id).await?).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PatchBody {
    #[serde(default)]
    name: Option<String>,
    /// The whole settings document; checked strictly.
    #[serde(default)]
    settings: Option<Value>,
}

pub(super) async fn patch(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<PatchBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(bad(
            "a Personal workspace keeps its name and has no settings of its own; use Preferences",
        ));
    }
    a.require(Perm::Settings)?;
    let name = b.name.as_deref().map(clean_name).transpose()?;
    let settings = b
        .settings
        .map(|v| WorkspaceSettings::parse(v, &m))
        .transpose()
        .map_err(bad)?
        .map(|s| serde_json::to_string(&s).expect("settings serialize"));
    let ws = a.id().to_string();
    m.inner
        .mirror
        .call(move |s| {
            if let Some(n) = &name {
                s.rename_workspace(&ws, n)?;
            }
            if let Some(json) = &settings {
                s.set_workspace_settings(&ws, json)?;
            }
            Ok::<_, StoreError>(())
        })
        .await?;
    changed(&m, false);
    detail(&m, &who, &access(&m, &who, &id).await?).await
}

#[derive(Deserialize)]
pub(super) struct MemoryBody {
    use_memory: bool,
}

/// The member's own switch: use their personal memory in this workspace's
/// chats.
pub(super) async fn set_memory(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<MemoryBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    let Some(uid) = who.user_id.clone() else {
        return Err(bad("the API token is no member and has no memory"));
    };
    if a.personal {
        return Err(bad("your memory is always used in your Personal workspace"));
    }
    let ws = a.id().to_string();
    let on = b.use_memory;
    m.inner
        .mirror
        .call(move |s| s.set_use_memory(&ws, &uid, on))
        .await?;
    changed(&m, false);
    detail(&m, &who, &access(&m, &who, &id).await?).await
}

/// Owners only. Its chats return to their owners' Personal workspaces.
pub(super) async fn delete(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(conflict("a Personal workspace cannot be deleted"));
    }
    if !a.is_owner() {
        return Err(forbidden("only an owner can delete this workspace"));
    }
    let ws = a.id().to_string();
    let members = delete_workspace(&m, ws).await?.unwrap_or_default();
    changed(&m, true);
    workspaces::sweep_revoked(&m, members);
    Ok(Json(json!({ "ok": true })))
}

/// Delete workspace `ws`; returns who was in it (for the revocation
/// sweep), or `None` when it did not exist. Its projects go with it, and
/// their facts.
async fn delete_workspace(m: &Master, ws: String) -> ApiResult<Option<Vec<String>>> {
    let projects = crate::projects::ids(m).await;
    let members = m
        .inner
        .mirror
        .call(move |s| {
            let members: Vec<String> = s.members(&ws)?.into_iter().map(|r| r.user_id).collect();
            Ok::<_, StoreError>(s.delete_workspace(&ws)?.then_some(members))
        })
        .await?;
    crate::projects::forget_gone(m, projects).await;
    Ok(members)
}

pub(super) async fn members(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let ws = access(&m, &who, &id).await?.id().to_string();
    let rows = m.inner.mirror.call(move |s| s.members(&ws)).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(member_json(&m, r).await);
    }
    Ok(Json(json!({ "members": out })))
}

fn member_role() -> String {
    Role::Member.id().into()
}

#[derive(Deserialize)]
pub(super) struct AddBody {
    username: String,
    #[serde(default = "member_role")]
    role: String,
}

/// Add an account by username. "No such user" tells whether a username
/// exists, so failed adds are throttled per caller like logins (T12).
pub(super) async fn add_member(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<AddBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(conflict(ONLY_YOU));
    }
    a.require(Perm::Members)?;
    let role = parse_role(&b.role)?;
    guard_grant(&a, role)?;
    let caller = who.user_id.clone().unwrap_or_else(|| "api-token".into());
    let now = harness_core::now_ms();
    let auth = &m.inner.auth;
    // Reserved before the lookup and kept when it fails, so parallel adds
    // cannot all pass the check before any failure counts.
    if !auth.try_reserve_lookup(&caller, now) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many unknown usernames; try again in 15 minutes".into(),
        ));
    }
    let name = b.username.trim().to_ascii_lowercase();
    let found = m
        .inner
        .mirror
        .call(move |s| s.user_by_username(&name))
        .await;
    let user = match found {
        Ok(u) => u.filter(|u| !u.disabled),
        Err(e) => {
            auth.release_lookup(&caller, now);
            return Err(e.into());
        }
    };
    let Some(user) = user else {
        return Err(ApiError(StatusCode::NOT_FOUND, "no such user".into()));
    };
    auth.release_lookup(&caller, now);
    let (ws, uid) = (a.id().to_string(), user.user_id.clone());
    let added = m
        .inner
        .mirror
        .call(move |s| s.add_member(&ws, &uid, role.id()))
        .await;
    match added {
        Ok(true) => {}
        Ok(false) => return Err(workspaces::WsError::NotFound.into()),
        Err(StoreError::Conflict(_)) => {
            return Err(conflict(format!("{} is already a member", user.username)));
        }
        Err(e) => return Err(e.into()),
    }
    let row = load_member(&m, a.id(), &user.user_id).await?;
    changed(&m, false);
    Ok(Json(member_json(&m, &row).await))
}

#[derive(Deserialize)]
pub(super) struct RoleBody {
    role: String,
}

pub(super) async fn set_role(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
    Json(b): Json<RoleBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(conflict(ONLY_YOU));
    }
    if who.user_id.as_deref() == Some(user_id.as_str()) {
        return Err(conflict("you cannot change your own role"));
    }
    a.require(Perm::Members)?;
    let role = parse_role(&b.role)?;
    let target = load_member(&m, a.id(), &user_id).await?;
    guard_target(&a, &target.role_id)?;
    guard_grant(&a, role)?;
    let (ws, uid, was) = (a.id().to_string(), user_id.clone(), target.role_id.clone());
    // The store refuses to take the last enabled owner away, and a change
    // when their role is no longer the one guarded above (409).
    let found = m
        .inner
        .mirror
        .call(move |s| s.set_member_role(&ws, &uid, Some(&was), role.id()))
        .await?;
    if !found {
        return Err(no_member());
    }
    let row = load_member(&m, a.id(), &user_id).await?;
    audit_owner_change(&m, &who, a.id(), &target, &row).await;
    // Their chats here may have become read-only, or writable again.
    changed(&m, true);
    // An owner made a member falls to their stored level: their running
    // chats above it stop.
    workspaces::sweep_revoked(&m, vec![user_id]);
    Ok(Json(member_json(&m, &row).await))
}

/// Remove a member, or leave when it is the caller. Their chats here
/// return to their Personal workspace; a workspace whose last member left
/// is deleted, since nobody could reach it again.
pub(super) async fn remove_member(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(conflict(ONLY_YOU));
    }
    // Leaving needs no guard; removing someone else is guarded on their
    // role, which must still be the same when the store removes them.
    let before = load_member(&m, a.id(), &user_id).await?;
    let guarded = if who.user_id.as_deref() == Some(user_id.as_str()) {
        None
    } else {
        a.require(Perm::Members)?;
        guard_target(&a, &before.role_id)?;
        Some(before.role_id.clone())
    };
    // Their chats here, read before they move to Personal: what runs in
    // them for this workspace stops (N.10a part 2).
    let (ws, uid) = (a.id().to_string(), user_id.clone());
    let theirs: Vec<_> = m
        .inner
        .mirror
        .call(move |s| s.sessions_of(Some(&uid)))
        .await?
        .into_iter()
        .filter(|r| r.owner_id.as_deref() == Some(user_id.as_str()))
        .filter(|r| r.workspace_id.as_deref() == Some(ws.as_str()))
        .map(|r| r.session_id)
        .collect();
    let (ws, uid) = (a.id().to_string(), user_id.clone());
    // The store refuses to take the last enabled owner away, and a removal
    // when their role is no longer the one guarded above (409).
    let removed = m
        .inner
        .mirror
        .call(move |s| {
            let removed = s.remove_member(&ws, &uid, guarded.as_deref())?;
            if removed && s.member_count(&ws)? == 0 {
                s.delete_workspace(&ws)?;
            }
            Ok::<_, StoreError>(removed)
        })
        .await?;
    if !removed {
        return Err(no_member());
    }
    let owner = before.role_id == Role::Owner.id();
    let from = levels::workspace_level(false, owner, before.exec_level);
    let detail = json!({"by": levels::by(&who), "user": user_id, "workspace": a.id(),
        "from": from, "to": null, "reason": "removed"});
    levels::audit(&m, "level.workspace", detail).await;
    changed(&m, true);
    // Their chats from here are back in Personal, where their account's
    // ceiling applies from their next action; what was running for this
    // workspace stops now.
    workspaces::stop_removed(&m, theirs);
    workspaces::sweep_revoked(&m, vec![user_id]);
    Ok(Json(json!({ "ok": true })))
}

/// One `level.workspace` audit line when a role change made or unmade an
/// owner, whose level there counts as `host` (N.10a part 2).
async fn audit_owner_change(
    m: &Master,
    who: &Principal,
    ws: &str,
    before: &MemberRow,
    after: &MemberRow,
) {
    let level =
        |r: &MemberRow| levels::workspace_level(false, r.role_id == Role::Owner.id(), r.exec_level);
    let (from, to) = (level(before), level(after));
    if from != to {
        let detail = json!({"by": levels::by(who), "user": after.user_id, "workspace": ws,
            "from": from, "to": to, "reason": "role"});
        levels::audit(m, "level.workspace", detail).await;
    }
}

// ---- server admins -------------------------------------------------------

fn admin_row(w: &WorkspaceRow, owners: &[String], members: u64) -> Value {
    json!({
        "workspace_id": w.workspace_id,
        "name": w.name,
        "personal": w.personal(),
        "owners": owners,
        "members": members,
        "created_ms": w.created_ms,
    })
}

/// One workspace as server admins list it; `None` when unknown.
async fn admin_one(m: &Master, ws: &str) -> ApiResult<Option<Value>> {
    let ws = ws.to_string();
    Ok(m.inner
        .mirror
        .call(move |s| {
            let Some(w) = s.workspace(&ws)? else {
                return Ok(None);
            };
            Ok::<_, StoreError>(Some(admin_row(&w, &s.owners(&ws)?, s.member_count(&ws)?)))
        })
        .await?)
}

/// Every workspace with its owners and member count: team ones by name,
/// then the Personal ones. No chats.
pub(super) async fn admin_list(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    let rows = m
        .inner
        .mirror
        .call(|s| {
            let mut out = Vec::new();
            for w in s.workspaces()? {
                let id = &w.workspace_id;
                out.push(admin_row(&w, &s.owners(id)?, s.member_count(id)?));
            }
            Ok::<_, StoreError>(out)
        })
        .await?;
    Ok(Json(json!({
        "workspaces": rows,
        "members_can_create": members_can_create(&m),
    })))
}

pub(super) async fn admin_members(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    let (w, rows) = m
        .inner
        .mirror
        .call(move |s| Ok::<_, StoreError>((s.workspace(&id)?, s.members(&id)?)))
        .await?;
    if w.is_none() {
        return Err(workspaces::WsError::NotFound.into());
    }
    Ok(Json(json!({
        "members": rows.iter().map(|r| json!({
            "user_id": r.user_id,
            "username": r.username,
            "role": r.role_id,
            "disabled": r.disabled,
        })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub(super) struct PolicyBody {
    members_can_create: bool,
}

/// Whether accounts other than server admins may create team workspaces
/// (config.toml `[workspaces]`).
pub(super) async fn admin_policy(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<PolicyBody>,
) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    let rt = runtime(&m)?;
    let _edits = rt.lock_edits().await;
    let mut cfg = rt.config();
    cfg.workspaces.members_can_create = b.members_can_create;
    rt.save(cfg)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e.to_string()))?;
    changed(&m, false);
    Ok(Json(json!({ "members_can_create": b.members_can_create })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdminPatchBody {
    /// The user id of an enabled member who becomes an owner.
    owner: String,
}

/// Owner recovery: make an existing, enabled member an owner (team
/// workspaces only; never the calling admin themselves).
pub(super) async fn admin_patch(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<AdminPatchBody>,
) -> ApiResult<Json<Value>> {
    let who = require_admin(&m, &headers).await?;
    let ws = id.clone();
    let w = m
        .inner
        .mirror
        .call(move |s| s.workspace(&ws))
        .await?
        .ok_or(workspaces::WsError::NotFound)?;
    if w.personal() {
        return Err(conflict("a Personal workspace has only its owner"));
    }
    if who.user_id.as_deref() == Some(b.owner.as_str()) {
        return Err(conflict(
            "you cannot make yourself an owner; ask another server admin",
        ));
    }
    let target = load_member(&m, &id, &b.owner).await?;
    if target.disabled {
        return Err(conflict(format!("{} is disabled", target.username)));
    }
    let (ws, uid) = (id.clone(), b.owner.clone());
    // Whatever their role is now, they become an owner.
    m.inner
        .mirror
        .call(move |s| s.set_member_role(&ws, &uid, None, Role::Owner.id()))
        .await?;
    let row = load_member(&m, &id, &b.owner).await?;
    audit_owner_change(&m, &who, &id, &target, &row).await;
    // Their chats here may have become writable.
    changed(&m, true);
    admin_one(&m, &id)
        .await?
        .map(Json)
        .ok_or_else(|| workspaces::WsError::NotFound.into())
}

/// Delete a team workspace; its chats return to their owners' Personal
/// workspaces.
pub(super) async fn admin_delete(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_admin(&m, &headers).await?;
    // A Personal workspace is refused by the store (409).
    let Some(members) = delete_workspace(&m, id).await? else {
        return Err(workspaces::WsError::NotFound.into());
    };
    changed(&m, true);
    workspaces::sweep_revoked(&m, members);
    Ok(Json(json!({ "ok": true })))
}
