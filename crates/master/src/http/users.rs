//! Accounts, as admins manage them: `/api/users`.
//!
//! Temporary passwords are made here, never chosen (AUDIT L14, FIX_PLAN
//! 3.10): creating an account and `POST /api/users/{id}/password` answer a
//! fresh one from [`auth::temporary_password`] in the response body, once.
//! Only its Argon2id hash is stored, the account must change it at its next
//! login, and a request that names a password is refused, so an admin can
//! neither pick a weak one nor type one where it can be read over their
//! shoulder. The plaintext is never logged, never in an event or a URL.
//! The owner's own first password is set by the claim (`/api/setup`), the
//! one place a password is chosen without an account.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_core::Secret;
use harness_core::policy::ExecLevel;
use harness_store::Role;
use serde::Deserialize;
use serde_json::json;

use super::{ApiError, ApiResult, require_admin};
use crate::workspaces as ws;
use crate::{Live, Master, auth};

pub(super) fn user_json(u: &harness_store::UserRow) -> serde_json::Value {
    json!({
        "user_id": u.user_id,
        "username": u.username,
        "role": u.role,
        "disabled": u.disabled,
        // As it counts (a server admin's is `host`), and as stored (what
        // applies once they are no longer an admin).
        "exec_ceiling": crate::levels::ceiling_of(u),
        "exec_ceiling_stored": u.exec_ceiling,
        "must_change_password": u.must_change_password,
        "created_ms": u.created_ms,
    })
}

/// The account, plus the temporary password this answer carries once.
fn with_password(u: &harness_store::UserRow, pw: &Secret<String>) -> serde_json::Value {
    let mut v = user_json(u);
    v["password"] = json!(pw.expose());
    v
}

const CHOSEN: &str = "the server makes temporary passwords: leave out \"password\" (reset with POST /api/users/{id}/password)";

pub(super) async fn list_users(
    State(m): State<Master>,
    headers: HeaderMap,
) -> ApiResult<Json<serde_json::Value>> {
    require_admin(&m, &headers).await?;
    let users = m.inner.mirror.call(|s| s.list_users()).await?;
    Ok(Json(
        json!({ "users": users.iter().map(user_json).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
pub(super) struct NewUserBody {
    username: String,
    /// Refused when present: see the module comment. Any JSON value, so
    /// the answer is that refusal and not a type error.
    #[serde(default)]
    password: Option<serde_json::Value>,
    #[serde(default = "member")]
    role: Role,
    /// `none` unless asked (the UI's form does not ask: the ceiling is set
    /// in the list, behind the host warning). Audited when not `none`.
    #[serde(default)]
    exec_ceiling: ExecLevel,
}

fn member() -> Role {
    Role::Member
}

pub(super) async fn create_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Json(b): Json<NewUserBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    if b.password.is_some() {
        return Err(ApiError(StatusCode::BAD_REQUEST, CHOSEN.into()));
    }
    let pw = auth::temporary_password();
    let u = auth::create_account(
        &m.inner.mirror,
        m.inner.auth.hasher(),
        b.username.trim().to_ascii_lowercase(),
        pw.expose().to_string(),
        b.role,
        b.exec_ceiling,
        true,
    )
    .await?;
    if b.exec_ceiling != ExecLevel::None {
        let detail = json!({"by": crate::levels::by(&who), "user": u.user_id,
            "from": ExecLevel::None, "to": b.exec_ceiling, "reason": "created"});
        crate::levels::audit(&m, "level.account", detail).await;
    }
    Ok(Json(with_password(&u, &pw)))
}

#[derive(Deserialize)]
pub(super) struct UserPatchBody {
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    disabled: Option<bool>,
    /// Refused when present: the ceiling has its own request
    /// (`PUT /api/users/{id}/exec-ceiling`), with its own checks.
    #[serde(default)]
    can_agentic: Option<serde_json::Value>,
    /// Refused when present: resets go through `POST …/password`.
    #[serde(default)]
    password: Option<serde_json::Value>,
}

/// Refuse changes that would lock everyone out or edit your own access.
async fn guard_admin_change(
    m: &Master,
    who: &auth::Principal,
    target: &harness_store::UserRow,
    removes_admin: bool,
) -> ApiResult<()> {
    if who.user_id.as_deref() == Some(target.user_id.as_str()) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "you cannot change your own role or access, or delete yourself".into(),
        ));
    }
    if removes_admin && target.role == Role::Admin && !target.disabled {
        let admins = m.inner.mirror.call(|s| s.active_admin_count()).await?;
        if admins <= 1 {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "this is the last active admin".into(),
            ));
        }
    }
    Ok(())
}

