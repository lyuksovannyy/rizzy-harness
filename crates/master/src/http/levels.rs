//! Setting execution levels (O13, N.10a part 2; `crate::levels`):
//!
//! * `PUT /api/users/{id}/exec-ceiling` — server admins set an account's
//!   ceiling (never their own).
//! * `PUT /api/workspaces/{id}/members/{user_id}/exec-level` — a
//!   workspace's admins set a member's level there: never their own, never
//!   above their own effective level, `host` only by an owner, and an
//!   owner's level is always `host`.
//!
//! Each change is conditional on what its guards read (two admins at once:
//! the second gets 409), writes one audit line in the same transaction
//! (who, whose, where, from, to), and a lowering stops what its new level
//! no longer allows through the revocation sweep.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_core::policy::ExecLevel;
use harness_store::StoreError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::workspaces::{changed, guard_target, load_member, member_json};
use super::{ApiError, ApiResult, require, require_admin};
use crate::workspaces::{self, Perm, Role, access};
use crate::{Live, Master, levels};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LevelBody {
    level: ExecLevel,
}

fn conflict(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg.into())
}

fn forbidden(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}

/// A change refused by the store because what the guards read changed
/// meanwhile is a 409, like a role change.
fn store_err(e: StoreError) -> ApiError {
    match e {
        StoreError::Refused(m) => conflict(m),
        other => other.into(),
    }
}

pub(super) async fn set_ceiling(
    State(m): State<Master>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<LevelBody>,
) -> ApiResult<Json<Value>> {
    let who = require_admin(&m, &headers).await?;
    if who.user_id.as_deref() == Some(id.as_str()) {
        return Err(conflict("you cannot change your own account's ceiling"));
    }
    let uid = id.clone();
    let target = m
        .inner
        .mirror
        .call(move |s| s.user_by_id(&uid))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown user".into()))?;
    let (from, to) = (target.exec_ceiling, b.level);
    if from != to {
        let detail = json!({"by": levels::by(&who), "user": target.user_id,
            "from": from, "to": to})
        .to_string();
        let uid = id.clone();
        let found = m
            .inner
            .mirror
            .call(move |s| s.set_exec_ceiling(&uid, from, to, &detail))
            .await
            .map_err(store_err)?;
        if !found {
            return Err(ApiError(StatusCode::NOT_FOUND, "unknown user".into()));
        }
        let _ = m.inner.live.send(Live::WorkspacesChanged);
        let _ = m.inner.live.send(Live::SessionsChanged);
        // Lowered: their chats above the new ceiling stop (every workspace).
        if to < from {
            workspaces::sweep_revoked(&m, vec![id.clone()]);
        }
    }
    let uid = id.clone();
    let after = m
        .inner
        .mirror
        .call(move |s| s.user_by_id(&uid))
        .await?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown user".into()))?;
    Ok(Json(super::users::user_json(&after)))
}

pub(super) async fn set_member_level(
    State(m): State<Master>,
    headers: HeaderMap,
    Path((id, user_id)): Path<(String, String)>,
    Json(b): Json<LevelBody>,
) -> ApiResult<Json<Value>> {
    let who = require(&m, &headers).await?;
    let a = access(&m, &who, &id).await?;
    if a.personal {
        return Err(conflict(
            "a Personal workspace has no levels: there the account's ceiling applies, which a \
             server admin sets",
        ));
    }
    if who.user_id.as_deref() == Some(user_id.as_str()) {
        return Err(conflict("you cannot change your own level"));
    }
    a.require(Perm::Members)?;
    let target = load_member(&m, a.id(), &user_id).await?;
    guard_target(&a, &target.role_id)?;
    if target.role_id == Role::Owner.id() {
        return Err(conflict(
            "an owner's level here is always host; their account's ceiling still applies",
        ));
    }
    let to = b.level;
    if to == ExecLevel::Host && !a.is_owner() {
        return Err(forbidden(format!(
            "only an owner of {} can grant host",
            a.workspace.name
        )));
    }
    let mine = levels::effective(&who, Some(&a));
    if to > mine {
        return Err(forbidden(format!(
            "you cannot grant a level above your own (yours in {} is {})",
            a.workspace.name,
            mine.id()
        )));
    }
    let from = target.exec_level;
    if from != to {
        let detail = json!({"by": levels::by(&who), "user": user_id, "workspace": a.id(),
            "from": from, "to": to})
        .to_string();
        let (ws, uid, role) = (a.id().to_string(), user_id.clone(), target.role_id.clone());
        let found = m
            .inner
            .mirror
            .call(move |s| s.set_member_level(&ws, &uid, &role, from, to, &detail))
            .await
            .map_err(store_err)?;
        if !found {
            return Err(ApiError(StatusCode::NOT_FOUND, "no such member".into()));
        }
        // `blocked` on their chats here may have changed.
        changed(&m, true);
        if to < from {
            workspaces::sweep_revoked(&m, vec![user_id.clone()]);
        }
    }
    let row = load_member(&m, a.id(), &user_id).await?;
    Ok(Json(member_json(&m, &row).await))
}
