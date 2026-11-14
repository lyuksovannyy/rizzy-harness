//! Setting execution levels (O13, N.10a part 2; `crate::levels`):
//!
//! * `PUT /api/users/{id}/exec-ceiling` — server admins set an account's
//!   ceiling (never their own).
//! * `PUT /api/workspaces/{id}/members/{user_id}/exec-level` — a
//!   workspace's admins set a member's level there: never their own, never
//!   to a level above their own effective level, never for a member whose
//!   level is above it, `host` only by an owner, and an owner's level is
//!   always `host`.
//!
//! Each change is conditional on what its guards read, the setter's own
//! standing included (two admins at once, or a setter lowered meanwhile:
//! 409), writes one audit line in the same transaction
//! (who, whose, where, from, to), and a lowering stops what its new level
//! no longer allows through the revocation sweep.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use harness_core::policy::ExecLevel;
use harness_store::{Setter, StoreError};
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
    // The setter as read: still an enabled server admin when it is written
    // (review L2-9). The API token: none.
    let setter = match &who.user_id {
        Some(uid) => {
            let uid = uid.clone();
            let u = m
                .inner
                .mirror
                .call(move |s| s.user_by_id(&uid))
                .await?
                .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "unknown user".into()))?;
            Some(Setter {
                user_id: u.user_id,
                member: None,
                exec_ceiling: u.exec_ceiling,
                server_admin: u.role == harness_store::Role::Admin,
            })
        }
        None => None,
    };
    if from != to {
        let detail = json!({"by": levels::by(&who), "user": target.user_id,
            "from": from, "to": to})
        .to_string();
        m.level_write_pause().await;
        let uid = id.clone();
        let found = m
            .inner
            .mirror
            .call(move |s| s.set_exec_ceiling(&uid, from, to, setter.as_ref(), &detail))
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
    // The setter as their guards read them, from one reading: the write
    // below holds only while it still stands (review L2-9). The API token
    // is no member and is `host`.
    let me = match &who.user_id {
        Some(uid) => Some(load_member(&m, a.id(), uid).await?),
        None => None,
    };
    let mine = me
        .as_ref()
        .map_or(ExecLevel::Host, levels::member_effective);
    if to > mine {
        return Err(forbidden(format!(
            "you cannot grant a level above your own (yours in {} is {})",
            a.workspace.name,
            mine.id()
        )));
    }
    let from = target.exec_level;
    // Nobody changes a level above their own, in either direction (review
    // L2-10, decided): an admin below host cannot undo an owner's grant.
    if from > mine {
        return Err(forbidden(format!(
            "you cannot change a member whose level is above your own (theirs in {} is {}, \
             yours is {})",
            a.workspace.name,
            from.id(),
            mine.id()
        )));
    }
    let setter = me.map(|r| Setter {
        member: Some((r.role_id.clone(), r.exec_level)),
        exec_ceiling: r.exec_ceiling,
        server_admin: r.server_admin,
        user_id: r.user_id,
    });
    if from != to {
        let detail = json!({"by": levels::by(&who), "user": user_id, "workspace": a.id(),
            "from": from, "to": to})
        .to_string();
        m.level_write_pause().await;
        let (ws, uid, role) = (a.id().to_string(), user_id.clone(), target.role_id.clone());
        let found = m
            .inner
            .mirror
            .call(move |s| s.set_member_level(&ws, &uid, &role, from, to, setter.as_ref(), &detail))
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
    Ok(Json(member_json(&m, &row, true).await))
}