async fn load_user(m: &Master, id: &str) -> ApiResult<harness_store::UserRow> {
    let id = id.to_string();
    m.inner
        .mirror
        .call(move |s| s.user_by_id(&id))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown user".into()))
}

pub(super) async fn update_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<UserPatchBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    if b.password.is_some() {
        return Err(ApiError(StatusCode::BAD_REQUEST, CHOSEN.into()));
    }
    if b.can_agentic.is_some() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "can_agentic is gone: set the account's ceiling with PUT /api/users/{id}/exec-ceiling"
                .into(),
        ));
    }
    let target = load_user(&m, &id).await?;
    let changes_access = b.role.is_some() || b.disabled.is_some();
    if changes_access {
        let removes_admin = b.disabled == Some(true) || b.role == Some(Role::Member);
        guard_admin_change(&m, &who, &target, removes_admin).await?;
        let patch = harness_store::UserPatch {
            role: b.role,
            disabled: b.disabled,
        };
        let uid = target.user_id.clone();
        let disable = b.disabled == Some(true);
        m.inner
            .mirror
            .call(move |s| {
                s.update_user(&uid, &patch)?;
                if disable {
                    s.ui_sessions_delete_for_user(&uid)?;
                }
                Ok::<_, harness_store::StoreError>(())
            })
            .await?;
        if disable {
            auth::close_sockets(&m, &target.user_id);
        }
        // Becoming or ceasing to be a server admin, or being disabled or
        // enabled, moves the ceiling as it counts: one audit line.
        let after = load_user(&m, &id).await?;
        let (from, to) = (
            crate::levels::ceiling_of(&target),
            crate::levels::ceiling_of(&after),
        );
        if from != to {
            let detail = json!({"by": crate::levels::by(&who), "user": target.user_id,
                "from": from, "to": to, "reason": "role or disabled"});
            crate::levels::audit(&m, "level.account", detail).await;
        }
        // Disabled owners stop counting as owners, and whether the user may
        // drive their agentic chats (`blocked`) may have changed: running
        // ones that lost their access stop.
        let _ = m.inner.live.send(Live::WorkspacesChanged);
        let _ = m.inner.live.send(Live::SessionsChanged);
        ws::sweep_revoked(&m, vec![target.user_id.clone()]);
    }
    Ok(Json(user_json(&load_user(&m, &id).await?)))
}

/// A new temporary password for someone else's account: the old one stops
/// working, every login of theirs ends (open sockets included), and the
/// next login must change it. Admins change their own at `/api/me/password`.
pub(super) async fn reset_password(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    let target = load_user(&m, &id).await?;
    if who.user_id.as_deref() == Some(target.user_id.as_str()) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "change your own password in your account settings".into(),
        ));
    }
    let pw = auth::temporary_password();
    auth::set_password(&m, &target.user_id, pw.expose().to_string(), true).await?;
    Ok(Json(with_password(&load_user(&m, &id).await?, &pw)))
}

pub(super) async fn delete_user(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let who = require_admin(&m, &headers).await?;
    let target = load_user(&m, &id).await?;
    guard_admin_change(&m, &who, &target, true).await?;
    let uid = target.user_id.clone();
    // Refused (409) while they are the only owner of a team workspace with
    // other members; their chats leave every team workspace. The projects of
    // the workspaces that go with them go too, and their facts.
    let projects = crate::projects::ids(&m).await;
    let owner = uid.clone();
    let chats: Vec<_> = m
        .inner
        .mirror
        .call(move |s| s.sessions_of(Some(&owner)))
        .await?
        .into_iter()
        .filter(|r| r.owner_id.as_deref() == Some(uid.as_str()))
        .map(|r| r.session_id)
        .collect();
    let of = target.user_id.clone();
    let workspaces: Vec<String> = m
        .inner
        .mirror
        .call(move |s| s.memberships(&of))
        .await?
        .into_iter()
        .map(|ms| ms.workspace.workspace_id)
        .collect();
    m.inner.mirror.call(move |s| s.delete_user(&uid)).await?;
    // What memory held about them goes with the account (PLAN.md T49).
    crate::memory::forget_person(&m, &target.user_id, &workspaces).await;
    auth::close_sockets(&m, &target.user_id);
    // Their stored files lost their rows with the account; unlink them.
    crate::storage::on_user_deleted(&m).await;
    crate::projects::forget_gone(&m, projects).await;
    let _ = m.inner.live.send(Live::WorkspacesChanged);
    let _ = m.inner.live.send(Live::SessionsChanged);
    // Their running agentic chats stop (the account is gone), and every
    // chat of theirs is deleted on the master and on its node (P-6).
    ws::sweep_revoked(&m, vec![target.user_id]);
    m.delete_sessions_of(chats);
    Ok(Json(json!({ "ok": true })))
}
